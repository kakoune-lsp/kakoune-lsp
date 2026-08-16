use crate::capabilities::{
    attempt_server_capability, server_has_capability, CAPABILITY_PULL_DIAGNOSTIC,
};
use crate::context::*;
use crate::markup::escape_kakoune_markup;
use crate::position::*;
use crate::types::*;
use crate::util::uri_to_file_path;
use crate::util::*;
use itertools::EitherOrBoth;
use itertools::Itertools;
use jsonrpc_core::Params;
use lsp_types::request::DocumentDiagnosticRequest;
use lsp_types::*;
use std::collections::HashMap;
use std::fmt::Write as _;

fn diagnostic_provider_identifier(server: &ServerSettings) -> Option<String> {
    let caps = server.capabilities.as_ref()?;
    match caps.diagnostic_provider.as_ref()? {
        DiagnosticServerCapabilities::Options(options) => options.identifier.clone(),
        DiagnosticServerCapabilities::RegistrationOptions(options) => {
            options.diagnostic_options.identifier.clone()
        }
    }
}

pub fn server_supports_pull_diagnostics(
    to_editor: &impl ToEditor,
    server: &ServerSettings,
) -> bool {
    server_has_capability(to_editor, server, CAPABILITY_PULL_DIAGNOSTIC)
}

/// Request document diagnostics via the LSP pull model for servers that advertise
/// `diagnosticProvider`.
pub fn pull_document_diagnostics(meta: EditorMeta, ctx: &mut Context) {
    let eligible_servers: Vec<_> = ctx
        .servers(&meta)
        .filter(|srv| attempt_server_capability(ctx, *srv, &meta, CAPABILITY_PULL_DIAGNOSTIC))
        .map(|(server_id, server)| {
            let identifier = diagnostic_provider_identifier(server);
            let previous_result_id = ctx
                .diagnostic_pull_result_ids
                .get(&(server_id, meta.buffile.clone()))
                .cloned();
            (
                server_id,
                vec![DocumentDiagnosticParams {
                    text_document: TextDocumentIdentifier {
                        uri: file_path_to_uri(&meta.buffile),
                    },
                    identifier,
                    previous_result_id,
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                }],
            )
        })
        .collect();
    if eligible_servers.is_empty() {
        return;
    }

    let buffile = meta.buffile.clone();
    let version = meta.version;
    ctx.call::<DocumentDiagnosticRequest, _>(
        meta,
        RequestParams::Each(eligible_servers.into_iter().collect()),
        move |ctx, meta, results| {
            // Drop stale responses after the buffer changed again.
            if ctx
                .documents
                .get(&buffile)
                .map(|doc| doc.version)
                .unwrap_or(-1)
                != version
            {
                return;
            }
            for (server_id, result) in results {
                handle_document_diagnostic_result(server_id, &buffile, result, ctx);
            }
            let _ = meta;
        },
    );
}

fn handle_document_diagnostic_result(
    server_id: ServerId,
    buffile: &str,
    result: DocumentDiagnosticReportResult,
    ctx: &mut Context,
) {
    let report = match result {
        DocumentDiagnosticReportResult::Report(report) => report,
        DocumentDiagnosticReportResult::Partial(_) => return,
    };

    match report {
        DocumentDiagnosticReport::Full(report) => {
            if let Some(result_id) = &report.full_document_diagnostic_report.result_id {
                ctx.diagnostic_pull_result_ids
                    .insert((server_id, buffile.to_string()), result_id.clone());
            }
            set_server_diagnostics(
                server_id,
                buffile,
                report.full_document_diagnostic_report.items,
                ctx,
            );
            if let Some(related) = report.related_documents {
                for (uri, related_report) in related {
                    handle_related_document_report(server_id, uri, related_report, ctx);
                }
            }
        }
        DocumentDiagnosticReport::Unchanged(report) => {
            ctx.diagnostic_pull_result_ids.insert(
                (server_id, buffile.to_string()),
                report
                    .unchanged_document_diagnostic_report
                    .result_id
                    .clone(),
            );
            if let Some(related) = report.related_documents {
                for (uri, related_report) in related {
                    handle_related_document_report(server_id, uri, related_report, ctx);
                }
            }
        }
    }
}

fn handle_related_document_report(
    server_id: ServerId,
    uri: Uri,
    report: DocumentDiagnosticReportKind,
    ctx: &mut Context,
) {
    let path = uri_to_file_path(&uri);
    let path = path.to_str().unwrap();
    match report {
        DocumentDiagnosticReportKind::Full(report) => {
            if let Some(result_id) = &report.result_id {
                ctx.diagnostic_pull_result_ids
                    .insert((server_id, path.to_string()), result_id.clone());
            }
            set_server_diagnostics(server_id, path, report.items, ctx);
        }
        DocumentDiagnosticReportKind::Unchanged(report) => {
            ctx.diagnostic_pull_result_ids
                .insert((server_id, path.to_string()), report.result_id);
        }
    }
}

