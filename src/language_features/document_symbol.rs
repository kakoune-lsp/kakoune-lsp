use std::convert::TryInto;

use crate::language_features::hover::editor_hover;
use crate::markup::escape_kakoune_markup;
use crate::position::{get_kakoune_position_with_fallback, get_lsp_position};
use crate::types::*;
use crate::util::*;
use crate::{context::*, position::get_file_contents};
use indoc::{formatdoc, indoc};
use lsp_types::request::*;
use lsp_types::*;
use serde::Deserialize;
use std::path::Path;
use url::Url;

pub fn text_document_document_symbol(meta: EditorMeta, ctx: &mut Context) {
    let req_params = DocumentSymbolParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        partial_result_params: Default::default(),
        work_done_progress_params: Default::default(),
    };
    ctx.call::<DocumentSymbolRequest, _>(
        meta,
        req_params,
        move |ctx: &mut Context, meta, result| editor_document_symbol(meta, result, ctx),
    );
}

pub fn next_or_prev_symbol(meta: EditorMeta, editor_params: EditorParams, ctx: &mut Context) {
    let req_params = DocumentSymbolParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        partial_result_params: Default::default(),
        work_done_progress_params: Default::default(),
    };
    ctx.call::<DocumentSymbolRequest, _>(
        meta,
        req_params,
        move |ctx: &mut Context, meta, result| {
            editor_next_or_prev_symbol(meta, editor_params, result, ctx)
        },
    );
}

pub fn editor_document_symbol(
    meta: EditorMeta,
    result: Option<DocumentSymbolResponse>,
    ctx: &mut Context,
) {
    let content = match result {
        Some(DocumentSymbolResponse::Flat(result)) => {
            if result.is_empty() {
                return;
            }
            format_symbol_information(result, ctx)
        }
        Some(DocumentSymbolResponse::Nested(result)) => {
            if result.is_empty() {
                return;
            }
            format_document_symbol(result, &meta, ctx)
        }
        None => {
            return;
        }
    };
    let command = format!(
        "lsp-show-document-symbol {} {}",
        editor_quote(&ctx.root_path),
        editor_quote(&content),
    );
    ctx.exec(meta, command);
}

fn editor_next_or_prev_symbol(
    meta: EditorMeta,
    editor_params: EditorParams,
    result: Option<DocumentSymbolResponse>,
    ctx: &mut Context,
) {
    let params = NextOrPrevSymbolParams::deserialize(editor_params).unwrap();
    let hover = params.hover;
    let maybe_details = match result {
        None => return,
        Some(DocumentSymbolResponse::Flat(mut result)) => {
            if result.is_empty() {
                return;
            }
            // Some language servers return symbol locations that are not sorted in ascending order.
            // Sort the results so we can find next and previous properly.
            result.sort_by(|a, b| a.location.range.start.cmp(&b.location.range.start));
            next_or_prev_symbol_information_details(&result, &params, ctx)
        }
        Some(DocumentSymbolResponse::Nested(mut result)) => {
            if result.is_empty() {
                return;
            }
            next_or_prev_document_symbol_details(&mut result, &params, &meta, ctx)
        }
    };

    editor_next_or_prev_for_details(meta, ctx, maybe_details, hover);
}

