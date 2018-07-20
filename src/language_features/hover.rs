use context::*;
use languageserver_types::request::Request;
use languageserver_types::*;
use serde::Deserialize;
use std::str;
use types::*;
use url::Url;

pub fn text_document_hover(params: EditorParams, meta: &EditorMeta, ctx: &mut Context) {
    let req_params = PositionParams::deserialize(params.clone());
    if req_params.is_err() {
        error!("Params should follow PositionParams structure");
        return;
    }
    let req_params = req_params.unwrap();
    let position = req_params.position;
    let req_params = TextDocumentPositionParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        position,
    };
    let id = ctx.next_request_id();
    ctx.response_waitlist.insert(
        id.clone(),
        (meta.clone(), request::HoverRequest::METHOD.into(), params),
    );
    ctx.call(id, request::HoverRequest::METHOD.into(), req_params);
}

pub fn editor_hover(
    meta: &EditorMeta,
    params: &PositionParams,
    result: Option<Hover>,
    ctx: &mut Context,
) {
    let diagnostics = ctx.diagnostics.get(&meta.buffile);
    let pos = params.position;
    let diagnostics = diagnostics
        .and_then(|x| {
            Some(
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
                    .map(|x| format!("• {}", str::trim(&x.message)))
                    .collect::<Vec<String>>()
                    .join("\n"),
            )
        })
        .unwrap_or_else(String::new);
    let contents = match result {
        None => "".to_string(),
        Some(result) => match result.contents {
            HoverContents::Scalar(contents) => contents.plaintext(),
            HoverContents::Array(contents) => contents
                .into_iter()
                .map(|x| format!("• {}", str::trim(&x.plaintext())))
                .collect::<Vec<String>>()
                .join("\n"),
            HoverContents::Markup(contents) => contents.value,
        },
    };

    if contents.is_empty() && diagnostics.is_empty() {
        return;
    }

    let position = format!("{}.{}", pos.line + 1, pos.character + 1);

    let command = if diagnostics.is_empty() {
        format!("lsp-show-hover {} %§{}§", position, contents)
    } else if contents.is_empty() {
        format!("lsp-show-hover {} %§{}§", position, diagnostics)
    } else {
        format!(
            "lsp-show-hover {} %§{}\n\n{}§",
            position, contents, diagnostics
        )
    };

    ctx.exec(meta.clone(), command);
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
