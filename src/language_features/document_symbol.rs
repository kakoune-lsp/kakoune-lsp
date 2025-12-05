use crate::capabilities::{attempt_server_capability, CAPABILITY_DOCUMENT_SYMBOL};
use crate::language_features::goto::edit_at_range;
use crate::language_features::hover::editor_hover;
use crate::markup::escape_kakoune_markup;
use crate::position::{
    get_kakoune_position_with_fallback, get_kakoune_range, get_kakoune_range_with_fallback,
    get_lsp_position, kakoune_position_to_lsp, lsp_range_to_kakoune, parse_kakoune_range,
};
use crate::types::*;
use crate::util::*;
use crate::{context::*, position::get_file_contents};
use indoc::{formatdoc, indoc};
use itertools::Itertools;
use lsp_types::request::*;
use lsp_types::*;
use std::any::TypeId;
use std::collections::HashMap;
use std::convert::TryInto;
use std::fmt;
use std::fmt::Write;
use std::path::{Path, PathBuf};
use url::Url;

pub fn text_document_document_symbol(meta: EditorMeta, params: PositionParams, ctx: &mut Context) {
    let eligible_servers: Vec<_> = ctx
        .servers(&meta)
        .filter(|srv| attempt_server_capability(ctx, *srv, &meta, CAPABILITY_DOCUMENT_SYMBOL))
        .collect();
    let req_params = eligible_servers
        .into_iter()
        .map(|(server_id, _)| {
            (
                server_id,
                vec![DocumentSymbolParams {
                    text_document: TextDocumentIdentifier {
                        uri: Url::from_file_path(&meta.buffile).unwrap(),
                    },
                    partial_result_params: Default::default(),
                    work_done_progress_params: Default::default(),
                }],
            )
        })
        .collect();
    let main_cursor = params.position;
    ctx.call::<DocumentSymbolRequest, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx: &mut Context, meta, results| {
            // Find the first non-empty result.
            let result = match results.into_iter().find(|(_, v)| v.is_some()) {
                Some(result) => result,
                None => (meta.servers[0], None),
            };

            editor_document_symbol(meta, main_cursor, result, ctx)
        },
    );
}

pub fn next_or_prev_symbol(meta: EditorMeta, params: NextOrPrevSymbolParams, ctx: &mut Context) {
    let eligible_servers: Vec<_> = ctx
        .servers(&meta)
        .filter(|srv| attempt_server_capability(ctx, *srv, &meta, CAPABILITY_DOCUMENT_SYMBOL))
        .collect();
    let req_params = eligible_servers
        .into_iter()
        .map(|(server_id, _)| {
            (
                server_id,
                vec![DocumentSymbolParams {
                    text_document: TextDocumentIdentifier {
                        uri: Url::from_file_path(&meta.buffile).unwrap(),
                    },
                    partial_result_params: Default::default(),
                    work_done_progress_params: Default::default(),
                }],
            )
        })
        .collect();
    ctx.call::<DocumentSymbolRequest, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx: &mut Context, meta, results| {
            // Find the first non-empty result.
            let result = match results.into_iter().find(|(_, v)| v.is_some()) {
                Some(result) => result,
                None => (meta.servers[0], None),
            };

            editor_next_or_prev_symbol(meta, params, result, ctx)
        },
    );
}

pub trait Symbol<T: Symbol<T>> {
    fn name(&self) -> &str;
    fn kind(&self) -> SymbolKind;
    fn uri(&self) -> Option<&Url>;
    fn range(&self) -> Range;
    fn selection_range(&self) -> Range;
    fn children(&self) -> &[T];
    fn children_mut(&mut self) -> &mut [T];
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
        self.name.split('\n').next().unwrap()
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
    fn children(&self) -> &[SymbolInformation] {
        &[]
    }
    fn children_mut(&mut self) -> &mut [SymbolInformation] {
        &mut []
    }
}

