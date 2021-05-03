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
    if character < line.len_bytes() {
        Some(line.byte_to_char(character))
    } else {
        None
    }
}

pub fn apply_text_edits_to_buffer(
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

    let select_edits = edits
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
            let end = &pair[0].range.end;
            let start = &pair[1].range.start;
            // Replacing adjoin selection with empty content effectively removes it.
            let remove_adjoin = pair[0].new_text.is_empty()
                && (end.line == start.line && end.column + 1 == start.column)
                || (end.line + 1 == start.line && end.column == EOL_OFFSET && start.column == 1);
            // Inserting in the same place doesn't produce extra selection.
            let insert_the_same = end.line == start.line && end.column == start.column;
            if remove_adjoin || insert_the_same {
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
                if !merged_selections.contains(&i) {
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
    uri.and_then(|uri| uri.to_file_path().ok())
        .and_then(|path| {
            path.to_str().map(|buffile| {
                format!(
                    "eval -buffer {} {}",
                    editor_quote(buffile),
                    editor_quote(&command)
                )
            })
        })
        .or(Some(command))
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

    let range = lsp_range_to_kakoune(&range, text, offset_encoding);

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
