use context::*;
use languageserver_types::request::Request;
use languageserver_types::*;
use serde::Deserialize;
use types::*;
use url::Url;

pub fn text_document_formatting(params: EditorParams, meta: &EditorMeta, ctx: &mut Context) {
    let options = FormattingOptions::deserialize(params.clone());
    if options.is_err() {
        error!("Params should follow FormattingOptions structure");
    }
    let options = options.unwrap();
    let req_params = DocumentFormattingParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        options,
    };
    let id = ctx.next_request_id();
    ctx.response_waitlist.insert(
        id.clone(),
        (meta.clone(), request::Formatting::METHOD.into(), params),
    );
    ctx.call(id, request::Formatting::METHOD.into(), req_params);
}

fn escape(s: &str) -> String {
    s.replace("'", "''")
}

pub fn editor_formatting(
    meta: &EditorMeta,
    _params: &FormattingOptions,
    result: TextEditResponse,
    ctx: &mut Context,
) {
    if let TextEditResponse::Array(text_edits) = result {
        let command = text_edits
            .iter()
            .map(|text_edit| {
                let TextEdit { range, new_text } = text_edit;
                // LSP ranges are 0-based, but Kakoune's 1-based.
                // LSP ranges are exclusive, but Kakoune's are inclusive.
                // Also from LSP spec: If you want to specify a range that contains a line including
                // the line ending character(s) then use an end position denoting the start of the next
                // line.
                // TODO: extract to util
                let mut end_line = range.end.line;
                let mut end_char = range.end.character;
                if end_char > 0 {
                    end_line += 1;
                } else {
                    end_char = 1_000_000;
                }
                format!(
                    "lsp-text-edit {}.{},{}.{} '{}'",
                    range.start.line + 1,
                    range.start.character + 1,
                    end_line,
                    end_char,
                    escape(&new_text)
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        ctx.exec(meta.clone(), command);
    }
}