impl Symbol<DocumentSymbol> for DocumentSymbol {
    fn name(&self) -> &str {
        self.name.split('\n').next().unwrap()
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
    fn children(&self) -> &[DocumentSymbol] {
        self.children.as_ref().map(|v| &v[..]).unwrap_or_default()
    }
    fn children_mut(&mut self) -> &mut [DocumentSymbol] {
        self.children
            .as_mut()
            .map(|v| &mut v[..])
            .unwrap_or_default()
    }
}

fn editor_document_symbol(
    meta: EditorMeta,
    main_cursor: KakounePosition,
    result: (ServerId, Option<DocumentSymbolResponse>),
    ctx: &mut Context,
) {
    let (server_id, result) = result;
    let Some(result) = result else {
        return;
    };
    let server = ctx.server(server_id);
    let (content, goto_file_line) = {
        match result {
            DocumentSymbolResponse::Flat(result) => {
                if result.is_empty() {
                    return;
                }
                format_symbol(result, Some(main_cursor), &meta, server, ctx)
            }
            DocumentSymbolResponse::Nested(result) => {
                if result.is_empty() {
                    return;
                }
                format_symbol(result, Some(main_cursor), &meta, server, ctx)
            }
        }
    };
    let bufname = meta
        .buffile
        .as_str()
        .strip_prefix(&server.roots[0])
        .and_then(|p| p.strip_prefix('/'))
        .unwrap_or(&meta.buffile);
    let command = format!(
        "lsp-show-document-symbol {} {} {} {}",
        editor_quote(&server.roots[0]),
        editor_quote(&meta.buffile),
        editor_quote(&(bufname.to_owned() + "\n" + &content)),
        goto_file_line + 1,
    );
    ctx.exec(meta, command);
}

enum Tree {
    Child,
    LastChild,
    Pipe,
    Empty,
}

impl fmt::Display for Tree {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Tree::Child => "├── ",
                Tree::LastChild => "└── ",
                Tree::Pipe => "│   ",
                Tree::Empty => "    ",
            }
        )
    }
}

fn symbol_kind_prefix<T: Symbol<T>>(
    symbol: &T,
    symbol_kind_mapping: Option<&HashMap<String, String>>,
) -> Option<String> {
    symbol_kind_mapping?
        .get(format!("{:?}", symbol.kind()).as_str())
        .map(|mapped_kind| {
            if mapped_kind.is_empty() {
                "".to_string()
            } else {
                format!("{} ", mapped_kind)
            }
        })
}