fn set_server_diagnostics(
    server_id: ServerId,
    buffile: &str,
    items: Vec<Diagnostic>,
    ctx: &mut Context,
) {
    let mut diagnostics: Vec<_> = ctx
        .diagnostics
        .remove(buffile)
        .unwrap_or_default()
        .into_iter()
        .filter(|(id, _)| id != &server_id)
        .collect();
    diagnostics.extend(items.into_iter().map(|d| (server_id, d)));
    ctx.diagnostics.insert(buffile.to_string(), diagnostics);
    refresh_diagnostics_display(buffile, ctx);
}

pub fn publish_diagnostics(server_id: ServerId, params: Params, ctx: &mut Context) {
    // Prefer the pull model when the server advertises diagnosticProvider.
    if server_supports_pull_diagnostics(ctx.to_editor(), ctx.server(server_id)) {
        return;
    }
    let params: PublishDiagnosticsParams = params.parse().expect("Failed to parse params");
    let buffile = uri_to_file_path(&params.uri);
    let buffile = buffile.to_str().unwrap();
    set_server_diagnostics(server_id, buffile, params.diagnostics, ctx);
}

pub fn refresh_diagnostics_display(buffile: &str, ctx: &mut Context) {
    let document = match ctx.documents.get(buffile) {
        Some(document) => document,
        None => return,
    };
    let version = document.version;
    let diagnostics = &ctx.diagnostics[buffile];
    let diagnostics_orderd_by_severity = diagnostics
        .iter()
        .sorted_unstable_by_key(|(_, x)| x.severity)
        .rev();
    let inline_diagnostics = diagnostics_orderd_by_severity
        .clone()
        .map(|(server_id, x)| {
            let server = ctx.server(*server_id);
            format!(
                "{}|{}",
                ForwardKakouneRange(lsp_range_to_kakoune(
                    &x.range,
                    &document.text,
                    server.offset_encoding
                )),
                match x.severity {
                    Some(DiagnosticSeverity::ERROR) => "DiagnosticError",
                    Some(DiagnosticSeverity::HINT) => "DiagnosticHint",
                    Some(DiagnosticSeverity::INFORMATION) => "DiagnosticInfo",
                    Some(DiagnosticSeverity::WARNING) | None => "DiagnosticWarning",
                    Some(_) => {
                        warn!(
                            ctx.to_editor(),
                            "Unexpected DiagnosticSeverity: {:?}", x.severity
                        );
                        "DiagnosticWarning"
                    }
                }
            )
        })
        .join(" ");
    let tagged_diagnostics = |tag, tag_face| {
        diagnostics_orderd_by_severity
            .clone()
            .filter_map(|(server_id, x)| {
                let server = ctx.server(*server_id);
                if x.tags.as_ref().is_some_and(|tags| tags.contains(&tag)) {
                    Some(format!(
                        "{}|{}",
                        ForwardKakouneRange(lsp_range_to_kakoune(
                            &x.range,
                            &document.text,
                            server.offset_encoding
                        )),
                        tag_face
                    ))
                } else {
                    None
                }
            })
            .join(" ")
    };
    let inline_diagnostics_deprecated =
        tagged_diagnostics(DiagnosticTag::DEPRECATED, "DiagnosticTagDeprecated");
    let inline_diagnostics_unnecessary =
        tagged_diagnostics(DiagnosticTag::UNNECESSARY, "DiagnosticTagUnnecessary");

    // Assemble a list of diagnostics by line number
    let mut lines_with_diagnostics = HashMap::new();
    for (server_id, diagnostic) in diagnostics {
        let face = match diagnostic.severity {
            Some(DiagnosticSeverity::ERROR) => "InlayDiagnosticError",
            Some(DiagnosticSeverity::HINT) => "InlayDiagnosticHint",
            Some(DiagnosticSeverity::INFORMATION) => "InlayDiagnosticInfo",
            Some(DiagnosticSeverity::WARNING) | None => "InlayDiagnosticWarning",
            Some(_) => {
                warn!(
                    ctx.to_editor(),
                    "Unexpected DiagnosticSeverity: {:?}", diagnostic.severity
                );
                "InlayDiagnosticWarning"
            }
        };
        let (_, line_diagnostics) = lines_with_diagnostics
            .entry(diagnostic.range.end.line)
            .or_insert((
                server_id,
                LineDiagnostics {
                    range_end: diagnostic.range.end,
                    symbols: String::new(),
                    text: "",
                    text_face: "",
                    text_severity: None,
                },
            ));

        let severity = diagnostic.severity.unwrap_or(DiagnosticSeverity::WARNING);
        if line_diagnostics
            .text_severity
            // Smaller == higher severity
            .map_or(true, |text_severity| severity < text_severity)
        {
            let first_line = diagnostic.message.split('\n').next().unwrap_or_default();
            line_diagnostics.text = first_line;
            line_diagnostics.text_face = face;
            line_diagnostics.text_severity = diagnostic.severity;
        }

        let _ = write!(
            line_diagnostics.symbols,
            "{{{}}}%opt[lsp_inlay_diagnostic_sign]",
            face
        );
    }

    // Assemble ranges based on the lines
    let inlay_diagnostics = lines_with_diagnostics
        .iter()
        .map(|(_, (server_id, line_diagnostics))| {
            let server = ctx.server(**server_id);
            let pos = lsp_position_to_kakoune(
                &line_diagnostics.range_end,
                &document.text,
                server.offset_encoding,
            );

            format!(
                "\"{}|%opt[lsp_inlay_diagnostic_gap]{} {{{}}}{}\"",
                pos.line,
                line_diagnostics.symbols,
                line_diagnostics.text_face,
                editor_escape_double_quotes(&escape_tuple_element(&escape_kakoune_markup(
                    line_diagnostics.text
                )))
            )
        })
        .join(" ");

    let (line_flags, error_count, hint_count, info_count, warning_count) =
        gather_line_flags(ctx, buffile);

    // Always show a space on line one if no other highlighter is there,
    // to make sure the column always has the right width
    // Also wrap line_flags in another eval and quotes, to make sure the %opt[] tags are expanded
    let command = format!(
        "set-option buffer lsp_diagnostic_error_count {error_count}; \
         set-option buffer lsp_diagnostic_hint_count {hint_count}; \
         set-option buffer lsp_diagnostic_info_count {info_count}; \
         set-option buffer lsp_diagnostic_warning_count {warning_count}; \
         set-option buffer lsp_inline_diagnostics {version} {inline_diagnostics}; \
         set-option buffer lsp_inline_diagnostics_deprecated {version} {inline_diagnostics_deprecated}; \
         set-option buffer lsp_inline_diagnostics_unnecessary {version} {inline_diagnostics_unnecessary}; \
         evaluate-commands \"set-option buffer lsp_diagnostic_lines {version} {line_flags} '0|%opt[lsp_diagnostic_line_error_sign]'\"; \
         set-option buffer lsp_inlay_diagnostics {version} {inlay_diagnostics}"
    );
    let command = format!(
        "evaluate-commands -buffer {} %§{}§",
        editor_quote(buffile),
        command.replace('§', "§§")
    );
    ctx.exec(EditorMeta::default(), command);
}

