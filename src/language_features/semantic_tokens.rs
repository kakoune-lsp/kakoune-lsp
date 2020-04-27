use crate::context::Context;
use crate::position::lsp_range_to_kakoune;
use crate::types::{EditorMeta, EditorParams};
use crate::util::editor_quote;
use lsp_types::request::SemanticTokensRequest;
use lsp_types::{
    Position, Range, SemanticToken, SemanticTokensOptions, SemanticTokensParams,
    SemanticTokensRegistrationOptions, SemanticTokensResult, SemanticTokensServerCapabilities::*,
    TextDocumentIdentifier,
};
use url::Url;

pub fn tokens_request(meta: EditorMeta, _params: EditorParams, ctx: &mut Context) {
    let req_params = SemanticTokensParams {
        partial_result_params: Default::default(),
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        work_done_progress_params: Default::default(),
    };
    ctx.call::<SemanticTokensRequest, _>(meta, req_params, move |ctx, meta, response| {
        if let Some(response) = response {
            tokens_response(meta, response, ctx);
        }
    });
}

pub fn tokens_response(meta: EditorMeta, tokens: SemanticTokensResult, ctx: &mut Context) {
    let legend = match ctx.capabilities.as_ref().unwrap().semantic_tokens_provider {
        Some(SemanticTokensOptions(SemanticTokensOptions { ref legend, .. }))
        | Some(SemanticTokensRegistrationOptions(SemanticTokensRegistrationOptions {
            semantic_tokens_options: SemanticTokensOptions { ref legend, .. },
            ..
        })) => legend,
        None => return,
    };
    let document = match ctx.documents.get(&meta.buffile) {
        Some(document) => document,
        None => return,
    };
    let tokens = match tokens {
        SemanticTokensResult::Tokens(tokens) => tokens.data,
        SemanticTokensResult::Partial(partial) => partial.data,
    };
    let mut line = 0;
    let mut start = 0;
    let ranges = tokens
        .into_iter()
        .filter_map(
            |SemanticToken {
                 delta_line,
                 delta_start,
                 length,
                 token_type,
                 token_modifiers_bitset,
             }| {
                if delta_line != 0 {
                    line += delta_line;
                    start = delta_start;
                } else {
                    start += delta_start;
                }
                let range = Range {
                    start: Position::new(line.into(), start.into()),
                    end: Position::new(line.into(), (start + length).into()),
                };
                let range = lsp_range_to_kakoune(&range, &document.text, &ctx.offset_encoding);
                let token = &legend.token_types[token_type as usize];
                (0..32)
                    .filter(|bit| ((token_modifiers_bitset >> bit) & 1) == 1)
                    .map(|bit| &legend.token_modifiers[bit as usize])
                    .filter_map(|token| ctx.config.semantic_token_modifiers.get(token.as_str()))
                    .chain(ctx.config.semantic_tokens.get(token.as_str()))
                    .next()
                    .map(|face| format!("{}|{}", range, face))
            },
        )
        .collect::<Vec<String>>()
        .join(" ");
    let command = format!(
        "set buffer lsp_semantic_tokens {} {}",
        meta.version, &ranges
    );
    let command = format!(
        "eval -buffer {} {}",
        editor_quote(&meta.buffile),
        editor_quote(&command)
    );
    ctx.exec(meta, command.to_string())
}
