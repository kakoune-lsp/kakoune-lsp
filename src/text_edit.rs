use crate::context::*;
use crate::editor_transport::ToEditorSender;
use crate::position::*;
use crate::types::*;
use crate::util::*;
use indoc::formatdoc;
#[cfg(test)]
use indoc::indoc;
use itertools::Itertools;
use lsp_types::notification::DidOpenTextDocument;
use lsp_types::*;
use ropey::Rope;
use std::borrow::Cow;
use std::fs::{self, File};
use std::io::{BufReader, Write};
use std::os::unix::io::FromRawFd;

pub trait TextEditish<T: TextEditish<T>> {
    fn text_edit(self) -> TextEdit;
    fn as_ref(&self) -> &TextEdit;
    fn from_text_edit(te: TextEdit) -> T;
}

impl TextEditish<TextEdit> for TextEdit {
    fn text_edit(self) -> TextEdit {
        self
    }
    fn as_ref(&self) -> &TextEdit {
        self
    }
    fn from_text_edit(te: TextEdit) -> TextEdit {
        te
    }
}

impl TextEditish<AnnotatedTextEdit> for AnnotatedTextEdit {
    fn text_edit(self) -> TextEdit {
        self.text_edit
    }
    fn as_ref(&self) -> &TextEdit {
        &self.text_edit
    }
    fn from_text_edit(te: TextEdit) -> AnnotatedTextEdit {
        AnnotatedTextEdit {
            text_edit: te,
            annotation_id: ChangeAnnotationIdentifier::default(),
        }
    }
}

impl TextEditish<OneOf<TextEdit, AnnotatedTextEdit>> for OneOf<TextEdit, AnnotatedTextEdit> {
    fn text_edit(self) -> TextEdit {
        match self {
            OneOf::Left(text_edit) => text_edit,
            OneOf::Right(annotated_text_edit) => annotated_text_edit.text_edit,
        }
    }
    fn as_ref(&self) -> &TextEdit {
        match self {
            OneOf::Left(text_edit) => text_edit,
            OneOf::Right(annotated_text_edit) => &annotated_text_edit.text_edit,
        }
    }
    fn from_text_edit(te: TextEdit) -> OneOf<TextEdit, AnnotatedTextEdit> {
        OneOf::Left(te)
    }
}

/// Apply text edits to the file pointed by uri either by asking Kakoune to modify corresponding
/// buffer or by editing file directly when it's not open in editor.
pub fn apply_text_edits<T: TextEditish<T>>(
    server_id: ServerId,
    meta: EditorMeta,
    uri: Url,
    edits: Vec<T>,
    ctx: &mut Context,
) {
    let mut command = String::new();
    apply_text_edits_try_deferred(&mut command, server_id, &meta, uri, edits, ctx);
    if !command.is_empty() {
        ctx.exec(meta, command);
    }
}

/// Apply text edits to the file pointed by uri either by asking Kakoune to modify corresponding
/// buffer or by editing file directly when it's not open in editor.
pub fn apply_text_edits_try_deferred<T: TextEditish<T>>(
    command: &mut String,
    server_id: ServerId,
    meta: &EditorMeta,
    uri: Url,
    edits: Vec<T>,
    ctx: &mut Context,
) {
    let path = uri.to_file_path().ok().unwrap();
    let buffile = path.to_str().unwrap();
    if let Some(document) = ctx.documents.get(buffile) {
        // Write hidden buffers unless they were already dirty.
        let write_to_disk = buffile != meta.buffile
            && fs::read_to_string(buffile)
                .map(|disk_contents| disk_contents == document.text)
                .unwrap_or(false);
        let server = ctx.server(server_id);
        if let Some(cmd) = apply_text_edits_to_buffer(
            ctx.to_editor(),
            &meta.client,
            Some(uri),
            edits,
            &document.text,
            server.offset_encoding,
            write_to_disk,
        ) {
            if !command.is_empty() {
                command.push('\n');
            }
            command.push_str(&cmd);
        }
    } else if let Err(e) = apply_text_edits_to_file(server_id, &uri, edits, &meta.language_id, ctx)
    {
        error!(
            ctx.to_editor(),
            "Failed to apply edits to file {} ({})", &uri, e
        );
    }
}

pub fn apply_text_edits_to_file<T: TextEditish<T>>(
    server_id: ServerId,
    uri: &Url,
    text_edits: Vec<T>,
    language_id: &LanguageId,
    ctx: &mut Context,
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

    let (temp_path, mut temp_file) = {
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

    let server = ctx.server(server_id);
    let updated_text = apply_text_edits_to_rope(text, text_edits, server.offset_encoding).and_then(
        |updated_text| {
            temp_file.write_all(&updated_text)?;
            Ok(updated_text)
        },
    );
    match updated_text {
        Ok(updated_text) => {
            std::fs::rename(&temp_path, filename)?;
            unsafe {
                libc::chmod(path.as_ptr(), stat.st_mode);
            }
            let params = DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: language_id.clone(),
                    version: 1,
                    text: String::from_utf8_lossy(&updated_text).to_string(),
                },
            };
            ctx.notify::<DidOpenTextDocument>(server_id, params);
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&temp_path);
            Err(e)
        }
    }
}