/// Represent list of symbols as filetype=grep buffer content.
/// Paths are converted into relative to project root.
pub fn format_symbol<T: Symbol<T>>(
    items: Vec<T>,
    single_file_main_cursor: Option<KakounePosition>,
    meta: &EditorMeta,
    server: &ServerSettings,
    ctx: &Context,
) -> (String, usize) {
    struct MainCursor<'a> {
        source_file: KakounePosition,
        goto_file_line: &'a mut Option<usize>,
    }

    #[allow(clippy::too_many_arguments)]
    fn format_symbol_at_depth<T: Symbol<T>>(
        output: &mut Vec<(String, String)>,
        items: &[T],
        meta: &EditorMeta,
        server: &ServerSettings,
        ctx: &Context,
        single_file_main_cursor: &mut Option<MainCursor<'_>>,
        prefix: &mut Vec<bool>,
        symbol_kind_mapping: Option<&HashMap<String, String>>,
    ) {
        let length = items.len();
        let is_root = prefix.is_empty();
        for (index, symbol) in items.iter().enumerate() {
            let is_last = index + 1 == length;

            let hierarchy = if is_root {
                "".to_string()
            } else {
                let last_hierarchy_symbol = if is_last {
                    Tree::LastChild
                } else {
                    Tree::Child
                };
                let prefixing_hierarchy_symbols = prefix
                    .iter()
                    .skip(1)
                    .map(|&was_child| if was_child { Tree::Empty } else { Tree::Pipe })
                    .join("");
                format!("{}{}", prefixing_hierarchy_symbols, last_hierarchy_symbol)
            };

            let symbol_designator = symbol_kind_prefix(symbol, symbol_kind_mapping).map_or(
                format!("{} ({:?})", symbol.name(), symbol.kind()),
                |prefix| format!("{prefix} {}", symbol.name()),
            );
            let description = format!("{}{}", hierarchy, symbol_designator);
            let mut filename_path = PathBuf::default();
            let filename = symbol_filename(meta, symbol, &mut filename_path);
            let position = get_kakoune_position_with_fallback(
                server,
                filename,
                symbol.selection_range().start,
                ctx,
            );
            output.push((
                format!(
                    "{}:{}:{}:",
                    if single_file_main_cursor.is_some() {
                        "%"
                    } else {
                        short_file_path(filename, &server.roots[0])
                    },
                    position.line,
                    position.column,
                ),
                description,
            ));
            if let Some(main_cursor) = single_file_main_cursor {
                if position <= main_cursor.source_file {
                    *main_cursor.goto_file_line = Some(output.len());
                }
            }

            let children = symbol.children();
            prefix.push(is_last);
            format_symbol_at_depth(
                output,
                children,
                meta,
                server,
                ctx,
                single_file_main_cursor,
                prefix,
                symbol_kind_mapping,
            );
            prefix.pop();
        }
    }

    let mut goto_file_line = None;
    let mut single_file_main_cursor = single_file_main_cursor.map(|main_cursor| MainCursor {
        source_file: main_cursor,
        goto_file_line: &mut goto_file_line,
    });
    let mut columns = vec![];
    format_symbol_at_depth(
        &mut columns,
        &items,
        meta,
        server,
        ctx,
        &mut single_file_main_cursor,
        &mut vec![],
        meta.language_server
            .get(&server.name)
            .map(|language_server_config| &language_server_config.symbol_kinds),
    );
    let goto_buffer_data = if single_file_main_cursor.is_some() {
        // Align symbol names (first column is %:line:col).
        if let Some(width) = columns
            .iter()
            .map(|(position, _)| {
                assert!(position.chars().all(|c| c.is_ascii_graphic()));
                // Every byte is a width-1 character.
                position.len()
            })
            .max()
        {
            columns
                .into_iter()
                .map(|(position, description)| {
                    format!("{position:width$} {description}\n", width = width)
                })
                .join("")
        } else {
            "".to_string()
        }
    } else {
        columns
            .into_iter()
            .map(|(position, description)| format!("{position} {description}\n"))
            .join("")
    };
    (goto_buffer_data, goto_file_line.unwrap_or_default())
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
    params: NextOrPrevSymbolParams,
    result: (ServerId, Option<DocumentSymbolResponse>),
    ctx: &mut Context,
) {
    let (server_id, result) = result;
    let hover = params.hover;

    let symbol_kinds_query: Vec<SymbolKind> = params
        .symbol_kinds
        .iter()
        .map(|kind_str| symbol_kind_from_string(kind_str).unwrap())
        .collect::<Vec<_>>();

    let server = ctx.server(server_id);
    let maybe_details = match result {
        None => return,
        Some(DocumentSymbolResponse::Flat(mut result)) => {
            if result.is_empty() {
                return;
            }
            next_or_prev_symbol_details(
                &mut result,
                &params,
                &symbol_kinds_query,
                &meta,
                (server_id, server),
                ctx,
            )
        }
        Some(DocumentSymbolResponse::Nested(mut result)) => {
            if result.is_empty() {
                return;
            }
            next_or_prev_symbol_details(
                &mut result,
                &params,
                &symbol_kinds_query,
                &meta,
                (server_id, server),
                ctx,
            )
        }
    };

    editor_next_or_prev_for_details(server_id, meta, ctx, maybe_details, hover);
}

