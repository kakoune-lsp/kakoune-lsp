//! Convert LSP Range to Kakoune's range-spec, and other position-related utilities.
//! Easy part:
//! * LSP ranges are 0-based, but Kakoune's are 1-based.
//! * LSP ranges are exclusive, but Kakoune's are inclusive.
//! This could be solved by applying a proper offset. A bit more tricky is that to include
//! line ending character LSP range uses an end position denoting the start of the next
//! line. This could be solved by keeping the current line, but setting character offset
//! to an arbitrarily large value, and Kakoune will clamp it to the end of line. The
//! hard part is that LSP uses UTF-16 code units to count character offset, but Kakoune
//! expects bytes. It requires analysis of the buffer content for proper translation.
//! The hardest part is that language servers mostly don't respect the spec, and in an
//! inconsistent way. See https://github.com/Microsoft/language-server-protocol/issues/376 and
//! https://www.reddit.com/r/vim/comments/b3yzq4/a_lsp_client_maintainers_view_of_the_lsp_protocol/
//! for a bit more details.
//! We used to violate the spec in a pragmatic way: we used code points instead of UTF-16 code
//! units. An update to the ropey library allowed us to handle UTF-16 code units, so now we can
//! adhere to the spec. This might cause breakage with servers that neither adhere to the spec
//! nor implement UTF-8 byte offsets (see https://clangd.llvm.org/extensions.html#utf-8-offsets).
//! Hopefully there are not too many of these servers left.
use crate::context::ServerSettings;
use crate::types::*;
use crate::{context::Context, util::read_document};
use lsp_types::*;
use ropey::{Rope, RopeSlice};
use std::cmp::min;

pub const EOL_OFFSET: u32 = 1_000_000;

/// Convert LSP Range to Kakoune's range-spec.
pub fn lsp_range_to_kakoune(
    range: &Range,
    text: &Rope,
    offset_encoding: OffsetEncoding,
) -> KakouneRange {
    match offset_encoding {
        OffsetEncoding::Utf8 => lsp_range_to_kakoune_utf_8(range),
        OffsetEncoding::Utf16 => lsp_range_to_kakoune_utf_16(range, text),
    }
}

pub fn lsp_position_to_kakoune(
    position: &Position,
    text: &Rope,
    offset_encoding: OffsetEncoding,
) -> KakounePosition {
    match offset_encoding {
        OffsetEncoding::Utf8 => lsp_position_to_kakoune_utf_8(position),
        OffsetEncoding::Utf16 => lsp_position_to_kakoune_utf_16(position, text),
    }
}

pub fn kakoune_range_to_lsp(
    range: &KakouneRange,
    text: &Rope,
    offset_encoding: OffsetEncoding,
) -> Range {
    Range {
        start: kakoune_position_to_lsp(&range.start, text, offset_encoding),
        end: kakoune_position_to_lsp(&range.end, text, offset_encoding),
    }
}

pub fn kakoune_position_to_lsp(
    position: &KakounePosition,
    text: &Rope,
    offset_encoding: OffsetEncoding,
) -> Position {
    match offset_encoding {
        OffsetEncoding::Utf8 => kakoune_position_to_lsp_utf_8(position),
        OffsetEncoding::Utf16 => kakoune_position_to_lsp_utf_16(position, text),
    }
}

/// Wrapper for kakoune_position_to_lsp which uses context to get buffer content and offset encoding.
pub fn get_lsp_position(
    server: &ServerSettings,
    filename: &str,
    position: &KakounePosition,
    ctx: &Context,
) -> Option<Position> {
    ctx.documents
        .get(filename)
        .map(|document| kakoune_position_to_lsp(position, &document.text, server.offset_encoding))
}

/// Wrapper for lsp_position_to_kakoune which uses context to get buffer content and offset encoding.
/// Reads the file directly if it is not present in context (is not open in editor).
pub fn get_kakoune_position(
    server: &ServerSettings,
    filename: &str,
    position: &Position,
    ctx: &Context,
) -> Option<KakounePosition> {
    get_file_contents(filename, ctx)
        .map(|text| lsp_position_to_kakoune(position, &text, server.offset_encoding))
}

/// Like get_kakoune_position but default to an approximate position if something goes wrong.
pub fn get_kakoune_position_with_fallback(
    server: &ServerSettings,
    filename_str: &str,
    position: Position,
    ctx: &Context,
) -> KakounePosition {
    get_kakoune_position(server, filename_str, &position, ctx).unwrap_or(KakounePosition {
        line: position.line + 1,
        column: position.character + 1,
    })
}

