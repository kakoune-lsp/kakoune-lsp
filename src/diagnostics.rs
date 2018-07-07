use context::*;
use languageserver_types::*;
use std::path::Path;
use types::*;

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
    let ranges = ctx.diagnostics
        .get(buffile)
        .unwrap()
        .iter()
        .map(|x| {
            // LSP ranges are 0-based, but Kakoune's 1-based.
            // LSP ranges are exclusive, but Kakoune's are inclusive.
            // Also from LSP spec: If you want to specify a range that contains a line including
            // the line ending character(s) then use an end position denoting the start of the next
            // line.
            let mut end_line = x.range.end.line;
            let mut end_char = x.range.end.character;
            if end_char > 0 {
                end_line += 1;
            } else {
                end_char = 1_000_000;
            }
            format!(
                "{}.{},{}.{}|{}",
                x.range.start.line + 1,
                x.range.start.character + 1,
                end_line,
                end_char,
                match x.severity {
                    Some(DiagnosticSeverity::Error) => "DiagnosticError",
                    _ => "DiagnosticWarning",
                }
            )
        })
        .collect::<Vec<String>>()
        .join(":");
    let command = format!(
        "eval -buffer %§{}§ %§set buffer lsp_errors {} {}§",
        buffile, version, ranges
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
    let content = ctx.diagnostics
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
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let command = format!(
        "lsp-show-diagnostics %§{}§ %§{}§",
        ctx.root_path, content,
    );
    ctx.exec(meta.clone(), command);
}
