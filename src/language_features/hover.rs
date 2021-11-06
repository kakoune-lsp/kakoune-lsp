use crate::context::*;
use crate::markup::*;
use crate::position::*;
use crate::types::*;
use indoc::formatdoc;
use itertools::Itertools;
use lsp_types::request::*;
use lsp_types::*;
use serde::Deserialize;
use url::Url;

pub fn text_document_hover(meta: EditorMeta, editor_params: EditorParams, ctx: &mut Context) {
    let params = PositionParams::deserialize(editor_params.clone()).unwrap();
    let hover_modal_params = HoverModalParams::deserialize(editor_params).unwrap();
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
        editor_hover(meta, hover_modal_params.hover_modal, params, result, ctx)
    });
}

pub fn editor_hover(
    meta: EditorMeta,
    maybe_hover_modal: Option<HoverModal>,
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
                .filter_map(|x| {
                    let face = x
                        .severity
                        .map(|sev| match sev {
                            DiagnosticSeverity::ERROR => FACE_INFO_DIAGNOSTIC_ERROR,
                            DiagnosticSeverity::WARNING => FACE_INFO_DIAGNOSTIC_WARNING,
                            DiagnosticSeverity::INFORMATION => FACE_INFO_DIAGNOSTIC_INFO,
                            DiagnosticSeverity::HINT => FACE_INFO_DIAGNOSTIC_HINT,
                            _ => {
                                warn!("Unexpected DiagnosticSeverity: {:?}", sev);
                                FACE_INFO_DEFAULT
                            }
                        })
                        .unwrap_or(FACE_INFO_DEFAULT);

                    if !x.message.is_empty() {
                        Some(format!(
                            "• {{{}}}{}{{{}}}",
                            face,
                            escape_kakoune_markup(x.message.trim())
                                // Indent line breaks to the same level as the bullet point
                                .replace("\n", "\n  "),
                            FACE_INFO_DEFAULT,
                        ))
                    } else {
                        None
                    }
                })
                .join("\n")
        })
        .unwrap_or_else(String::new);

    let force_plaintext = ctx
        .config
        .language
        .get(&ctx.language_id)
        .and_then(|l| l.workaround_server_sends_plaintext_labeled_as_markdown)
        .unwrap_or(false);
    let mut contents = match result {
        None => "".to_string(),
        Some(result) => match result.contents {
            HoverContents::Scalar(contents) => {
                marked_string_to_kakoune_markup(contents, force_plaintext)
            }
            HoverContents::Array(contents) => contents
                .into_iter()
                .map(|md| marked_string_to_kakoune_markup(md, force_plaintext))
                .filter(|markup| !markup.is_empty())
                .join(&format!(
                    "\n{{{}}}---{{{}}}\n",
                    FACE_INFO_RULE, FACE_INFO_DEFAULT
                )),
            HoverContents::Markup(contents) => match contents.kind {
                MarkupKind::Markdown => markdown_to_kakoune_markup(contents.value, force_plaintext),
                MarkupKind::PlainText => contents.value,
            },
        },
    };

    if contents.is_empty() && diagnostics.is_empty() && maybe_hover_modal.is_none() {
        return;
    }

    let anchor_or_style = if let Some(hover_modal) = maybe_hover_modal.as_ref() {
        contents = format!("{}\n---\n{}", hover_modal.context, contents);
        "modal".to_string()
    } else {
        format!("{}", params.position)
    };

    let mut command = format!(
        "lsp-show-hover {} %§{}§ %§{}§",
        anchor_or_style,
        contents.replace("§", "§§"),
        diagnostics.replace("§", "§§")
    );

    // Wrap if we're using a HoverModal
    if let Some(hover_modal) = maybe_hover_modal {
        command = formatdoc!(
            "eval %§
                 {}
                 {}
             §",
            command.replace("§", "§§"),
            hover_modal.do_after.replace("§", "§§")
        );
    }

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
