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
use serde::Deserialize;
use url::Url;

pub fn text_document_signature_help(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    if meta.fifo.is_none() && !attempt_server_capability(ctx, CAPABILITY_SIGNATURE_HELP) {
        return;
    }

    let params = PositionParams::deserialize(params).unwrap();
    let req_params = SignatureHelpParams {
        context: None,
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::from_file_path(&meta.buffile).unwrap(),
            },
            position: get_lsp_position(&meta.buffile, &params.position, ctx).unwrap(),
        },
        work_done_progress_params: Default::default(),
    };
    ctx.call::<SignatureHelpRequest, _>(
        meta,
        req_params,
        move |ctx: &mut Context, meta, result| editor_signature_help(meta, params, result, ctx),
    );
}

fn editor_signature_help(
    meta: EditorMeta,
    params: PositionParams,
    result: Option<SignatureHelp>,
    ctx: &mut Context,
) {
    let result = match result {
        Some(result) => result,
        None => return,
    };

    let active_signature = result.active_signature.unwrap_or(0);

    let active_signature = match result.signatures.get(active_signature as usize) {
        Some(active_signature) => active_signature,
        None => return,
    };

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
            let begin = label.char_to_byte(offsets[0] as usize);
            let end = label.char_to_byte(offsets[1] as usize);
            Some([begin, end])
        }
        None => None,
    };

    let mut contents = active_signature.label.clone();
    if let Some(range) = parameter_range {
        if range[0] >= contents.len() || range[1] >= contents.len() {
            warn!("invalid range for active parameter");
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
