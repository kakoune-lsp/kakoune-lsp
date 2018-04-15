use context::*;
use languageserver_types::*;
use types::*;

pub fn publish_diagnostics(params: PublishDiagnosticsParams, ctx: &mut Context) {
    let session = ctx.session.clone();
    let client = None;
    let buffile = params.uri.path().to_string();
    let version = ctx.versions.get(&buffile);
    if version.is_none() {
        return;
    }
    let version = *version.unwrap();
    let ranges = params
        .diagnostics
        .iter()
        .map(|x| {
            format!(
                "{}.{},{}.{}|Error",
                x.range.start.line + 1,
                x.range.start.character + 1,
                x.range.end.line + 1,
                // LSP ranges are exclusive, but Kakoune's are inclusive
                x.range.end.character
            )
        })
        .collect::<Vec<String>>()
        .join(":");
    let command = format!(
        "eval -buffer %§{}§ %§set buffer lsp_errors \"{}:{}\"§",
        buffile, version, ranges
    );
    ctx.diagnostics.insert(buffile.clone(), params.diagnostics);
    let meta = EditorMeta {
        session,
        client,
        buffile,
        version,
    };
    ctx.exec(meta, command.to_string());
}
