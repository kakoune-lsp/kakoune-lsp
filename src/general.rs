use crate::context::*;
use crate::controller;
use crate::settings::request_initialization_options_from_kakoune;
use crate::types::*;
use crate::util::*;
use itertools::Itertools;
use lsp_types::notification::*;
use lsp_types::request::*;
use lsp_types::*;
use std::collections::HashSet;
use std::process;
use url::Url;

pub fn initialize(root_path: &str, meta: EditorMeta, ctx: &mut Context) {
    let initialization_options =
        request_initialization_options_from_kakoune(&meta, ctx).or_else(|| {
            ctx.config
                .language
                .get(&ctx.language_id)
                .unwrap()
                .initialization_options
                .clone()
        });
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
                            groups_on_label: None,
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
                workspace_folders: Some(false),
                configuration: Some(true),
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
                        label_details_support: None,
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
                    insert_text_mode: None,
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
                semantic_tokens: Some(SemanticTokensClientCapabilities {
                    dynamic_registration: Some(false),
                    requests: SemanticTokensClientCapabilitiesRequests {
                        range: Some(false),
                        full: Some(SemanticTokensFullOptions::Bool(true)),
                    },
                    token_types: ctx
                        .config
                        .semantic_tokens
                        .iter()
                        .map(|token_config| token_config.token.clone().into())
                        // Collect into set first to remove duplicates
                        .collect::<HashSet<SemanticTokenType>>()
                        .into_iter()
                        .collect(),
                    token_modifiers: ctx
                        .config
                        .semantic_tokens
                        .iter()
                        // Get all modifiers used in token definitions
                        .flat_map(|token_config| token_config.modifiers.clone())
                        // Collect into set first to remove duplicates
                        .collect::<HashSet<SemanticTokenModifier>>()
                        .into_iter()
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
            offset_encoding: Some(vec![match ctx.offset_encoding {
                OffsetEncoding::Utf8 => "utf-8".to_string(),
                OffsetEncoding::Utf16 => "utf-16".to_string(),
            }]),
            experimental: None,
        },
        initialization_options,
        process_id: Some(process::id()),
        root_uri: Some(Url::from_file_path(root_path).unwrap()),
        root_path: Some(root_path.to_string()),
        trace: Some(TraceOption::Off),
        workspace_folders: None,
        client_info: Some(ClientInfo {
            name: env!("CARGO_PKG_NAME").to_owned(),
            version: Some(env!("CARGO_PKG_VERSION").to_owned()),
        }),
        locale: None,
    };

    ctx.call::<Initialize, _>(meta, params, move |ctx: &mut Context, _meta, result| {
        ctx.capabilities = Some(result.capabilities);
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

    let command = format!(
        "info 'kak-lsp commands supported by {} language server:\n\n{}'",
        ctx.language_id,
        editor_escape(&features.join("\n"))
    );
    ctx.exec(meta, command);
}
