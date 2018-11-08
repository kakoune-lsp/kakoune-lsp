use context::*;
use languageserver_types::request::Request;
use languageserver_types::*;
use serde::Deserialize;
use serde_json::{self, Value};
use std::fs;
use types::*;
use url::Url;
use util::*;

pub fn text_document_rename(meta: &EditorMeta, params: EditorParams, ctx: &mut Context) {
    let options = TextDocumentRenameParams::deserialize(params.clone());
    if options.is_err() {
        error!("Params should follow TextDocumentRenameParams structure");
    }
    let options = options.unwrap();
    let req_params = RenameParams {
        text_document: TextDocumentIdentifier {
            uri: Url::from_file_path(&meta.buffile).unwrap(),
        },
        position: options.position,
        new_name: options.new_name,
    };
    let id = ctx.next_request_id();
    ctx.response_waitlist.insert(
        id.clone(),
        (meta.clone(), request::Rename::METHOD.into(), params),
    );
    ctx.call(id, request::Rename::METHOD.into(), req_params);
}

pub fn editor_rename(meta: &EditorMeta, _params: EditorParams, result: Value, ctx: &mut Context) {
    let result: Option<WorkspaceEdit> =
        serde_json::from_value(result).expect("Failed to parse formatting response");
    if result.is_none() {
        return;
    }
    let result = result.unwrap();
    if let Some(document_changes) = result.document_changes {
        match document_changes {
            DocumentChanges::Edits(edits) => {
                for edit in edits {
                    // TODO handle version
                    apply_text_edits(Some(&edit.text_document.uri), &edit.edits, meta, ctx);
                }
            }
            DocumentChanges::Operations(ops) => {
                for op in ops {
                    match op {
                        DocumentChangeOperation::Edit(edit) => {
                            apply_text_edits(Some(&edit.text_document.uri), &edit.edits, meta, ctx)
                        }
                        DocumentChangeOperation::Op(op) => match op {
                            ResourceOp::Create(op) => {
                                let path = Url::parse(&op.uri).unwrap().to_file_path().unwrap();
                                let ignore_if_exists = if let Some(options) = op.options {
                                    !options.overwrite.unwrap_or(false)
                                        && options.ignore_if_exists.unwrap_or(false)
                                } else {
                                    false
                                };
                                if !(ignore_if_exists && path.exists())
                                    && fs::write(&path, []).is_err()
                                {
                                    error!(
                                        "Failed to create file: {}",
                                        path.to_str().unwrap_or("")
                                    );
                                }
                            }
                            ResourceOp::Delete(op) => {
                                let path = Url::parse(&op.uri).unwrap().to_file_path().unwrap();
                                if path.is_dir() {
                                    let recursive = if let Some(options) = op.options {
                                        options.recursive.unwrap_or(false)
                                    } else {
                                        false
                                    };
                                    if recursive {
                                        if fs::remove_dir_all(&path).is_err() {
                                            error!(
                                                "Failed to delete directory: {}",
                                                path.to_str().unwrap_or("")
                                            );
                                        }
                                    } else if fs::remove_dir(&path).is_err() {
                                        error!(
                                            "Failed to delete directory: {}",
                                            path.to_str().unwrap_or("")
                                        );
                                    }
                                } else if path.is_file() && fs::remove_file(&path).is_err() {
                                    error!(
                                        "Failed to delete file: {}",
                                        path.to_str().unwrap_or("")
                                    );
                                }
                            }
                            ResourceOp::Rename(op) => {
                                let from = Url::parse(&op.old_uri).unwrap().to_file_path().unwrap();
                                let to = Url::parse(&op.new_uri).unwrap().to_file_path().unwrap();
                                let ignore_if_exists = if let Some(options) = op.options {
                                    !options.overwrite.unwrap_or(false)
                                        && options.ignore_if_exists.unwrap_or(false)
                                } else {
                                    false
                                };
                                if !(ignore_if_exists && to.exists())
                                    && fs::rename(&from, &to).is_err()
                                {
                                    error!(
                                        "Failed to rename file: {} -> {}",
                                        from.to_str().unwrap_or(""),
                                        to.to_str().unwrap_or("")
                                    );
                                }
                            }
                        },
                    }
                }
            }
        }
    } else if let Some(changes) = result.changes {
        for (uri, change) in &changes {
            apply_text_edits(Some(uri), &change, meta, ctx);
        }
    }
}
