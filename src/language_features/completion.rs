use crate::context::*;
use crate::types::*;
use crate::util::*;
use itertools::Itertools;
use lsp_types::request::Request;
use lsp_types::*;
use regex::Regex;
use serde::Deserialize;
use serde_json::{self, Value};
use std;
use url::Url;

pub fn text_document_completion(meta: &EditorMeta, params: EditorParams, ctx: &mut Context) {
    let req_params = TextDocumentCompletionParams::deserialize(params.clone()).unwrap();
    let req_params = CompletionParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        position: get_lsp_position(&meta.buffile, &req_params.position, ctx).unwrap(),
        context: None,
    };
    let id = ctx.next_request_id();
    ctx.response_waitlist.insert(
        id.clone(),
        (meta.clone(), request::Completion::METHOD.into(), params),
    );
    ctx.call(id, request::Completion::METHOD.into(), req_params);
}

pub fn editor_completion(
    meta: &EditorMeta,
    params: EditorParams,
    result: Value,
    ctx: &mut Context,
) {
    let params = TextDocumentCompletionParams::deserialize(params).expect("Failed to parse params");
    let result = serde_json::from_value(result).expect("Failed to parse completion response");
    let items = match result {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };
    let unescape_markdown_re = Regex::new(r"\\(?P<c>.)").unwrap();
    let maxlen = items.iter().map(|x| x.label.len()).max().unwrap_or(0);
    let escape_bar = |s: &str| s.replace("|", r"\|");

    let items = items
        .into_iter()
        .map(|x| {
            let mut doc: String = match &x.documentation {
                None => "".to_string(),
                Some(doc) => match doc {
                    Documentation::String(st) => st.clone(),
                    Documentation::MarkupContent(mup) => match mup.kind {
                        MarkupKind::PlainText => mup.value.clone(),
                        // NOTE just in case server ignored our documentationFormat capability
                        // we want to unescape markdown to make text a bit more readable
                        MarkupKind::Markdown => unescape_markdown_re
                            .replace_all(&mup.value, r"$c")
                            .to_string(),
                    },
                },
            };
            if let Some(d) = x.detail {
                doc = format!("{}\n\n{}", d, doc);
            }
            let doc = format!("info -style menu {}", editor_quote(&doc));
            let mut entry = x.label.clone();
            if let Some(k) = x.kind {
                entry += &std::iter::repeat(" ")
                    .take(maxlen - x.label.len())
                    .collect::<String>();
                entry += &format!(" {{MenuInfo}}{:?}", k);
            }
            editor_quote(&format!(
                "{}|{}|{}",
                escape_bar(&x.insert_text.unwrap_or(x.label)),
                escape_bar(&doc),
                escape_bar(&entry),
            ))
        })
        .join(" ");
    let p = params.position;
    let command = format!(
        "set window lsp_completions {}.{}@{} {}\n",
        p.line, params.completion.offset, meta.version, items
    );
    ctx.exec(meta.clone(), command);
}
