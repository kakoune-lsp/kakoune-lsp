use context::*;
use languageserver_types::notification::Notification;
use languageserver_types::*;
use serde::Deserialize;
use types::*;
use url::Url;

pub fn text_document_did_open(meta: &EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = TextDocumentDidOpenParams::deserialize(params);
    if params.is_err() {
        error!("Params should follow TextDocumentDidOpenParams structure");
        return;
    }
    let params = params.unwrap();
    let language_id = ctx.language_id.clone();
    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
            language_id,
            version: meta.version,
            text: params.draft,
        },
    };
    ctx.versions.insert(meta.buffile.clone(), meta.version);
    ctx.notify(notification::DidOpenTextDocument::METHOD.into(), params);
}

pub fn text_document_did_change(meta: &EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = TextDocumentDidChangeParams::deserialize(params);
    if params.is_err() {
        error!("Params should follow TextDocumentDidChangeParams structure");
        return;
    }
    let params = params.unwrap();
    let uri = Url::from_file_path(&meta.buffile).unwrap();
    let version = meta.version;
    let old_version = ctx.versions.get(&meta.buffile).cloned().unwrap_or(0);
    if old_version >= version {
        return;
    }
    ctx.versions.insert(meta.buffile.clone(), version);
    ctx.diagnostics.insert(meta.buffile.clone(), Vec::new());
    let params = DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            uri,
            version: Some(meta.version),
        },
        content_changes: vec![TextDocumentContentChangeEvent {
            range: None,
            range_length: None,
            text: params.draft,
        }],
    };
    ctx.notify(notification::DidChangeTextDocument::METHOD.into(), params);
}

pub fn text_document_did_close(meta: &EditorMeta, ctx: &mut Context) {
    let uri = Url::from_file_path(&meta.buffile).unwrap();
    let params = DidCloseTextDocumentParams {
        text_document: TextDocumentIdentifier { uri },
    };
    ctx.notify(notification::DidCloseTextDocument::METHOD.into(), params);
}

pub fn text_document_did_save(meta: &EditorMeta, ctx: &mut Context) {
    let uri = Url::from_file_path(&meta.buffile).unwrap();
    let params = DidSaveTextDocumentParams {
        text_document: TextDocumentIdentifier { uri },
    };
    ctx.notify(notification::DidSaveTextDocument::METHOD.into(), params);
}
