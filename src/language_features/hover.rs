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
    let HoverDetails {
        hover_fifo: maybe_hover_fifo,
        hover_client: maybe_hover_client,
    } = HoverDetails::deserialize(params.clone()).unwrap();

    let hover_type = match maybe_hover_fifo {
        Some(fifo) => HoverType::HoverBuffer {
            fifo,
            client: maybe_hover_client.unwrap(),
        },
        None => HoverType::InfoBox,
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
    let for_hover_buffer = matches!(hover_type, HoverType::HoverBuffer { .. });
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
                    if for_hover_buffer {
                        // We are typically creating Markdown, so use a standard Markdown enumerator.
                        return Some(format!("* {}", x.message.trim().replace('\n', "\n  ")));
                    }

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
                                .replace('\n', "\n  "),
                            FACE_INFO_DEFAULT,
                        ))
                    } else {
                        None
                    }
                })
                .join("\n")
        })
        .unwrap_or_else(String::new);

    let marked_string_to_hover = |ms: MarkedString| {
        if for_hover_buffer {
            match ms {
                MarkedString::String(markdown) => markdown,
                MarkedString::LanguageString(LanguageString { language, value }) => formatdoc!(
                    "```{}
                     {}
                     ```",
                    &language,
                    &value,
                ),
            }
        } else {
            marked_string_to_kakoune_markup(ms)
        }
    };

    let (is_markdown, contents) = match result {
        None => (false, "".to_string()),
        Some(result) => match result.contents {
            HoverContents::Scalar(contents) => (true, marked_string_to_hover(contents)),
            HoverContents::Array(contents) => (
                true,
                contents
                    .into_iter()
                    .map(marked_string_to_hover)
                    .filter(|markup| !markup.is_empty())
                    .join(&if for_hover_buffer {
                        "\n---\n".to_string()
                    } else {
                        format!("\n{{{}}}---{{{}}}\n", FACE_INFO_RULE, FACE_INFO_DEFAULT)
                    }),
            ),
            HoverContents::Markup(contents) => match contents.kind {
                MarkupKind::Markdown => (
                    true,
                    if for_hover_buffer {
                        contents.value
                    } else {
                        markdown_to_kakoune_markup(contents.value)
                    },
                ),
                MarkupKind::PlainText => (false, contents.value),
            },
        },
    };

    match hover_type {
        HoverType::InfoBox => {
            if contents.is_empty() && diagnostics.is_empty() {
                return;
            }

            let command = format!(
                "lsp-show-hover {} %§{}§ %§{}§",
                params.position,
                contents.replace('§', "§§"),
                diagnostics.replace('§', "§§"),
            );
            ctx.exec(meta, command);
        }
        HoverType::Modal {
            modal_heading,
            do_after,
        } => {
            show_hover_modal(meta, ctx, modal_heading, do_after, contents, diagnostics);
        }
        HoverType::HoverBuffer { fifo, client } => {
            if contents.is_empty() && diagnostics.is_empty() {
                return;
            }

            show_hover_in_hover_client(meta, ctx, fifo, client, is_markdown, contents, diagnostics);
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
        contents.replace('§', "§§"),
        diagnostics.replace('§', "§§"),
    );
    let command = formatdoc!(
        "evaluate-commands %§
             {}
             {}
         §",
        command.replace('§', "§§"),
        do_after.replace('§', "§§")
    );
    ctx.exec(meta, command);
}

fn show_hover_in_hover_client(
    meta: EditorMeta,
    ctx: &Context,
    hover_fifo: String,
    hover_client: String,
    is_markdown: bool,
    contents: String,
    diagnostics: String,
) {
    let handle = std::thread::spawn(move || {
        let contents = if diagnostics.is_empty() {
            contents
        } else {
            formatdoc!(
                "{}

                 ## Diagnostics
                 {}",
                contents,
                diagnostics,
            )
        };
        fs::write(hover_fifo, contents.as_bytes()).unwrap();
    });

    let command = format!(
        "%[ edit! -existing -fifo %opt[lsp_hover_fifo] *hover*; \
             set-option buffer=*hover* filetype {}; \
             try %[ add-highlighter buffer/lsp_wrap wrap -word ] \
         ]",
        if is_markdown { "markdown" } else { "''" },
    );

    let command = formatdoc!(
        "try %[
             evaluate-commands -client {} {}
         ] catch %[
             new %[
                 rename-client {}
                 evaluate-commands {}
                 focus {}
             ]
         ]",
        &hover_client,
        command,
        &hover_client,
        command,
        meta.client.as_ref().unwrap(),
    );

    ctx.exec(meta, command);
    handle.join().unwrap();
}
