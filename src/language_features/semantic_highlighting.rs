use std::collections::HashMap;
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
    let faces = &ctx.semantic_highlighting_faces;
    let ranges = params
        .lines
        .iter()
        .flat_map(|info| {
            let line: u64 = info.line as u64;
            let offset_encoding = &ctx.offset_encoding;
            info.tokens.iter().map(move |t| {
                let face = faces.get(t.scope as usize)
                    .expect("Semantic highlighting token sent for out-of-range scope");
                let range = Range {
                    start: Position::new(line, t.character.into()),
                    end: Position::new(line, (t.character + u32::from(t.length)).into()),
                };
                format!(
                    "{}|{}",
                    lsp_range_to_kakoune(&range, &document.text, offset_encoding),
                    face
                )
            })
        })
        .join(" ");
    let command = format!(
        "set buffer lsp_semantic_highlighting {} {}",
        meta.version, &ranges
    );
    let command = format!(
        "eval -buffer {} {}",
        editor_quote(&buffile),
        editor_quote(&command)
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

pub fn make_scope_map(ctx: &mut Context) -> std::vec::Vec<std::string::String> {
    let faces: HashMap<String, &String> = ctx.config.semantic_scopes.iter().map(|(k,v)| (k.replace("_", "."), v)).collect();

    let scopes = ctx
        .capabilities
        .as_ref()
        .and_then(|x| x.semantic_highlighting.as_ref())
        .and_then(|x| x.scopes.as_ref());

    if scopes == None {
      return Vec::new();
    }
    let scopes = scopes.unwrap();

    let faces: Vec<String> = scopes.iter().map(|scopes| {
      for scope in scopes {
        let elements: Vec<&str> = scope.chars().enumerate().filter(|(_,x)| x == &'.').map(|(i,_)| &scope[0..i]).collect();
        for element in elements.iter().rev() {
          match faces.get(*element) {
            Some(face) => return String::from(face.as_str()),
            None => ()
          }
        }
      }
      return String::new();
    }).collect();

    faces
}
