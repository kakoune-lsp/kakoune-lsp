use crate::context::*;
use crate::position::*;
use crate::types::*;
use crate::util::*;
use itertools::Itertools;
use jsonrpc_core::Params;
use lsp_types::*;
use serde::Deserialize;
use std::collections::HashMap;

pub fn semantic_highlighting_notification(params: Params, ctx: &mut Context) {
    let params = params.parse::<SemanticHighlightingParams>();
    let params = params.unwrap();
    let path = params.text_document.uri.to_file_path().unwrap();
    let buffile = path.to_str().unwrap();
    let meta = match ctx.meta_for_buffer(buffile.to_string()) {
        Some(meta) => meta,
        None => return,
    };
    ctx.semantic_highlighting_lines.insert(buffile.to_string(), params.lines);
    let command = "lsp-update-semantic-highlighting";
    let command = format!(
        "eval -buffer {} {}",
        editor_quote(&buffile),
        editor_quote(&command)
    );
    ctx.exec(meta, command.to_string());
}

#[derive(Deserialize)]
struct EditorUpdateParams {
    current: String,
}

pub fn editor_update(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = EditorUpdateParams::deserialize(params).expect("Failed to parse params");
    let buffile = &meta.buffile;
    let document = match ctx.documents.get(buffile) {
        Some(document) => document,
        None => return,
    };
    let faces = &ctx.semantic_highlighting_faces;
    let cur_lines = params.current.split(" ");
    let updated_lines = match ctx.semantic_highlighting_lines.get(buffile) {
      Some(lines) => lines,
      None => return,
    };
    let old_ranges = cur_lines.filter(|&x| {
        let x = x.trim();
        x.find(".")
            .and_then(|p| x[0..p].parse::<i32>().ok())
            // +1 because LSP ranges are 0-based, but kakoune's are 1-based.
            .map(|line| !updated_lines.iter().any(|info| info.line + 1 == line))
            .unwrap_or(false)
    }).join(" ");
    let ranges = updated_lines
        .iter()
        .flat_map(|info| {
            let line: u64 = info.line as u64;
            let offset_encoding = &ctx.offset_encoding;
            info.tokens.iter().flat_map(|v| v.iter()).map(move |t| {
                let face = faces
                    .get(t.scope as usize)
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
    let ranges = old_ranges + " " + &ranges;
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
    let faces: HashMap<String, &String> = ctx
        .config
        .semantic_scopes
        .iter()
        .map(|(k, v)| (k.replace("_", "."), v))
        .collect();

    let scopes = ctx
        .capabilities
        .as_ref()
        .and_then(|x| x.semantic_highlighting.as_ref())
        .and_then(|x| x.scopes.as_ref());

    if scopes.is_none() {
        return Vec::new();
    }
    let scopes = scopes.unwrap();

    map_scopes_to_faces(scopes, faces)
}

fn map_scopes_to_faces(
    scopes: &Vec<Vec<String>>,
    faces: HashMap<String, &String>,
) -> std::vec::Vec<std::string::String> {
    let find_face = |scope: &String| {
        scope
            .chars()
            .enumerate()
            .filter_map(|(i, x)| if x == '.' { Some(&scope[0..i]) } else { None })
            .enumerate()
            .filter_map(|(n, element)| faces.get(element).map(|face| (n, face)))
            .last()
            .map(|(n, face)| (n, face.to_string()))
    };
    scopes
        .iter()
        .map(|scopes| {
            scopes
                .iter()
                .filter_map(find_face)
                .max_by_key(|(n, _)| *n)
                .map(|(_, x)| x)
                .unwrap_or_else(|| String::new())
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_scopes_to_faces_should_map_unknown_scope_to_empty_face() {
        let scopes = vec![
            vec![String::from("just.noise")],
            vec![String::from("some.scope")],
        ];
        let mut faces: HashMap<String, &String> = Default::default();
        let some_face = String::from("some.face");
        faces.insert(String::from("some.another.scope"), &some_face);
        let faces = map_scopes_to_faces(&scopes, faces);
        assert_eq!(Some(&String::from("")), faces.get(1));
    }

    #[test]
    fn map_scopes_to_faces_should_map_scope_by_prefix() {
        let scopes = vec![
            vec![String::from("just.noise")],
            vec![
                String::from("some.non-matching.scope"),
                String::from("some.nested.scope"),
            ],
        ];
        let mut faces: HashMap<String, &String> = Default::default();
        let some_face = String::from("some.face");
        faces.insert(String::from("some.nested"), &some_face);
        let faces = map_scopes_to_faces(&scopes, faces);
        assert_eq!(Some(&String::from("some.face")), faces.get(1));
    }

    #[test]
    fn map_scopes_to_faces_should_map_scope_by_longest_prefix() {
        let scopes = vec![
            vec![String::from("just.noise")],
            vec![
                String::from("some.scope.matching.short.prefix"),
                String::from("some.nested.scope"),
            ],
        ];
        let mut faces: HashMap<String, &String> = Default::default();
        let some = String::from("some");
        let some_face = String::from("some.face");
        faces.insert(String::from("some"), &some);
        faces.insert(String::from("some.nested"), &some_face);
        let faces = map_scopes_to_faces(&scopes, faces);
        assert_eq!(Some(&String::from("some.face")), faces.get(1));
    }
}
