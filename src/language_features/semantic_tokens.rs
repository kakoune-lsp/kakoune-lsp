use crate::context::Context;
use crate::position::lsp_range_to_kakoune;
use crate::types::EditorMeta;
use crate::util::editor_quote;
use lsp_types::request::SemanticTokensFullRequest;
use lsp_types::{
    Position, Range, SemanticToken, SemanticTokenModifier, SemanticTokensOptions,
    SemanticTokensParams, SemanticTokensRegistrationOptions, SemanticTokensResult,
    SemanticTokensServerCapabilities::*, TextDocumentIdentifier,
};
use url::Url;

pub fn tokens_request(meta: EditorMeta, ctx: &mut Context) {
    let req_params = SemanticTokensParams {
        partial_result_params: Default::default(),
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        work_done_progress_params: Default::default(),
    };
    ctx.call::<SemanticTokensFullRequest, _>(meta, req_params, move |ctx, meta, response| {
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
                    start: Position::new(line, start),
                    end: Position::new(line, start + length),
                };
                let range = lsp_range_to_kakoune(&range, &document.text, ctx.offset_encoding);
                // See the spec for information on the integer encoding:
                // https://microsoft.github.io/language-server-protocol/specifications/specification-current/#textDocument_semanticTokens
                let token_name = legend.token_types[token_type as usize].as_str();
                let token_modifiers: Vec<&SemanticTokenModifier> = (0..32)
                    // Find bits in the mask that equal `1`
                    .filter(|bit| ((token_modifiers_bitset >> bit) & 1u32) == 1u32)
                    // Map bits to modifiers
                    .map(|bit| &legend.token_modifiers[bit as usize])
                    .collect();

                let candidates = ctx
                    .config
                    .semantic_tokens
                    .faces
                    .iter()
                    .filter(|token_config| {
                        token_name == token_config.token &&
                        // All the config's modifiers must exist on the token for this
                        // config to match.
                        token_config
                        .modifiers
                        .iter()
                        .all(|modifier| token_modifiers.contains(&modifier))
                    });

                // But not all the token's modifiers must exist on the config.
                // Therefore, we use the config that matches the most modifiers.
                let best = candidates.max_by_key(|token_config| {
                    token_modifiers
                        .iter()
                        .filter(|modifier| token_config.modifiers.contains(modifier))
                        .count()
                });

                best.map(|token_config| format!("{}|{}", range, token_config.face))
            },
        )
        .collect::<Vec<String>>()
        .join(" ");

    let command = format!(
        "set buffer lsp_semantic_tokens {} {}",
        meta.version, &ranges
    );
    let command = format!(
        "eval -buffer {} -verbatim -- {}",
        editor_quote(&meta.buffile),
        command
    );
    ctx.exec(meta, command)
}
