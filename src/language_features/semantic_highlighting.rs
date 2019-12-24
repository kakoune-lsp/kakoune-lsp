use crate::context::*;
use crate::position::*;
use crate::types::*;
use crate::util::*;
use itertools::Itertools;
use jsonrpc_core::Params;
use lsp_types::*;

pub fn semantic_highlighting_notification(params: Params, ctx: &mut Context) {
    let params = params.parse::<SemanticHighlightingParams>();
    let params = params.unwrap();
    let path = params.text_document.uri.to_file_path().unwrap();
    let buffile = path.to_str().unwrap();
    let document = match ctx.documents.get(buffile) {
        Some(document) => document,
        None => return,
    };
    let meta = match ctx.meta_for_buffer(buffile.to_string()) {
        Some(meta) => meta,
        None => return,
    };
    let scopes = ctx
        .capabilities
        .as_ref()
        .unwrap()
        .semantic_highlighting
        .as_ref()
        .unwrap()
        .scopes
        .as_ref()
        .expect("Server sent semantic highlight notification without setting capability");
    let offset_encoding = ctx.offset_encoding.to_owned();
    let ranges = params
        .lines
        .iter()
        .map(|info| {
            let line: u64 = info.line as u64;
            info.tokens
                .iter()
                .map(|t| {
                    let scope = scopes
                        .get(t.scope as usize)
                        .expect("Semantic highlighting token sent for out-of-range scope");
                    let face = get_face_for_scope(scope);
                    let range = Range {
                        start: Position::new(line, t.character.into()),
                        end: Position::new(line, (t.character + u32::from(t.length)).into()),
                    };
                    format!(
                        "{}|{}",
                        lsp_range_to_kakoune(&range, &document.text, &offset_encoding),
                        face
                    )
                })
                .join(" ")
        })
        .join(" ");
    let command = format!("set buffer lsp_semantic_highlighting {} {}", meta.version, editor_quote(&ranges));
    let command = format!(
        "eval -buffer {} {}",
        editor_quote(&buffile), editor_quote(&command)
    );
    ctx.exec(meta, command.to_string());
}

pub fn debug_scopes(meta: EditorMeta, ctx: &mut Context) {
    let semhl = ctx
        .capabilities
        .as_ref()
        .unwrap()
        .semantic_highlighting
        .as_ref();
    semhl.map(|sh| {
        if sh.scopes.is_some() {
            let command = format!("echo -debug %§{:?}§", sh.scopes.as_ref().unwrap());
            ctx.exec(meta, command.to_string());
        }
    });
}

// TODO: Have an actual usable lookup table for faces
fn get_face_for_scope(scope: &Vec<String>) -> String {
    scope.iter().flat_map(|s| s.split(".")).join("_")
}