fn apply_text_edits_to_rope<T: TextEditish<T>>(
    text: Rope,
    text_edits: Vec<T>,
    offset_encoding: OffsetEncoding,
) -> Result<Vec<u8>, std::io::Error> {
    let mut output: Vec<u8> = vec![];

    let text_len_lines = text.len_lines() as u64;
    let mut cursor = 0;

    for te in text_edits {
        let TextEdit {
            range: Range { start, end },
            new_text,
        } = te.as_ref();

        if start.line as u64 >= text_len_lines || end.line as u64 >= text_len_lines {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Text edit range extends past end of file.",
            ));
        }

        let start_offset = lsp_character_to_byte_offset(
            text.line(start.line as _),
            start.character as _,
            offset_encoding,
        );
        let end_offset = lsp_character_to_byte_offset(
            text.line(end.line as _),
            end.character as _,
            offset_encoding,
        );

        if start_offset.is_none() || end_offset.is_none() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Text edit range points past end of line.",
            ));
        }

        let start_byte = text.line_to_byte(start.line as _) + start_offset.unwrap();
        let end_byte = text.line_to_byte(end.line as _) + end_offset.unwrap();

        for chunk in text.byte_slice(cursor..start_byte).chunks() {
            output.extend_from_slice(chunk.as_bytes());
        }

        output.extend_from_slice(new_text.as_bytes());
        cursor = end_byte;
    }

    for chunk in text.byte_slice(cursor..).chunks() {
        output.extend_from_slice(chunk.as_bytes());
    }
    Ok(output)
}

// Adapted from std/src/sys/unix/mod.rs.
fn cvt(t: i32) -> std::io::Result<i32> {
    if t == -1 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(t)
    }
}

pub fn lsp_text_edits_to_kakoune<T: TextEditish<T>>(
    to_editor: &ToEditorSender,
    client: &Option<ClientId>,
    mut text_edits: Vec<T>,
    text: &Rope,
    offset_encoding: OffsetEncoding,
) -> Option<String> {
    // Empty text edits processed as a special case because Kakoune's `select` command
    // doesn't support empty arguments list.
    if text_edits.is_empty() {
        return None;
    }

    // If the text edit just replaces the whole buffer, compute a minimal edit sequence to
    // maintain selections better.
    if client.is_some() && text_edits.len() == 1 {
        let range = text_edits[0].as_ref().range;

        let text_begin = Position {
            line: 0,
            character: 0,
        };
        let last_line = text.line(text.len_lines() - 1);
        let last_line_len = last_line.len_chars();
        let text_end = if last_line_len == 0 && text.len_lines() >= 2 {
            Position {
                line: (text.len_lines() - 2) as _,
                character: text.line(text.len_lines() - 2).len_chars() as _,
            }
        } else {
            Position {
                line: text.len_lines() as _,
                character: 0,
            }
        };

        if range.start == text_begin && range.end >= text_end {
            let new_text = &text_edits[0].as_ref().new_text;
            let new_text = if new_text.ends_with('\n') {
                Cow::Borrowed(new_text)
            } else {
                Cow::Owned(new_text.to_string() + "\n")
            };
            text_edits = minimal_edit_sequence(text, &Rope::from_str(&new_text));
            debug!(
                to_editor,
                "Computed edit script to split up whole-buffer text edit"
            );
            for te in &text_edits {
                debug!(to_editor, "{:?}", te.as_ref());
            }
        }
    }

    // Adjoin selections detection and Kakoune side editing relies on edits being ordered left to
    // right. Language servers usually send them such, but spec doesn't say anything about the order
    // hence we ensure it by sorting. It's important to use stable sort to handle properly cases
    // like multiple inserts in the same place.
    text_edits.sort_by_key(|x| {
        let range = x.as_ref().range;
        (range.start, range.end)
    });

    let mut offset = 0;

    let mut coalesced_edits: Vec<TextEdit> = vec![];
    for edit in text_edits {
        let edit = edit.text_edit();
        let Range { start, end } = edit.range;
        let start_line = text.get_line(start.line as _);
        let start_column = start_line.and_then(|start_line| {
            lsp_character_to_byte_offset(start_line, start.character as _, offset_encoding)
        });
        let start_offset = text.line_to_byte(start.line as _) + start_column.unwrap_or(0);
        let end_line = text.get_line(end.line as _);
        let end_column = end_line.and_then(|end_line| {
            lsp_character_to_byte_offset(end_line, end.character as _, offset_encoding)
        });
        let end_offset = text.line_to_byte(end.line as _) + end_column.unwrap_or(0);
        if offset == start_offset && !coalesced_edits.is_empty() {
            let last = coalesced_edits.last_mut().unwrap();
            assert!(start == last.range.end);
            last.range.end = end;
            last.new_text += &edit.new_text;
        } else {
            coalesced_edits.push(edit)
        }
        offset = end_offset;
    }

    let edits = coalesced_edits
        .into_iter()
        .filter(|text_edit| {
            // Drop redundant text edits because Kakoune treats them differently. Here's how
            // (Note that we no longer use the actual Z/z commands), but compute equivalent
            // selections ourselves.)
            //
            // 0. Assume we have two adjacent selections "foo" "bar".
            // 1. Use "Z" to save the two selection.
            // 2. Use ",<esc>" to select "foo"
            // 3. Type "|echo foo<ret>"
            // 4. Run "z" to restore the two selections. Observe that "foo" is still selected.
            //
            // If we repeat step 3 with any other text, running "z" will show that the first
            // selection goes away because it was merged into the second one. This is what our
            // logic to compute merged selections will do later. It doesn't account for Kakoune
            // optimizing redundant text edits, so just drop them here.
            let TextEdit { range, new_text } = text_edit.as_ref();
            // TODO Also drop redundant edits that span multiple lines.
            if range.start.line != range.end.line {
                return true;
            }
            let line = text.line(range.start.line as _);
            let Some(start_byte) =
                lsp_character_to_byte_offset(line, range.start.character as _, offset_encoding)
            else {
                error!(
                    to_editor,
                    "Failed to resolve edit start position {}",
                    lsp_position_to_kakoune(&range.start, text, offset_encoding)
                );
                return false;
            };
            let Some(end_byte) =
                lsp_character_to_byte_offset(line, range.end.character as _, offset_encoding)
            else {
                error!(
                    to_editor,
                    "Failed to resolve edit end position {}",
                    lsp_position_to_kakoune(&range.end, text, offset_encoding)
                );
                return false;
            };
            let bytes = line.bytes_at(start_byte);
            let contents = bytes.take(end_byte - start_byte).collect::<Vec<u8>>();
            let redundant = new_text.as_bytes() == contents;
            !redundant
        })
        .map(|text_edit| lsp_text_edit_to_kakoune(&text_edit, text, offset_encoding))
        .collect::<Vec<_>>();

    let apply_edits = edits
        .iter()
        .rev()
        .map(
            |KakouneTextEdit {
                 range,
                 new_text,
                 command,
             }| {
                use KakouneTextEditCommand::{InsertBefore, Replace};
                let command = match command {
                    InsertBefore => 'i',
                    Replace => 'c',
                };
                formatdoc!(
                    "select {range}
                     execute-keys \"{command}{}<esc>\"",
                    editor_escape_double_quotes(&escape_keys(new_text))
                )
            },
        )
        .join("\n");

    Some(apply_edits)
}