/// Send the response back to Kakoune. This could be either:
/// a) Instructions to move the cursor to the next/previous symbol.
/// b) Instructions to show hover information of the next/previous symbol (without actually
/// moving the cursor just yet).
fn editor_next_or_prev_for_details(
    server_id: ServerId,
    meta: EditorMeta,
    ctx: &mut Context,
    maybe_details: Option<(String, KakounePosition, String, SymbolKind)>,
    hover: bool,
) {
    let (filename, symbol_position, name, kind) = match maybe_details {
        Some((filename, symbol_position, name, kind)) => (filename, symbol_position, name, kind),
        None => {
            let no_symbol_found = indoc!(
                "evaluate-commands %[
                     info -style modal 'Not found!\n\nPress any key to continue'
                     on-key %[info -style modal]
                 ]"
            );
            ctx.exec(meta, no_symbol_found);
            return;
        }
    };

    let server = ctx.server(server_id);
    if !hover {
        let path = Path::new(&filename);
        let filename_abs = if path.is_absolute() {
            filename
        } else {
            Path::new(&server.roots[0])
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

    let mut req_params = HashMap::new();
    req_params.insert(
        server_id,
        vec![HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::from_file_path(&meta.buffile).unwrap(),
                },
                position: get_lsp_position(server, &meta.buffile, &symbol_position, ctx).unwrap(),
            },
            work_done_progress_params: Default::default(),
        }],
    );

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
             evaluate-commands %sh[
                 if [ \"$kak_key\" = \"g\" ]; then
                     echo 'exec {}g{}lh'
                 fi
             ]
         ]",
        symbol_position.line,
        symbol_position.column
    );

    ctx.call::<HoverRequest, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx: &mut Context, meta, result| {
            editor_hover(
                meta,
                HoverType::Modal {
                    modal_heading,
                    do_after,
                },
                symbol_position,
                KakouneRange {
                    start: symbol_position,
                    end: symbol_position,
                },
                0,
                result,
                ctx,
            )
        },
    );
}

/// Gets (filename, kakoune position, name) of the next/previous symbol in the buffer.
fn next_or_prev_symbol_details<T: Symbol<T> + 'static>(
    items: &mut [T],
    params: &NextOrPrevSymbolParams,
    symbol_kinds_query: &[SymbolKind],
    meta: &EditorMeta,
    server: (ServerId, &ServerSettings),
    ctx: &Context,
) -> Option<(String, KakounePosition, String, SymbolKind)> {
    // Some language servers return symbol locations that are not sorted in ascending order.
    // Sort the results so we can find next and previous properly.
    items.sort_by(|a, b| a.selection_range().start.cmp(&b.selection_range().start));

    // Setup an iterator dependending on whether we are searching forwards or backwards
    let it: Box<dyn Iterator<Item = &mut T>> = if params.search_next {
        Box::new(items.iter_mut())
    } else {
        Box::new(items.iter_mut().rev())
    };

    let cursor = params.position;
    let (_, server_settings) = server;

    for symbol in it {
        let kind = symbol.kind();
        let mut filename_path = PathBuf::default();
        let filename = symbol_filename(meta, symbol, &mut filename_path).to_string();

        let mut symbol_position = symbol.selection_range().start;
        if TypeId::of::<T>() == TypeId::of::<SymbolInformation>() {
            symbol_position = find_identifier_in_file(
                ctx,
                &filename,
                symbol_position,
                unadorned_name(&meta.language_id, symbol.name()),
            )
            .unwrap_or(symbol_position);
        }
        let symbol_position = get_kakoune_position_with_fallback(
            server_settings,
            &meta.buffile,
            symbol_position,
            ctx,
        );

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

        if let Some(from_children) = next_or_prev_symbol_details(
            symbol.children_mut(),
            params,
            symbol_kinds_query,
            meta,
            server,
            ctx,
        ) {
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
fn unadorned_name<'a>(language_id: &LanguageId, name: &'a str) -> &'a str {
    if *language_id == "erlang" {
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

pub fn object(meta: EditorMeta, params: ObjectParams, ctx: &mut Context) {
    let eligible_servers: Vec<_> = ctx
        .servers(&meta)
        .filter(|srv| attempt_server_capability(ctx, *srv, &meta, CAPABILITY_DOCUMENT_SYMBOL))
        .collect();
    let req_params = eligible_servers
        .into_iter()
        .map(|(server_id, _)| {
            (
                server_id,
                vec![DocumentSymbolParams {
                    text_document: TextDocumentIdentifier {
                        uri: Url::from_file_path(&meta.buffile).unwrap(),
                    },
                    partial_result_params: Default::default(),
                    work_done_progress_params: Default::default(),
                }],
            )
        })
        .collect();
    ctx.call::<DocumentSymbolRequest, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx: &mut Context, meta, results| {
            let result = match results.into_iter().find(|(_, v)| v.is_some()) {
                Some(result) => result,
                None => (meta.servers[0], None),
            };

            editor_object(meta, params, result, ctx)
        },
    );
}

fn editor_object(
    meta: EditorMeta,
    params: ObjectParams,
    result: (ServerId, Option<DocumentSymbolResponse>),
    ctx: &mut Context,
) {
    let (server_id, result) = result;

    let selections: Vec<(KakouneRange, KakounePosition)> = params
        .selections_desc
        .iter()
        .map(|s| parse_kakoune_range(s))
        .collect();

    let mut symbol_kinds_query: Vec<SymbolKind> = vec![];
    for kind_str in &params.symbol_kinds {
        symbol_kinds_query.push(match symbol_kind_from_string(kind_str) {
            Some(kind) => kind,
            None => {
                let err = format!("invalid symbol kind '{}'", &kind_str);
                ctx.show_error(meta, err);
                return;
            }
        });
    }

    let document = match ctx.documents.get(&meta.buffile) {
        Some(document) => document,
        None => {
            let err = format!("Missing document for {}", &meta.buffile);
            ctx.show_error(meta, err);
            return;
        }
    };
    let server = ctx.server(server_id);
    let mut ranges = match result {
        None => return,
        Some(DocumentSymbolResponse::Flat(symbols)) => {
            flat_symbol_ranges(server, document, symbols, symbol_kinds_query)
        }
        Some(DocumentSymbolResponse::Nested(symbols)) => {
            flat_symbol_ranges(server, document, symbols, symbol_kinds_query)
        }
    };

    if ranges.is_empty() {
        ctx.show_error(meta, "lsp-object: no matching symbol found");
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
                    let matched_lsp_pos = kakoune_position_to_lsp(
                        &matched_pos,
                        &document.text,
                        server.offset_encoding,
                    );
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
                ctx.show_error(meta, "lsp-object: invalid mode");
                return;
            }
        };
        new_selections.push(KakouneRange { start, end })
    }
    if new_selections.is_empty() {
        ctx.show_error(meta, "lsp-object: no selections remaining");
        return;
    }
    ctx.exec(
        meta,
        format!(
            "select {}",
            new_selections
                .into_iter()
                .map(|range| format!(
                    "{}",
                    // N.B. this can be forward or backward (determined above).
                    ForwardKakouneRange(range)
                ))
                .join(" ")
        ),
    );
}

