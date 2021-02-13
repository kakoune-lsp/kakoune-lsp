use crate::context::*;
use crate::types::*;
use lsp_types::notification::*;
use lsp_types::*;
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
    ctx.documents.insert(meta.buffile, document);
    ctx.notify::<DidOpenTextDocument>(params);
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
    let params = DidChangeTextDocumentParams {
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
    ctx.notify::<DidChangeTextDocument>(params);
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
    let uri = Url::from_file_path(&meta.buffile).unwrap();
    let params = DidSaveTextDocumentParams {
        text_document: TextDocumentIdentifier { uri },
        text: None,
    };
    ctx.notify::<DidSaveTextDocument>(params);
}
