use crate::{
    context::*,
    language_features::hover::editor_hover,
    markup::escape_kakoune_markup,
    position::{get_kakoune_position_with_fallback, get_lsp_position},
    types::*,
    util::*,
};
use lsp_types::{request::*, *};
use serde::Deserialize;
use url::Url;

/// Entry point for the `kak-lsp/next-or-previous-symbol` functionality
///
/// Essentially all this method is doing is making a DocumentSymbolRequest
/// and then passing on the results to `process_results_for_next_or_prev()`.
///
/// This function's code is derived from `language_features::text_document_document_symbol()`
/// and similar to it. At this stage we just need to issue a `DocumentSymbol` request.
pub fn document_next_or_prev_symbol_request(
    meta: EditorMeta,
    editor_params: EditorParams,
    ctx: &mut Context,
) {
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
            process_results_for_next_or_prev(meta, editor_params, result, ctx)
        },
    );
}

/// When this function is called we have got the results of DocumentSymbolRequest.
/// This function does some minimal processing and then hands the data off to an
/// appropriate function that will search for the next/previous symbol.
///
/// After that is done, we call `next_or_prev_response()` which does the job
/// of actually sending the response back to Kakoune.
///
/// This function's code is derived from `document_symbol::editor_document_symbol()`.
/// and its basic structure is similar to it.
fn process_results_for_next_or_prev(
    meta: EditorMeta,
    editor_params: EditorParams,
    result: Option<DocumentSymbolResponse>,
    ctx: &mut Context,
) {
    let params = NextOrPrevSymbolParams::deserialize(editor_params).unwrap();
    let hover = params.hover;
    let maybe_details = match result {
        Some(DocumentSymbolResponse::Flat(mut result)) => {
            if result.is_empty() {
                return;
            }
            // First let's sort the results so we can find next and previous properly
            // This step does _not_ happen in `document_symbol::editor_document_symbol()`.
            //
            // Some language servers return symbol locations in unsorted order
            // or non-ascending order.
            result.sort_by(|a, b| a.location.range.start.cmp(&b.location.range.start));
            get_next_or_prev_symbol_information_details(result, params, ctx)
        }
        Some(DocumentSymbolResponse::Nested(mut result)) => {
            if result.is_empty() {
                return;
            }
            // First let's sort the results so we can find next and previous properly
            // This step does _not_ happen in `document_symbol::editor_document_symbol()`.
            //
            // Some language servers return symbol locations in unsorted order
            // or non-ascending order.
            result.sort_by(|a, b| a.range.start.cmp(&b.range.start));
            get_next_or_prev_document_symbol_details(result, params, &meta, ctx)
        }
        None => {
            return;
        }
    };

    next_or_prev_response(meta, ctx, maybe_details, hover);
}

/// Send the response back to Kakoune. This could be either:
/// a) Instructions to move the cursor to the next/previous symbol.
/// b) Instructions to show hover information (without actually moving the
///    visible user cursor) _of_ the next/previous symbol
fn next_or_prev_response(
    meta: EditorMeta,
    ctx: &mut Context,
    maybe_details: Option<(String, KakounePosition, String, SymbolKind)>,
    hover: bool,
) {
    if let Some((filename, symbol_position, name, kind)) = maybe_details {
        if hover {
            // Create hover `req_params` just like we do in `hover::text_document_hover()`
            let req_params = HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: Url::from_file_path(&meta.buffile).unwrap(),
                    },
                    position: get_lsp_position(&meta.buffile, &symbol_position, ctx).unwrap(),
                },
                work_done_progress_params: Default::default(),
            };

            // This context is shown at the top of the modal
            let context = format!(
                                "line {}:{}:{{+b@KindAndName}}{:?} {}{{KindAndName}} (Press 'g' to goto this position. Press any other key to continue)",
                                symbol_position.line, symbol_position.column, kind, escape_kakoune_markup(&name)
                            );

            // This kak script is appended after the kakoune `lsp-show-hover`
            let do_after = format!(
                "on-key %[eval %sh[
                     if [ \"$kak_key\" = \"g\" ];
                     then echo 'info -style modal
                                exec {}g{}lh
                                ';
                     else echo 'info -style modal';
                     fi
                 ]]",
                symbol_position.line, symbol_position.column
            );

            // Make a HoverRequest to the backend language server and then call `hover::editor_hover()`
            ctx.call::<HoverRequest, _>(
                meta,
                req_params,
                move |ctx: &mut Context, meta, result| {
                    editor_hover(
                        meta,
                        Some(HoverModal { context, do_after }),
                        PositionParams {
                            position: symbol_position,
                        },
                        result,
                        ctx,
                    )
                },
            );
        } else {
            let location = format!(
                "%§{}§ {} {}",
                filename.replace("§", "§§"),
                symbol_position.line,
                symbol_position.column
            );
            let command = format!("edit! -existing -- {}", location);
            ctx.exec(meta, command);
        }
    } else {
        ctx.exec(
            meta,
            "eval %[
                info -style modal 'Not found!\n\nPress any key to continue'
                on-key %[info -style modal]
            ]",
        );
    }
}