fn flat_symbol_ranges<T: Symbol<T>>(
    server: &ServerSettings,
    document: &Document,
    symbols: Vec<T>,
    symbol_kinds_query: Vec<SymbolKind>,
) -> Vec<(KakouneRange, KakounePosition)> {
    fn walk<T, F>(
        result: &mut Vec<(KakouneRange, KakounePosition)>,
        symbol_kinds_query: &[SymbolKind],
        convert: &F,
        s: &T,
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
    let convert = |range| lsp_range_to_kakoune(&range, &document.text, server.offset_encoding);
    for s in symbols {
        walk(&mut result, &symbol_kinds_query, &convert, &s);
    }
    result
}

pub fn document_symbol_menu(meta: EditorMeta, params: GotoSymbolParams, ctx: &mut Context) {
    let eligible_servers: Vec<_> = ctx
        .servers(&meta)
        .filter(|srv| attempt_server_capability(ctx, *srv, &meta, CAPABILITY_DOCUMENT_SYMBOL))
        .collect();
    let req_params = eligible_servers
        .into_iter()
        .map(|(server_id, _)| {
            (
                server_id,
                vec![DocumentSymbolParams {
                    text_document: TextDocumentIdentifier {
                        uri: Url::from_file_path(&meta.buffile).unwrap(),
                    },
                    partial_result_params: Default::default(),
                    work_done_progress_params: Default::default(),
                }],
            )
        })
        .collect();
    ctx.call::<DocumentSymbolRequest, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx: &mut Context, meta, results| {
            let result = match results.into_iter().find(|(_, v)| v.is_some()) {
                Some(result) => result,
                None => (meta.servers[0], None),
            };

            let maybe_goto_symbol = params.goto_symbol;
            match maybe_goto_symbol {
                Some(goto_symbol) => editor_document_symbol_goto(meta, goto_symbol, result, ctx),
                None => editor_document_symbol_menu(meta, result, ctx),
            }
        },
    );
}