/// Send the response back to Kakoune. This could be either:
/// a) Instructions to move the cursor to the next/previous symbol.
/// b) Instructions to show hover information of the next/previous symbol (without actually
/// moving the cursor just yet).
fn editor_next_or_prev_for_details(
    meta: EditorMeta,
    ctx: &mut Context,
    maybe_details: Option<(String, KakounePosition, String, SymbolKind)>,
    hover: bool,
) {
    let (filename, symbol_position, name, kind) = match maybe_details {
        Some((filename, symbol_position, name, kind)) => (filename, symbol_position, name, kind),
        None => {
            let no_symbol_found = indoc!(
                "eval %[
                     info -style modal 'Not found!\n\nPress any key to continue'
                     on-key %[info -style modal]
                 ]"
            );
            ctx.exec(meta, no_symbol_found);
            return;
        }
    };

    if !hover {
        let path = Path::new(&filename);
        let filename_abs = if path.is_absolute() {
            filename
        } else {
            Path::new(&ctx.root_path)
                .join(filename)
                .to_str()
                .unwrap()
                .to_string()
        };
        let command = format!(
            "edit -existing -- {} {} {}",
            editor_quote(&filename_abs),
            symbol_position.line,
            symbol_position.column
        );
        ctx.exec(meta, command);
        return;
    }

    let req_params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::from_file_path(&meta.buffile).unwrap(),
            },
            position: get_lsp_position(&meta.buffile, &symbol_position, ctx).unwrap(),
        },
        work_done_progress_params: Default::default(),
    };

    // This context is shown at the top of the modal.
    let context = format!(
        "line {}:{}:{{+b@KindAndName}}{:?} {}{{KindAndName}} \
            (Press 'g' to goto this position. Press any other key to continue)",
        symbol_position.line,
        symbol_position.column,
        kind,
        escape_kakoune_markup(&name)
    );

    // This script is run after showing the hover info.
    let do_after = formatdoc!(
        "on-key %[
             info -style modal
             eval %sh[
                 if [ \"$kak_key\" = \"g\" ]; then
                     echo 'exec {}g{}lh'
                 fi
             ]
         ]",
        symbol_position.line,
        symbol_position.column
    );

    ctx.call::<HoverRequest, _>(meta, req_params, move |ctx: &mut Context, meta, result| {
        editor_hover(
            meta,
            Some(HoverModal { context, do_after }),
            PositionParams {
                position: symbol_position,
            },
            result,
            ctx,
        )
    });
}

/// Does the symbol's location "exceed" the cursor location?
fn exceeds(
    symbol_position: KakounePosition,
    symbol_kind: SymbolKind,
    cursor_position: KakounePosition,
    search_next: bool,
    expected_kind: &str,
) -> bool {
    // Expected kind is the symbol for which the user is searching for, like "Function",
    // "Constructor" etc.
    if !expected_kind.is_empty() && format!("{:?}", symbol_kind) != expected_kind {
        return false;
    }

    // If searching forwards, the first element that has a greater line/column combination
    // If searching backwards, the first element that has a smaller line/column combination
    if search_next {
        symbol_position > cursor_position
    } else {
        symbol_position < cursor_position
    }
}

/// Gets (filename, kakoune position, name) of the next/previous
/// DocumentSymbol symbol in the document
fn next_or_prev_document_symbol_details(
    items: &mut Vec<DocumentSymbol>,
    params: &NextOrPrevSymbolParams,
    meta: &EditorMeta,
    ctx: &Context,
) -> Option<(String, KakounePosition, String, SymbolKind)> {
    // Some language servers return symbol locations that are not sorted in ascending order.
    // Sort the results so we can find next and previous properly.
    items.sort_by(|a, b| a.selection_range.start.cmp(&b.selection_range.start));

    // Setup an iterator dependending on whether we are searching forwards or backwards
    let it: Box<dyn Iterator<Item = &mut DocumentSymbol>> = if params.search_next {
        Box::new(items.iter_mut())
    } else {
        Box::new(items.iter_mut().rev())
    };

    for DocumentSymbol {
        selection_range,
        kind,
        name,
        ref mut children,
        ..
    } in it
    {
        // The selection range should give us the exact extent of the symbol, excluding its
        // surrounding data e.g. doc comments.
        let symbol_position =
            get_kakoune_position_with_fallback(&meta.buffile, selection_range.start, ctx);

        if exceeds(
            symbol_position,
            *kind,
            params.position,
            params.search_next,
            &params.symbol_kind,
        ) {
            let filename = short_file_path(&meta.buffile, &ctx.root_path).to_owned();
            return Some((filename, symbol_position, name.to_owned(), *kind));
        }

        let from_children = next_or_prev_document_symbol_details(
            children.as_mut().unwrap_or(&mut vec![]),
            params,
            meta,
            ctx,
        );
        if from_children.is_some() {
            return from_children;
        }
    }

    None
}

