use crate::position::*;
use crate::types::*;
use crate::util::*;
use itertools::Itertools;
use lsp_types::*;
use ropey::{Rope, RopeSlice};
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};

pub fn apply_text_edits_to_file(uri: &Url, text_edits: &[TextEdit], offset_encoding: &str) {
    let path = uri.to_file_path().unwrap();
    let filename = path.to_str().unwrap();
    let text = Rope::from_reader(BufReader::new(File::open(filename).unwrap())).unwrap();
    let mut output = BufWriter::new(File::create(filename).unwrap());
    let character_to_char = match offset_encoding {
        "utf-8" => character_to_char_utf_8_bytes,
        _ => character_to_char_utf_8_scalar,
    };
    let mut cursor = 0;
    for TextEdit { range, new_text } in text_edits {
        let column_to_char =
            character_to_char(text.line(range.start.line as _), range.start.character as _);
        let start = text.line_to_char(range.start.line as _) + column_to_char;
        let column_to_char =
            character_to_char(text.line(range.end.line as _), range.end.character as _);
        let end = text.line_to_char(range.end.line as _) + column_to_char;
        for chunk in text.slice(cursor..start).chunks() {
            output.write_all(chunk.as_bytes()).unwrap();
        }
        output.write_all(new_text.as_bytes()).unwrap();
        cursor = end;
    }
    for chunk in text.slice(cursor..).chunks() {
        output.write_all(chunk.as_bytes()).unwrap();
    }
}

fn character_to_char_utf_8_scalar(_line: RopeSlice, character: usize) -> usize {
    character
}

fn character_to_char_utf_8_bytes(line: RopeSlice, character: usize) -> usize {
    line.byte_to_char(character)
}

pub fn apply_text_edits_to_buffer(
    uri: Option<&Url>,
    text_edits: &[TextEdit],
    text: &Rope,
    offset_encoding: &str,
) -> String {
    // Empty text edits processed as a special case because Kakoune's `select` command
    // doesn't support empty arguments list.
    if text_edits.is_empty() {
        // Nothing to do, but sending command back to the editor is required to handle case when
        // editor is blocked waiting for response via fifo.
        return "nop".to_string();
    }
    let mut edits = text_edits
        .iter()
        .map(|text_edit| lsp_text_edit_to_kakoune(text_edit, text, offset_encoding))
        .collect::<Vec<_>>();

    // Adjoin selections detection and Kakoune side editing relies on edits being ordered left to
    // right. Language servers usually send them such, but spec doesn't say anything about the order
    // hence we ensure it by sorting. It's improtant to use stable sort to handle properly cases
    // like multiple inserts in the same place.
    edits.sort_by_key(|x| {
        (
            x.range.start.line,
            x.range.start.column,
            x.range.end.line,
            x.range.end.column,
        )
    });

    let select_edits = edits.iter().map(|edit| format!("{}", edit.range)).join(" ");

    let adjoin_selections = edits
        .windows(2)
        .enumerate()
        .filter_map(|(i, pair)| {
            let end = &pair[0].range.end;
            let start = &pair[1].range.start;
            if (end.line == start.line && end.column + 1 == start.column)
                || (end.line + 1 == start.line && end.column == EOL_OFFSET && start.column == 1)
            {
                Some(i)
            } else {
                None
            }
        })
        .collect::<HashSet<_>>();

    let mut selection_index = 0;
    let apply_edits = edits
        .iter()
        .enumerate()
        .map(
            |(
                i,
                KakouneTextEdit {
                    new_text, command, ..
                },
            )| {
                let command = match command {
                    KakouneTextEditCommand::InsertBefore => "lsp-insert-before-selection",
                    KakouneTextEditCommand::InsertAfter => "lsp-insert-after-selection",
                    KakouneTextEditCommand::Replace => "lsp-replace-selection",
                };
                let command = format!(
                    "exec 'z{}<space>'
                    {} {}",
                    if selection_index > 0 {
                        format!("{})", selection_index)
                    } else {
                        String::new()
                    },
                    command,
                    editor_quote(&new_text)
                );
                // Replacing adjoin selection with empty content effectively removes it and requires one
                // less selection cycle after the next restore to get to the next selection.
                if !(adjoin_selections.contains(&i) && new_text.is_empty()) {
                    selection_index += 1;
                }
                command
            },
        )
        .join("\n");

    let command = format!(
        "select {}
            exec -save-regs '' Z
            {}",
        select_edits, apply_edits
    );
    let command = format!("eval -draft -save-regs '^' {}", editor_quote(&command));
    match uri {
        Some(uri) => {
            let buffile = uri.to_file_path().unwrap();
            format!(
                "evaluate-commands -buffer {} {}",
                editor_quote(buffile.to_str().unwrap()),
                editor_quote(&command)
            )
        }
        None => command,
    }
}

enum KakouneTextEditCommand {
    InsertBefore,
    InsertAfter,
    Replace,
}

struct KakouneTextEdit {
    range: KakouneRange,
    new_text: String,
    command: KakouneTextEditCommand,
}

fn lsp_text_edit_to_kakoune(
    text_edit: &TextEdit,
    text: &Rope,
    offset_encoding: &str,
) -> KakouneTextEdit {
    let TextEdit { range, new_text } = text_edit;
    let Range { start, end } = range;
    let kakoune_range = lsp_range_to_kakoune(range, text, offset_encoding);
    let insert = start.line == end.line && start.character == end.character;
    // Beginning of line is a very special case as we need to produce selection on the line
    // to insert, and then insert before that selection. Selecting end of the previous line
    // and inserting after selection doesn't work well for delete+insert cases like this:
    /*
        [
          {
            "range": {
              "start": {
                "line": 5,
                "character": 0
              },
              "end": {
                "line": 6,
                "character": 0
              }
            },
            "newText": ""
          },
          {
            "range": {
              "start": {
                "line": 6,
                "character": 0
              },
              "end": {
                "line": 6,
                "character": 0
              }
            },
            "newText": "	fmt.Println(\"Hello, world!\")\n"
          }
        ]
    */
    let bol_insert = insert && range.end.character == 0;

    let range = if bol_insert {
        let KakouneRange { start, end } = kakoune_range;
        KakouneRange {
            start,
            end: KakounePosition {
                line: end.line + 1,
                column: 1,
            },
        }
    } else {
        kakoune_range
    };

    let command = if bol_insert {
        KakouneTextEditCommand::InsertBefore
    } else if insert {
        KakouneTextEditCommand::InsertAfter
    } else {
        KakouneTextEditCommand::Replace
    };

    KakouneTextEdit {
        range,
        new_text: new_text.to_owned(),
        command,
    }
}
