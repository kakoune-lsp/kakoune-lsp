use context::*;
use itertools::Itertools;
use languageserver_types::request::Request;
use languageserver_types::*;
use serde::Deserialize;
use serde_json::{self, Value};
use types::*;
use url::Url;
use util::*;

pub fn text_document_formatting(meta: &EditorMeta, params: EditorParams, ctx: &mut Context) {
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

pub fn editor_formatting(
    meta: &EditorMeta,
    _params: EditorParams,
    result: Value,
    ctx: &mut Context,
) {
    let result = serde_json::from_value(result).expect("Failed to parse formatting response");
    if let TextEditResponse::Array(text_edits) = result {
        // empty text edits processed as a special case because Kakoune's `select` command
        // doesn't support empty arguments list
        if text_edits.is_empty() {
            // nothing to do, but sending command back to the editor is required to handle case when
            // editor is blocked waiting for response via fifo
            ctx.exec(meta.clone(), "nop".to_string());
            return;
        }
        let edits = text_edits
            .iter()
            .map(|text_edit| {
                let TextEdit { range, new_text } = text_edit;
                // LSP ranges are 0-based, but Kakoune's 1-based.
                // LSP ranges are exclusive, but Kakoune's are inclusive.
                // Also from LSP spec: If you want to specify a range that contains a line including
                // the line ending character(s) then use an end position denoting the start of the next
                // line.
                let mut start_line = range.start.line;
                let mut start_char = range.start.character;
                let mut end_line = range.end.line;
                let mut end_char = range.end.character;

                if start_line == end_line && start_char == end_char && start_char == 0 {
                    start_char = 1_000_000;
                } else {
                    start_line += 1;
                    start_char += 1;
                }

                if end_char > 0 {
                    end_line += 1;
                } else {
                    end_char = 1_000_000;
                }

                let insert = start_line == end_line && start_char - 1 == end_char;

                (
                    format!(
                        "{}.{}",
                        start_line,
                        if !insert { start_char } else { end_char }
                    ),
                    format!("{}.{}", end_line, end_char),
                    new_text,
                    insert,
                )
            }).collect::<Vec<_>>();

        let select_edits = edits
            .iter()
            .map(|(start, end, _, _)| format!("{},{}", start, end))
            .join(" ");

        let apply_edits = edits
            .iter()
            .enumerate()
            .map(|(i, (_, _, content, insert))| {
                format!(
                    "exec 'z{}<space>'
                    {} {}",
                    if i > 0 {
                        format!("{})", i)
                    } else {
                        "".to_string()
                    },
                    if *insert {
                        "lsp-insert-after-selection"
                    } else {
                        "lsp-replace-selection"
                    },
                    editor_quote(&content)
                )
            }).join("\n");

        let command = format!(
            "select {}
            exec -save-regs '' Z
            {}",
            select_edits, apply_edits
        );
        let command = format!("eval -draft -save-regs '^' {}", editor_quote(&command));
        ctx.exec(meta.clone(), command);
    }
}
