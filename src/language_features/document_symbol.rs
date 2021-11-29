use crate::language_features::hover::editor_hover;
use crate::markup::escape_kakoune_markup;
use crate::position::{get_kakoune_position_with_fallback, get_lsp_position};
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
            let position = get_kakoune_position_with_fallback(filename, symbol.range().start, ctx);
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

    let symbol_kinds: Vec<SymbolKind> = params
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
            next_or_prev_symbol_details(result, &params, &symbol_kinds, &meta, ctx)
        }
        Some(DocumentSymbolResponse::Nested(result)) => {
            if result.is_empty() {
                return;
            }
            next_or_prev_symbol_details(result, &params, &symbol_kinds, &meta, ctx)
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
    symbol_kinds: &[SymbolKind],
    meta: &EditorMeta,
    ctx: &Context,
) -> Option<(String, KakounePosition, String, SymbolKind)> {
    // Some language servers return symbol locations that are not sorted in ascending order.
    // Sort the results so we can find next and previous properly.
    items.sort_by(|a, b| a.range().start.cmp(&b.range().start));

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

        let mut symbol_position = symbol.range().start;
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

        let symbol_name = symbol.name().to_owned();

        let want_symbol = symbol_kinds.is_empty() || symbol_kinds.contains(&kind);

        // Assume that children always have a starting position higher than (or equal to)
        // their parent's starting position.  This means that when searching for the node with
        // the next-higher position (anywhere in the tree) we need to check the parent first.
        // Conversely, when looking for the node with the next-lower position, we need to check
        // children first.
        if params.search_next && want_symbol && symbol_position > cursor {
            return Some((filename, symbol_position, symbol_name, kind));
        }

        if let Some(from_children) =
            next_or_prev_symbol_details(symbol.children(), params, symbol_kinds, meta, ctx)
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