fn editor_document_symbol_menu(
    meta: EditorMeta,
    result: (ServerId, Option<DocumentSymbolResponse>),
    ctx: &mut Context,
) {
    let (server_id, result) = result;
    let server = ctx.server(server_id);
    let choices = match result {
        Some(DocumentSymbolResponse::Flat(result)) => {
            if result.is_empty() {
                return;
            }
            symbol_menu(&result, &meta, server, ctx)
        }
        Some(DocumentSymbolResponse::Nested(result)) => {
            if result.is_empty() {
                return;
            }
            symbol_menu(&result, &meta, server, ctx)
        }
        None => return,
    };
    let command = format!("lsp-menu -select-cmds {}", choices);
    ctx.exec(meta, command);
}

fn editor_document_symbol_goto(
    meta: EditorMeta,
    goto_symbol: String,
    result: (ServerId, Option<DocumentSymbolResponse>),
    ctx: &mut Context,
) {
    let (server_id, result) = result;
    let server = ctx.server(server_id);
    let navigate_command = match result {
        Some(DocumentSymbolResponse::Flat(result)) => {
            if result.is_empty() {
                return;
            }
            symbol_search(result, goto_symbol, &meta, server, ctx)
        }
        Some(DocumentSymbolResponse::Nested(result)) => {
            if result.is_empty() {
                return;
            }
            symbol_search(result, goto_symbol, &meta, server, ctx)
        }
        None => return,
    };
    if navigate_command.is_empty() {
        return;
    }
    ctx.exec(meta, navigate_command);
}

fn symbols_walk<'a, T, V>(visit: &mut V, s: &'a T) -> bool
where
    T: Symbol<T>,
    V: FnMut(bool, &'a T) -> bool,
{
    if !visit(true, s) {
        return false;
    }
    for child in s.children() {
        if !symbols_walk(visit, child) {
            return false;
        }
    }

    let ok = visit(false, s);
    assert!(ok);

    true
}

fn symbol_menu<'a, T: Symbol<T>>(
    symbols: &'a Vec<T>,
    meta: &EditorMeta,
    server: &ServerSettings,
    ctx: &Context,
) -> String {
    let mut choices = vec![];
    let mut qualified_name = vec![];
    let mut add_symbol = |pre: bool, symbol: &'a T| {
        if !pre {
            qualified_name.pop();
            return true;
        }
        let mut filename_path = PathBuf::default();
        let filename = symbol_filename(meta, symbol, &mut filename_path);
        let range =
            get_kakoune_range_with_fallback(server, filename, &symbol.selection_range(), ctx);
        qualified_name.push(symbol.name());
        let qualified_name = qualified_name.join(" > ");
        choices.push((qualified_name, edit_at_range(filename, range, false)));
        true
    };
    for symbol in symbols {
        symbols_walk(&mut add_symbol, symbol);
    }
    choices
        .into_iter()
        .map(|(qualified_name, goto_command)| {
            let goto_command = editor_quote(&goto_command);
            format!(
                "{} {goto_command} {goto_command}",
                editor_quote(&qualified_name),
            )
        })
        .join(" ")
}

fn symbol_search<T: Symbol<T>>(
    symbols: Vec<T>,
    goto_symbol: String,
    meta: &EditorMeta,
    server: &ServerSettings,
    ctx: &Context,
) -> String {
    let mut navigate_cmd = String::new();
    let mut symbol_matches = |pre: bool, symbol: &T| {
        if !pre {
            return true;
        }
        if symbol.name() == goto_symbol {
            let mut filename_path = PathBuf::default();
            let filename = symbol_filename(meta, symbol, &mut filename_path);
            let range =
                get_kakoune_range_with_fallback(server, filename, &symbol.selection_range(), ctx);
            write!(
                &mut navigate_cmd,
                "evaluate-commands '{}'",
                editor_escape(&edit_at_range(filename, range, true))
            )
            .unwrap();
            false
        } else {
            true
        }
    };
    for symbol in symbols {
        if !symbols_walk(&mut symbol_matches, &symbol) {
            break;
        }
    }
    navigate_cmd
}

