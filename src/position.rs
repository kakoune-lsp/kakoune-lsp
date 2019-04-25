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
//! The hardest part is that language servers mostly don't respect the spec, and in a
//! inconsistent way. See https://github.com/Microsoft/language-server-protocol/issues/376 and
//! https://www.reddit.com/r/vim/comments/b3yzq4/a_lsp_client_maintainers_view_of_the_lsp_protocol/
//! for a bit more details.
//! Temporarily resolution for this problem in kak-lsp is as follows: treat LSP character offset as
//! Unicode scalar value in UTF-8 encoding (and then convert it into byte offset for Kakoune) by
//! default, and treat offset as byte one if specified in the config. It's a horrible violation of
//! both spec and the most obvious spec alternative (UTF-8 code units aka just bytes), but it seems
//! like a viable pragmatic solution before we start to dig deep into the proper support.
//! Pros of this solution for UTF-8 encoded text (and kak-lsp doesn't support other encodings yet):
//! * It's relatively easy to implement in a performant way (thanks to ropey).
//! * It works for entire Basic Multilingual Plane when language server adheres to spec.
//! * It just works when language server sends offset in UTF-8 scalar values (i.e. RLS).
//! * It works for at least Basic Latin when language server sends offset in UTF-8 bytes
//!   (i.e. pyls, clangd with offsetEncoding: utf-8).
//!   And just works when `offset_encoding: utf-8` is provided in the config.
use crate::types::*;
use lsp_types::*;
use ropey::Rope;

pub const EOL_OFFSET: u64 = 1_000_000;

/// Convert LSP Range to Kakoune's range-spec.
pub fn lsp_range_to_kakoune(
    range: &Range,
    text: &Rope,
    offset_encoding: &OffsetEncoding,
) -> KakouneRange {
    match offset_encoding {
        OffsetEncoding::Utf8 => lsp_range_to_kakoune_utf_8_bytes(range),
        // Not a proper UTF-16 code units handling, but works within BMP
        OffsetEncoding::Utf16 => lsp_range_to_kakoune_utf_8_scalar(range, text),
    }
}

pub fn lsp_position_to_kakoune(
    position: &Position,
    text: &Rope,
    offset_encoding: &OffsetEncoding,
) -> KakounePosition {
    match offset_encoding {
        OffsetEncoding::Utf8 => lsp_position_to_kakoune_utf_8_bytes(position),
        // Not a proper UTF-16 code units handling, but works within BMP
        OffsetEncoding::Utf16 => lsp_position_to_kakoune_utf_8_scalar(position, text),
    }
}

pub fn kakoune_position_to_lsp(
    position: &KakounePosition,
    text: &Rope,
    offset_encoding: &OffsetEncoding,
) -> Position {
    match offset_encoding {
        OffsetEncoding::Utf8 => kakoune_position_to_lsp_utf_8_bytes(position),
        // Not a proper UTF-16 code units handling, but works within BMP
        OffsetEncoding::Utf16 => kakoune_position_to_lsp_utf_8_scalar(position, text),
    }
}

// Position.character in UTF-8 code points.
fn lsp_range_to_kakoune_utf_8_scalar(range: &Range, text: &Rope) -> KakouneRange {
    let Range { start, end } = range;
    let start_line = text.line(start.line as _);
    let start_byte = start_line.char_to_byte(start.character as _) as u64;
    let end_line = text.line(end.line as _);
    let end_byte = end_line.char_to_byte(end.character as _) as u64;

    lsp_range_to_kakoune_utf_8_bytes(&Range {
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

// Position.character in UTF-8 code units.
fn lsp_range_to_kakoune_utf_8_bytes(range: &Range) -> KakouneRange {
    let Range { start, end } = range;
    let start_byte = start.character;

    let end_byte;
    // Exclusive->inclusive range.end conversion will make 0-length LSP range into the 2-length
    // Kakoune range, but we want 1-length (the closest to 0 it can get in Kakoune ;-)).
    if start.line == end.line && start.character == end.character {
        end_byte = start_byte;
    } else if end.character > 0 {
        // -1 because LSP ranges are exclusive, but Kakoune's are inclusive.
        end_byte = end.character - 1;
    } else {
        end_byte = EOL_OFFSET;
    }

    let end_line = if end.character > 0 {
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

// Position.character in UTF-8 code points.
fn kakoune_position_to_lsp_utf_8_scalar(position: &KakounePosition, text: &Rope) -> Position {
    // -1 because LSP & Rope ranges are 0-based, but Kakoune's are 1-based.
    let line = position.line - 1;
    let character = text
        .line(line as _)
        .byte_to_char((position.column - 1) as _) as _;
    Position { line, character }
}

// Position.character in UTF-8 code units.
fn kakoune_position_to_lsp_utf_8_bytes(position: &KakounePosition) -> Position {
    // -1 because LSP ranges are 0-based, but Kakoune's are 1-based.
    Position {
        line: position.line - 1,
        character: position.column - 1,
    }
}

// Position.character in UTF-8 code points.
fn lsp_position_to_kakoune_utf_8_scalar(position: &Position, text: &Rope) -> KakounePosition {
    let byte: u64 = text
        .line(position.line as _)
        .char_to_byte(position.character as _) as _;
    // +1 because LSP ranges are 0-based, but Kakoune's are 1-based.
    KakounePosition {
        line: position.line + 1,
        column: byte + 1,
    }
}

// Position.character in UTF-8 code units.
fn lsp_position_to_kakoune_utf_8_bytes(position: &Position) -> KakounePosition {
    // +1 because LSP ranges are 0-based, but Kakoune's are 1-based.
    KakounePosition {
        line: position.line + 1,
        column: position.character + 1,
    }
}
