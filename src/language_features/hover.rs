use std::fs;

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

pub fn text_document_hover(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let maybe_hover_fifo = HoverDetails::deserialize(params.clone())
        .unwrap()
        .hover_fifo;

    let hover_type = match maybe_hover_fifo {
        Some(fifo) => HoverType::InfoInHoverClient { fifo },
        None => HoverType::Normal,
    };

    let params = PositionParams::deserialize(params).unwrap();
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
        editor_hover(meta, hover_type, params, result, ctx)
    });
}

pub fn editor_hover(
    meta: EditorMeta,
    hover_type: HoverType,
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
    let contents = match result {
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
                MarkupKind::Markdown => {
                    if let HoverType::InfoInHoverClient { .. } = hover_type {
                        contents.value
                    } else {
                        markdown_to_kakoune_markup(contents.value, force_plaintext)
                    }
                }
                MarkupKind::PlainText => contents.value,
            },
        },
    };

    match hover_type {
        HoverType::Normal => {
            if contents.is_empty() && diagnostics.is_empty() {
                return;
            }

            let command = format!(
                "lsp-show-hover {} %§{}§ %§{}§",
                params.position,
                contents.replace("§", "§§"),
                diagnostics.replace("§", "§§"),
            );
            ctx.exec(meta, command);
        }
        HoverType::Modal {
            modal_heading,
            do_after,
        } => {
            show_hover_modal(meta, ctx, modal_heading, do_after, contents, diagnostics);
        }
        HoverType::InfoInHoverClient { fifo } => {
            show_hover_in_hover_client(meta, ctx, fifo, contents);
        }
    };
}

fn show_hover_modal(
    meta: EditorMeta,
    ctx: &Context,
    modal_heading: String,
    do_after: String,
    contents: String,
    diagnostics: String,
) {
    let contents = format!("{}\n---\n{}", modal_heading, contents);
    let command = format!(
        "lsp-show-hover modal %§{}§ %§{}§",
        contents.replace("§", "§§"),
        diagnostics.replace("§", "§§"),
    );
    let wrapped_command = formatdoc!(
        "eval %§
                 {}
                 {}
             §",
        command.replace("§", "§§"),
        do_after.replace("§", "§§")
    );
    ctx.exec(meta, wrapped_command);
}

fn show_hover_in_hover_client(
    meta: EditorMeta,
    ctx: &Context,
    hover_fifo: String,
    contents: String,
) {
    if contents.is_empty() {
        return;
    }

    let handle = std::thread::spawn(move || {
        // Note that we don't show diagnostics in the hover client
        fs::write(hover_fifo, contents.as_bytes()).unwrap();
    });

    let command = formatdoc!(
        "
        %[
            edit! -existing -readonly -fifo %opt[lsp_hover_fifo] *hover*
            set buffer=*hover* filetype markdown
        ]"
    );

    let client = meta.client.clone().unwrap_or_default();
    let command = formatdoc!(
        "
        try %[
            eval -client hoverclient {}
        ] catch %[
            new %[
                rename-client hoverclient
                eval {}
                addhl -override window/wrap wrap
                focus {}
            ]
        ]",
        command,
        command,
        client
    );

    ctx.exec(meta, command);
    handle.join().unwrap();
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
