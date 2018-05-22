use context::*;
use languageserver_types::*;
use types::*;

pub fn publish_diagnostics(params: PublishDiagnosticsParams, ctx: &mut Context) {
    let session = ctx.session.clone();
    let client = None;
    let path = params.uri.to_file_path().unwrap();
    let buffile = path.to_str().unwrap();
    let version = ctx.versions.get(buffile);
    if version.is_none() {
        return;
    }
    let version = *version.unwrap();
    let ranges = params
        .diagnostics
        .iter()
        .map(|x| {
            format!(
                "{}.{},{}.{}|{}",
                x.range.start.line + 1,
                x.range.start.character + 1,
                x.range.end.line + 1,
                // LSP ranges are exclusive, but Kakoune's are inclusive
                x.range.end.character,
                match x.severity {
                    Some(::languageserver_types::DiagnosticSeverity::Error) => "Error",
                    _ => "Information",
                }
            )
        })
        .collect::<Vec<String>>()
        .join(":");
    let command = format!(
        "eval -buffer %§{}§ %§set buffer lsp_errors \"{}:{}\"§",
        buffile, version, ranges
    );
    ctx.diagnostics
        .insert(buffile.to_string(), params.diagnostics);
    let meta = EditorMeta {
        session,
        client,
        buffile: buffile.to_string(),
        version,
    };
    ctx.exec(meta, command.to_string());
}
