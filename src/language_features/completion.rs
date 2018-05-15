use context::*;
use languageserver_types::request::Request;
use languageserver_types::*;
use regex::Regex;
use serde::Deserialize;
use std;
use types::*;
use url::Url;

pub fn text_document_completion(params: EditorParams, meta: &EditorMeta, ctx: &mut Context) {
    let req_params = TextDocumentCompletionParams::deserialize(params.clone())
        .expect("Params should follow TextDocumentCompletionParams structure");
    let position = req_params.position;
    let offset = req_params.completion.offset;
    if offset == 0
        && !ctx.config
            .editor
            .get("zero_char_completion")
            .unwrap_or(&false)
    {
        let p = position;
        let command = format!(
            "set window lsp_completions %§{}.{}@{}:§\n",
            p.line + 1,
            p.character + 1,
            meta.version,
        );
        ctx.exec(meta.clone(), command);
        return;
    }
    let req_params = CompletionParams {
        text_document: TextDocumentIdentifier {
            uri: Url::parse(&format!("file://{}", &meta.buffile)).unwrap(),
        },
        position,
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
    params: &TextDocumentCompletionParams,
    result: CompletionResponse,
    ctx: &mut Context,
) {
    let items = match result {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };
    let re = Regex::new(r"(?P<c>[:|$])").unwrap();
    let maxlen = items.iter().map(|x| x.label.len()).max().unwrap_or(0);

    let items = items
        .into_iter()
        .map(|x| {
            let mut doc: String = match &x.documentation {
                None => "".to_string(),
                Some(doc) => match doc {
                    Documentation::String(st) => st.clone(),
                    Documentation::MarkupContent(mup) => mup.value.clone(),
                },
            };
            if let Some(d) = x.detail {
                doc = format!("{}\n\n{}", d, doc);
            }
            let mut entry = x.label.clone();
            if let Some(k) = x.kind {
                entry += &std::iter::repeat(" ")
                    .take(maxlen - x.label.len())
                    .collect::<String>();
                entry += &format!(" {{MenuInfo}}{:?}", k);
            }
            format!(
                "{}|{}|{}",
                re.replace_all(&x.insert_text.unwrap_or(x.label), r"\$c"),
                re.replace_all(&doc, r"\$c"),
                re.replace_all(&entry, r"\$c"),
            )
        })
        .collect::<Vec<String>>()
        .join(":");
    let p = params.position;
    let command = format!(
        "set window lsp_completions %§{}.{}@{}:{}§\n",
        p.line + 1,
        p.character + 1 - params.completion.offset,
        meta.version,
        items
    );
    ctx.exec(meta.clone(), command);
}
