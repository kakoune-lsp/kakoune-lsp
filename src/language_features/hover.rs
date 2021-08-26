use crate::context::*;
use crate::markup::*;
use crate::types::*;
use crate::util::*;
use itertools::Itertools;
use lsp_types::request::*;
use lsp_types::*;
use serde::Deserialize;
use std::str;
use url::Url;

pub fn text_document_hover(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = PositionParams::deserialize(params).unwrap();
    let req_params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::from_file_path(&meta.buffile).unwrap(),
            },
            position: get_lsp_position(&meta.buffile, &params.position, ctx).unwrap(),
        },
        work_done_progress_params: Default::default(),
    };
    ctx.call::<HoverRequest, _>(meta, req_params, move |ctx: &mut Context, meta, result| {
        editor_hover(meta, params, result, ctx)
    });
}

pub fn editor_hover(
    meta: EditorMeta,
    params: PositionParams,
    result: Option<Hover>,
    ctx: &mut Context,
) {
    let diagnostics = ctx.diagnostics.get(&meta.buffile);
    let pos = get_lsp_position(&meta.buffile, &params.position, ctx).unwrap();
    let diagnostics = diagnostics
        .map(|x| {
            x.iter()
                .filter(|x| {
                    let start = x.range.start;
                    let end = x.range.end;
                    (start.line < pos.line && pos.line < end.line)
                        || (start.line == pos.line
                            && pos.line == end.line
                            && start.character <= pos.character
                            && pos.character <= end.character)
                        || (start.line == pos.line
                            && pos.line <= end.line
                            && start.character <= pos.character)
                        || (start.line <= pos.line
                            && end.line == pos.line
                            && pos.character <= end.character)
                })
                .map(|x| str::trim(&x.message))
                .filter(|x| !x.is_empty())
                .map(|x| format!("• {}", x))
                .join("\n")
        })
        .unwrap_or_else(String::new);
    let contents = match result {
        None => "".to_string(),
        Some(result) => match result.contents {
            HoverContents::Scalar(contents) => contents.plaintext(),
            HoverContents::Array(contents) => contents
                .into_iter()
                .filter_map(|x| {
                    let text = x.plaintext();

                    if !text.is_empty() {
                        Some(format!("• {}", text.trim()))
                    } else {
                        None
                    }
                })
                .join("\n"),
            HoverContents::Markup(contents) => match contents.kind {
                MarkupKind::Markdown => markdown_to_kakoune_markup(contents.value),
                MarkupKind::PlainText => contents.value,
            },
        },
    };

    if contents.is_empty() && diagnostics.is_empty() {
        return;
    }

    let command = format!(
        "lsp-show-hover {} %§{}§ %§{}§",
        params.position,
        contents.replace("§", "\\§"),
        diagnostics.replace("§", "\\§")
    );

    ctx.exec(meta, command);
}

trait PlainText {
    fn plaintext(self) -> String;
}

impl PlainText for MarkedString {
    fn plaintext(self) -> String {
        match self {
            MarkedString::String(contents) => contents,
            MarkedString::LanguageString(contents) => contents.value,
        }
    }
}
