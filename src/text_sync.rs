use context::*;
use languageserver_types::notification::Notification;
use languageserver_types::*;
use serde::Deserialize;
use std::fs::{remove_file, File};
use std::io::Read;
use types::*;
use url::Url;

pub fn text_document_did_open(_params: EditorParams, meta: &EditorMeta, ctx: &mut Context) {
    let language_id = ctx.language_id.clone();
    let mut file = File::open(&meta.buffile).expect("Failed to open file");
    let mut text = String::new();
    if file.read_to_string(&mut text).is_err() {
        error!("Failed to read from file: {}", meta.buffile);
        return;
    }
    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: Url::parse(&format!("file://{}", &meta.buffile)).unwrap(),
            language_id,
            version: meta.version,
            text,
        },
    };
    ctx.versions.insert(meta.buffile.clone(), meta.version);
    ctx.notify(notification::DidOpenTextDocument::METHOD.into(), params);
}

pub fn text_document_did_change(params: EditorParams, meta: &EditorMeta, ctx: &mut Context) {
    let params = TextDocumentDidChangeParams::deserialize(params)
        .expect("Params should follow TextDocumentDidChangeParams structure");
    let uri = Url::parse(&format!("file://{}", &meta.buffile)).unwrap();
    let version = meta.version;
    let old_version = ctx.versions.get(&meta.buffile).cloned().unwrap_or(0);
    if old_version >= version {
        return;
    }
    ctx.versions.insert(meta.buffile.clone(), version);
    ctx.diagnostics.insert(meta.buffile.clone(), Vec::new());
    let file_path = params.draft;
    let mut text = String::new();
    let result;
    {
        let mut file = File::open(&file_path).expect("Failed to open file");
        result = file.read_to_string(&mut text);
    }
    remove_file(file_path).expect("Failed to remove temporary file");
    if result.is_err() {
        error!("Failed to read from file: {}", meta.buffile);
        return;
    }
    let params = DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            uri,
            version: Some(meta.version),
        },
        content_changes: vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text,
        }],
    };
    ctx.notify(notification::DidChangeTextDocument::METHOD.into(), params);
}

pub fn text_document_did_close(_params: EditorParams, meta: &EditorMeta, ctx: &mut Context) {
    let uri = Url::parse(&format!("file://{}", &meta.buffile)).unwrap();
    let params = DidCloseTextDocumentParams {
        text_document: TextDocumentIdentifier { uri },
    };
    ctx.notify(notification::DidCloseTextDocument::METHOD.into(), params);
}

pub fn text_document_did_save(_params: EditorParams, meta: &EditorMeta, ctx: &mut Context) {
    let uri = Url::parse(&format!("file://{}", &meta.buffile)).unwrap();
    let params = DidSaveTextDocumentParams {
        text_document: TextDocumentIdentifier { uri },
    };
    ctx.notify(notification::DidSaveTextDocument::METHOD.into(), params);
}
