use crate::context::*;
use crate::controller;
use crate::language_features::semantic_highlighting;
use crate::types::*;
use crate::util::*;
use lsp_types::notification::*;
use lsp_types::request::*;
use lsp_types::*;
use serde_json::Value;
use std::process;
use toml;
use url::Url;

pub fn initialize(
    root_path: &str,
    initialization_options: Option<Value>,
    meta: EditorMeta,
    ctx: &mut Context,
) {
    let initialization_options =
        request_initialization_options_from_kakoune(&meta, ctx).or(initialization_options);
    #[allow(deprecated)] // for root_path
    let params = InitializeParams {
        capabilities: ClientCapabilities {
            workspace: Some(WorkspaceClientCapabilities {
                workspace_edit: Some(WorkspaceEditCapability {
                    document_changes: Some(true),
                    resource_operations: Some(vec![
                        ResourceOperationKind::Create,
                        ResourceOperationKind::Delete,
                        ResourceOperationKind::Rename,
                    ]),
                    ..WorkspaceEditCapability::default()
                }),
                ..WorkspaceClientCapabilities::default()
            }),
            text_document: Some(TextDocumentClientCapabilities {
                completion: Some(CompletionCapability {
                    completion_item: Some(CompletionItemCapability {
                        documentation_format: Some(vec![
                            MarkupKind::Markdown,
                            MarkupKind::PlainText,
                        ]),
                        snippet_support: Some(ctx.config.snippet_support),
                        ..CompletionItemCapability::default()
                    }),
                    ..CompletionCapability::default()
                }),
                code_action: Some(CodeActionCapability {
                    code_action_literal_support: Some(CodeActionLiteralSupport {
                        code_action_kind: CodeActionKindLiteralSupport {
                            value_set: [
                                "quickfix",
                                "refactor",
                                "refactor.extract",
                                "refactor.inline",
                                "refactor.rewrite",
                                "source",
                                "source.organizeImports",
                            ]
                            .iter()
                            .map(|s| s.to_string())
                            .collect(),
                        },
                    }),
                    ..CodeActionCapability::default()
                }),
                hover: Some(HoverCapability {
                    content_format: Some(vec![MarkupKind::Markdown, MarkupKind::PlainText]),
                    ..HoverCapability::default()
                }),
                semantic_highlighting_capabilities: Some(SemanticHighlightingClientCapability {
                    semantic_highlighting: true,
                }),
                signature_help: Some(SignatureHelpCapability {
                    signature_information: Some(SignatureInformationSettings {
                        documentation_format: Some(vec![
                            MarkupKind::Markdown,
                            MarkupKind::PlainText,
                        ]),
                        parameter_information: None,
                    }),
                    ..SignatureHelpCapability::default()
                }),
                ..TextDocumentClientCapabilities::default()
            }),
            experimental: None,
            ..ClientCapabilities::default()
        },
        initialization_options,
        process_id: Some(process::id().into()),
        root_uri: Some(Url::from_file_path(root_path).unwrap()),
        root_path: None,
        trace: Some(TraceOption::Off),
        workspace_folders: None,
        client_info: None,
    };

    ctx.call::<Initialize, _>(meta, params, move |ctx: &mut Context, _meta, result| {
        ctx.capabilities = Some(result.capabilities);
        ctx.semantic_highlighting_faces = semantic_highlighting::make_scope_map(ctx);
        ctx.notify::<Initialized>(InitializedParams {});
        controller::dispatch_pending_editor_requests(ctx)
    });
}

pub fn exit(ctx: &mut Context) {
    ctx.notify::<Exit>(());
}

pub fn capabilities(meta: EditorMeta, ctx: &mut Context) {
    // NOTE controller should park request for capabilities until they are available thus it should
    // be safe to unwrap here (otherwise something unexpectedly wrong and it's better to panic)

    let server_capabilities = ctx.capabilities.as_ref().unwrap();

    let mut features = vec![];

    if server_capabilities.hover_provider.unwrap_or(false) {
        features.push("lsp-hover");
    }

    if server_capabilities.completion_provider.is_some() {
        features.push("lsp-completion (hooked on InsertIdle)");
    }

    if server_capabilities.definition_provider.unwrap_or(false) {
        features.push("lsp-definition (mapped to `gd` by default)");
    }

    if server_capabilities.implementation_provider.is_some() {
        features.push("lsp-implementation");
    }

    if server_capabilities.references_provider.unwrap_or(false) {
        features.push("lsp-references (mapped to `gr` by default)");
    }

    if server_capabilities
        .workspace_symbol_provider
        .unwrap_or(false)
    {
        features.push("lsp-workspace-symbol");
    }

    if server_capabilities
        .document_formatting_provider
        .unwrap_or(false)
    {
        features.push("lsp-formatting");
    }

    if let Some(ref rename_provider) = server_capabilities.rename_provider {
        match rename_provider {
            RenameProviderCapability::Simple(true) | RenameProviderCapability::Options(_) => {
                features.push("lsp-rename")
            }
            _ => (),
        }
    }

    if let Some(ref code_action_provider) = server_capabilities.code_action_provider {
        match code_action_provider {
            CodeActionProviderCapability::Simple(x) => {
                if *x {
                    features.push("lsp-code-actions");
                }
            }
            CodeActionProviderCapability::Options(_) => features.push("lsp-code-actions"),
        }
    }

    features.push("lsp-diagnostics");

    let command = format!(
        "info 'kak-lsp commands supported by {} language server:\n\n{}'",
        ctx.language_id,
        editor_escape(&features.join("\n"))
    );
    ctx.exec(meta, command);
}

/// User may override `initialization_options` provided in kak-lsp.toml on per-language server basis
/// with `lsp_server_initialization_options` option in Kakoune
/// (i.e. to customize it for specific project).
/// This function asks Kakoune to give such override if any.
fn request_initialization_options_from_kakoune(
    meta: &EditorMeta,
    ctx: &mut Context,
) -> Option<Value> {
    let mut path = temp_dir();
    path.push(format!("{:x}", rand::random::<u64>()));
    let path = path.to_str().unwrap();
    let fifo_result = unsafe {
        let path = std::ffi::CString::new(path).unwrap();
        libc::mkfifo(path.as_ptr(), 0o600)
    };
    if fifo_result != 0 {
        return None;
    }
    ctx.exec(
        meta.clone(),
        format!(
            "lsp-get-server-initialization-options {}",
            editor_quote(path)
        ),
    );
    let options = std::fs::read_to_string(path).unwrap();
    debug!("lsp_server_initialization_options:\n{}", options);
    let _ = std::fs::remove_file(path);
    if options.trim().is_empty() {
        None
    } else {
        let options = toml::from_str::<Value>(&options);
        match options {
            Ok(options) => Some(options),
            Err(e) => {
                error!("Failed to parse lsp_server_initialization_options: {:?}", e);
                None
            }
        }
    }
}
