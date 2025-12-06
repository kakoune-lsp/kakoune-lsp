use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

use crate::thread_worker::Worker;
use crate::types::*;
use crate::{context::*, editor_transport::ToEditorSender};
use crossbeam_channel::{Receiver, Sender};
use jsonrpc_core::Value;
use lsp_types::notification::*;
use lsp_types::*;
use notify_debouncer_full::{
    new_debouncer,
    notify::{self, RecursiveMode, Watcher},
    DebounceEventResult,
};
use ropey::Rope;
use serde::Deserialize;
use url::Url;

pub fn text_document_did_open(
    meta: EditorMeta,
    params: TextDocumentDidOpenParams,
    ctx: &mut Context,
) {
    let document = Document {
        version: meta.version,
        text: Rope::from_str(&params.draft),
    };
    ctx.documents.insert(meta.buffile.clone(), document);

    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
            language_id: meta.language_id.clone(),
            version: meta.version,
            text: params.draft,
        },
    };
    for &server_id in &meta.servers {
        ctx.notify::<DidOpenTextDocument>(server_id, params.clone());
    }

    // Request pull diagnostics if supported
    crate::diagnostics::text_document_diagnostic(meta.clone(), ctx);
}

pub fn text_document_did_change(
    meta: EditorMeta,
    params: TextDocumentDidChangeParams,
    ctx: &mut Context,
) {
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

    // Resets metadata for buffer.
    ctx.documents.insert(meta.buffile.clone(), document);

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
    for &server_id in &meta.servers {
        ctx.notify::<DidChangeTextDocument>(server_id, req_params.clone());
    }

    // Request pull diagnostics if supported (debounced by Kakoune's InsertIdle/NormalIdle)
    crate::diagnostics::text_document_diagnostic(meta, ctx);
}

pub fn text_document_did_close(meta: EditorMeta, ctx: &mut Context) {
    ctx.documents.remove(&meta.buffile);
    let uri = Url::from_file_path(&meta.buffile).unwrap();
    let params = DidCloseTextDocumentParams {
        text_document: TextDocumentIdentifier { uri },
    };
    for &server_id in &meta.servers {
        ctx.notify::<DidCloseTextDocument>(server_id, params.clone());
    }
}

pub fn text_document_did_save(meta: EditorMeta, ctx: &mut Context) {
    for &server_id in &meta.servers {
        let server = ctx.server(server_id);
        let options = match &server.capabilities.as_ref().unwrap().text_document_sync {
            Some(TextDocumentSyncCapability::Options(TextDocumentSyncOptions {
                save: Some(opts),
                ..
            })) if !matches!(opts, TextDocumentSyncSaveOptions::Supported(false)) => opts,
            _ => continue, // don't send didSave by default
        };
        let text = match options {
            TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                include_text: Some(true),
            }) => ctx
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
        ctx.notify::<DidSaveTextDocument>(server_id, params);
    }

    // Request pull diagnostics if supported
    crate::diagnostics::text_document_diagnostic(meta, ctx);
}

pub fn spawn_file_watcher(
    to_editor: ToEditorSender,
    log_path: &'static Option<PathBuf>,
    watch_requests: HashMap<(ServerId, String, Option<PathBuf>), Vec<CompiledFileSystemWatcher>>,
) -> Worker<ToEditorSender, (), Vec<FileEvent>> {
    debug!(&to_editor, "starting file watcher");
    Worker::spawn(
        to_editor,
        "File system change watcher",
        1024, // arbitrary
        move |to_editor: Sender<EditorResponse>,
              receiver: Receiver<()>,
              sender: Sender<Vec<FileEvent>>| {
            let mut debouncers = Vec::new();
            for ((_, root_path, path), path_watch_requests) in watch_requests {
                let sender = sender.clone();
                let callback = {
                    let to_editor = to_editor.clone();
                    move |res: DebounceEventResult| {
                        match res {
                            Ok(debounced_events) => {
                                let mut file_changes = vec![];
                                for debounced_event in debounced_events {
                                    event_file_changes(
                                        &mut file_changes,
                                        log_path,
                                        &path_watch_requests,
                                        debounced_event.event,
                                    );
                                }
                                if !file_changes.is_empty() {
                                    if let Err(err) = sender.send(file_changes) {
                                        error!(&to_editor, "{}", err);
                                    }
                                }
                            }
                            Err(errors) => {
                                for e in errors {
                                    error!(&to_editor, "{}", e)
                                }
                            }
                        };
                    }
                };

                let mut debouncer = match new_debouncer(Duration::from_secs(1), None, callback) {
                    Ok(debouncer) => debouncer,
                    Err(err) => {
                        error!(&to_editor, "{}", err);
                        return;
                    }
                };

                let path = path.as_deref().unwrap_or_else(|| Path::new(&root_path));
                if let Err(err) = debouncer.watcher().watch(path, RecursiveMode::Recursive) {
                    error!(&to_editor, "{:?}: {}", path, err);
                }
                debouncers.push(debouncer);
            }
            if let Err(err) = receiver.recv() {
                error!(&to_editor, "{}", err);
            }
        },
    )
}

