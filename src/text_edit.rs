use crate::position::*;
use crate::types::*;
use crate::util::*;
use itertools::Itertools;
use lsp_types::*;
use ropey::{Rope, RopeSlice};
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::os::unix::io::FromRawFd;

pub fn apply_text_edits_to_file(
    uri: &Url,
    text_edits: &[OneOf<TextEdit, AnnotatedTextEdit>],
    offset_encoding: OffsetEncoding,
) -> std::io::Result<()> {
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

    let (temp_path, temp_file) = {
        let template = format!("{}.XXXXXX", filename);
        let cstr = std::ffi::CString::new(template).unwrap();
        let ptr = cstr.into_raw();
        let temp_fd = unsafe { libc::mkstemp(ptr) };
        let cstr = unsafe { std::ffi::CString::from_raw(ptr) };
        let temp_fd = cvt(temp_fd)?;
        let temp_path = cstr.into_string().unwrap();
        let temp_file = unsafe { File::from_raw_fd(temp_fd) };
        (temp_path, temp_file)
    };
    fn apply_text_edits_to_file_impl(
        text: Rope,
        temp_file: File,
        text_edits: &[OneOf<TextEdit, AnnotatedTextEdit>],
        offset_encoding: OffsetEncoding,
    ) -> Result<(), std::io::Error> {
        let mut output = BufWriter::new(temp_file);

        let character_to_offset = match offset_encoding {
            OffsetEncoding::Utf8 => character_to_offset_utf_8_code_units,
            // Not a proper UTF-16 code units handling, but works within BMP
            OffsetEncoding::Utf16 => character_to_offset_utf_8_code_points,
        };

        let text_len_lines = text.len_lines() as u64;
        let mut cursor = 0;

        for te in text_edits {
            let TextEdit {
                range: Range { start, end },
                new_text,
            } = match te {
                OneOf::Left(edit) => edit,
                OneOf::Right(annotated_edit) => &annotated_edit.text_edit,
            };
            if start.line as u64 >= text_len_lines || end.line as u64 >= text_len_lines {
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
        .map(|_| unsafe {
            libc::chmod(path.as_ptr(), stat.st_mode);
        })
        .map_err(|e| {
            let _ = std::fs::remove_file(&temp_path);
            e
        })
}

// Adapted from std/src/sys/unix/mod.rs.
fn cvt(t: i32) -> std::io::Result<i32> {
    if t == -1 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(t)
    }
}

fn character_to_offset_utf_8_code_points(line: RopeSlice, character: usize) -> Option<usize> {
    if character < line.len_chars() {
        Some(character)
    } else {
        None
    }
}

fn character_to_offset_utf_8_code_units(line: RopeSlice, character: usize) -> Option<usize> {
    if character <= line.len_bytes() {
        Some(line.byte_to_char(character))
    } else {
        None
    }
}

pub fn apply_text_edits_to_buffer(
    client: &Option<String>,
    uri: Option<&Url>,
    text_edits: &[OneOf<TextEdit, AnnotatedTextEdit>],
    text: &Rope,
    offset_encoding: OffsetEncoding,
) -> Option<String> {
    // Empty text edits processed as a special case because Kakoune's `select` command
    // doesn't support empty arguments list.
    if text_edits.is_empty() {
        return None;
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

    let selection_descs = edits
        .iter()
        .map(|edit| format!("{}", edit.range))
        .dedup()
        .join(" ");

    // Merged selections require one less selection cycle after the next restore
    // to get to the next selection.
    let merged_selections = edits
        .windows(2)
        .enumerate()
        .filter_map(|(i, pair)| {
            let first_end = &pair[0].range.end;
            let second_start = &pair[1].range.start;
            let second_end = &pair[1].range.end;
            // Replacing adjacent selection with empty content effectively removes it.
            let remove_adjacent = pair[0].new_text.is_empty()
                && (first_end.line == second_start.line
                    && first_end.column + 1 == second_start.column)
                || (first_end.line + 1 == second_start.line
                    && first_end.column == EOL_OFFSET
                    && second_start.column == 1);
            let second_is_insert =
                second_start.line == second_end.line && second_start.column == second_end.column;
            // Inserting in the same place doesn't produce extra selection.
            let insert_the_same = first_end.line == second_start.line
                && (first_end.column == second_start.column
                    || second_is_insert && first_end.column + 1 == second_start.column);
            if remove_adjacent || insert_the_same {
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
                    KakouneTextEditCommand::Replace => "lsp-replace-selection",
                };
                let command = format!(
                    "exec \"z{}<space>\"\n{} {}",
                    if selection_index > 0 {
                        format!("{})", selection_index)
                    } else {
                        String::new()
                    },
                    command,
                    editor_quote(new_text)
                );
                if !merged_selections.contains(&i) {
                    selection_index += 1;
                }
                command
            },
        )
        .join("\n");

    let apply_edits = format!(
        "select {}\nexec -save-regs \"\" Z\n{}",
        selection_descs, apply_edits
    );
    let apply_edits = uri
        .and_then(|uri| uri.to_file_path().ok())
        .and_then(|path| {
            path.to_str().map(|buffile| {
                format!(
                    "eval -buffer {} -save-regs ^ {}",
                    editor_quote(buffile),
                    editor_quote(&apply_edits)
                )
            })
        })
        .unwrap_or_else(|| format!("eval -draft -save-regs ^ {}", editor_quote(&apply_edits)));

    let client = match client {
        None => return Some(apply_edits),
        Some(client) => client,
    };

    let apply_edits_and_restore_selections = format!(
        "{}\n{}\n{}",
        "set-register s %val{selections_desc}", &apply_edits, "select %reg{s}",
    );

    Some(format!(
        "eval -client {} -save-regs s {}",
        client,
        &editor_quote(&apply_edits_and_restore_selections)
    ))
}

enum KakouneTextEditCommand {
    InsertBefore,
    Replace,
}

struct KakouneTextEdit {
    range: KakouneRange,
    new_text: String,
    command: KakouneTextEditCommand,
}

fn lsp_text_edit_to_kakoune(
    text_edit: &OneOf<TextEdit, AnnotatedTextEdit>,
    text: &Rope,
    offset_encoding: OffsetEncoding,
) -> KakouneTextEdit {
    let TextEdit { range, new_text } = match text_edit {
        OneOf::Left(edit) => edit,
        OneOf::Right(annotated_edit) => &annotated_edit.text_edit,
    };
    let Range { start, end } = range;
    let insert = start.line == end.line && start.character == end.character;

    let range = lsp_range_to_kakoune(range, text, offset_encoding);

    let command = if insert {
        KakouneTextEditCommand::InsertBefore
    } else {
        KakouneTextEditCommand::Replace
    };

    KakouneTextEdit {
        range,
        new_text: new_text.to_owned(),
        command,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(
        start_line: u32,
        start_character: u32,
        end_line: u32,
        end_character: u32,
        new_text: &str,
    ) -> OneOf<TextEdit, AnnotatedTextEdit> {
        OneOf::Left(TextEdit {
            range: Range {
                start: Position {
                    line: start_line,
                    character: start_character,
                },
                end: Position {
                    line: end_line,
                    character: end_character,
                },
            },
            new_text: new_text.to_string(),
        })
    }

    #[test]
    pub fn apply_text_edits_to_buffer_issue_521() {
        let text_edits = vec![
            edit(0, 4, 0, 7, "std"),
            edit(0, 7, 0, 9, ""),
            edit(0, 9, 0, 12, ""),
            edit(0, 14, 0, 21, "ffi"),
            edit(0, 21, 0, 21, "::"),
            edit(0, 21, 0, 21, "{CStr, CString}"),
        ];
        let buffer = Rope::from_str("use std::ffi::CString;");
        let result =
            apply_text_edits_to_buffer(&None, None, &text_edits, &buffer, OffsetEncoding::Utf8);
        let expected =
            r#"eval -draft -save-regs ^ 'select 1.5,1.7 1.8,1.9 1.10,1.12 1.15,1.21 1.22,1.22
exec -save-regs "" Z
exec "z<space>"
lsp-replace-selection ''std''
exec "z1)<space>"
lsp-replace-selection ''''
exec "z1)<space>"
lsp-replace-selection ''''
exec "z2)<space>"
lsp-replace-selection ''ffi''
exec "z2)<space>"
lsp-insert-before-selection ''::''
exec "z2)<space>"
lsp-insert-before-selection ''{CStr, CString}'''"#
                .to_string();
        assert_eq!(result, Some(expected));
    }
}
