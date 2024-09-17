use crate::capabilities::attempt_server_capability;
use crate::capabilities::CAPABILITY_SIGNATURE_HELP;
use crate::context::*;
use crate::markup::escape_kakoune_markup;
use crate::position::*;
use crate::types::*;
use crate::util::*;
use lsp_types::request::*;
use lsp_types::*;
use ropey::Rope;
use url::Url;

pub fn text_document_signature_help(meta: EditorMeta, params: PositionParams, ctx: &mut Context) {
    let eligible_servers: Vec<_> = ctx
        .servers(&meta)
        .filter(|srv| attempt_server_capability(*srv, &meta, CAPABILITY_SIGNATURE_HELP))
        .collect();
    if meta.fifo.is_none() && eligible_servers.is_empty() {
        return;
    }

    let (first_server, _) = *eligible_servers.first().unwrap();
    let first_server = first_server.to_owned();

    let req_params = eligible_servers
        .into_iter()
        .map(|(server_id, server_settings)| {
            (
                server_id,
                vec![SignatureHelpParams {
                    context: None,
                    text_document_position_params: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier {
                            uri: Url::from_file_path(&meta.buffile).unwrap(),
                        },
                        position: get_lsp_position(
                            server_settings,
                            &meta.buffile,
                            &params.position,
                            ctx,
                        )
                        .unwrap(),
                    },
                    work_done_progress_params: Default::default(),
                }],
            )
        })
        .collect();
    ctx.call::<SignatureHelpRequest, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx: &mut Context, meta, results| {
            let result = match results.into_iter().find(|(_, v)| v.is_some()) {
                Some(result) => result,
                None => (first_server, None),
            };

            editor_signature_help(meta, params, result, ctx)
        },
    );
}

fn editor_signature_help(
    meta: EditorMeta,
    params: PositionParams,
    result: (ServerId, Option<SignatureHelp>),
    ctx: &mut Context,
) {
    let (server_id, result) = result;
    let result = match result {
        Some(result) => result,
        None => return,
    };

    let active_signature = result.active_signature.unwrap_or(0);

    let active_signature = match result.signatures.get(active_signature as usize) {
        Some(active_signature) => active_signature,
        None => return,
    };

    let server = ctx.server(server_id);
    let active_parameter = active_signature
        .active_parameter
        .or(result.active_parameter)
        .unwrap_or(0);
    let parameter_range = match active_signature
        .parameters
        .as_ref()
        .and_then(|p| p.get(active_parameter as usize))
        .map(|p| &p.label)
    {
        Some(ParameterLabel::Simple(param)) => active_signature
            .label
            .find(param.as_str())
            .map(|begin| [begin, begin + param.len()]),
        Some(ParameterLabel::LabelOffsets(offsets)) => {
            let label = Rope::from_str(&active_signature.label);
            let begin = lsp_character_to_byte_offset(
                label.slice(..),
                offsets[0] as usize,
                server.offset_encoding,
            )
            .unwrap();
            let end = lsp_character_to_byte_offset(
                label.slice(..),
                offsets[1] as usize,
                server.offset_encoding,
            )
            .unwrap();
            Some([begin, end])
        }
        None => None,
    };

    let mut contents = active_signature.label.clone();
    if let Some(range) = parameter_range {
        if range[0] >= contents.len() || range[1] >= contents.len() {
            warn!(meta.session, "invalid range for active parameter");
        } else {
            let (left, tail) = contents.split_at(range[0]);
            let (param, right) = tail.split_at(range[1] - range[0]);
            contents = escape_kakoune_markup(left)
                + "{+b}"
                + &escape_kakoune_markup(param)
                + "{}"
                + &escape_kakoune_markup(right)
        }
    };

    let command = format!(
        "lsp-show-signature-help {} {}",
        params.position,
        editor_quote(&contents)
    );
    ctx.exec(meta, command);
}