pub fn breadcrumbs(meta: EditorMeta, params: BreadcrumbsParams, ctx: &mut Context) {
    let eligible_servers: Vec<_> = ctx
        .servers(&meta)
        .filter(|srv| attempt_server_capability(ctx, *srv, &meta, CAPABILITY_DOCUMENT_SYMBOL))
        .collect();
    let req_params = eligible_servers
        .into_iter()
        .map(|(server_id, _)| {
            (
                server_id,
                vec![DocumentSymbolParams {
                    text_document: TextDocumentIdentifier {
                        uri: Url::from_file_path(&meta.buffile).unwrap(),
                    },
                    partial_result_params: Default::default(),
                    work_done_progress_params: Default::default(),
                }],
            )
        })
        .collect();
    ctx.call::<DocumentSymbolRequest, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx: &mut Context, meta, results| {
            let Some(result) = results.into_iter().find(|(_, v)| v.is_some()) else {
                return;
            };
            let (server_id, Some(symbols)) = result else {
                return;
            };
            match symbols {
                DocumentSymbolResponse::Nested(symbols) => {
                    editor_breadcrumbs(symbols, ctx, server_id, meta, params)
                }
                DocumentSymbolResponse::Flat(symbols) => {
                    editor_breadcrumbs(symbols, ctx, server_id, meta, params)
                }
            };
        },
    );
}

fn editor_breadcrumbs<T: Symbol<T>>(
    symbols: Vec<T>,
    ctx: &mut Context,
    server_id: ServerId,
    meta: EditorMeta,
    params: BreadcrumbsParams,
) {
    if symbols.is_empty() {
        return;
    }
    let server = ctx.server(server_id);
    let mut filename_path = PathBuf::default();
    let filename = symbol_filename(&meta, &symbols[0], &mut filename_path).to_string();
    let mut surrounding_symbols = Vec::default();
    let mut breadcrumbs = "".to_string();
    compute_surrounding_symbols(
        &mut surrounding_symbols,
        &symbols,
        params.position_line,
        ctx,
        server,
        &filename,
    );
    for (i, symbol) in surrounding_symbols.iter().enumerate() {
        if i != 0 {
            breadcrumbs += " > ";
        }
        if i + 1 == surrounding_symbols.len() {
            if let Some(kind_prefix) = symbol_kind_prefix(
                *symbol,
                meta.language_server
                    .get(&server.name)
                    .map(|language_server_config| &language_server_config.symbol_kinds),
            ) {
                breadcrumbs += &kind_prefix;
            }
        }
        breadcrumbs += symbol.name();
    }
    breadcrumbs += " ";
    let command = formatdoc!(
        "buffer {}
         try 'set-option window lsp_modeline_breadcrumbs {}'",
        editor_quote(&meta.buffile),
        editor_escape(&editor_quote(&breadcrumbs))
    );
    let command = format!("evaluate-commands -draft -- {}", editor_quote(&command));
    ctx.exec(meta, command);
}

fn compute_surrounding_symbols<'a, T: Symbol<T>>(
    symbol_stack: &mut Vec<&'a T>,
    nested_symbols: &'a [T],
    line: u32,
    ctx: &Context,
    server: &ServerSettings,
    filename: &str,
) {
    let Some(symbol) = nested_symbols.iter().find(|symbol| {
        let symbol_range = get_kakoune_range(server, filename, &symbol.range(), ctx).unwrap();
        let symbol_lines = symbol_range.start.line..=symbol_range.end.line;
        symbol_lines.contains(&line)
    }) else {
        return;
    };
    symbol_stack.push(symbol);
    compute_surrounding_symbols(symbol_stack, symbol.children(), line, ctx, server, filename)
}