pub fn gather_line_flags(ctx: &Context, buffile: &str) -> (String, u32, u32, u32, u32) {
    let diagnostics = ctx.diagnostics.get(buffile);
    let mut error_count: u32 = 0;
    let mut warning_count: u32 = 0;
    let mut info_count: u32 = 0;
    let mut hint_count: u32 = 0;

    let empty = vec![];
    let lenses = ctx
        .code_lenses
        .get(buffile)
        .unwrap_or(&empty)
        .iter()
        .map(|(_, lens)| (lens.range.start.line, "%opt[lsp_code_lens_sign]"));

    let empty = vec![];
    let diagnostics = diagnostics.unwrap_or(&empty).iter().map(|(_, x)| {
        (
            x.range.start.line,
            match x.severity {
                Some(DiagnosticSeverity::ERROR) => {
                    error_count += 1;
                    "{LineFlagError}%opt[lsp_diagnostic_line_error_sign]"
                }
                Some(DiagnosticSeverity::HINT) => {
                    hint_count += 1;
                    "{LineFlagHint}%opt[lsp_diagnostic_line_hint_sign]"
                }
                Some(DiagnosticSeverity::INFORMATION) => {
                    info_count += 1;
                    "{LineFlagInfo}%opt[lsp_diagnostic_line_info_sign]"
                }
                Some(DiagnosticSeverity::WARNING) | None => {
                    warning_count += 1;
                    "{LineFlagWarning}%opt[lsp_diagnostic_line_warning_sign]"
                }
                Some(_) => {
                    warn!(
                        ctx.to_editor(),
                        "Unexpected DiagnosticSeverity: {:?}", x.severity
                    );
                    ""
                }
            },
        )
    });

    let line_flags = diagnostics
        .merge_join_by(lenses, |left, right| left.0.cmp(&right.0))
        .map(|r| match r {
            EitherOrBoth::Left((line, diagnostic_label)) => (line, diagnostic_label),
            EitherOrBoth::Right((line, lens_label)) => (line, lens_label),
            EitherOrBoth::Both((line, diagnostic_label), _) => (line, diagnostic_label),
        })
        .map(|(line, label)| format!("'{}|{}'", line + 1, label))
        .join(" ");

    (
        line_flags,
        error_count,
        hint_count,
        info_count,
        warning_count,
    )
}

