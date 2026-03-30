use std::collections::HashMap;

use itertools::Itertools;
use lsp_types::{request::Request, Location, TextDocumentIdentifier, Uri};
use ropey::Rope;

use crate::{
    context::{Context, Document, RequestParams},
    language_features::goto::{goto_location, goto_locations},
    types::{EditorMeta, ServerId},
};

pub const SCHEME: &str = "deno";

struct VirtualTextDocument {}

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct VirtualTextDocumentParams {
    text_document: TextDocumentIdentifier,
}

impl Request for VirtualTextDocument {
    type Params = VirtualTextDocumentParams;
    type Result = String;

    const METHOD: &'static str = "deno/virtualTextDocument";
}

pub fn handle_virtual_locations(
    meta: EditorMeta,
    ctx: &mut Context,
    mut virtual_locations: Vec<(ServerId, Location)>,
    other_locations: Vec<(ServerId, Location)>,
) {
    let unique_uris: Vec<(ServerId, Uri)> = virtual_locations
        .iter()
        .map(|(server_id, Location { uri, .. })| (*server_id, uri.clone()))
        .unique()
        .collect();

    let req_params: HashMap<usize, Vec<VirtualTextDocumentParams>> =
        unique_uris
            .iter()
            .fold(HashMap::new(), |mut m, (server_id, uri)| {
                m.entry(*server_id)
                    .or_default()
                    .push(VirtualTextDocumentParams {
                        text_document: TextDocumentIdentifier {
                            uri: (*uri).clone(),
                        },
                    });

                m
            });

    ctx.call::<VirtualTextDocument, _>(
        meta.clone(),
        RequestParams::Each(req_params),
        move |ctx, _, results| {
            for ((_, uri), (_, content)) in unique_uris.into_iter().zip(results.into_iter()) {
                _ = ctx.virtual_documents.insert(uri.to_string());
                _ = ctx.documents.insert(
                    uri.to_string(),
                    Document {
                        text: Rope::from_str(&content),
                        version: meta.version,
                    },
                );
            }

            match virtual_locations.len() {
                0 => {}
                1 => goto_location(meta, virtual_locations.pop().unwrap(), ctx),
                _ => {
                    goto_locations(meta, other_locations, virtual_locations, ctx);
                }
            };
        },
    );
}
