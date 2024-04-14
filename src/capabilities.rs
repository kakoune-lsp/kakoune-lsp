use crate::context::*;
use crate::controller;
use crate::settings::request_initialization_options_from_kakoune;
use crate::types::*;
use crate::util::*;
use indoc::formatdoc;
use itertools::Itertools;
use lsp_types::notification::*;
use lsp_types::request::*;
use lsp_types::*;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::process;
use url::Url;

pub fn initialize(meta: EditorMeta, ctx: &mut Context) {
    let initialization_options = request_initialization_options_from_kakoune(&meta, ctx);
    let symbol_kind_capability = Some(SymbolKindCapability {
        value_set: Some(vec![
            SymbolKind::FILE,
            SymbolKind::MODULE,
            SymbolKind::NAMESPACE,
            SymbolKind::PACKAGE,
            SymbolKind::CLASS,
            SymbolKind::METHOD,
            SymbolKind::PROPERTY,
            SymbolKind::FIELD,
            SymbolKind::CONSTRUCTOR,
            SymbolKind::ENUM,
            SymbolKind::INTERFACE,
            SymbolKind::FUNCTION,
            SymbolKind::VARIABLE,
            SymbolKind::CONSTANT,
            SymbolKind::STRING,
            SymbolKind::NUMBER,
            SymbolKind::BOOLEAN,
            SymbolKind::ARRAY,
            SymbolKind::OBJECT,
            SymbolKind::KEY,
            SymbolKind::NULL,
            SymbolKind::ENUM_MEMBER,
            SymbolKind::STRUCT,
            SymbolKind::EVENT,
            SymbolKind::OPERATOR,
            SymbolKind::TYPE_PARAMETER,
        ]),
    });
    #[allow(deprecated)] // for root_path
    let req_params = ctx
        .language_servers
        .iter()
        .enumerate()
        .map(
            |(
                idx,
                (
                    server_name,
                    ServerSettings {
                        root_path,
                        preferred_offset_encoding,
                        ..
                    },
                ),
            )| {
                (
                    server_name.clone(),
                    vec![InitializeParams {
                        capabilities: ClientCapabilities {
                            workspace: Some(WorkspaceClientCapabilities {
                                apply_edit: Some(true),
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
                                did_change_configuration: Some(
                                    DynamicRegistrationClientCapabilities {
                                        dynamic_registration: Some(false),
                                    },
                                ),
                                did_change_watched_files: ctx.config.file_watch_support.then_some(
                                    DidChangeWatchedFilesClientCapabilities {
                                        dynamic_registration: Some(true),
                                        relative_pattern_support: Some(true),
                                    },
                                ),
                                symbol: Some(WorkspaceSymbolClientCapabilities {
                                    dynamic_registration: Some(false),
                                    symbol_kind: symbol_kind_capability.clone(),
                                    tag_support: None,
                                    resolve_support: None,
                                }),
                                execute_command: Some(DynamicRegistrationClientCapabilities {
                                    dynamic_registration: Some(false),
                                }),
                                workspace_folders: Some(true),
                                configuration: Some(true),
                                semantic_tokens: None,
                                code_lens: Some(CodeLensWorkspaceClientCapabilities {
                                    refresh_support: None,
                                }),
                                file_operations: None,
                                inline_value: None,
                                inlay_hint: Some(InlayHintWorkspaceClientCapabilities {
                                    refresh_support: Some(false),
                                }),
                                diagnostic: None,
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
                                        documentation_format: Some(vec![
                                            MarkupKind::Markdown,
                                            MarkupKind::PlainText,
                                        ]),
                                        deprecated_support: Some(false),
                                        preselect_support: Some(false),
                                        tag_support: None,
                                        insert_replace_support: None,
                                        resolve_support: Some(
                                            CompletionItemCapabilityResolveSupport {
                                                properties: vec![
                                                    "additionalTextEdits".to_string(),
                                                    "detail".to_string(),
                                                    "documentation".to_string(),
                                                ],
                                            },
                                        ),
                                        insert_text_mode_support: None,
                                        label_details_support: None,
                                    }),
                                    completion_item_kind: Some(CompletionItemKindCapability {
                                        value_set: Some(vec![
                                            CompletionItemKind::TEXT,
                                            CompletionItemKind::METHOD,
                                            CompletionItemKind::FUNCTION,
                                            CompletionItemKind::CONSTRUCTOR,
                                            CompletionItemKind::FIELD,
                                            CompletionItemKind::VARIABLE,
                                            CompletionItemKind::CLASS,
                                            CompletionItemKind::INTERFACE,
                                            CompletionItemKind::MODULE,
                                            CompletionItemKind::PROPERTY,
                                            CompletionItemKind::UNIT,
                                            CompletionItemKind::VALUE,
                                            CompletionItemKind::ENUM,
                                            CompletionItemKind::KEYWORD,
                                            CompletionItemKind::SNIPPET,
                                            CompletionItemKind::COLOR,
                                            CompletionItemKind::FILE,
                                            CompletionItemKind::REFERENCE,
                                            CompletionItemKind::FOLDER,
                                            CompletionItemKind::ENUM_MEMBER,
                                            CompletionItemKind::CONSTANT,
                                            CompletionItemKind::STRUCT,
                                            CompletionItemKind::EVENT,
                                            CompletionItemKind::OPERATOR,
                                            CompletionItemKind::TYPE_PARAMETER,
                                        ]),
                                    }),
                                    context_support: Some(false),
                                    insert_text_mode: None,
                                    completion_list: None,
                                }),
                                hover: Some(HoverClientCapabilities {
                                    dynamic_registration: Some(false),
                                    content_format: Some(vec![
                                        MarkupKind::Markdown,
                                        MarkupKind::PlainText,
                                    ]),
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
                                    symbol_kind: symbol_kind_capability.clone(),
                                    hierarchical_document_symbol_support: Some(true),
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
                                                "source.fixAll",
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
                                    resolve_support: Some(CodeActionCapabilityResolveSupport {
                                        properties: ["edit"]
                                            .iter()
                                            .map(|s| s.to_string())
                                            .collect(),
                                    }),
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
                                    related_information: Some(true),
                                    tag_support: None,
                                    version_support: None,
                                    code_description_support: None,
                                    data_support: None,
                                }),
                                folding_range: None,
                                selection_range: Some(SelectionRangeClientCapabilities {
                                    dynamic_registration: None,
                                }),
                                semantic_tokens: Some(SemanticTokensClientCapabilities {
                                    dynamic_registration: Some(true),
                                    requests: SemanticTokensClientCapabilitiesRequests {
                                        range: Some(false),
                                        full: Some(SemanticTokensFullOptions::Bool(true)),
                                    },
                                    token_types: ctx
                                        .config
                                        .semantic_tokens
                                        .faces
                                        .iter()
                                        .map(|token_config| token_config.token.clone().into())
                                        // Collect into set first to remove duplicates
                                        .collect::<HashSet<SemanticTokenType>>()
                                        .into_iter()
                                        .collect(),
                                    token_modifiers: ctx
                                        .config
                                        .semantic_tokens
                                        .faces
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
                                    augments_syntax_tokens: None,
                                    server_cancel_support: Some(true),
                                }),
                                linked_editing_range: None,
                                call_hierarchy: Some(CallHierarchyClientCapabilities {
                                    dynamic_registration: Some(false),
                                }),
                                moniker: None,
                                inline_value: None,
                                type_hierarchy: None,
                                inlay_hint: Some(InlayHintClientCapabilities {
                                    dynamic_registration: Some(false),
                                    resolve_support: None,
                                }),
                                diagnostic: None,
                                inline_completion: None,
                            }),
                            window: Some(WindowClientCapabilities {
                                work_done_progress: Some(true),
                                show_message: Some(ShowMessageRequestClientCapabilities {
                                    message_action_item: Some(MessageActionItemCapabilities {
                                        additional_properties_support: Some(true),
                                    }),
                                }),
                                show_document: None,
                            }),
                            general: Some(GeneralClientCapabilities {
                                regular_expressions: Some(RegularExpressionsClientCapabilities {
                                    engine: "Rust regex".to_string(),
                                    version: None,
                                }),
                                markdown: Some(MarkdownClientCapabilities {
                                    parser: "kakoune-lsp".to_string(),
                                    version: Some(env!("CARGO_PKG_VERSION").to_string()),
                                    allowed_tags: None,
                                }),
                                stale_request_support: None,
                                position_encodings: Some(match preferred_offset_encoding {
                                    None | Some(OffsetEncoding::Utf8) => {
                                        vec![
                                            PositionEncodingKind::UTF8,
                                            PositionEncodingKind::UTF16,
                                        ]
                                    }
                                    Some(OffsetEncoding::Utf16) => {
                                        vec![
                                            PositionEncodingKind::UTF16,
                                            PositionEncodingKind::UTF8,
                                        ]
                                    }
                                }),
                            }),
                            offset_encoding: Some(
                                match preferred_offset_encoding {
                                    None | Some(OffsetEncoding::Utf8) => ["utf-8", "utf-16"],
                                    Some(OffsetEncoding::Utf16) => ["utf-16", "utf-8"],
                                }
                                .iter()
                                .map(|s| s.to_string())
                                .collect(),
                            ),
                            experimental: Some(serde_json::json!({
                                "hoverActions": true,
                                "commands": {
                                    "commands": [
                                        "rust-analyzer.runSingle",
                                    ]
                                }
                            })),
                        },
                        initialization_options: initialization_options[idx].clone(),
                        process_id: Some(process::id()),
                        root_uri: Some(Url::from_file_path(root_path).unwrap()),
                        root_path: Some(root_path.to_string()),
                        trace: Some(TraceValue::Off),
                        workspace_folders: Some(vec![WorkspaceFolder {
                            uri: Url::from_file_path(root_path).unwrap(),
                            name: root_path.to_string(),
                        }]),
                        client_info: Some(ClientInfo {
                            name: "kakoune-lsp".to_string(),
                            version: Some(env!("CARGO_PKG_VERSION").to_string()),
                        }),
                        locale: None,
                        work_done_progress_params: WorkDoneProgressParams {
                            work_done_token: None,
                        },
                    }],
                )
            },
        )
        .collect();

    ctx.call::<Initialize, _>(meta, RequestParams::Each(req_params) , move |ctx, _meta, results| {
        let results: HashMap<_,_> = results.into_iter().collect();
        let servers: Vec<_> = ctx.language_servers.keys().cloned().collect();

        for server_name in &servers {
            let result = &results[server_name];
            if let Some(server) = ctx.language_servers.get_mut(server_name) {
                server.offset_encoding = result
                    .capabilities
                    .position_encoding
                    .as_ref()
                    .map(|enc| enc.as_str())
                    .or(result.offset_encoding.as_deref())
                    .map(|encoding| match encoding {
                        "utf-8" => OffsetEncoding::Utf8,
                        "utf-16" => OffsetEncoding::Utf16,
                        encoding => {
                            error!("Language server sent unsupported offset encoding: {encoding}");
                            OffsetEncoding::Utf16
                        }
                    })
                    .unwrap_or_default();
                if matches!(
                    (server.preferred_offset_encoding, server.offset_encoding),
                    (Some(OffsetEncoding::Utf8), OffsetEncoding::Utf16)) {
                        warn!(
                            "Requested offset encoding utf-8 is not supported by {} server, falling back to utf-16",
                            server_name,
                        );
                }
                server.capabilities = Some(result.capabilities.clone());
                ctx.notify::<Initialized>(server_name, InitializedParams {});
            }
        }
        controller::dispatch_pending_editor_requests(ctx)
    });
}

