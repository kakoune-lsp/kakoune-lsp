use crate::context::*;
use crate::controller;
use crate::language_features::semantic_highlighting;
use crate::types::*;
use crate::util::*;
use itertools::Itertools;
use lsp_types::notification::*;
use lsp_types::request::*;
use lsp_types::*;
use serde_json::Value;
use std::process;
use url::Url;

pub fn workspace_folders_from_string(folders_string: String) -> Option<Vec<WorkspaceFolder>> {
    let workspace_folders = folders_string
        .split(":")
        .map(|entry| entry.chars().skip_while(|c| c == &'/').collect::<String>())
        .filter(|entry| !entry.is_empty())
        .map(|entry| format!("file://{}", entry)) // to uri format
        .filter_map(|url_candidate| match Url::parse(&url_candidate) {
            Ok(uri) => Some(WorkspaceFolder {
                uri,
                name: url_candidate.to_string(),
            }),
            Err(_) => None,
        })
        .collect::<Vec<WorkspaceFolder>>();

    eprintln!("workspace_folders = {:#?}", workspace_folders);
    if workspace_folders.is_empty() {
        return None;
    }
    Some(workspace_folders)
}

pub fn workspace_folders_from_env() -> Option<Vec<WorkspaceFolder>> {
    let folders_string: String = std::env::var("KAK_LSP_WORKSPACE_FOLDERS").ok()?;
    workspace_folders_from_string(folders_string)
}

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
                apply_edit: Some(false),
                workspace_edit: Some(WorkspaceEditClientCapabilities {
                    document_changes: Some(true),
                    resource_operations: Some(vec![
                        ResourceOperationKind::Create,
                        ResourceOperationKind::Delete,
                        ResourceOperationKind::Rename,
                    ]),
                    failure_handling: Some(FailureHandlingKind::Abort),
                    normalizes_line_endings: Some(false),
                    change_annotation_support: Some(
                        ChangeAnnotationWorkspaceEditClientCapabilities {
                            groups_on_labels: None,
                        },
                    ),
                }),
                did_change_configuration: Some(DynamicRegistrationClientCapabilities {
                    dynamic_registration: Some(false),
                }),
                did_change_watched_files: None,
                symbol: Some(WorkspaceSymbolClientCapabilities {
                    dynamic_registration: Some(false),
                    symbol_kind: Some(SymbolKindCapability {
                        value_set: Some(vec![
                            SymbolKind::File,
                            SymbolKind::Module,
                            SymbolKind::Namespace,
                            SymbolKind::Package,
                            SymbolKind::Class,
                            SymbolKind::Method,
                            SymbolKind::Property,
                            SymbolKind::Field,
                            SymbolKind::Constructor,
                            SymbolKind::Enum,
                            SymbolKind::Interface,
                            SymbolKind::Function,
                            SymbolKind::Variable,
                            SymbolKind::Constant,
                            SymbolKind::String,
                            SymbolKind::Number,
                            SymbolKind::Boolean,
                            SymbolKind::Array,
                            SymbolKind::Object,
                            SymbolKind::Key,
                            SymbolKind::Null,
                            SymbolKind::EnumMember,
                            SymbolKind::Struct,
                            SymbolKind::Event,
                            SymbolKind::Operator,
                            SymbolKind::TypeParameter,
                        ]),
                    }),
                    tag_support: None,
                }),
                execute_command: Some(DynamicRegistrationClientCapabilities {
                    dynamic_registration: Some(false),
                }),
                workspace_folders: Some(workspace_folders_from_env().is_some()),
                configuration: Some(false),
                semantic_tokens: None,
                code_lens: None,
                file_operations: None,
            }),
            text_document: Some(TextDocumentClientCapabilities {
                synchronization: Some(TextDocumentSyncClientCapabilities {
                    dynamic_registration: Some(false),
                    will_save: Some(false),
                    will_save_wait_until: Some(false),
                    did_save: Some(true),
                }),
                completion: Some(CompletionClientCapabilities {
                    dynamic_registration: Some(false),
                    completion_item: Some(CompletionItemCapability {
                        snippet_support: Some(ctx.config.snippet_support),
                        commit_characters_support: Some(false),
                        documentation_format: Some(vec![MarkupKind::PlainText]),
                        deprecated_support: Some(false),
                        preselect_support: Some(false),
                        tag_support: None,
                        insert_replace_support: None,
                        resolve_support: None,
                        insert_text_mode_support: None,
                    }),
                    completion_item_kind: Some(CompletionItemKindCapability {
                        value_set: Some(vec![
                            CompletionItemKind::Text,
                            CompletionItemKind::Method,
                            CompletionItemKind::Function,
                            CompletionItemKind::Constructor,
                            CompletionItemKind::Field,
                            CompletionItemKind::Variable,
                            CompletionItemKind::Class,
                            CompletionItemKind::Interface,
                            CompletionItemKind::Module,
                            CompletionItemKind::Property,
                            CompletionItemKind::Unit,
                            CompletionItemKind::Value,
                            CompletionItemKind::Enum,
                            CompletionItemKind::Keyword,
                            CompletionItemKind::Snippet,
                            CompletionItemKind::Color,
                            CompletionItemKind::File,
                            CompletionItemKind::Reference,
                            CompletionItemKind::Folder,
                            CompletionItemKind::EnumMember,
                            CompletionItemKind::Constant,
                            CompletionItemKind::Struct,
                            CompletionItemKind::Event,
                            CompletionItemKind::Operator,
                            CompletionItemKind::TypeParameter,
                        ]),
                    }),
                    context_support: Some(false),
                }),
                hover: Some(HoverClientCapabilities {
                    dynamic_registration: Some(false),
                    content_format: Some(vec![MarkupKind::PlainText]),
                }),
                signature_help: Some(SignatureHelpClientCapabilities {
                    dynamic_registration: Some(false),
                    signature_information: Some(SignatureInformationSettings {
                        documentation_format: Some(vec![MarkupKind::PlainText]),
                        parameter_information: Some(ParameterInformationSettings {
                            label_offset_support: Some(false),
                        }),
                        active_parameter_support: None,
                    }),
                    context_support: Some(false),
                }),
                references: Some(DynamicRegistrationClientCapabilities {
                    dynamic_registration: Some(false),
                }),
                document_highlight: Some(DynamicRegistrationClientCapabilities {
                    dynamic_registration: Some(false),
                }),
                document_symbol: Some(DocumentSymbolClientCapabilities {
                    dynamic_registration: Some(false),
                    symbol_kind: None,
                    hierarchical_document_symbol_support: None,
                    tag_support: None,
                }),
                formatting: Some(DynamicRegistrationClientCapabilities {
                    dynamic_registration: Some(false),
                }),
                range_formatting: Some(DynamicRegistrationClientCapabilities {
                    dynamic_registration: Some(false),
                }),
                on_type_formatting: Some(DynamicRegistrationClientCapabilities {
                    dynamic_registration: Some(false),
                }),
                declaration: Some(GotoCapability {
                    dynamic_registration: Some(false),
                    link_support: Some(false),
                }),
                definition: Some(GotoCapability {
                    dynamic_registration: Some(false),
                    link_support: Some(false),
                }),
                type_definition: Some(GotoCapability {
                    dynamic_registration: Some(false),
                    link_support: Some(false),
                }),
                implementation: Some(GotoCapability {
                    dynamic_registration: Some(false),
                    link_support: Some(false),
                }),
                code_action: Some(CodeActionClientCapabilities {
                    dynamic_registration: Some(false),
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
                    is_preferred_support: Some(false),
                    disabled_support: None,
                    data_support: None,
                    resolve_support: None,
                    honors_change_annotations: None,
                }),
                code_lens: Some(DynamicRegistrationClientCapabilities {
                    dynamic_registration: Some(false),
                }),
                document_link: Some(DocumentLinkClientCapabilities {
                    dynamic_registration: Some(false),
                    tooltip_support: Some(false),
                }),
                color_provider: Some(DynamicRegistrationClientCapabilities {
                    dynamic_registration: Some(false),
                }),
                rename: Some(RenameClientCapabilities {
                    dynamic_registration: Some(false),
                    prepare_support: Some(false),
                    prepare_support_default_behavior: None,
                    honors_change_annotations: None,
                }),
                publish_diagnostics: Some(PublishDiagnosticsClientCapabilities {
                    related_information: Some(false),
                    tag_support: None,
                    version_support: None,
                    code_description_support: None,
                    data_support: None,
                }),
                folding_range: None,
                selection_range: None,
                semantic_highlighting_capabilities: Some(SemanticHighlightingClientCapability {
                    semantic_highlighting: true,
                }),
                semantic_tokens: Some(SemanticTokensClientCapabilities {
                    dynamic_registration: Some(false),
                    requests: SemanticTokensClientCapabilitiesRequests {
                        range: Some(false),
                        full: Some(SemanticTokensFullOptions::Bool(true)),
                    },
                    token_types: ctx
                        .config
                        .semantic_tokens
                        .keys()
                        .cloned()
                        .map(|x| x.into())
                        .collect(),
                    token_modifiers: ctx
                        .config
                        .semantic_token_modifiers
                        .keys()
                        .cloned()
                        .map(|x| x.into())
                        .collect(),
                    formats: vec![TokenFormat::RELATIVE],
                    overlapping_token_support: None,
                    multiline_token_support: None,
                }),
                linked_editing_range: None,
                call_hierarchy: None,
                moniker: None,
            }),
            window: Some(WindowClientCapabilities {
                work_done_progress: Some(false),
                show_message: None,
                show_document: None,
            }),
            general: None,
            experimental: None,
        },
        initialization_options,
        process_id: Some(process::id()),
        root_uri: Some(Url::from_file_path(root_path).unwrap()),
        root_path: Some(root_path.to_string()),
        trace: Some(TraceOption::Off),
        workspace_folders: workspace_folders_from_env(),
        client_info: Some(ClientInfo {
            name: env!("CARGO_PKG_NAME").to_owned(),
            version: Some(env!("CARGO_PKG_VERSION").to_owned()),
        }),
        locale: None,
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

    let mut features: Vec<String> = vec![];

    match server_capabilities
        .hover_provider
        .as_ref()
        .unwrap_or(&HoverProviderCapability::Simple(false))
    {
        HoverProviderCapability::Simple(false) => (),
        _ => features.push("lsp-hover".to_string()),
    }

    if server_capabilities.completion_provider.is_some() {
        features.push("lsp-completion (hooked on InsertIdle)".to_string());
    }

    match server_capabilities.definition_provider {
        Some(OneOf::Left(true)) | Some(OneOf::Right(_)) => {
            features.push("lsp-definition (mapped to `gd` by default)".to_string());
        }
        _ => (),
    };

    if server_capabilities.implementation_provider.is_some() {
        features.push("lsp-implementation".to_string());
    }

    match server_capabilities.references_provider {
        Some(OneOf::Left(true)) | Some(OneOf::Right(_)) => {
            features.push("lsp-references (mapped to `gr` by default)".to_string());
        }
        _ => (),
    };

    match server_capabilities.workspace_symbol_provider {
        Some(OneOf::Left(true)) | Some(OneOf::Right(_)) => {
            features.push("lsp-workspace-symbol".to_string());
        }
        _ => (),
    };

    match server_capabilities.document_formatting_provider {
        Some(OneOf::Left(true)) | Some(OneOf::Right(_)) => {
            features.push("lsp-formatting".to_string());
        }
        _ => (),
    };

    match server_capabilities.document_range_formatting_provider {
        Some(OneOf::Left(true)) | Some(OneOf::Right(_)) => {
            features.push("lsp-range-formatting".to_string());
        }
        _ => (),
    };

    if let Some(ref rename_provider) = server_capabilities.rename_provider {
        match rename_provider {
            OneOf::Left(true) | OneOf::Right(_) => features.push("lsp-rename".to_string()),
            _ => (),
        }
    }

    if let Some(ref code_action_provider) = server_capabilities.code_action_provider {
        match code_action_provider {
            CodeActionProviderCapability::Simple(x) => {
                if *x {
                    features.push("lsp-code-actions".to_string());
                }
            }
            CodeActionProviderCapability::Options(_) => {
                features.push("lsp-code-actions".to_string())
            }
        }
    }

    features.push("lsp-diagnostics".to_string());

    if let Some(ref provider) = server_capabilities.semantic_tokens_provider {
        let legend = match provider {
            SemanticTokensServerCapabilities::SemanticTokensOptions(options) => &options.legend,
            SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(regopts) => {
                &regopts.semantic_tokens_options.legend
            }
        };

        features.push(format!(
            "lsp-semantic-tokens:     types: [{}]",
            legend
                .token_types
                .iter()
                .map(SemanticTokenType::as_str)
                .join(", ")
        ));
        features.push(format!(
            "lsp-semantic-tokens: modifiers: [{}]",
            legend
                .token_modifiers
                .iter()
                .map(SemanticTokenModifier::as_str)
                .join(", ")
        ));
    }

    if let Some(ref cap) = server_capabilities.semantic_highlighting {
        if let Some(ref scopes) = cap.scopes {
            features.push(format!(
                "lsp-semantic-highlighting: scopes: [{}]",
                scopes.iter().map(|xs| xs.join(".")).join(", ")
            ));
        }
    }

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

#[cfg(test)]
mod test_initialization {
    use super::*;

    #[test]
    fn test_single_entry() {
        assert!(
            dbg!(workspace_folders_from_string(
                "/home/user/programming/my-awesome-project".to_string()
            )
            .unwrap())
            .len()
                == 1
        );
        assert!(workspace_folders_from_string("".to_string()).is_none());
    }
}
