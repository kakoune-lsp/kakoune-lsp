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
                        "{}:{}:{}:{}",
                        Path::new(filename)
                            .strip_prefix(&ctx.root_path)
                            .ok()
                            .and_then(|p| Some(p.to_str().unwrap()))
                            .or_else(|| Some(filename))
                            .unwrap(),
                        x.range.start.line + 1,
                        x.range.start.character + 1,
                        x.message
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let command = format!(
        "eval -try-client %opt[toolsclient] %☠
         edit! -scratch *diagnostics*
         cd %§{}§
         try %{{ set buffer working_folder %sh{{pwd}} }}
         set buffer filetype grep
         set-register '\"' %§{}§
         exec -no-hooks p
         ☠",
        ctx.root_path, content,
    );
    ctx.exec(meta.clone(), command);
}
