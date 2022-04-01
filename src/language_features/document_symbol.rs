use crate::language_features::hover::editor_hover;
use crate::markup::escape_kakoune_markup;
use crate::position::{
    get_kakoune_position_with_fallback, get_lsp_position, kakoune_position_to_lsp,
    lsp_range_to_kakoune, parse_kakoune_range,
};
use crate::types::*;
use crate::util::*;
use crate::{context::*, position::get_file_contents};
use indoc::{formatdoc, indoc};
use itertools::Itertools;
use lsp_types::request::*;
use lsp_types::*;
use serde::Deserialize;
use std::any::TypeId;
use std::convert::TryInto;
use std::path::{Path, PathBuf};
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

pub trait Symbol<T: Symbol<T>> {
    fn name(&self) -> &str;
    fn kind(&self) -> SymbolKind;
    fn uri(&self) -> Option<&Url>;
    fn range(&self) -> Range;
    fn selection_range(&self) -> Range;
    fn children(self) -> Vec<T>;
}

fn symbol_filename<'a, T: Symbol<T>>(
    meta: &'a EditorMeta,
    symbol: &'a T,
    filename_path: &'a mut PathBuf,
) -> &'a str {
    if let Some(filename) = symbol.uri() {
        *filename_path = filename.to_file_path().unwrap();
        filename_path.to_str().unwrap()
    } else {
        &meta.buffile
    }
}

impl Symbol<SymbolInformation> for SymbolInformation {
    fn name(&self) -> &str {
        &self.name
    }
    fn kind(&self) -> SymbolKind {
        self.kind
    }
    fn uri(&self) -> Option<&Url> {
        Some(&self.location.uri)
    }
    fn range(&self) -> Range {
        self.location.range
    }
    fn selection_range(&self) -> Range {
        self.range()
    }
    fn children(self) -> Vec<SymbolInformation> {
        vec![]
    }
}