// Currently renders everything but range, severity, tags and related information
pub fn diagnostic_text(
    to_editor: &impl ToEditor,
    d: &Diagnostic,
    server_name: Option<&str>,
    for_hover: bool,
) -> String {
    let is_multiline = for_hover;
    let mut text = String::new();
    let severity = match d.severity {
        Some(DiagnosticSeverity::ERROR) => "error",
        Some(DiagnosticSeverity::HINT) => "hint",
        Some(DiagnosticSeverity::INFORMATION) => "info",
        Some(DiagnosticSeverity::WARNING) | None => "warning",
        Some(_) => {
            warn!(to_editor, "Unexpected DiagnosticSeverity: {:?}", d.severity);
            "warning"
        }
    };
    text.push('[');
    text.push_str(severity);
    text.push(']');
    if let Some(server_name) = &server_name {
        text.push('[');
        text.push_str(server_name);
        text.push(']');
    }
    if let Some(source) = &d.source {
        text.push('[');
        text.push_str(source);
        text.push(']');
    }
    if let Some(code) = &d.code {
        text.push('[');
        match code {
            NumberOrString::Number(code) => write!(&mut text, "{}", code).unwrap(),
            NumberOrString::String(code) => text.push_str(code),
        };
        text.push(']');
    }
    if is_multiline {
        text.push('\n');
    } else if !text.is_empty() {
        text.push(' ');
    }
    text.push_str(d.message.trim());
    // Typically a long URL, so put it last and possibly on a separate line.
    if let Some(code_description) = &d.code_description {
        text.push_str(if is_multiline { "\n" } else { " -- " });
        text.push_str("Description: ");
        text.push_str(code_description.href.as_str());
    }
    if !is_multiline {
        text.replace("\n", "␊")
    } else {
        text
    }
}

pub fn editor_diagnostics(meta: EditorMeta, params: PositionParams, ctx: &mut Context) {
    let mut goto_buffer_line = None;
    let mut line = 1;
    let content = ctx
        .diagnostics
        .iter()
        .flat_map(|(filename, diagnostics)| {
            diagnostics
                .iter()
                .map(|(server_id, x)| {
                    let server_id = *server_id;
                    let server = ctx.server(server_id);
                    let p = match get_kakoune_position(server, filename, &x.range.start, ctx) {
                        Some(position) => position,
                        None => {
                            line += 1;
                            warn!(
                                ctx.to_editor(),
                                "Cannot get position from file {}", filename
                            );
                            return "".to_string();
                        }
                    };
                    if filename == &meta.buffile
                        && (goto_buffer_line.is_none() || p <= params.position)
                    {
                        goto_buffer_line = Some(line);
                    }
                    let mut entry = format!(
                        "{}:{}:{}: {}{}",
                        short_file_path(filename, ctx.main_root(&meta)),
                        p.line,
                        p.column,
                        diagnostic_text(
                            ctx.to_editor(),
                            x,
                            (diagnostics.len() > 1).then_some(&server.name),
                            false,
                        ),
                        format_related_information(x, server, diagnostics.len() > 1, &meta, ctx)
                            .unwrap_or_default()
                    );
                    if entry.contains('\n') {
                        entry.push('\n');
                    }
                    line += 1 + entry.chars().filter(|&c| c == '\n').count();
                    entry
                })
                .collect::<Vec<_>>()
        })
        .join("\n");
    let command = format!(
        "lsp-show-goto-buffer *diagnostics* lsp-diagnostics {} {} {}",
        editor_quote(ctx.main_root(&meta)),
        editor_quote(&content),
        goto_buffer_line.unwrap_or(1)
    );
    ctx.exec(meta, command);
}

pub fn format_related_information(
    d: &Diagnostic,
    server: &ServerSettings,
    label_with_server: bool,
    meta: &EditorMeta,
    ctx: &Context,
) -> Option<String> {
    d.related_information
        .as_ref()
        .filter(|infos| !infos.is_empty())
        .map(|infos| {
            "\n".to_string()
                + &infos
                    .iter()
                    .map(|info| {
                        let filename = uri_to_file_path(&info.location.uri);
                        let filename = filename.to_str().unwrap();
                        let p = get_kakoune_position_with_fallback(
                            server,
                            filename,
                            info.location.range.start,
                            ctx,
                        );
                        format!(
                            "{}:{}:{}: {}{}",
                            short_file_path(filename, ctx.main_root(meta)),
                            p.line,
                            p.column,
                            &if label_with_server {
                                format!("[{}] ", &server.name)
                            } else {
                                "".to_string()
                            },
                            info.message
                        )
                    })
                    .join("\n")
        })
}
