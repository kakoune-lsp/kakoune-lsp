use std::collections::HashMap;

use itertools::Itertools;
use lsp_types::request::DocumentLinkRequest;
use lsp_types::request::DocumentLinkResolve;
use lsp_types::DocumentLink;
use lsp_types::DocumentLinkParams;
use lsp_types::Range;
use lsp_types::TextDocumentIdentifier;
use url::Url;

use crate::capabilities::attempt_server_capability;
use crate::capabilities::CAPABILITY_DOCUMENT_LINKS;
use crate::context::Context;
use crate::context::RequestParams;
use crate::context::ServerSettings;
use crate::editor_quote;
use crate::position::kakoune_range_to_lsp;
use crate::position::lsp_position_to_kakoune;
use crate::position::lsp_range_to_kakoune;
use crate::position::parse_kakoune_range;
use crate::position::ranges_overlap;
use crate::position::ranges_touch_same_line;
use crate::EditorMeta;
use crate::ServerId;

pub fn document_links(meta: EditorMeta, ctx: &mut Context) {
    let eligible_servers: Vec<_> = ctx
        .servers(&meta)
        .filter(|srv| attempt_server_capability(ctx, *srv, &meta, CAPABILITY_DOCUMENT_LINKS))
        .collect();
    if eligible_servers.is_empty() {
        return;
    }

    let (first_server, _) = *eligible_servers.first().unwrap();
    let first_server = first_server.to_owned();

    let req_params = eligible_servers
        .into_iter()
        .map(|(server_id, _)| {
            (
                server_id,
                vec![DocumentLinkParams {
                    partial_result_params: Default::default(),
                    text_document: TextDocumentIdentifier {
                        uri: Url::from_file_path(&meta.buffile).unwrap(),
                    },
                    work_done_progress_params: Default::default(),
                }],
            )
        })
        .collect();
    ctx.call::<DocumentLinkRequest, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx, meta, results| {
            document_links_response(meta, results, ctx);
        },
    );
}

fn document_links_response(
    meta: EditorMeta,
    results: Vec<(ServerId, Option<Vec<DocumentLink>>)>,
    ctx: &mut Context,
) {
    let mut links = results
        .into_iter()
        .flat_map(|(server_id, v)| {
            let v: Vec<_> = v
                .unwrap_or_default()
                .into_iter()
                .map(|v| (server_id, v))
                .collect();
            v
        })
        .collect::<Vec<_>>();
    links.sort_by_key(|(_, link)| link.range.start);

    let buffile = &meta.buffile;
    let document = match ctx.documents.get(buffile) {
        Some(document) => document,
        None => {
            ctx.document_links.remove(buffile);
            return;
        }
    };
    let version = document.version;
    let range_specs = links
        .iter()
        .map(|(server_id, link)| {
            let server = ctx.server(*server_id);
            format!(
                "{}|LspDocumentLink",
                lsp_range_to_kakoune(&link.range, &document.text, server.offset_encoding)
            )
        })
        .join(" ");

    ctx.document_links.insert(buffile.clone(), links);

    let command = format!("set-option buffer lsp_document_links {version} {range_specs}");
    let command = format!(
        "evaluate-commands -buffer {} %§{}§",
        editor_quote(buffile),
        command.replace('§', "§§")
    );
    ctx.exec(EditorMeta::default(), command);
}

#[derive(Clone, Debug)]
pub struct DocumentLinkOptions {
    pub selection_desc: String,
}

pub fn resolve_and_follow_document_link(
    meta: EditorMeta,
    params: DocumentLinkOptions,
    ctx: &mut Context,
) {
    let (range, _cursor) = parse_kakoune_range(&params.selection_desc);
    let document = match ctx.documents.get(&meta.buffile) {
        Some(document) => document,
        None => return,
    };

    if let Some((server_id, link)) = ctx
        .document_links
        .get(&meta.buffile)
        .and_then(|links| {
            links.iter().find(|(server_id, link)| {
                let ServerSettings {
                    offset_encoding, ..
                } = ctx.server(*server_id);
                let range = kakoune_range_to_lsp(&range, &document.text, *offset_encoding);
                ranges_overlap(link.range, range)
            })
        })
        .filter(|(_, link)| link.target.is_none())
        .cloned()
    {
        let mut req_params = HashMap::new();
        req_params.insert(server_id, vec![link]);

        ctx.call::<DocumentLinkResolve, _>(
            meta,
            RequestParams::Each(req_params),
            |ctx: &mut Context, meta, results| follow_document_link(meta, &results, ctx),
        );
        return;
    }

    let no_links = vec![];
    let links = ctx.document_links.get(&meta.buffile).unwrap_or(&no_links);
    let mut links = links
        .iter()
        .filter(|(server_id, link)| {
            let ServerSettings {
                offset_encoding, ..
            } = ctx.server(*server_id);
            let range = kakoune_range_to_lsp(&range, &document.text, *offset_encoding);
            ranges_overlap(link.range, range)
        })
        .map(|(a, b)| (*a, b.clone()))
        .collect::<Vec<_>>();

    links.sort_by_key(|(_server_name, link)| {
        let Range { start, end } = link.range;
        end.line - start.line
    });

    follow_document_link(meta, &links, ctx);
}

fn follow_document_link(meta: EditorMeta, links: &[(ServerId, DocumentLink)], ctx: &mut Context) {
    let commands = links
        .iter()
        .filter(|(_, link)| link.target.is_some())
        .map(|(_, link)| {
            let target = link.target.as_ref().unwrap();
            let label = editor_quote(target.as_str());
            let path = Some(target)
                .filter(|x| x.scheme() == "file")
                .and_then(|x| x.to_file_path().ok())
                .map(|x| x.to_string_lossy().to_string());
            let command = match path {
                Some(path) => format!("edit -existing {}", &editor_quote(&path)),
                None => format!("lsp-open-url {}", &editor_quote(target.as_str())),
            };
            (label, command)
        })
        .collect_vec();

    match commands.len() {
        0 => ctx.show_error(meta, "no document link in selection"),
        1 => ctx.exec(meta, commands.into_iter().next().unwrap().1),
        _ => ctx.exec(
            meta,
            format!(
                "lsp-follow-document-link {}",
                commands
                    .iter()
                    .map(|(label, command)| format!(
                        "{} {}",
                        &editor_quote(label),
                        &editor_quote(command)
                    ))
                    .join(" ")
            ),
        ),
    }
}
