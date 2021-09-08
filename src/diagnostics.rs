use crate::context::*;
use crate::position::*;
use crate::types::*;
use crate::util::*;
use itertools::Itertools;
use jsonrpc_core::Params;
use lsp_types::*;
use std::collections::HashSet;

pub fn publish_diagnostics(params: Params, ctx: &mut Context) {
    let params: PublishDiagnosticsParams = params.parse().expect("Failed to parse params");
    let session = ctx.session.clone();
    let client = None;
    let path = params.uri.to_file_path().unwrap();
    let buffile = path.to_str().unwrap();
    ctx.diagnostics
        .insert(buffile.to_string(), params.diagnostics);
    let document = ctx.documents.get(buffile);
    if document.is_none() {
        return;
    }
    let document = document.unwrap();
    let version = document.version;
    let diagnostics = &ctx.diagnostics[buffile];
    let ranges = diagnostics
        .iter()
        .map(|x| {
            format!(
                "{}|{}",
                lsp_range_to_kakoune(&x.range, &document.text, ctx.offset_encoding),
                match x.severity {
                    Some(DiagnosticSeverity::Error) => "DiagnosticError",
                    Some(DiagnosticSeverity::Hint) => "DiagnosticHint",
                    Some(DiagnosticSeverity::Information) => "DiagnosticInfo",
                    Some(DiagnosticSeverity::Warning) | None => "DiagnosticWarning",
                }
            )
        })
        .join(" ");

    let mut error_count: u32 = 0;
    let mut warning_count: u32 = 0;
    let mut info_count: u32 = 0;
    let mut hint_count: u32 = 0;
    let line_flags = diagnostics
        .iter()
        .map(|x| {
            format!(
                "{}|{}",
                x.range.start.line + 1,
                match x.severity {
                    Some(DiagnosticSeverity::Error) => {
                        error_count += 1;
                        "{LineFlagError}%opt[lsp_diagnostic_line_error_sign]"
                    }
                    Some(DiagnosticSeverity::Hint) => {
                        hint_count += 1;
                        "{LineFlagHint}%opt[lsp_diagnostic_line_hint_sign]"
                    }
                    Some(DiagnosticSeverity::Information) => {
                        info_count += 1;
                        "{LineFlagInfo}%opt[lsp_diagnostic_line_info_sign]"
                    }
                    Some(DiagnosticSeverity::Warning) | None => {
                        warning_count += 1;
                        "{LineFlagWarning}%opt[lsp_diagnostic_line_warning_sign]"
                    }
                }
            )
        })
        .join(" ");
    let mut lines_with_errors = HashSet::new();
    let diagnostic_ranges = diagnostics
        .iter()
        .map(|x| {
            let face = match x.severity {
                Some(DiagnosticSeverity::Error) => "InlayDiagnosticError",
                Some(DiagnosticSeverity::Hint) => "InlayDiagnosticHint",
                Some(DiagnosticSeverity::Information) => "InlayDiagnosticInfo",
                Some(DiagnosticSeverity::Warning) | None => "InlayDiagnosticWarning",
            };
            // Pretend the language server sent us the diagnostic past the end of line
            let line = x.range.end.line;
            let line_text = get_line(line as usize, &document.text);
            let mut pos =
                lsp_position_to_kakoune(&x.range.end, &document.text, ctx.offset_encoding);
            pos.column = line_text.len_bytes() as u32;
            // separate all but the first diagnostic on the same line
            let sep = if lines_with_errors.insert(line) {
                ""
            } else {
                ", "
            };
            editor_quote(&format!(
                "{}+0|{{{}}}{{\\}}{} {}",
                pos,
                face,
                sep,
                x.message.replace("|", "\\|")
            ))
        })
        .join(" ");
    // Always show a space on line one if no other highlighter is there,
    // to make sure the column always has the right width
    // Also wrap line_flags in another eval and quotes, to make sure the %opt[] tags are expanded
    let command = format!(
        "set buffer lsp_diagnostic_error_count {}; \
         set buffer lsp_diagnostic_hint_count {}; \
         set buffer lsp_diagnostic_info_count {}; \
         set buffer lsp_diagnostic_warning_count {}; \
         set buffer lsp_errors {} {}; \
         eval \"set buffer lsp_error_lines {} {} '0| '\"; \
         set buffer lsp_diagnostics {} {}",
        error_count,
        hint_count,
        info_count,
        warning_count,
        version,
        ranges,
        version,
        line_flags,
        version,
        diagnostic_ranges,
    );
    let command = format!(
        "eval -buffer {} %§{}§",
        editor_quote(buffile),
        command.replace("§", "\\§")
    );
    let meta = EditorMeta {
        session,
        client,
        buffile: buffile.to_string(),
        filetype: "".to_string(), // filetype is not used by ctx.exec, but it's definitely a code smell
        version,
        fifo: None,
    };
    ctx.exec(meta, command);
}

pub fn editor_diagnostics(meta: EditorMeta, ctx: &mut Context) {
    let content = ctx
        .diagnostics
        .iter()
        .flat_map(|(filename, diagnostics)| {
            diagnostics
                .iter()
                .map(|x| {
                    let p = get_kakoune_position(filename, &x.range.start, ctx).unwrap();
                    format!(
                        "{}:{}:{}: {}:{}",
                        short_file_path(filename, &ctx.root_path),
                        p.line,
                        p.column,
                        match x.severity {
                            Some(DiagnosticSeverity::Error) => "error",
                            Some(DiagnosticSeverity::Hint) => "hint",
                            Some(DiagnosticSeverity::Information) => "info",
                            Some(DiagnosticSeverity::Warning) | None => "warning",
                        },
                        x.message
                    )
                })
                .collect::<Vec<_>>()
        })
        .join("\n");
    let command = format!(
        "lsp-show-diagnostics {} {}",
        editor_quote(&ctx.root_path),
        editor_quote(&content),
    );
    ctx.exec(meta, command);
}