/// Wrapper for lsp_range_to_kakoune which uses context to get buffer content and offset encoding.
/// Reads the file directly if it is not present in context (is not open in editor).
pub fn get_kakoune_range(
    server: &ServerSettings,
    filename: &str,
    range: &Range,
    ctx: &Context,
) -> Option<KakouneRange> {
    get_file_contents(filename, ctx)
        .map(|text| lsp_range_to_kakoune(range, &text, server.offset_encoding))
}

/// Like get_kakoune_range but default to an approximate range if something goes wrong.
pub fn get_kakoune_range_with_fallback(
    server: &ServerSettings,
    filename: &str,
    range: &Range,
    ctx: &Context,
) -> KakouneRange {
    get_kakoune_range(server, filename, range, ctx).unwrap_or(KakouneRange {
        start: KakounePosition {
            line: range.start.line + 1,
            column: range.start.character + 1,
        },
        end: KakounePosition {
            line: range.end.line + 1,
            column: range.end.character + 1,
        },
    })
}

/// Get the contents of a file.
/// Searches ctx.documents first and falls back to reading the file directly.
pub fn get_file_contents(filename: &str, ctx: &Context) -> Option<Rope> {
    if let Some(doc) = ctx.documents.get(filename) {
        return Some(doc.text.clone());
    }

    match read_document(filename) {
        Ok(text) => Some(Rope::from_str(&text)),
        Err(err) => {
            error!("Failed to read file {}: {}", filename, err);
            None
        }
    }
}

/// Get a line from a Rope
///
/// If the line number is out-of-bounds, this will return the
/// last line. This is useful because the language server might
/// use a large value to convey "end of file".
pub fn get_line(line_number: usize, text: &Rope) -> RopeSlice {
    text.line(min(line_number, text.len_lines() - 1))
}

/// Parse a range of the form <line1>.<column1>,<line2>.<column2>.
/// The range is normalized, so the lower coordinate comes first.
/// Returns the cursor position separately.
pub fn parse_kakoune_range(range_desc: &str) -> (KakouneRange, KakounePosition) {
    let mut parts = range_desc.split(',');
    let mut convert = || {
        let coords = parts.next().unwrap();
        let mut coords = coords.split('.');
        KakounePosition {
            line: coords.next().unwrap().parse().ok().unwrap(),
            column: coords.next().unwrap().parse().ok().unwrap(),
        }
    };
    let anchor = convert();
    let cursor = convert();
    let (start, end) = if anchor < cursor {
        (anchor, cursor)
    } else {
        (cursor, anchor)
    };
    (KakouneRange { start, end }, cursor)
}

/// Returns true if there is a line that is included by both ranges.
pub fn ranges_touch_same_line(a: Range, b: Range) -> bool {
    a.start.line.max(b.start.line) <= a.end.line.min(b.end.line)
}

/// Returns true if the intersection of two ranges is a valid range (possibly empty but not negative).
pub fn ranges_overlap(a: Range, b: Range) -> bool {
    a.start.max(b.start) <= a.end.min(b.end)
}

/// Get the byte index of a character in a Rope slice
///
/// If the char number is out-of-bounds, this will return one past
/// the last character. This is useful because the language
/// server might use a large value to convey "end of file".
fn get_byte_index(char_index: usize, text: RopeSlice) -> usize {
    text.char_to_byte(text.utf16_cu_to_char(min(char_index, text.len_utf16_cu())))
}

fn lsp_range_to_kakoune_utf_16(range: &Range, text: &Rope) -> KakouneRange {
    let Range { start, end } = range;

    let start_line = get_line(start.line as _, text);
    let start_byte = get_byte_index(start.character as _, start_line) as u32;
    let end_line = get_line(end.line as _, text);
    let end_byte = get_byte_index(end.character as _, end_line) as u32;

    lsp_range_to_kakoune_utf_8(&Range {
        start: Position {
            line: start.line,
            character: start_byte,
        },
        end: Position {
            line: end.line,
            character: end_byte,
        },
    })
}

fn lsp_range_to_kakoune_utf_8(range: &Range) -> KakouneRange {
    let Range { start, end } = range;
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
    let bol_insert = insert && end.character == 0;
    let start_byte = start.character;

    // Exclusive->inclusive range.end conversion will make 0-length LSP range into the reversed
    // 2-length Kakoune range, but we want 1-length (the closest to 0 it can get in Kakoune ;-)).
    let end_byte = if insert {
        start_byte
    } else if end.character > 0 {
        // -1 because LSP ranges are exclusive, but Kakoune's are inclusive.
        end.character - 1
    } else {
        EOL_OFFSET - 1
    };

    let end_line = if bol_insert || end.character > 0 {
        end.line
    } else {
        end.line - 1
    };

    // +1 because LSP ranges are 0-based, but Kakoune's are 1-based.
    KakouneRange {
        start: KakounePosition {
            line: start.line + 1,
            column: start_byte + 1,
        },
        end: KakounePosition {
            line: end_line + 1,
            column: end_byte + 1,
        },
    }
}