pub fn apply_text_edits_to_buffer<T: TextEditish<T>>(
    to_editor: &ToEditorSender,
    client: &Option<ClientId>,
    uri: Option<Url>,
    text_edits: Vec<T>,
    text: &Rope,
    offset_encoding: OffsetEncoding,
    write_to_disk: bool,
) -> Option<String> {
    // TODO local scope
    let mut apply_edits = formatdoc!(
        "{}
         set-option buffer lsp_fail_if_disabled nop
         try %[
             lsp-did-change
             unset-option buffer lsp_fail_if_disabled
         ] catch %[
             unset-option buffer lsp_fail_if_disabled
             fail -- %val[error]
         ]",
        lsp_text_edits_to_kakoune(to_editor, client, text_edits, text, offset_encoding)?
    );

    if write_to_disk {
        apply_edits = formatdoc!(
            "{}
             write",
            apply_edits
        );
    }

    let maybe_buffile = uri
        .and_then(|uri| uri.to_file_path().ok())
        .and_then(|path| path.to_str().map(|buffile| buffile.to_string()));

    let client = match client {
        None => {
            return Some(
                maybe_buffile
                    .map(|buffile| {
                        format!(
                            "evaluate-commands -buffer {} -save-regs ^ {}",
                            editor_quote(&buffile),
                            editor_quote(&apply_edits)
                        )
                    })
                    .unwrap_or_else(|| {
                        format!(
                            "evaluate-commands -draft -save-regs ^ {}",
                            editor_quote(&apply_edits)
                        )
                    }),
            );
        }
        Some(client) => client,
    };

    // Go to the target file, in case it's not active.
    let apply_edits = maybe_buffile
        .map(|buffile| {
            formatdoc!(
                "edit -existing -- {}
                 {}",
                editor_quote(&buffile),
                &apply_edits
            )
        })
        .unwrap_or(apply_edits);

    Some(format!(
        "evaluate-commands -client {} -draft -save-regs ^ {}",
        client,
        &editor_quote(&apply_edits)
    ))
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum KakouneTextEditCommand {
    InsertBefore,
    Replace,
}

struct KakouneTextEdit {
    range: KakouneRange,
    new_text: String,
    command: KakouneTextEditCommand,
}

fn lsp_text_edit_to_kakoune<T: TextEditish<T>>(
    text_edit: &T,
    text: &Rope,
    offset_encoding: OffsetEncoding,
) -> KakouneTextEdit {
    let TextEdit { range, new_text } = text_edit.as_ref();
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
        new_text: new_text.to_string(),
        command,
    }
}

