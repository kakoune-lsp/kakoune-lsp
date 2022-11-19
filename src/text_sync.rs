use std::path::Path;

use crate::context::*;
use crate::language_features::code_lens::text_document_code_lens;
use crate::thread_worker::Worker;
use crate::types::*;
use crossbeam_channel::{Receiver, Sender};
use jsonrpc_core::Value;
use lsp_types::notification::*;
use lsp_types::*;
use notify::{RecursiveMode, Watcher};
use ropey::Rope;
use serde::Deserialize;
use url::Url;

pub fn text_document_did_open(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = TextDocumentDidOpenParams::deserialize(params)
        .expect("Params should follow TextDocumentDidOpenParams structure");
    let language_id = ctx.language_id.clone();
    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
            language_id,
            version: meta.version,
            text: params.draft,
        },
    };
    let document = Document {
        version: meta.version,
        text: Rope::from_str(&params.text_document.text),
    };
    ctx.documents.insert(meta.buffile.clone(), document);
    ctx.notify::<DidOpenTextDocument>(params);
    text_document_code_lens(meta, ctx);
}

pub fn text_document_did_change(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = TextDocumentDidChangeParams::deserialize(params)
        .expect("Params should follow TextDocumentDidChangeParams structure");
    let uri = Url::from_file_path(&meta.buffile).unwrap();
    let version = meta.version;
    let old_version = ctx
        .documents
        .get(&meta.buffile)
        .map(|doc| doc.version)
        .unwrap_or(0);
    if old_version >= version {
        return;
    }
    let document = Document {
        version,
        text: Rope::from_str(&params.draft),
    };
    ctx.documents.insert(meta.buffile.clone(), document);
    ctx.diagnostics.insert(meta.buffile.clone(), Vec::new());
    let req_params = DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            uri,
            version: meta.version,
        },
        content_changes: vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: params.draft,
        }],
    };
    ctx.notify::<DidChangeTextDocument>(req_params);
    text_document_code_lens(meta, ctx);
}

pub fn text_document_did_close(meta: EditorMeta, ctx: &mut Context) {
    ctx.documents.remove(&meta.buffile);
    let uri = Url::from_file_path(&meta.buffile).unwrap();
    let params = DidCloseTextDocumentParams {
        text_document: TextDocumentIdentifier { uri },
    };
    ctx.notify::<DidCloseTextDocument>(params);
}

pub fn text_document_did_save(meta: EditorMeta, ctx: &mut Context) {
    let text = match ctx.capabilities.as_ref().unwrap().text_document_sync {
        Some(TextDocumentSyncCapability::Options(TextDocumentSyncOptions {
            save:
                Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                    include_text: Some(true),
                })),
            ..
        })) => ctx
            .documents
            .get(&meta.buffile)
            .map(|doc| doc.text.to_string()),
        _ => None,
    };

    let uri = Url::from_file_path(&meta.buffile).unwrap();
    let params = DidSaveTextDocumentParams {
        text_document: TextDocumentIdentifier { uri },
        text,
    };
    ctx.notify::<DidSaveTextDocument>(params);
}

pub fn spawn_file_watcher(
    root_path: String,
    watch_requests: Vec<CompiledFileSystemWatcher>,
) -> Worker<(), Vec<FileEvent>> {
    info!("starting file watcher");
    Worker::spawn(
        "File system change watcher",
        1024, // arbitrary
        move |receiver: Receiver<()>, sender: Sender<Vec<FileEvent>>| {
            let callback = move |res: notify::Result<notify::Event>| {
                match res {
                    Ok(event) => {
                        let file_changes = event_file_changes(&watch_requests, event);
                        if !file_changes.is_empty() {
                            if let Err(err) = sender.send(file_changes) {
                                error!("{}", err);
                            }
                        }
                    }
                    Err(e) => error!("{}", e),
                };
            };
            let mut watcher = match notify::recommended_watcher(callback) {
                Ok(watcher) => watcher,
                Err(err) => {
                    error!("{}", err);
                    return;
                }
            };
            let path = Path::new(&root_path);
            if let Err(err) = watcher.watch(path, RecursiveMode::Recursive) {
                error!("{}", err);
            }
            if let Err(err) = receiver.recv() {
                error!("{}", err);
            }
        },
    )
}

fn event_file_changes(
    // sender: Sender<Vec<FileEvent>>,
    watch_requests: &Vec<CompiledFileSystemWatcher>,
    event: notify::Event,
) -> Vec<FileEvent> {
    let mut file_changes = vec![];

    for path in &event.paths {
        for watch_request in watch_requests {
            let watch_kind = watch_request.kind;
            let file_change_type = match event.kind {
                notify::EventKind::Create(_) => {
                    if !watch_kind.contains(WatchKind::Create) {
                        continue;
                    }
                    FileChangeType::CREATED
                }
                notify::EventKind::Modify(_) => {
                    if !watch_kind.contains(WatchKind::Change) {
                        continue;
                    }
                    FileChangeType::CHANGED
                }
                notify::EventKind::Remove(_) => {
                    if !watch_kind.contains(WatchKind::Delete) {
                        continue;
                    }
                    FileChangeType::DELETED
                }
                notify::EventKind::Any
                | notify::EventKind::Access(_)
                | notify::EventKind::Other => continue,
            };
            if watch_request.pattern.matches_path(path) {
                file_changes.push(FileEvent {
                    uri: Url::from_file_path(&path).unwrap(),
                    typ: file_change_type,
                });
                break;
            }
        }
    }
    file_changes
}

pub fn workspace_did_change_watched_files(changes: Vec<FileEvent>, ctx: &mut Context) {
    let params = DidChangeWatchedFilesParams { changes };
    ctx.notify::<DidChangeWatchedFiles>(params);
}

#[derive(Clone)]
pub struct CompiledFileSystemWatcher {
    kind: WatchKind,
    pattern: glob::Pattern,
}

pub fn register_workspace_did_change_watched_files(options: Option<Value>, ctx: &mut Context) {
    let options = options.unwrap();
    let options = DidChangeWatchedFilesRegistrationOptions::deserialize(options).unwrap();
    let watchers = options
        .watchers
        .into_iter()
        .filter_map(|watcher| {
            if watcher.glob_pattern.contains('{') {
                error!(
                    "unsupported braces in glob patttern: '{}'",
                    &watcher.glob_pattern
                );
                return None;
            }
            let pattern = match glob::Pattern::new(&watcher.glob_pattern) {
                Ok(pattern) => pattern,
                Err(err) => {
                    error!(
                        "failed to compile glob pattern '{}': {}",
                        &watcher.glob_pattern, err
                    );
                    return None;
                }
            };
            let default_watch_kind = WatchKind::Create | WatchKind::Change | WatchKind::Delete;
            let kind = watcher.kind.unwrap_or(default_watch_kind);
            Some(CompiledFileSystemWatcher { kind, pattern })
        })
        .collect();
    assert!(ctx.pending_file_watchers.is_empty());
    ctx.pending_file_watchers = watchers;
}
