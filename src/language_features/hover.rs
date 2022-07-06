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
    let hover_supported = ctx
        .capabilities
        .as_ref()
        .map(|caps| {
            matches!(
                caps.hover_provider,
                Some(HoverProviderCapability::Simple(true) | HoverProviderCapability::Options(_))
            )
        })
        .unwrap_or(false);
    if !hover_supported && meta.fifo.is_none() {
        return;
    }

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

    let params = MainSelectionParams::deserialize(params).unwrap();
    let (range, cursor) = parse_kakoune_range(&params.selection_desc);
    let req_params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::from_file_path(&meta.buffile).unwrap(),
            },
            position: get_lsp_position(&meta.buffile, &cursor, ctx).unwrap(),
        },
        work_done_progress_params: Default::default(),
    };
    ctx.call::<HoverRequest, _>(meta, req_params, move |ctx: &mut Context, meta, result| {
        editor_hover(meta, hover_type, cursor, range, result, ctx)
    });
}

pub fn editor_hover(
    meta: EditorMeta,
    hover_type: HoverType,
    cursor: KakounePosition,
    range: KakouneRange,
    result: Option<Hover>,
    ctx: &mut Context,
) {
    let doc = &ctx.documents[&meta.buffile];
    let lsp_range = kakoune_range_to_lsp(&range, &doc.text, ctx.offset_encoding);
    let for_hover_buffer = matches!(hover_type, HoverType::HoverBuffer { .. });
    let diagnostics = ctx.diagnostics.get(&meta.buffile);
    let diagnostics = diagnostics
        .map(|x| {
            x.iter()
                .filter(|x| ranges_touch_same_line(x.range, lsp_range))
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

    let code_lenses = ctx
        .code_lenses
        .get(&meta.buffile)
        .map(|lenses| {
            lenses
                .iter()
                .filter(|lens| ranges_touch_same_line(lens.range, lsp_range))
                .map(|lens| {
                    lens.command
                        .as_ref()
                        .map(|cmd| cmd.title.as_str())
                        .unwrap_or("(unresolved)")
                })
                .map(|title| {
                    if for_hover_buffer {
                        // We are typically creating Markdown, so use a standard Markdown enumerator.
                        return format!("* {}", &title);
                    }
                    format!(
                        "• {{{}}}{}{{{}}}",
                        FACE_INFO_DIAGNOSTIC_HINT,
                        escape_kakoune_markup(title),
                        FACE_INFO_DEFAULT,
                    )
                })
                .join("\n")
        })
        .unwrap_or_default();

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
            if contents.is_empty() && diagnostics.is_empty() && code_lenses.is_empty() {
                return;
            }

            let command = format!(
                "lsp-show-hover {} %§{}§ %§{}§ %§{}§",
                cursor,
                contents.replace('§', "§§"),
                diagnostics.replace('§', "§§"),
                code_lenses.replace('§', "§§"),
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
        "lsp-show-hover modal %§{}§ %§{}§ ''",
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
