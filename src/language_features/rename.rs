use crate::context::*;
use crate::types::*;
use crate::util::*;
use lsp_types::request::*;
use lsp_types::*;
use serde::Deserialize;
use std::fs;
use url::Url;

pub fn text_document_rename(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = TextDocumentRenameParams::deserialize(params).unwrap();
    let req_params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Url::from_file_path(&meta.buffile).unwrap(),
            },
            position: get_lsp_position(&meta.buffile, &params.position, ctx).unwrap(),
        },
        new_name: params.new_name,
    };
    ctx.call::<Rename, _>(meta, req_params, move |ctx: &mut Context, meta, result| {
        editor_rename(meta, result, ctx)
    });
}

// TODO handle version, so change is not applied if buffer is modified (and need to show a warning)
pub fn editor_rename(meta: EditorMeta, result: Option<WorkspaceEdit>, ctx: &mut Context) {
    if result.is_none() {
        return;
    }
    let result = result.unwrap();
    if let Some(document_changes) = result.document_changes {
        match document_changes {
            DocumentChanges::Edits(edits) => {
                for edit in edits {
                    apply_text_edits(&meta, &edit.text_document.uri, &edit.edits, ctx);
                }
            }
            DocumentChanges::Operations(ops) => {
                for op in ops {
                    match op {
                        DocumentChangeOperation::Edit(edit) => {
                            apply_text_edits(&meta, &edit.text_document.uri, &edit.edits, ctx);
                        }
                        DocumentChangeOperation::Op(op) => match op {
                            ResourceOp::Create(op) => {
                                let path = op.uri.to_file_path().unwrap();
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
                                let path = op.uri.to_file_path().unwrap();
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
                                let from = op.old_uri.to_file_path().unwrap();
                                let to = op.new_uri.to_file_path().unwrap();
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
            apply_text_edits(&meta, uri, change, ctx);
        }
    }
}