impl Symbol<DocumentSymbol> for DocumentSymbol {
    fn name(&self) -> &str {
        &self.name
    }
    fn kind(&self) -> SymbolKind {
        self.kind
    }
    fn uri(&self) -> Option<&Url> {
        None
    }
    fn range(&self) -> Range {
        self.range
    }
    fn selection_range(&self) -> Range {
        self.selection_range
    }
    fn children(self) -> Vec<DocumentSymbol> {
        self.children.unwrap_or_default()
    }
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
            format_symbol(result, &meta, ctx)
        }
        Some(DocumentSymbolResponse::Nested(result)) => {
            if result.is_empty() {
                return;
            }
            format_symbol(result, &meta, ctx)
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

/// Represent list of symbols as filetype=grep buffer content.
/// Paths are converted into relative to project root.
pub fn format_symbol<T: Symbol<T>>(items: Vec<T>, meta: &EditorMeta, ctx: &Context) -> String {
    items
        .into_iter()
        .map(|symbol| {
            let mut filename_path = PathBuf::default();
            let filename = symbol_filename(meta, &symbol, &mut filename_path);
            let position =
                get_kakoune_position_with_fallback(filename, symbol.selection_range().start, ctx);
            let description = format!("{:?} {}", symbol.kind(), symbol.name());
            format!(
                "{}:{}:{}:{}\n",
                short_file_path(filename, &ctx.root_path),
                position.line,
                position.column,
                description
            ) + &format_symbol(symbol.children(), meta, ctx)
        })
        .join("")
}

fn symbol_kind_from_string(value: &str) -> Option<SymbolKind> {
    match value {
        "File" => Some(SymbolKind::FILE),
        "Module" => Some(SymbolKind::MODULE),
        "Namespace" => Some(SymbolKind::NAMESPACE),
        "Package" => Some(SymbolKind::PACKAGE),
        "Class" => Some(SymbolKind::CLASS),
        "Method" => Some(SymbolKind::METHOD),
        "Property" => Some(SymbolKind::PROPERTY),
        "Field" => Some(SymbolKind::FIELD),
        "Constructor" => Some(SymbolKind::CONSTRUCTOR),
        "Enum" => Some(SymbolKind::ENUM),
        "Interface" => Some(SymbolKind::INTERFACE),
        "Function" => Some(SymbolKind::FUNCTION),
        "Variable" => Some(SymbolKind::VARIABLE),
        "Constant" => Some(SymbolKind::CONSTANT),
        "String" => Some(SymbolKind::STRING),
        "Number" => Some(SymbolKind::NUMBER),
        "Boolean" => Some(SymbolKind::BOOLEAN),
        "Array" => Some(SymbolKind::ARRAY),
        "Object" => Some(SymbolKind::OBJECT),
        "Key" => Some(SymbolKind::KEY),
        "Null" => Some(SymbolKind::NULL),
        "EnumMember" => Some(SymbolKind::ENUM_MEMBER),
        "Struct" => Some(SymbolKind::STRUCT),
        "Event" => Some(SymbolKind::EVENT),
        "Operator" => Some(SymbolKind::OPERATOR),
        "TypeParameter" => Some(SymbolKind::TYPE_PARAMETER),
        _ => None,
    }
}

fn editor_next_or_prev_symbol(
    meta: EditorMeta,
    editor_params: EditorParams,
    result: Option<DocumentSymbolResponse>,
    ctx: &mut Context,
) {
    let params = NextOrPrevSymbolParams::deserialize(editor_params).unwrap();
    let hover = params.hover;

    let symbol_kinds_query: Vec<SymbolKind> = params
        .symbol_kinds
        .iter()
        .map(|kind_str| symbol_kind_from_string(kind_str).unwrap())
        .collect::<Vec<_>>();

    let maybe_details = match result {
        None => return,
        Some(DocumentSymbolResponse::Flat(result)) => {
            if result.is_empty() {
                return;
            }
            next_or_prev_symbol_details(result, &params, &symbol_kinds_query, &meta, ctx)
        }
        Some(DocumentSymbolResponse::Nested(result)) => {
            if result.is_empty() {
                return;
            }
            next_or_prev_symbol_details(result, &params, &symbol_kinds_query, &meta, ctx)
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

    let modal_heading = format!(
        "line {}:{}:{{+b@InfoHeader}}{:?} {}{{InfoHeader}} \
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
            HoverType::Modal {
                modal_heading,
                do_after,
            },
            PositionParams {
                position: symbol_position,
            },
            result,
            ctx,
        )
    });
}

/// Gets (filename, kakoune position, name) of the next/previous symbol in the buffer.
fn next_or_prev_symbol_details<T: Symbol<T> + 'static>(
    mut items: Vec<T>,
    params: &NextOrPrevSymbolParams,
    symbol_kinds_query: &[SymbolKind],
    meta: &EditorMeta,
    ctx: &Context,
) -> Option<(String, KakounePosition, String, SymbolKind)> {
    // Some language servers return symbol locations that are not sorted in ascending order.
    // Sort the results so we can find next and previous properly.
    items.sort_by(|a, b| a.selection_range().start.cmp(&b.selection_range().start));

    // Setup an iterator dependending on whether we are searching forwards or backwards
    let it: Box<dyn Iterator<Item = T>> = if params.search_next {
        Box::new(items.into_iter())
    } else {
        Box::new(items.into_iter().rev())
    };

    let cursor = params.position;

    for symbol in it {
        let kind = symbol.kind();
        let mut filename_path = PathBuf::default();
        let filename = symbol_filename(meta, &symbol, &mut filename_path).to_string();

        let mut symbol_position = symbol.selection_range().start;
        if TypeId::of::<T>() == TypeId::of::<SymbolInformation>() {
            symbol_position = find_identifier_in_file(
                ctx,
                &filename,
                symbol_position,
                unadorned_name(ctx, symbol.name()),
            )
            .unwrap_or(symbol_position);
        }
        let symbol_position =
            get_kakoune_position_with_fallback(&meta.buffile, symbol_position, ctx);

        let symbol_name = symbol.name().to_string();

        let want_symbol = symbol_kinds_query.is_empty() || symbol_kinds_query.contains(&kind);

        // Assume that children always have a starting position higher than (or equal to)
        // their parent's starting position.  This means that when searching for the node with
        // the next-higher position (anywhere in the tree) we need to check the parent first.
        // Conversely, when looking for the node with the next-lower position, we need to check
        // children first.
        if params.search_next && want_symbol && symbol_position > cursor {
            return Some((filename, symbol_position, symbol_name, kind));
        }

        if let Some(from_children) =
            next_or_prev_symbol_details(symbol.children(), params, symbol_kinds_query, meta, ctx)
        {
            return Some(from_children);
        }

        if !params.search_next && want_symbol && symbol_position < cursor {
            return Some((filename, symbol_position, symbol_name, kind));
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

pub fn object(meta: EditorMeta, editor_params: EditorParams, ctx: &mut Context) {
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
        move |ctx: &mut Context, meta, result| editor_object(meta, editor_params, result, ctx),
    );
}

fn editor_object(
    meta: EditorMeta,
    editor_params: EditorParams,
    result: Option<DocumentSymbolResponse>,
    ctx: &mut Context,
) {
    let params = ObjectParams::deserialize(editor_params).unwrap();

    let selections: Vec<(KakouneRange, KakounePosition)> = params
        .selections_desc
        .split_ascii_whitespace()
        .into_iter()
        .map(parse_kakoune_range)
        .collect();

    let symbol_kinds_query: Vec<SymbolKind> = params
        .symbol_kinds
        .iter()
        .map(|kind_str| symbol_kind_from_string(kind_str).unwrap())
        .collect::<Vec<_>>();

    let document = ctx.documents.get(&meta.buffile).unwrap();
    let mut ranges = match result {
        None => return,
        Some(DocumentSymbolResponse::Flat(symbols)) => {
            flat_symbol_ranges(ctx, document, symbols, symbol_kinds_query)
        }
        Some(DocumentSymbolResponse::Nested(symbols)) => {
            flat_symbol_ranges(ctx, document, symbols, symbol_kinds_query)
        }
    };

    if ranges.is_empty() {
        ctx.exec(
            meta,
            "lsp-show-error 'lsp-object: no matching symbol found'",
        );
        return;
    }

    let mode = params.mode;
    let forward = !["[", "{"].contains(&mode.as_str());
    let surround = ["<a-i>", "<a-a>"].contains(&mode.as_str());

    ranges.sort_by_key(|range| {
        let start = range.1;
        let start = (i64::from(start.line), i64::from(start.column));
        if forward && !surround {
            start
        } else {
            (-start.0, -start.1)
        }
    });

    let mut new_selections = vec![];
    for (selection, cursor) in selections {
        let mut ranges = ranges.clone();
        if surround {
            ranges = ranges
                .into_iter()
                .filter(|r| r.0.start <= cursor && cursor < r.0.end)
                .collect::<Vec<_>>();
            if ranges.is_empty() {
                continue;
            }
        }

        let mut count = params.count.max(1);
        let mut cur = cursor;
        let mut i = 0;
        let sym_range = loop {
            let (range, matched_pos) = ranges[i];
            let start = range.start;
            let end = range.end;
            let is_start = matched_pos == start;
            assert!(is_start || matched_pos == end);
            if surround {
                assert!(start <= cur && cur < end);
                count -= 1;
            } else if forward
                && cur < matched_pos
                && (cur.line < matched_pos.line || {
                    let matched_lsp_pos =
                        kakoune_position_to_lsp(&matched_pos, &document.text, ctx.offset_encoding);
                    let line = document.text.line(matched_lsp_pos.line as usize);
                    (matched_lsp_pos.character as usize) < line.len_chars()
                })
            {
                count -= 1;
                cur = end;
            } else if !forward && cur > matched_pos {
                count -= 1;
                cur = start;
            }
            if count == 0 {
                break range;
            }
            i += 1;
            if i == ranges.len() {
                if surround {
                    break range;
                }
                cur = if forward {
                    KakounePosition { line: 0, column: 0 }
                } else {
                    KakounePosition {
                        line: u32::MAX,
                        column: u32::MAX,
                    }
                };
                i = 0
            }
        };

        let sel_max = selection.start.max(selection.end);
        let sel_min = selection.start.min(selection.end);
        let sym_start = sym_range.start;
        let sym_end = sym_range.end;
        let (start, end) = match mode.as_str() {
            "<a-i>" | "<a-a>" => (sym_start, sym_end),
            "[" => (cursor.min(sym_end), sym_start),
            "]" => (cursor.max(sym_start), sym_end),
            "{" => (sel_max, sym_start),
            "}" => (sel_min, sym_end),
            _ => {
                ctx.exec(meta, "lsp-show-error 'lsp-object: invalid mode'");
                return;
            }
        };
        new_selections.push(KakouneRange { start, end })
    }
    if new_selections.is_empty() {
        ctx.exec(meta, "lsp-show-error 'lsp-object: no selections remaining'");
        return;
    }
    ctx.exec(
        meta,
        format!(
            "select {}",
            new_selections
                .into_iter()
                .map(|range| format!("{}", range))
                .join(" ")
        ),
    );
}

fn flat_symbol_ranges<T: Symbol<T>>(
    ctx: &Context,
    document: &Document,
    symbols: Vec<T>,
    symbol_kinds_query: Vec<SymbolKind>,
) -> Vec<(KakouneRange, KakounePosition)> {
    fn walk<T, F>(
        result: &mut Vec<(KakouneRange, KakounePosition)>,
        symbol_kinds_query: &[SymbolKind],
        convert: &F,
        s: T,
    ) where
        T: Symbol<T>,
        F: Fn(Range) -> KakouneRange,
    {
        let want_symbol = symbol_kinds_query.is_empty() || symbol_kinds_query.contains(&s.kind());
        if want_symbol {
            let range = convert(s.range());
            result.push((range, range.start));
            result.push((range, range.end));
        }
        for child in s.children() {
            walk(result, symbol_kinds_query, convert, child);
        }
    }
    let mut result = vec![];
    let convert = |range| lsp_range_to_kakoune(&range, &document.text, ctx.offset_encoding);
    for s in symbols {
        walk(&mut result, &symbol_kinds_query, &convert, s);
    }
    result
}
