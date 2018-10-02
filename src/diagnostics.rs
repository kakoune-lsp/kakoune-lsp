use context::*;
use languageserver_types::*;
use std::path::Path;
use types::*;
use util::*;

pub fn publish_diagnostics(params: PublishDiagnosticsParams, ctx: &mut Context) {
    let session = ctx.session.clone();
    let client = None;
    let path = params.uri.to_file_path().unwrap();
    let buffile = path.to_str().unwrap();
    ctx.diagnostics
        .insert(buffile.to_string(), params.diagnostics);
    let version = ctx.versions.get(buffile);
    if version.is_none() {
        return;
    }
    let version = *version.unwrap();
    let ranges = ctx
        .diagnostics
        .get(buffile)
        .unwrap()
        .iter()
        .map(|x| {
            format!(
                "{}|{}",
                lsp_range_to_kakoune(x.range),
                match x.severity {
                    Some(DiagnosticSeverity::Error) => "DiagnosticError",
                    _ => "DiagnosticWarning",
                }
            )
        }).collect::<Vec<String>>()
        .join(" ");

    let line_flags = ctx
        .diagnostics
        .get(buffile)
        .unwrap()
        .iter()
        .map(|x| {
            // See above
            format!(
                "{}|{}",
                x.range.start.line + 1,
                match x.severity {
                    Some(DiagnosticSeverity::Error) => "%opt[lsp_diagnostic_line_error_sign]",
                    _ => "%opt[lsp_diagnostic_line_warning_sign]",
                }
            )
        }).collect::<Vec<String>>()
        .join(" ");
    let command = format!(
        // Allways show a space on line one if no other highlighter is there,
        // to make sure the column always has the right width
        // Also wrap it in another eval and quotes, to make sure the %opt[] tags are expanded
        "eval -buffer %§{}§ %§set buffer lsp_errors {} {} ; eval \"set buffer lsp_error_lines {} {} '1| ' \" §",
        buffile, version, ranges, version, line_flags
    );
    let meta = EditorMeta {
        session,
        client,
        buffile: buffile.to_string(),
        version,
    };
    ctx.exec(meta, command.to_string());
}

pub fn editor_diagnostics(_params: EditorParams, meta: &EditorMeta, ctx: &mut Context) {
    let content = ctx
        .diagnostics
        .iter()
        .flat_map(|(filename, diagnostics)| {
            diagnostics
                .iter()
                .map(|x| {
                    format!(
                        "{}:{}:{}: {}:{}",
                        Path::new(filename)
                            .strip_prefix(&ctx.root_path)
                            .ok()
                            .and_then(|p| Some(p.to_str().unwrap()))
                            .or_else(|| Some(filename))
                            .unwrap(),
                        x.range.start.line + 1,
                        x.range.start.character + 1,
                        match x.severity {
                            Some(DiagnosticSeverity::Error) => "error",
                            _ => "warning",
                        },
                        x.message
                    )
                }).collect::<Vec<_>>()
        }).collect::<Vec<_>>()
        .join("\n");
    let command = format!(
        "lsp-show-diagnostics %§{}§ %§{}§",
        ctx.root_path, content,
    );
    ctx.exec(meta.clone(), command);
}