pub const CAPABILITY_CALL_HIERARCHY: &str = "lsp-incoming-calls, lsp-outgoing-calls";
pub const CAPABILITY_CODE_ACTIONS: &str = "lsp-code-actions";
pub const CAPABILITY_CODE_ACTIONS_RESOLVE: &str = "lsp-code-actions-resolve";
pub const CAPABILITY_CODE_LENS: &str = "lsp-code-lens";
pub const CAPABILITY_COMPLETION: &str = "lsp-completion (hooked on InsertIdle)";
pub const CAPABILITY_DEFINITION: &str = "lsp-definition (mapped to `gd` by default)";
pub const CAPABILITY_DOCUMENT_HIGHLIGHT: &str = "lsp-highlight-references";
pub const CAPABILITY_DOCUMENT_SYMBOL: &str = "lsp-document-symbol";
pub const CAPABILITY_EXECUTE_COMMANDS: &str = "lsp-execute-commands";
pub const CAPABILITY_FORMATTING: &str = "lsp-formatting";
pub const CAPABILITY_HOVER: &str = "lsp-hover";
pub const CAPABILITY_IMPLEMENTATION: &str = "lsp-implementation";
pub const CAPABILITY_INLAY_HINTS: &str = "lsp-inlay-hints";
pub const CAPABILITY_RANGE_FORMATTING: &str = "lsp-range-formatting";
pub const CAPABILITY_REFERENCES: &str = "lsp-references (mapped to `gr` by default)";
pub const CAPABILITY_RENAME: &str = "lsp-rename";
pub const CAPABILITY_SELECTION_RANGE: &str = "lsp-selection-range";
pub const CAPABILITY_SEMANTIC_TOKENS: &str = "lsp-semantic-tokens";
pub const CAPABILITY_SIGNATURE_HELP: &str = "lsp-signature-help";
pub const CAPABILITY_TYPE_DEFINITION: &str = "lsp-type-definition";
pub const CAPABILITY_WORKSPACE_SYMBOL: &str = "lsp-workspace-symbol";

