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
use lsp_types::*;
use ropey::Rope;
use std::fmt::Display;

pub const EOL_OFFSET: u64 = 1_000_000;

pub struct KakounePosition {
    pub line: u64,
    pub byte: u64,
}
pub struct KakouneRange {
    pub start: KakounePosition,
    pub end: KakounePosition,
}

impl Display for KakounePosition {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}.{}", self.line, self.byte)
    }
}

impl Display for KakouneRange {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{},{}", self.start, self.end)
    }
}

/// Convert LSP Range to Kakoune's range-spec.
pub fn lsp_range_to_kakoune(range: &Range, text: &Rope, offset_encoding: &str) -> KakouneRange {
    match offset_encoding {
        "utf-8" => lsp_range_to_kakoune_utf_8_bytes(range),
        _ => lsp_range_to_kakoune_utf_8_scalar(range, text),
    }
}

fn lsp_range_to_kakoune_utf_8_scalar(range: &Range, text: &Rope) -> KakouneRange {
    let Range { start, end } = range;
    let start_line = text.line(start.line as _);
    let start_byte = start_line.char_to_byte(start.character as _) as u64;

    let end_byte: u64;
    // Exclusive->inclusive range.end conversion will make 0-length LSP range into the 2-length
    // Kakoune range, but we want 1-length (the closest to 0 it can get in Kakoune ;-)).
    if start.line == end.line && start.character == end.character {
        end_byte = start_byte;
    } else if end.character > 0 {
        let end_line = text.line(end.line as _);
        // -1 because LSP ranges are exclusive, but Kakoune's are inclusive.
        end_byte = (end_line.char_to_byte(end.character as _) - 1) as _;
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
            byte: start_byte + 1,
        },
        end: KakounePosition {
            line: end_line + 1,
            byte: end_byte + 1,
        },
    }
}

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
            byte: start_byte + 1,
        },
        end: KakounePosition {
            line: end_line + 1,
            byte: end_byte + 1,
        },
    }
}
