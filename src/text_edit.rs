use crate::position::*;
use crate::util::*;
use itertools::Itertools;
use lsp_types::*;
use ropey::Rope;
use std::collections::HashSet;

pub fn apply_text_edits_to_file(uri: Option<&Url>, text_edits: &[TextEdit]) {}

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
            x.range.start.byte,
            x.range.end.line,
            x.range.end.byte,
        )
    });

    let select_edits = edits.iter().map(|edit| format!("{}", edit.range)).join(" ");

    let adjoin_selections = edits
        .windows(2)
        .enumerate()
        .filter_map(|(i, pair)| {
            let end = &pair[0].range.end;
            let start = &pair[1].range.start;
            if (end.line == start.line && end.byte + 1 == start.byte)
                || (end.line + 1 == start.line && end.byte == EOL_OFFSET && start.byte == 1)
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
                "lsp-apply-edits-to-file {} {}",
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
                byte: 1,
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