/// Does the current symbol's location "exceed" the
/// current cursor location?
fn exceeds(
    symbol_position: KakounePosition,
    symbol_kind: SymbolKind,
    cur_position: KakounePosition,
    search_next: bool,
    expected_kind: &str,
) -> bool {
    // expected kind is the symbol for which the user is searching for
    // e.g. `Function`, `Constructor` etc.
    if !expected_kind.is_empty() && format!("{:?}", symbol_kind) != expected_kind {
        return false;
    }

    // If searching forwards, the first element that has a greater line/column
    // combination
    // If searching backwards, the first element that has a smaller line/column
    // combination
    if search_next {
        symbol_position > cur_position
    } else {
        symbol_position < cur_position
    }
}

/// Some languages modify the name of the function. This function normalizes
/// them so that they can be found in the document.
fn process_name<'a>(ctx: &Context, name: &'a str) -> &'a str {
    if ctx.language_id == "erlang" {
        // In erlang the arity of the function is added to the function name
        // e.g. `foo` function may be named something like `foo/3`
        name.split('/').next().unwrap()
    } else {
        name
    }
}

fn special_case_position(ctx: &Context, pos: Position) -> Option<Position> {
    // In Elixir we don't need to search forward.
    // The returned position is basically the selection range start.
    if ctx.language_id == "elixir" {
        Some(pos)
    } else {
        None
    }
}

/// Gets (filename, kakoune position, name) of the next/previous
/// SymbolInformation symbol in the document
fn get_next_or_prev_symbol_information_details(
    items: Vec<SymbolInformation>,
    params: NextOrPrevSymbolParams,
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

        let symbol_position = get_symbol_hover_pos(&filename, ctx, location, name);

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

fn get_symbol_hover_pos(
    filename: &String,
    ctx: &Context,
    location: &Location,
    name: &String,
) -> KakounePosition {
    // We need to handle some language servers specially
    if let Some(pos) = special_case_position(ctx, location.range.start) {
        return get_kakoune_position_with_fallback(filename, pos, ctx);
    }

    if let Some(lsp_pos) = find_name_in_file(
        filename,
        ctx,
        location.range.start,
        process_name(ctx, name),
        true,
    ) {
        get_kakoune_position_with_fallback(filename, lsp_pos, ctx)
    } else {
        get_kakoune_position_with_fallback(filename, location.range.start, ctx)
    }
}

/// Gets (filename, kakoune position, name) of the next/previous
/// DocumentSymbol symbol in the document
fn get_next_or_prev_document_symbol_details(
    items: Vec<DocumentSymbol>,
    params: NextOrPrevSymbolParams,
    meta: &EditorMeta,
    ctx: &Context,
) -> Option<(String, KakounePosition, String, SymbolKind)> {
    // Setup an iterator dependending on whether we are searching forwards or backwards
    let it: Box<dyn Iterator<Item = &DocumentSymbol>> = if params.search_next {
        Box::new(items.iter())
    } else {
        Box::new(items.iter().rev())
    };

    for DocumentSymbol {
        // Note the use of selection range instead of Range
        selection_range,
        kind,
        name,
        ..
    } in it
    {
        // The selection range should give us the exact extent of the symbol and not it surrounding data
        // e.g. doc comments. So we should not need to call the additional logic in `get_symbol_hover_pos()`
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
    }

    None
}