pub fn attempt_server_capability(
    server: (&ServerName, &ServerSettings),
    meta: &EditorMeta,
    feature: &'static str,
) -> bool {
    let (server_name, server_settings) = server;
    if server_has_capability(server_settings, feature) {
        return true;
    }

    if !meta.hook {
        warn!("{server_name} server does not support {feature}, refusing to send request");
    }

    false
}

pub fn server_has_capability(server: &ServerSettings, feature: &'static str) -> bool {
    let server_capabilities = match &server.capabilities {
        Some(caps) => caps,
        None => return false,
    };

    match feature {
        CAPABILITY_CODE_ACTIONS => match server_capabilities.code_action_provider {
            Some(CodeActionProviderCapability::Simple(ok)) => ok,
            Some(_) => true,
            None => false,
        },
        CAPABILITY_CODE_ACTIONS_RESOLVE => matches!(
            server_capabilities.code_action_provider,
            Some(CodeActionProviderCapability::Options(CodeActionOptions {
                resolve_provider: Some(true),
                ..
            }))
        ),
        CAPABILITY_CODE_LENS => server_capabilities.code_lens_provider.is_some(),
        CAPABILITY_CALL_HIERARCHY => match server_capabilities.call_hierarchy_provider {
            Some(CallHierarchyServerCapability::Simple(ok)) => ok,
            Some(_) => true,
            None => false,
        },
        CAPABILITY_COMPLETION => server_capabilities.completion_provider.is_some(),
        CAPABILITY_SIGNATURE_HELP => server_capabilities.signature_help_provider.is_some(),
        CAPABILITY_DEFINITION => match server_capabilities.definition_provider {
            Some(OneOf::Left(ok)) => ok,
            Some(OneOf::Right(_)) => true,
            None => false,
        },
        CAPABILITY_DOCUMENT_HIGHLIGHT => match server_capabilities.document_highlight_provider {
            Some(OneOf::Left(ok)) => ok,
            Some(OneOf::Right(_)) => true,
            None => false,
        },
        CAPABILITY_DOCUMENT_SYMBOL => match server_capabilities.document_symbol_provider {
            Some(OneOf::Left(ok)) => ok,
            Some(OneOf::Right(_)) => true,
            None => false,
        },
        CAPABILITY_FORMATTING => match server_capabilities.document_formatting_provider {
            Some(OneOf::Left(ok)) => ok,
            Some(OneOf::Right(_)) => true,
            None => false,
        },
        CAPABILITY_HOVER => match server_capabilities.hover_provider {
            Some(HoverProviderCapability::Simple(ok)) => ok,
            Some(_) => true,
            None => false,
        },
        CAPABILITY_IMPLEMENTATION => match server_capabilities.implementation_provider {
            Some(ImplementationProviderCapability::Simple(ok)) => ok,
            Some(_) => true,
            None => false,
        },
        CAPABILITY_INLAY_HINTS => match server_capabilities.inlay_hint_provider {
            Some(OneOf::Left(ok)) => ok,
            Some(OneOf::Right(_)) => true,
            None => false,
        },
        CAPABILITY_RANGE_FORMATTING => match server_capabilities.document_range_formatting_provider
        {
            Some(OneOf::Left(ok)) => ok,
            Some(OneOf::Right(_)) => true,
            None => false,
        },
        CAPABILITY_REFERENCES => match server_capabilities.references_provider {
            Some(OneOf::Left(ok)) => ok,
            Some(OneOf::Right(_)) => true,
            None => false,
        },
        CAPABILITY_RENAME => match server_capabilities.rename_provider {
            Some(OneOf::Left(ok)) => ok,
            Some(OneOf::Right(_)) => true,
            None => false,
        },
        CAPABILITY_SELECTION_RANGE => match server_capabilities.selection_range_provider {
            Some(SelectionRangeProviderCapability::Simple(ok)) => ok,
            Some(_) => true,
            None => false,
        },
        CAPABILITY_SEMANTIC_TOKENS => server_capabilities.semantic_tokens_provider.is_some(),
        CAPABILITY_EXECUTE_COMMANDS => server_capabilities.execute_command_provider.is_some(),
        CAPABILITY_TYPE_DEFINITION => match server_capabilities.type_definition_provider {
            Some(TypeDefinitionProviderCapability::Simple(ok)) => ok,
            Some(_) => true,
            None => false,
        },
        CAPABILITY_WORKSPACE_SYMBOL => match server_capabilities.workspace_symbol_provider {
            Some(OneOf::Left(ok)) => ok,
            Some(OneOf::Right(_)) => true,
            None => false,
        },
        _ => panic!("BUG: missing case"),
    }
}

