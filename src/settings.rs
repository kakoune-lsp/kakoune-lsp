use crate::context::*;
use crate::editor_transport::ToEditorSender;
use crate::types::*;
use serde_json::Value;

pub fn initialization_options(
    servers: &[ServerId],
    meta: &EditorMeta,
    ctx: &mut Context,
) -> Vec<Option<Value>> {
    let mut sections = Vec::with_capacity(servers.len());
    for &server_id in servers {
        let server_name = &ctx.server(server_id).name;
        let settings = ctx
            .dynamic_config
            .language_server
            .get(server_name)
            .and_then(|v| v.settings.as_ref());
        let settings = configured_section(meta, ctx, false, server_id, settings);
        if settings.is_some() {
            sections.push(settings);
            continue;
        }

        let legacy_settings = legacy_initialization_options(ctx, meta);
        if legacy_settings.is_some() {
            sections.push(legacy_settings);
            continue;
        }

        let server_name = &ctx.server(server_id).name;
        let server_config = ctx.server_config(meta, server_name).unwrap();
        let settings =
            configured_section(meta, ctx, false, server_id, server_config.settings.as_ref());
        sections.push(settings);
    }
    sections
}

pub fn configured_section(
    meta: &EditorMeta,
    ctx: &Context,
    for_did_change_configuration: bool,
    server_id: ServerId,
    settings: Option<&Value>,
) -> Option<Value> {
    let server_name = &ctx.server(server_id).name;
    settings.and_then(|settings| {
        ctx.server_config(meta, server_name)
            .and_then(|cfg| {
                cfg.settings_section.as_ref().map(|section| {
                    (
                        section,
                        if for_did_change_configuration {
                            cfg.workspace_did_change_configuration_subsection.as_ref()
                        } else {
                            None
                        },
                    )
                })
            })
            .and_then(|(section, subsection)| {
                settings
                    .get(section)
                    .and_then(|section| {
                        if let Some(subsection) = subsection {
                            section.get(subsection)
                        } else {
                            Some(section)
                        }
                    })
                    .cloned()
            })
    })
}

pub fn record_dynamic_config(meta: &EditorMeta, ctx: &mut Context, config: &str) {
    debug!(ctx.to_editor(), "lsp_config:\n{}", config);
    match toml::from_str(config) {
        Ok(cfg) => {
            ctx.dynamic_config = cfg;
        }
        Err(e) => {
            let msg = format!("failed to parse %opt{{lsp_config}}: {}", e);
            ctx.show_error(meta.clone(), msg);
        }
    };
    if !is_using_legacy_toml(&ctx.config) {
        for (server_name, server) in &meta.language_server {
            let server_id = ctx
                .route_cache
                .get(&(server_name.clone(), server.root.clone()))
                .unwrap();
            let server_config = ctx.language_servers.get_mut(server_id).unwrap();
            server_config.settings.clone_from(&server.settings);
            server_config.workaround_eslint = server.workaround_eslint.unwrap_or_default();
        }
    }
}

/// User may override initialization options on per-language server basis
/// with `lsp_server_initialization_options` option in Kakoune
/// (i.e. to customize it for specific project).
/// This function asks Kakoune to give such override if any.
fn legacy_initialization_options(ctx: &Context, meta: &EditorMeta) -> Option<Value> {
    #[allow(deprecated)]
    if meta.legacy_server_initialization_options.is_empty() {
        None
    } else {
        Some(Value::Object(explode_str_to_str_map(
            ctx.to_editor(),
            &meta.legacy_server_initialization_options,
        )))
    }
}

fn insert_value<'a, 'b, P>(
    target: &'b mut serde_json::Map<String, Value>,
    mut path: P,
    local_key: String,
    value: Value,
) -> Result<(), String>
where
    P: Iterator<Item = &'a str>,
    P: 'a,
{
    match path.next() {
        Some(key) => {
            let maybe_new_target = target
                .entry(key)
                .or_insert_with(|| Value::Object(serde_json::Map::new()))
                .as_object_mut();

            if maybe_new_target.is_none() {
                return Err(format!(
                    "Expected path {:?} to be object, found {:?}",
                    key, &maybe_new_target,
                ));
            }

            insert_value(maybe_new_target.unwrap(), path, local_key, value)
        }
        None => match target.insert(local_key, value) {
            Some(old_value) => Err(format!("Replaced old value: {:?}", old_value)),
            None => Ok(()),
        },
    }
}
// Take flattened tables like "a.b=1" and produce "{"a":{"b":1}}".
pub fn explode_str_to_str_map(
    to_editor: &ToEditorSender,
    map: &[String],
) -> serde_json::value::Map<String, Value> {
    let mut settings = serde_json::Map::new();

    for map_entry in map.iter() {
        let (raw_key, raw_value) = map_entry.split_once('=').unwrap();
        let mut key_parts = raw_key.split('.');
        let local_key = match key_parts.next_back() {
            Some(name) => name,
            None => {
                warn!(
                    to_editor,
                    "Got a setting with an empty local name: {:?}", raw_key
                );
                continue;
            }
        };
        let toml_value: toml::Value = match toml::from_str(raw_value) {
            Ok(toml_value) => toml_value,
            Err(e) => {
                warn!(
                    to_editor,
                    "Could not parse TOML setting {:?}: {}", raw_value, e
                );
                continue;
            }
        };

        let value: Value = match toml_value.try_into() {
            Ok(value) => value,
            Err(e) => {
                warn!(
                    to_editor,
                    "Could not convert setting {:?} to JSON: {}", raw_value, e
                );
                continue;
            }
        };

        match insert_value(&mut settings, key_parts, local_key.into(), value) {
            Ok(_) => (),
            Err(e) => {
                warn!(
                    to_editor,
                    "Could not set {:?} to {:?}: {}", raw_key, raw_value, e
                );
                continue;
            }
        }
    }

    settings
}