/// Find the location and name of the next/previous SymbolInformation symbol in the buffer.
fn next_or_prev_symbol_information_details(
    items: &Vec<SymbolInformation>,
    params: &NextOrPrevSymbolParams,
    ctx: &Context,
) -> Option<(String, KakounePosition, String, SymbolKind)> {
    // Setup an iterator dependending on whether we are searching forwards or backwards
    let it: Box<dyn Iterator<Item = &SymbolInformation>> = if params.search_next {
        Box::new(items.iter())
    } else {
        Box::new(items.iter().rev())
    };

    for SymbolInformation {
        location,
        kind,
        name,
        ..
    } in it
    {
        let filename_path = location.uri.to_file_path().unwrap();
        let filename = filename_path.to_str().unwrap().to_owned();

        let symbol_position = guess_symbol_position(ctx, &filename, location, name);

        if exceeds(
            symbol_position,
            *kind,
            params.position,
            params.search_next,
            &params.symbol_kind,
        ) {
            return Some((filename, symbol_position, name.to_owned(), *kind));
        }
    }

    None
}

/// Some languages modify the name of the function. This function normalizes
/// them so that they can be found in the document.
fn unadorned_name<'a>(ctx: &Context, name: &'a str) -> &'a str {
    if ctx.language_id == "erlang" {
        // In erlang the arity of the function is added to the function name
        // e.g. `foo` function may be named something like `foo/3`
        name.split('/').next().unwrap()
    } else {
        name
    }
}

fn guess_symbol_position(
    ctx: &Context,
    filename: &String,
    location: &Location,
    name: &String,
) -> KakounePosition {
    let position = find_identifier_in_file(
        ctx,
        filename,
        location.range.start,
        unadorned_name(ctx, name),
    )
    .unwrap_or(location.range.start);

    get_kakoune_position_with_fallback(filename, position, ctx)
}

/// Given a file and starting position, try to find the identifier `name`.
///
/// Try to avoid false positives by detecting some word boundaries.
fn find_identifier_in_file(
    ctx: &Context,
    filename: &str,
    start_from: Position,
    ident: &str,
) -> Option<Position> {
    if ident.is_empty() {
        return Some(start_from);
    }
    let contents = get_file_contents(filename, ctx)?;
    let line_offset: usize = contents.line_to_char(start_from.line.try_into().unwrap());
    let within_line_offset: usize = start_from.character.try_into().unwrap();
    let char_offset = line_offset + within_line_offset;
    let mut it = ident.chars();
    let mut maybe_found = None;
    for (num, c) in contents.chars_at(char_offset).enumerate() {
        match it.next() {
            Some(itc) => {
                if itc != c {
                    it = ident.chars();
                    let itc2 = it.next().unwrap();
                    if itc2 != c {
                        it = ident.chars();
                    }
                }
            }
            None => {
                // Example. Take the Python expression `[f.name for f in fields]`
                // Let us say we get -------------------------------^
                // i.e. We get char 8 starting position when searching for variable `f`.
                // Now `for` starts with `f` also so we will wrongly return 8 when we
                // should return 12.
                //
                // A simple heuristic will be that if the next character is alphanumeric then
                // we have _not_ found our symbol and we have to continue our search.
                // This heuristic may not work in all cases but is "good enough".
                if c.is_alphanumeric() || c == '_' {
                    it = ident.chars();
                    let itc = it.next().unwrap();
                    if itc != c {
                        it = ident.chars();
                    }
                } else {
                    maybe_found = Some(num - 1);
                    // We're done!
                    break;
                }
            }
        }
    }

    maybe_found.map(|found| {
        let line = contents.char_to_line(char_offset + found);
        let character = char_offset + found - contents.line_to_char(line);
        Position {
            line: line.try_into().unwrap(),
            character: character.try_into().unwrap(),
        }
    })
}