pub fn capabilities(meta: EditorMeta, ctx: &mut Context) {
    let mut features: BTreeMap<String, Vec<&ServerName>> = BTreeMap::new();

    fn probe_feature<'a>(
        server: (&'a ServerName, &'a ServerSettings),
        features: &mut BTreeMap<String, Vec<&'a ServerName>>,
        feature: &'static str,
    ) {
        let (server_name, server_settings) = server;
        if server_has_capability(server_settings, feature) {
            features
                .entry(feature.to_string())
                .or_default()
                .push(server_name);
        }
    }

    for entry in &ctx.language_servers {
        let (server_name, server_settings) = entry;

        probe_feature(entry, &mut features, CAPABILITY_SELECTION_RANGE);
        probe_feature(entry, &mut features, CAPABILITY_HOVER);
        probe_feature(entry, &mut features, CAPABILITY_COMPLETION);
        probe_feature(entry, &mut features, CAPABILITY_SIGNATURE_HELP);
        probe_feature(entry, &mut features, CAPABILITY_DEFINITION);
        probe_feature(entry, &mut features, CAPABILITY_TYPE_DEFINITION);
        probe_feature(entry, &mut features, CAPABILITY_IMPLEMENTATION);
        probe_feature(entry, &mut features, CAPABILITY_REFERENCES);
        probe_feature(entry, &mut features, CAPABILITY_DOCUMENT_HIGHLIGHT);
        if server_has_capability(server_settings, CAPABILITY_DOCUMENT_SYMBOL) {
            features
                .entry("lsp-document-symbol, lsp-object, lsp-goto-document-symbol".to_string())
                .or_default()
                .push(server_name);
        }
        probe_feature(entry, &mut features, CAPABILITY_WORKSPACE_SYMBOL);
        probe_feature(entry, &mut features, CAPABILITY_FORMATTING);
        probe_feature(entry, &mut features, CAPABILITY_RANGE_FORMATTING);
        probe_feature(entry, &mut features, CAPABILITY_RENAME);
        probe_feature(entry, &mut features, CAPABILITY_CODE_ACTIONS);
        probe_feature(entry, &mut features, CAPABILITY_CODE_ACTIONS_RESOLVE);
        probe_feature(entry, &mut features, CAPABILITY_CODE_LENS);
        probe_feature(entry, &mut features, CAPABILITY_CALL_HIERARCHY);
        features
            .entry("lsp-diagnostics".to_string())
            .or_default()
            .push(server_name);
        probe_feature(entry, &mut features, CAPABILITY_INLAY_HINTS);

        // NOTE controller should park request for capabilities until they are available thus it should
        // be safe to unwrap here (otherwise something unexpectedly wrong and it's better to panic)
        let server_capabilities = server_settings.capabilities.as_ref().unwrap();

        if let Some(ref provider) = server_capabilities.execute_command_provider {
            features
                .entry(format!(
                    "lsp-execute-command: commands: [{}]",
                    provider.commands.iter().join(", ")
                ))
                .or_default()
                .push(server_name);
        }

        if let Some(ref provider) = server_capabilities.semantic_tokens_provider {
            let legend = match provider {
                SemanticTokensServerCapabilities::SemanticTokensOptions(options) => &options.legend,
                SemanticTokensServerCapabilities::SemanticTokensRegistrationOptions(regopts) => {
                    &regopts.semantic_tokens_options.legend
                }
            };

            features
                .entry(format!(
                    "lsp-semantic-tokens: types: [{}]",
                    legend
                        .token_types
                        .iter()
                        .map(SemanticTokenType::as_str)
                        .join(", ")
                ))
                .or_default()
                .push(server_name);
            features
                .entry(format!(
                    "lsp-semantic-tokens: modifiers: [{}]",
                    legend
                        .token_modifiers
                        .iter()
                        .map(SemanticTokenModifier::as_str)
                        .join(", ")
                ))
                .or_default()
                .push(server_name);
        }
    }

    let command = formatdoc!(
        "info 'LSP commands supported by language servers ({}):

         {}'",
        editor_escape(&ctx.language_servers.keys().join(", ")),
        editor_escape(
            &features
                .into_iter()
                .map(|(feature, server_names)| {
                    if ctx.language_servers.len() > 1 {
                        format!("{} [{}]", feature, server_names.iter().join(", "))
                    } else {
                        feature
                    }
                })
                .join("\n")
        )
    );
    ctx.exec(meta, command);
}