fn kakoune_position_to_lsp_utf_16(position: &KakounePosition, text: &Rope) -> Position {
    // -1 because LSP & Rope ranges are 0-based, but Kakoune's are 1-based.
    let line_idx = position.line - 1;
    let col_idx = position.column - 1;
    if line_idx as usize >= text.len_lines() {
        return Position {
            line: line_idx,
            character: col_idx,
        };
    }

    let line = text.line(line_idx as _);
    if col_idx as usize >= line.len_bytes() {
        return Position {
            line: line_idx,
            character: col_idx,
        };
    }

    let character = line.char_to_utf16_cu(line.byte_to_char(col_idx as _)) as _;
    Position {
        line: line_idx,
        character,
    }
}

fn kakoune_position_to_lsp_utf_8(position: &KakounePosition) -> Position {
    // -1 because LSP ranges are 0-based, but Kakoune's are 1-based.
    Position {
        line: position.line - 1,
        character: position.column - 1,
    }
}

fn lsp_position_to_kakoune_utf_16(position: &Position, text: &Rope) -> KakounePosition {
    if position.line as usize >= text.len_lines() {
        return KakounePosition {
            line: position.line + 1,
            column: 999999999,
        };
    }

    let line = text.line(position.line as _);
    if position.character as usize >= line.len_chars() {
        return KakounePosition {
            line: position.line + 1,
            column: 999999999,
        };
    }

    let byte = line.char_to_byte(line.utf16_cu_to_char(position.character as _)) as u32;
    // +1 because LSP ranges are 0-based, but Kakoune's are 1-based.
    KakounePosition {
        line: position.line + 1,
        column: byte + 1,
    }
}

fn lsp_position_to_kakoune_utf_8(position: &Position) -> KakounePosition {
    // +1 because LSP ranges are 0-based, but Kakoune's are 1-based.
    KakounePosition {
        line: position.line + 1,
        column: position.character + 1,
    }
}

pub fn lsp_character_to_byte_offset(
    line: RopeSlice,
    character: usize,
    offset_encoding: OffsetEncoding,
) -> Option<usize> {
    match offset_encoding {
        OffsetEncoding::Utf8 => lsp_character_to_byte_offset_utf_8(line, character),
        OffsetEncoding::Utf16 => lsp_character_to_byte_offset_utf_16(line, character),
    }
}

fn lsp_character_to_byte_offset_utf_16(line: RopeSlice, character: usize) -> Option<usize> {
    if character <= line.len_chars() {
        Some(line.char_to_byte(line.utf16_cu_to_char(character)))
    } else {
        None
    }
}

fn lsp_character_to_byte_offset_utf_8(line: RopeSlice, character: usize) -> Option<usize> {
    if character <= line.len_bytes() {
        Some(character)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsp_range_to_kakoune_utf_8_bol_insert() {
        assert_eq!(
            lsp_range_to_kakoune_utf_8(&Range {
                start: Position {
                    line: 10,
                    character: 0
                },
                end: Position {
                    line: 10,
                    character: 0
                }
            }),
            KakouneRange {
                start: KakounePosition {
                    line: 11,
                    column: 1
                },
                end: KakounePosition {
                    line: 11,
                    column: 1
                }
            }
        );
    }

    #[test]
    fn lsp_range_to_kakoune_utf_8_bof_insert() {
        assert_eq!(
            lsp_range_to_kakoune_utf_8(&Range {
                start: Position {
                    line: 0,
                    character: 0
                },
                end: Position {
                    line: 0,
                    character: 0
                }
            }),
            KakouneRange {
                start: KakounePosition { line: 1, column: 1 },
                end: KakounePosition { line: 1, column: 1 }
            }
        );
    }

    #[test]
    fn lsp_range_to_kakoune_utf_8_eol() {
        assert_eq!(
            lsp_range_to_kakoune_utf_8(&Range {
                start: Position {
                    line: 10,
                    character: 0
                },
                end: Position {
                    line: 11,
                    character: 0
                }
            }),
            KakouneRange {
                start: KakounePosition {
                    line: 11,
                    column: 1
                },
                end: KakounePosition {
                    line: 11,
                    column: EOL_OFFSET
                }
            }
        );
    }
}