fn event_file_changes(
    file_changes: &mut Vec<FileEvent>,
    log_path: &'static Option<PathBuf>,
    watch_requests: &Vec<CompiledFileSystemWatcher>,
    event: notify::Event,
) {
    for path in &event.paths {
        if log_path.as_ref() == Some(path) {
            continue;
        }
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
                    uri: Url::from_file_path(path).unwrap(),
                    typ: file_change_type,
                });
                break;
            }
        }
    }
}

pub fn workspace_did_change_watched_files(
    server_id: ServerId,
    changes: Vec<FileEvent>,
    ctx: &mut Context,
) {
    let params = DidChangeWatchedFilesParams { changes };
    ctx.notify::<DidChangeWatchedFiles>(server_id, params);
}

#[derive(Clone)]
pub struct CompiledFileSystemWatcher {
    kind: WatchKind,
    pattern: glob::Pattern,
}

pub fn register_workspace_did_change_watched_files(
    server_id: ServerId,
    options: Option<Value>,
    ctx: &mut Context,
) {
    if !ctx.config.file_watch_support {
        error!(
            ctx.to_editor(),
            "file watch support is disabled, ignoring spurious {} server request",
            notification::DidChangeWatchedFiles::METHOD
        );
        return;
    }
    let options = options.unwrap();
    let options = DidChangeWatchedFilesRegistrationOptions::deserialize(options).unwrap();
    assert!(ctx.pending_file_watchers.is_empty());
    for watcher in options.watchers {
        {
            let bare_pattern = match &watcher.glob_pattern {
                GlobPattern::String(pattern) => pattern,
                GlobPattern::Relative(relative) => &relative.pattern,
            };
            if bare_pattern.contains('{') {
                error!(
                    ctx.to_editor(),
                    "unsupported braces in glob pattern: '{}'", &bare_pattern
                );
                continue;
            }
        }
        let (root_path, glob_pattern) = match watcher.glob_pattern {
            GlobPattern::String(pattern) => (None, pattern),
            GlobPattern::Relative(RelativePattern { base_uri, pattern }) => {
                let url = match base_uri {
                    OneOf::Left(workspace_folder) => workspace_folder.uri,
                    OneOf::Right(url) => url,
                };
                let root = match url.to_file_path() {
                    Ok(root) => root,
                    Err(_) => {
                        error!(ctx.to_editor(), "URL is not a file path: {}", url);
                        continue;
                    }
                };
                (Some(root), pattern)
            }
        };
        let pattern = match glob::Pattern::new(&glob_pattern) {
            Ok(pattern) => pattern,
            Err(err) => {
                error!(
                    ctx.to_editor(),
                    "failed to compile glob pattern '{}': {}", &glob_pattern, err
                );
                continue;
            }
        };
        let default_watch_kind = WatchKind::Create | WatchKind::Change | WatchKind::Delete;
        let kind = watcher.kind.unwrap_or(default_watch_kind);
        ctx.pending_file_watchers
            .entry((server_id, ctx.server(server_id).roots[0].clone(), root_path))
            .or_default()
            .push(CompiledFileSystemWatcher { kind, pattern });
    }
}