fn minimal_edit_sequence<T: TextEditish<T>>(old: &Rope, new: &Rope) -> Vec<T> {
    let oldv = old.lines().collect::<Vec<_>>();
    let newv = new.lines().collect::<Vec<_>>();
    struct BuildEditScript<'a, T: TextEditish<T>> {
        new: &'a Rope,
        edits: Vec<T>,
    }
    impl<T: TextEditish<T>> diffs::Diff for BuildEditScript<'_, T> {
        type Error = ();
        fn delete(&mut self, o: usize, len: usize, _n: usize) -> Result<(), ()> {
            let start = Position {
                line: o as _,
                character: 0,
            };
            let end = Position {
                line: (o + len) as _,
                character: 0,
            };
            self.edits.push(T::from_text_edit(TextEdit {
                range: Range { start, end },
                new_text: "".to_string(),
            }));
            Ok(())
        }
        fn insert(&mut self, o: usize, n: usize, new_len: usize) -> Result<(), ()> {
            let start = Position {
                line: o as _,
                character: 0,
            };
            self.edits.push(T::from_text_edit(TextEdit {
                range: Range { start, end: start },
                new_text: self.new.lines_at(n).take(new_len).join(""),
            }));
            Ok(())
        }
        fn replace(&mut self, o: usize, len: usize, n: usize, new_len: usize) -> Result<(), ()> {
            let start = Position {
                line: o as _,
                character: 0,
            };
            let end = Position {
                line: (o + len) as _,
                character: 0,
            };
            self.edits.push(T::from_text_edit(TextEdit {
                range: Range { start, end },
                new_text: self.new.lines_at(n).take(new_len).join(""),
            }));
            Ok(())
        }
    }
    let mut builder = BuildEditScript::<T> { new, edits: vec![] };
    let _result = diffs::patience::diff(&mut builder, &oldv, 0, oldv.len(), &newv, 0, newv.len());
    builder.edits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_transport::mock_to_editor;

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
    pub fn apply_text_edits_to_rope_unicode() {
        let text_edits = vec![
            edit(0, 29, 0, 37, "ToEditorSender"),
            edit(2, 36, 2, 44, "ToEditorSender"),
        ];
        let text = Rope::from_str(indoc!(
            r#"use crate::editor_transport::ToEditor;
               // §§§§
               fn completion_menu_text(to_editor: &ToEditor, x: &CompletionItem) -> String"#
        ));
        let updated_text =
            apply_text_edits_to_rope(text, text_edits, OffsetEncoding::Utf8).unwrap();
        let updated_text = String::from_utf8_lossy(&updated_text);
        assert_eq!(
            updated_text,
            indoc!(
                r#"use crate::editor_transport::ToEditorSender;
                   // §§§§
                   fn completion_menu_text(to_editor: &ToEditorSender, x: &CompletionItem) -> String"#
            )
        );
    }

    #[test]
    pub fn lsp_text_edits_to_kakoune_issue_521() {
        let text_edits = vec![
            edit(0, 4, 0, 7, "std"),
            edit(0, 7, 0, 9, ""),
            edit(0, 9, 0, 12, ""),
            edit(0, 14, 0, 21, "ffi"),
            edit(0, 21, 0, 21, "::"),
            edit(0, 21, 0, 21, "{CStr, CString}"),
        ];
        let buffer = Rope::from_str("use std::ffi::CString;");
        let result = lsp_text_edits_to_kakoune(
            &mock_to_editor(),
            &None,
            text_edits,
            &buffer,
            OffsetEncoding::Utf8,
        );
        let expected = indoc!(
            r#"select 1.5,1.12
               execute-keys "cstd<esc>"
               select 1.10,1.16
               execute-keys "cffi::{CStr, CString}<esc>""#
        )
        .to_string();
        assert_eq!(result, Some(expected));
    }

    #[test]
    pub fn lsp_text_edits_to_kakoune_insert_adjacent_to_replace() {
        let text_edits = vec![edit(0, 1, 0, 1, "inserted"), edit(0, 2, 0, 3, "replaced")];
        let buffer = Rope::from_str("0123");
        let result = lsp_text_edits_to_kakoune(
            &mock_to_editor(),
            &None,
            text_edits,
            &buffer,
            OffsetEncoding::Utf8,
        );
        let expected = indoc!(
            r#"select 1.2,1.2
               execute-keys "iinserted<esc>"
               select 1.11,1.11
               execute-keys "creplaced<esc>""#
        )
        .to_string();
        assert_eq!(result, Some(expected));
    }

    #[test]
    pub fn lsp_text_edits_to_kakoune_issue_527() {
        let text_edits = vec![
            edit(0, 4, 0, 9, "if"),
            edit(0, 10, 0, 13, "let"),
            edit(0, 13, 0, 13, " "),
            edit(0, 13, 0, 13, "Test::Foo"),
            edit(0, 13, 0, 13, " "),
            edit(0, 13, 0, 13, "="),
            edit(0, 13, 0, 13, " "),
            edit(0, 13, 0, 13, "foo"),
            edit(1, 8, 1, 12, "println"),
            edit(1, 12, 1, 14, ""),
        ];

        let buffer = Rope::from_str(
            "    match foo {
        Test::Foo => println!(\"foo\"),
        _ => {}
    }",
        );
        let result = lsp_text_edits_to_kakoune(
            &mock_to_editor(),
            &None,
            text_edits,
            &buffer,
            OffsetEncoding::Utf8,
        );
        let expected = indoc!(
            r#"select 1.5,1.9
               execute-keys "cif<esc>"
               select 1.8,1.10
               execute-keys "clet Test::Foo = foo<esc>"
               select 2.9,2.14
               execute-keys "cprintln<esc>""#
        )
        .to_string();
        assert_eq!(result, Some(expected));
    }

    #[test]
    pub fn lsp_text_edits_to_kakoune_issue_848() {
        let text_edits = vec![
            edit(1, 10, 1, 10, " "),
            edit(1, 11, 2, 0, "\n  "),
            edit(2, 10, 3, 0, "\n  "),
            edit(3, 10, 4, 0, "\n  "),
            edit(4, 10, 5, 0, "\n  "),
            edit(5, 10, 6, 0, "\n  "),
        ];
        let buffer = Rope::from_str(indoc!(
            "#include <cstdio>
             int main(){
             // comment
             // comment
             // comment
             // comment
             return 0;
             }"
        ));
        let result = lsp_text_edits_to_kakoune(
            &mock_to_editor(),
            &None,
            text_edits,
            &buffer,
            OffsetEncoding::Utf8,
        );
        let expected = indoc!(
            r#"select 2.11,2.11
               execute-keys "i <esc>"
               select 2.13,2.1000000
               execute-keys "c
                 <esc>"
               select 3.13,3.1000000
               execute-keys "c
                 <esc>"
               select 4.13,4.1000000
               execute-keys "c
                 <esc>"
               select 5.13,5.1000000
               execute-keys "c
                 <esc>"
               select 6.13,6.1000000
               execute-keys "c
                 <esc>""#
        )
        .to_string();
        assert_eq!(result, Some(expected));
    }

    #[test]
    pub fn lsp_text_edits_to_kakoune_join_lines() {
        let text_edits = vec![
            edit(1, 42, 2, 4, " "),
            edit(2, 13, 2, 13, "PROGRAM_NAME, "),
            edit(2, 38, 2, 53, "}"),
            edit(2, 54, 3, 4, " "),
            edit(3, 10, 3, 10, "EnvStack, Environment, "),
            edit(3, 18, 3, 42, "}"),
            edit(3, 43, 4, 4, " flog::NoFlog, "),
            edit(4, 25, 5, 4, " "),
            edit(6, 8, 6, 8, "CharEvent, InputEventQueue, InputEventQueuer, KeyEvent, QueryResponseEvent, TerminalQuery, "),
            edit(6, 30, 8, 5, "\n    }"),
            edit(8, 6, 9, 4, " "),
            edit(9, 10, 9, 10, "Key, "),
            edit(9, 24, 9, 30, "}"),
            edit(9, 31, 10, 4, " "),
            edit(10, 16, 11, 4, " "),
            edit(11, 25, 12, 4, " "),
            edit(12, 27, 13, 4, " "),
            edit(13, 34, 14, 4, " "),
            edit(14, 72, 15, 4, " "),
            edit(15, 32, 16, 4, " "),
            edit(16, 25, 17, 4, " "),
            edit(17, 12, 18, 4, " "),
            edit(18, 38, 19, 4, " "),
            edit(20, 8, 20, 8, "TtyHandoff, "),
            edit(20, 93, 22, 5, "\n    }"),
            edit(22, 6, 23, 4, " "),
            edit(23, 22, 24, 4, " "),
            edit(24, 14, 24, 14, "ArgType, WGetopter, WOption, "),
            edit(24, 18, 25, 1, "}\n}"),
        ];
        let buffer = Rope::from_str(indoc!(
            "use crate::{
                 builtins::shared::BUILTIN_ERR_UNKNOWN,
                 common::{shell_modes, str2wcstring, PROGRAM_NAME},
                 env::{env_init, EnvStack, Environment},
                 future_feature_flags,
                 input_common::{
                     match_key_event_to_key, CharEvent, InputEventQueue, InputEventQueuer, KeyEvent,
                     QueryResponseEvent, TerminalQuery,
                 },
                 key::{char_to_symbol, Key},
                 nix::isatty,
                 panic::panic_handler,
                 print_help::print_help,
                 proc::set_interactive_session,
                 reader::{check_exit_loop_maybe_warning, initial_query, reader_init},
                 signal::signal_set_handlers,
                 terminal::Capability,
                 threads,
                 topic_monitor::topic_monitor_init,
                 tty_handoff::{
                     get_kitty_keyboard_capability, initialize_tty_metadata, set_kitty_keyboard_capability,
                     TtyHandoff,
                 },
                 wchar::prelude::*,
                 wgetopt::{wopt, ArgType, WGetopter, WOption},
             };"
        ));
        let result = lsp_text_edits_to_kakoune(
            &mock_to_editor(),
            &None,
            text_edits,
            &buffer,
            OffsetEncoding::Utf8,
        );
        let expected = indoc!(
            r#"select 2.43,3.4
               execute-keys "c <esc>"
               select 2.53,2.53
               execute-keys "iPROGRAM_NAME, <esc>"
               select 2.92,2.106
               execute-keys "c}<esc>"
               select 2.94,3.4
               execute-keys "c <esc>"
               select 2.101,2.101
               execute-keys "iEnvStack, Environment, <esc>"
               select 2.132,2.155
               execute-keys "c}<esc>"
               select 2.134,3.4
               execute-keys "c flog::NoFlog, <esc>"
               select 2.170,3.4
               execute-keys "c <esc>"
               select 3.9,3.9
               execute-keys "iCharEvent, InputEventQueue, InputEventQueuer, KeyEvent, QueryResponseEvent, TerminalQuery, <esc>"
               select 3.122,5.5
               execute-keys "c
                   }<esc>"
               select 4.7,5.4
               execute-keys "c <esc>"
               select 4.14,4.14
               execute-keys "iKey, <esc>"
               select 4.33,4.38
               execute-keys "c}<esc>"
               select 4.35,5.4
               execute-keys "c <esc>"
               select 4.48,5.4
               execute-keys "c <esc>"
               select 4.70,5.4
               execute-keys "c <esc>"
               select 4.94,5.4
               execute-keys "c <esc>"
               select 4.125,5.4
               execute-keys "c <esc>"
               select 4.194,5.4
               execute-keys "c <esc>"
               select 4.223,5.4
               execute-keys "c <esc>"
               select 4.245,5.4
               execute-keys "c <esc>"
               select 4.254,5.4
               execute-keys "c <esc>"
               select 4.289,5.4
               execute-keys "c <esc>"
               select 5.9,5.9
               execute-keys "iTtyHandoff, <esc>"
               select 5.106,7.5
               execute-keys "c
                   }<esc>"
               select 6.7,7.4
               execute-keys "c <esc>"
               select 6.26,7.4
               execute-keys "c <esc>"
               select 6.37,6.37
               execute-keys "iArgType, WGetopter, WOption, <esc>"
               select 6.70,7.1
               execute-keys "c}
               }<esc>""#
        )
        .to_string();
        assert_eq!(result, Some(expected));
    }

    #[test]
    pub fn lsp_text_edits_to_kakoune_remove_imports() {
        let text_edits = vec![
            edit(17, 26, 17, 53, "PROGRAM_NAME}"),
            edit(18, 4, 18, 7, "input_common"),
            edit(18, 10, 18, 18, "\n        match_key_event_to_key"),
            edit(18, 20, 18, 28, "CharEvent"),
            edit(18, 30, 18, 41, "ImplicitEvent, InputEventQueue, InputEventQueuer,\n        KeyEvent,\n    "),
            edit(19, 4, 19, 24, "key::{char_to_symbol, Key}"),
            edit(20, 4, 23, 5, "nix::isatty"),
            edit(24, 4, 24, 30, "print_help::print_help"),
            edit(25, 4, 25, 15, "reader::{\n        check_exit_loop_maybe_warning, reader_sighup, set_shell_modes,\n    }"),
            edit(26, 4, 26, 9, "tty_handoff"),
            edit(26, 11, 26, 24, "TtyHandoff"),
            edit(27, 4, 27, 14, "wchar"),
            edit(27, 16, 27, 26, "prelude::*"),
            edit(28, 4, 28, 33, "wgetopt::{wopt, ArgType, WGetopter, WOption}"),
            edit(28, 34, 37, 1, "\n}"),
        ];
        let buffer = Rope::from_str(indoc!(
            r#"//! A small utility to print information related to pressing keys. This is similar to using tools
               //! like `xxd` and `od -tx1z` but provides more information such as the time delay between each
               //! character. It also allows pressing and interpreting keys that are normally special such as
               //! [ctrl-c] (interrupt the program) or [ctrl-d] (EOF to signal the program should exit).
               //! And unlike those other tools this one disables ICRNL mode so it can distinguish between
               //! carriage-return (\cM) and newline (\cJ).
               //!
               //! Type "exit" or "quit" to terminate the program.

               use std::{ops::ControlFlow, os::unix::prelude::OsStrExt};

               use libc::{STDIN_FILENO, VEOF, VINTR};

               #[allow(unused_imports)]
               use crate::future::IsSomeAnd;
               use crate::{
                   builtins::shared::BUILTIN_ERR_UNKNOWN,
                   common::{shell_modes, str2wcstring, PROGRAM_NAME},
                   env::{env_init, EnvStack, Environment},
                   future_feature_flags,
                   input_common::{
                       match_key_event_to_key, CharEvent, ImplicitEvent, InputEventQueue, InputEventQueuer,
                       KeyEvent, QueryResultEvent,
                   },
                   key::{char_to_symbol, Key},
                   nix::isatty,
                   panic::panic_handler,
                   print_help::print_help,
                   proc::set_interactive_session,
                   reader::{
                       check_exit_loop_maybe_warning, reader_init, reader_sighup, set_shell_modes, terminal_init,
                   },
                   threads,
                   topic_monitor::topic_monitor_init,
                   tty_handoff::TtyHandoff,
                   wchar::prelude::*,
                   wgetopt::{wopt, ArgType, WGetopter, WOption},
               };

               use super::prelude::*;

               /// Return true if the recent sequence of characters indicates the user wants to exit the program.
               fn should_exit(
                   streams: &mut IoStreams,
                   recent_keys: &mut Vec<KeyEvent>,
                   key_evt: KeyEvent,
               ) -> bool {
                   false
               }"#
        ));
        let result = lsp_text_edits_to_kakoune(
            &mock_to_editor(),
            &None,
            text_edits,
            &buffer,
            OffsetEncoding::Utf8,
        );
        let expected = indoc!(
            r#"select 18.27,18.53
               execute-keys "cPROGRAM_NAME}<esc>"
               select 19.5,19.7
               execute-keys "cinput_common<esc>"
               select 19.20,19.27
               execute-keys "c
                       match_key_event_to_key<esc>"
               select 20.43,20.50
               execute-keys "cCharEvent<esc>"
               select 20.54,20.64
               execute-keys "cImplicitEvent, InputEventQueue, InputEventQueuer,
                       KeyEvent,
                   <esc>"
               select 23.5,23.24
               execute-keys "ckey::{char_to_symbol, Key}<esc>"
               select 24.5,27.5
               execute-keys "cnix::isatty<esc>"
               select 25.5,25.30
               execute-keys "cprint_help::print_help<esc>"
               select 26.5,26.15
               execute-keys "creader::{
                       check_exit_loop_maybe_warning, reader_sighup, set_shell_modes,
                   }<esc>"
               select 29.5,29.9
               execute-keys "ctty_handoff<esc>"
               select 29.18,29.30
               execute-keys "cTtyHandoff<esc>"
               select 30.5,30.14
               execute-keys "cwchar<esc>"
               select 30.12,30.21
               execute-keys "cprelude::*<esc>"
               select 31.5,31.33
               execute-keys "cwgetopt::{wopt, ArgType, WGetopter, WOption}<esc>"
               select 31.50,40.1
               execute-keys "c
               }<esc>""#
        )
        .to_string();
        assert_eq!(result, Some(expected));
    }

    #[test]
    pub fn lsp_text_edits_to_kakoune_gopls_code_actions() {
        let text_edits = vec![
            edit(4, 0, 4, 1, ""),
            edit(4, 10, 4, 11, ""),
            edit(4, 13, 4, 13, " "),
        ];
        let buffer = Rope::from_str(indoc!(
            r#"package main

               import "os"

               	func main (){}"#
        ));
        let result = lsp_text_edits_to_kakoune(
            &mock_to_editor(),
            &None,
            text_edits,
            &buffer,
            OffsetEncoding::Utf8,
        );
        let expected = indoc!(
            r#"select 5.1,5.1
               execute-keys "c<esc>"
               select 5.10,5.10
               execute-keys "c<esc>"
               select 5.12,5.12
               execute-keys "i <esc>""#
        )
        .to_string();
        assert_eq!(result, Some(expected));
    }

    #[test]
    pub fn lsp_text_edits_to_kakoune_merge_imports() {
        let text_edits = vec![
                edit(0, 4, 0, 7, "std"),
                edit(0, 7, 0, 9, ""),
                edit(0, 9, 0, 13, ""),
                edit(0, 13, 0, 15, ""),
                edit(0, 15, 0, 19, ""),
                edit(0, 19, 0, 19, "::"),
                edit(0, 19, 0, 19, "{path::Path, process::Stdio}"),
                edit(1, 0, 1, 24, "\n"),
                edit(1, 24, 1, 24, "fn main() {\n    let matches = App::new(\"kak-lsp\").get_matches();\n\n    if matches.is_present(\"kakoune\") {}\n}"),
                edit(3, 3, 3, 7, "kakoune"),
                edit(4, 8, 4, 15, "script"),
                edit(4, 15, 4, 15, ":"),
                edit(4, 16, 4, 16, "&str"),
                edit(4, 16, 4, 16, " "),
                edit(4, 18, 4, 21, "include_str"),
                edit(4, 21, 4, 23, ""),
                edit(4, 23, 4, 26, ""),
                edit(4, 26, 4, 37, ""),
                edit(4, 37, 4, 38, "!"),
                edit(4, 38, 4, 49, "("),
                edit(4, 49, 4, 49, "\"../rc/lsp.kak\""),
                edit(4, 49, 4, 49, ")"),
                edit(4, 49, 4, 51, ""),
                edit(4, 52, 6, 4, "\n    "),
                edit(6, 4, 6, 6, "let"),
                edit(6, 7, 6, 14, "args"),
                edit(6, 14, 6, 15, ""),
                edit(6, 15, 6, 25, ""),
                edit(6, 25, 6, 36, ""),
                edit(6, 37, 6, 39, "="),
                edit(6, 39, 6, 39, " "),
                edit(6, 39, 6, 39, "env::args().skip(1)"),
                edit(6, 39, 6, 39, ";"),
                edit(7, 1, 9, 0, "\n"),
                edit(9, 0, 12, 1, ""),
                edit(12, 1, 13, 0, ""),
        ];
        let buffer = Rope::from_str(indoc!(
            r#"use std::path::Path;
               use std::process::Stdio;

               fn main() {
                   let matches = App::new("kak-lsp").get_matches();

                   if matches.is_present("kakoune") {}
               }

               fn kakoune() {
                   let script: &str = include_str!("../rc/lsp.kak");
                   let args = env::args().skip(1);
               }
               "#
        ));
        let result = lsp_text_edits_to_kakoune(
            &mock_to_editor(),
            &None,
            text_edits,
            &buffer,
            OffsetEncoding::Utf8,
        );

        let expected = indoc!(
            r#"select 1.5,1.19
               execute-keys "cstd::{path::Path, process::Stdio}<esc>"
               select 2.1,2.24
               execute-keys "c
               fn main() {
                   let matches = App::new(""kak-lsp"").get_matches();

                   if matches.is_present(""kakoune"") {}
               }<esc>"
               select 9.4,9.7
               execute-keys "ckakoune<esc>"
               select 10.9,10.15
               execute-keys "cscript:<esc>"
               select 10.17,10.17
               execute-keys "i&str <esc>"
               select 10.24,10.56
               execute-keys "cinclude_str!(""../rc/lsp.kak"")<esc>"
               select 10.54,12.6
               execute-keys "c
                   let<esc>"
               select 11.8,11.36
               execute-keys "cargs<esc>"
               select 11.13,11.14
               execute-keys "c= env::args().skip(1);<esc>"
               select 12.2,17.1000000
               execute-keys "c
               <esc>""#
        )
        .to_string();
        assert_eq!(result, Some(expected));
    }

    #[test]
    pub fn lsp_text_edits_to_kakoune_rewrite_whole_buffer_missing_eol() {
        let buffer = Rope::from_str(indoc!(
            r#"<head/>
                   <body>
                   asdf
               </body>
             "#
        ));
        let text_edits = vec![edit(
            0,
            0,
            4,
            0,
            indoc!(
                r#"<head/>

                   <body>
                           asdf
                   </body>
                 "#
            )
            .trim_end(),
        )];
        let result = lsp_text_edits_to_kakoune(
            &mock_to_editor(),
            &Some(ClientId("test_client".to_string())),
            text_edits,
            &buffer,
            OffsetEncoding::Utf8,
        );
        let expected = indoc!(
            r#"select 2.1,3.1000000
               execute-keys "c
               <lt>body>
                       asdf
               <esc>""#
        )
        .to_string();
        assert_eq!(result, Some(expected));
    }

    #[test]
    pub fn lsp_text_edits_to_kakoune_rewrite_whole_buffer_text_edit_missing_eol() {
        let buffer = Rope::from_str(indoc!(
            r#"<body>
                               asdf

                       asdf
                       asdf
                       asdf
                       asdf
                       sadf
               </body>
             "#
        ));
        let text_edits = vec![edit(
            0,
            0,
            9,
            0,
            indoc!(
                r#"<body>
                           asdf

                           asdf
                           asdf
                           asdf
                           asdf
                           sadf
                   </body>"#
            )
            .trim_end(),
        )];
        let result = lsp_text_edits_to_kakoune(
            &mock_to_editor(),
            &Some(ClientId("test_client".to_string())),
            text_edits,
            &buffer,
            OffsetEncoding::Utf8,
        );
        /* This used to produce
              select 2.1,2.1000000 9.1,10.1000000
              execute-keys -save-regs "" Z
              execute-keys "z<space><esc>,<esc>c        asdf
              <esc>z1)<space><esc>,<esc>c<lt>/body><esc>"
        */
        let expected = indoc!(
            r#"select 2.1,2.1000000
               execute-keys "c        asdf
               <esc>""#
        )
        .to_string();
        assert_eq!(result, Some(expected));
    }
}
