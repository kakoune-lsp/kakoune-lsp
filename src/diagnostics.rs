use crate::context::*;
use crate::markup::escape_kakoune_markup;
use crate::position::*;
use crate::types::*;
use crate::util::*;
use indoc::formatdoc;
use itertools::EitherOrBoth;
use itertools::Itertools;
use jsonrpc_core::Params;
use lsp_types::request::*;
use lsp_types::*;
use std::collections::HashMap;
use std::fmt::Write as _;

/// Store diagnostics for a specific file and server
fn store_diagnostics(
    server_id: ServerId,
    buffile: &str,
    new_diagnostics: Vec<Diagnostic>,
    ctx: &mut Context,
) {
    // Remove old diagnostics for this server
    let mut diagnostics: Vec<_> = ctx
        .diagnostics
        .remove(buffile)
        .unwrap_or_default()
        .into_iter()
        .filter(|(id, _)| id != &server_id)
        .collect();

    // Add new diagnostics
    let new_diagnostics: Vec<_> = new_diagnostics
        .into_iter()
        .map(|d| (server_id, d))
        .collect();
    diagnostics.extend(new_diagnostics);
    ctx.diagnostics.insert(buffile.to_string(), diagnostics);
}

pub fn publish_diagnostics(server_id: ServerId, params: Params, ctx: &mut Context) {
    let params: PublishDiagnosticsParams = params.parse().expect("Failed to parse params");
    let path = params.uri.to_file_path().unwrap();
    let buffile = path.to_str().unwrap();

    store_diagnostics(server_id, buffile, params.diagnostics, ctx);

    let document = ctx.documents.get(buffile);
    if document.is_none() {
        return;
    }
    let document = document.unwrap();
    let version = document.version;
    let diagnostics = &ctx.diagnostics[buffile];
    let diagnostics_ordered_by_severity = diagnostics
        .iter()
        .sorted_unstable_by_key(|(_, x)| x.severity)
        .rev();
    let inline_diagnostics = diagnostics_ordered_by_severity
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
        diagnostics_ordered_by_severity
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
    let command = formatdoc!(
        "lsp-show-goto-buffer *diagnostics* lsp-diagnostics {} {}
         lsp-initial-goto-position {}",
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
                        let path = info.location.uri.to_file_path().unwrap();
                        let filename = path.to_str().unwrap();
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

// Pull diagnostics support (LSP 3.17+)
pub fn text_document_diagnostic(meta: EditorMeta, ctx: &mut Context) {
    let req_params: HashMap<_, _> = ctx
        .servers(&meta)
        .filter_map(|(server_id, server)| {
            if !crate::capabilities::server_has_capability(
                ctx.to_editor(),
                server,
                crate::capabilities::CAPABILITY_DIAGNOSTIC,
            ) {
                return None;
            }

            let buffile = &meta.buffile;
            let uri = Url::from_file_path(buffile).unwrap();

            let params = DocumentDiagnosticParams {
                text_document: TextDocumentIdentifier { uri },
                identifier: None,
                previous_result_id: ctx
                    .diagnostic_result_ids
                    .get(&(server_id, buffile.clone()))
                    .cloned(),
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            };

            Some((server_id, vec![params]))
        })
        .collect();

    if req_params.is_empty() {
        return;
    }

    ctx.call::<DocumentDiagnosticRequest, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx, meta, results| {
            for (server_id, report) in results {
                handle_document_diagnostic_response(server_id, report, &meta, ctx);
            }
        },
    );
}

fn handle_document_diagnostic_response(
    server_id: ServerId,
    report: DocumentDiagnosticReportResult,
    meta: &EditorMeta,
    ctx: &mut Context,
) {
    let buffile = &meta.buffile;

    match report {
        DocumentDiagnosticReportResult::Report(report) => match report {
            DocumentDiagnosticReport::Full(full_report) => {
                let inner = &full_report.full_document_diagnostic_report;

                // Store the result ID for future requests
                if let Some(result_id) = &inner.result_id {
                    ctx.diagnostic_result_ids
                        .insert((server_id, buffile.clone()), result_id.clone());
                }

                store_diagnostics(server_id, buffile, inner.items.clone(), ctx);

                // Update the editor display
                let document = ctx.documents.get(buffile);
                if document.is_none() {
                    return;
                }
                let document = document.unwrap();
                let version = document.version;
                let diagnostics = &ctx.diagnostics[buffile];

                update_diagnostics_display(buffile, version, diagnostics, ctx, meta);
            }
            DocumentDiagnosticReport::Unchanged(unchanged_report) => {
                let inner = &unchanged_report.unchanged_document_diagnostic_report;
                // Diagnostics haven't changed, keep the result ID
                ctx.diagnostic_result_ids
                    .insert((server_id, buffile.clone()), inner.result_id.clone());
                // No need to update display since diagnostics are unchanged
            }
        },
        DocumentDiagnosticReportResult::Partial(_) => {
            warn!(
                ctx.to_editor(),
                "Partial diagnostic results are not supported"
            );
        }
    }
}

fn update_diagnostics_display(
    buffile: &str,
    version: i32,
    diagnostics: &[(ServerId, Diagnostic)],
    ctx: &Context,
    _meta: &EditorMeta,
) {
    let diagnostics_ordered_by_severity = diagnostics
        .iter()
        .sorted_unstable_by_key(|(_, x)| x.severity)
        .rev();

    let document = ctx.documents.get(buffile).unwrap();

    let inline_diagnostics = diagnostics_ordered_by_severity
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
        diagnostics_ordered_by_severity
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

// Handle workspace/diagnostics request (display in buffer)
// This shows all diagnostics from all files (that have been opened/checked)
// It's essentially the same as lsp-diagnostics but the name makes the intent clearer
pub fn editor_workspace_diagnostics(meta: EditorMeta, ctx: &mut Context) {
    // First try to request workspace diagnostics from servers that support it
    let has_workspace_support = ctx.servers(&meta).any(|(_, server)| {
        crate::capabilities::server_has_capability(
            ctx.to_editor(),
            server,
            crate::capabilities::CAPABILITY_WORKSPACE_DIAGNOSTICS,
        )
    });

    if has_workspace_support {
        // If server supports it, request from server and display when ready
        workspace_diagnostic_impl(meta, ctx, true);
    } else {
        // Otherwise, just show all cached diagnostics immediately
        editor_diagnostics(
            meta,
            PositionParams {
                position: KakounePosition { line: 1, column: 1 },
            },
            ctx,
        );
    }
}

fn workspace_diagnostic_impl(meta: EditorMeta, ctx: &mut Context, show_in_buffer: bool) {
    let req_params: HashMap<_, _> = ctx
        .servers(&meta)
        .filter_map(|(server_id, server)| {
            if !crate::capabilities::server_has_capability(
                ctx.to_editor(),
                server,
                crate::capabilities::CAPABILITY_WORKSPACE_DIAGNOSTICS,
            ) {
                return None;
            }

            // Collect all previous result IDs for this server
            let previous_result_ids: Vec<PreviousResultId> = ctx
                .diagnostic_result_ids
                .iter()
                .filter_map(|((sid, uri), result_id)| {
                    if *sid == server_id {
                        Some(PreviousResultId {
                            uri: Url::from_file_path(uri).ok()?,
                            value: result_id.clone(),
                        })
                    } else {
                        None
                    }
                })
                .collect();

            let params = WorkspaceDiagnosticParams {
                identifier: None,
                previous_result_ids,
                work_done_progress_params: WorkDoneProgressParams::default(),
                partial_result_params: PartialResultParams::default(),
            };

            Some((server_id, vec![params]))
        })
        .collect();

    if req_params.is_empty() {
        if show_in_buffer {
            let command = "info 'No language server supports workspace diagnostics'".to_string();
            ctx.exec(meta, command);
        }
        return;
    }

    ctx.call::<WorkspaceDiagnosticRequest, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx, meta, results| {
            for (server_id, report) in results {
                handle_workspace_diagnostic_response(server_id, report, &meta, ctx);
            }

            // Display results in buffer if requested
            if show_in_buffer {
                editor_diagnostics(
                    meta,
                    PositionParams {
                        position: KakounePosition { line: 1, column: 1 },
                    },
                    ctx,
                );
            }
        },
    );
}

fn handle_workspace_diagnostic_response(
    server_id: ServerId,
    report: WorkspaceDiagnosticReportResult,
    meta: &EditorMeta,
    ctx: &mut Context,
) {
    match report {
        WorkspaceDiagnosticReportResult::Report(report) => {
            // Process each document's diagnostics
            for item in report.items {
                match item {
                    WorkspaceDocumentDiagnosticReport::Full(full) => {
                        let uri_str = match full.uri.to_file_path() {
                            Ok(path) => match path.to_str() {
                                Some(s) => s.to_string(),
                                None => continue,
                            },
                            Err(_) => continue,
                        };

                        // Create a meta for this specific document
                        let doc_meta = EditorMeta {
                            buffile: uri_str.clone(),
                            version: full.version.unwrap_or(0) as i32,
                            ..meta.clone()
                        };

                        let inner = &full.full_document_diagnostic_report;
                        // Store the result ID
                        if let Some(result_id) = &inner.result_id {
                            ctx.diagnostic_result_ids
                                .insert((server_id, uri_str.clone()), result_id.clone());
                        }

                        store_diagnostics(server_id, &uri_str, inner.items.clone(), ctx);

                        // Update display if document is open
                        if let Some(document) = ctx.documents.get(&uri_str) {
                            let version = document.version;
                            let diagnostics = &ctx.diagnostics[&uri_str];
                            update_diagnostics_display(
                                &uri_str,
                                version,
                                diagnostics,
                                ctx,
                                &doc_meta,
                            );
                        }
                    }
                    WorkspaceDocumentDiagnosticReport::Unchanged(unchanged) => {
                        let uri_str = match unchanged.uri.to_file_path() {
                            Ok(path) => match path.to_str() {
                                Some(s) => s.to_string(),
                                None => continue,
                            },
                            Err(_) => continue,
                        };

                        let inner = &unchanged.unchanged_document_diagnostic_report;
                        // Update result ID
                        ctx.diagnostic_result_ids
                            .insert((server_id, uri_str.clone()), inner.result_id.clone());
                        // Diagnostics unchanged, no display update needed
                    }
                }
            }
        }
        WorkspaceDiagnosticReportResult::Partial(_) => {
            warn!(
                ctx.to_editor(),
                "Partial workspace diagnostic results are not supported"
            );
        }
    }
}

// Handle workspace/diagnostic/refresh notification from server
pub fn workspace_diagnostic_refresh(server_id: ServerId, _params: Params, ctx: &mut Context) {
    // When the server sends a workspace/diagnostic/refresh notification,
    // we should refresh diagnostics for all open documents.
    let server = ctx.server(server_id);
    debug!(
        ctx.to_editor(),
        "Received workspace/diagnostic/refresh from server: {}", server.name
    );

    // We request diagnostics for each currently open document individually
    let buffiles: Vec<String> = ctx.documents.keys().cloned().collect();

    for buffile in buffiles {
        let document = &ctx.documents[&buffile];
        let meta = EditorMeta {
            session: ctx.session.clone(),
            buffile: buffile.clone(),
            version: document.version,
            servers: vec![server_id],
            ..Default::default()
        };
        text_document_diagnostic(meta, ctx);
    }
}
