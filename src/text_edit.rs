use crate::position::*;
use crate::types::*;
use crate::util::*;
use itertools::Itertools;
use lsp_types::*;
use ropey::{Rope, RopeSlice};
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};

pub fn apply_text_edits_to_file(
    uri: &Url,
    text_edits: &[TextEdit],
    offset_encoding: &OffsetEncoding,
) -> std::io::Result<()> {
    let mut temp_path = temp_dir();
    temp_path.push(format!("{:x}", rand::random::<u64>()));

    let path = uri.to_file_path().unwrap();
    let filename = path.to_str().unwrap();

    let path = std::ffi::CString::new(filename).unwrap();
    let mut stat;
    if unsafe {
        stat = std::mem::zeroed();
        libc::stat(path.as_ptr(), &mut stat)
    } != 0
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("Failed to stat {}", filename),
        ));
    }

    let file = File::open(filename)?;
    let text = Rope::from_reader(BufReader::new(file))?;

    let temp_file = File::create(&temp_path)?;

    fn apply_text_edits_to_file_impl(
        text: Rope,
        temp_file: File,
        text_edits: &[TextEdit],
        offset_encoding: &OffsetEncoding,
    ) -> Result<(), std::io::Error> {
        let mut output = BufWriter::new(temp_file);

        let character_to_offset = match offset_encoding {
            OffsetEncoding::Utf8 => character_to_offset_utf_8_code_units,
            // Not a proper UTF-16 code units handling, but works within BMP
            OffsetEncoding::Utf16 => character_to_offset_utf_8_code_points,
        };

        let text_len_lines = text.len_lines() as u64;
        let mut cursor = 0;

        for TextEdit {
            range: Range { start, end },
            new_text,
        } in text_edits
        {
            if start.line >= text_len_lines || end.line >= text_len_lines {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Text edit range extends past end of file.",
                ));
            }

            let start_offset =
                character_to_offset(text.line(start.line as _), start.character as _);
            let end_offset = character_to_offset(text.line(end.line as _), end.character as _);

            if start_offset.is_none() || end_offset.is_none() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Text edit range points past end of line.",
                ));
            }

            let start_char = text.line_to_char(start.line as _) + start_offset.unwrap();
            let end_char = text.line_to_char(end.line as _) + end_offset.unwrap();

            for chunk in text.slice(cursor..start_char).chunks() {
                output.write_all(chunk.as_bytes())?;
            }

            output.write_all(new_text.as_bytes())?;
            cursor = end_char;
        }

        for chunk in text.slice(cursor..).chunks() {
            output.write_all(chunk.as_bytes())?;
        }

        Ok(())
    }

    apply_text_edits_to_file_impl(text, temp_file, text_edits, offset_encoding)
        .and_then(|_| std::fs::rename(&temp_path, filename))
        .and_then(|_| {
            Ok(unsafe {
                libc::chmod(path.as_ptr(), stat.st_mode);
            })
        })
        .or_else(|e| {
            let _ = std::fs::remove_file(&temp_path);
            Err(e)
        })
}

fn character_to_offset_utf_8_code_points(line: RopeSlice, character: usize) -> Option<usize> {
    if character < line.len_chars() {
        Some(character)
    } else {
        None
    }
}

fn character_to_offset_utf_8_code_units(line: RopeSlice, character: usize) -> Option<usize> {
    if character < line.len_bytes() {
        Some(line.byte_to_char(character))
    } else {
        None
    }
}

pub fn apply_text_edits_to_buffer(
    uri: Option<&Url>,
    text_edits: &[TextEdit],
    text: &Rope,
    offset_encoding: &OffsetEncoding,
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
    offset_encoding: &OffsetEncoding,
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
