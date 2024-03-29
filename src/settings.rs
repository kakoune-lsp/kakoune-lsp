use crate::context::*;
use crate::types::*;
use crate::util::*;
use serde_json::Value;

pub fn request_dynamic_configuration_from_kakoune(
    meta: &EditorMeta,
    ctx: &mut Context,
) -> Option<()> {
    let fifo = temp_fifo();
    ctx.exec(
        meta.clone(),
        format!("lsp-get-config {}", editor_quote(&fifo.path)),
    );
    let config = std::fs::read_to_string(&fifo.path).unwrap();
    record_dynamic_config(meta, ctx, &config);
    Some(())
}

pub fn request_initialization_options_from_kakoune(
    meta: &EditorMeta,
    ctx: &mut Context,
) -> Vec<Option<Value>> {
    request_dynamic_configuration_from_kakoune(meta, ctx);
    let mut sections = Vec::with_capacity(ctx.language_servers.len());
    let servers: Vec<_> = ctx.language_servers.keys().cloned().collect();
    for server_name in &servers {
        let settings = ctx
            .dynamic_config
            .language_server
            .get(server_name)
            .and_then(|v| v.settings.as_ref());
        let settings = configured_section(ctx, server_name, settings);
        if settings.is_some() {
            sections.push(settings);
            continue;
        }

        let legacy_settings = request_legacy_initialization_options_from_kakoune(meta, ctx);
        if legacy_settings.is_some() {
            sections.push(legacy_settings);
            continue;
        }

        let lang = ctx.config.language_server.get(server_name).unwrap();
        let settings = configured_section(ctx, server_name, lang.settings.as_ref());
        sections.push(settings);
    }
    sections
}

pub fn configured_section(
    ctx: &Context,
    server_name: &ServerName,
    settings: Option<&Value>,
) -> Option<Value> {
    settings.and_then(|settings| {
        ctx.config
            .language_server
            .get(server_name)
            .and_then(|cfg| cfg.settings_section.as_ref())
            .and_then(|section| settings.get(section).cloned())
    })
}

pub fn record_dynamic_config(meta: &EditorMeta, ctx: &mut Context, config: &str) {
    debug!("lsp_config:\n{}", config);
    match toml::from_str(config) {
        Ok(cfg) => {
            ctx.dynamic_config = cfg;
        }
        Err(e) => {
            let msg = format!("failed to parse %opt{{lsp_config}}: {}", e);
            ctx.exec(
                meta.clone(),
                format!("lsp-show-error {}", editor_quote(&msg)),
            );
            panic!("{}", msg)
        }
    };
}

/// User may override initialization options provided in kak-lsp.toml on per-language server basis
/// with `lsp_server_initialization_options` option in Kakoune
/// (i.e. to customize it for specific project).
/// This function asks Kakoune to give such override if any.
pub fn request_legacy_initialization_options_from_kakoune(
    meta: &EditorMeta,
    ctx: &mut Context,
) -> Option<Value> {
    let fifo = temp_fifo();
    ctx.exec(
        meta.clone(),
        format!(
            "lsp-get-server-initialization-options {}",
            editor_quote(&fifo.path)
        ),
    );
    let options = std::fs::read_to_string(&fifo.path).unwrap();
    debug!("lsp_server_initialization_options:\n{}", options);
    if options.trim().is_empty() {
        None
    } else {
        match toml::from_str::<toml::value::Table>(&options) {
            Ok(table) => Some(Value::Object(explode_string_table(&table))),
            Err(e) => {
                error!("Failed to parse lsp_server_initialization_options: {:?}", e);
                None
            }
        }
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
// Take flattened tables like "a.b = 1" and produce "{"a":{"b":1}}".
pub fn explode_string_table(
    raw_settings: &toml::value::Table,
) -> serde_json::value::Map<String, Value> {
    let mut settings = serde_json::Map::new();

    for (raw_key, raw_value) in raw_settings.iter() {
        let mut key_parts = raw_key.split('.');
        let local_key = match key_parts.next_back() {
            Some(name) => name,
            None => {
                warn!("Got a setting with an empty local name: {:?}", raw_key);
                continue;
            }
        };

        let value: Value = match raw_value.clone().try_into() {
            Ok(value) => value,
            Err(e) => {
                warn!("Could not convert setting {:?} to JSON: {}", raw_value, e,);
                continue;
            }
        };

        match insert_value(&mut settings, key_parts, local_key.into(), value) {
            Ok(_) => (),
            Err(e) => {
                warn!("Could not set {:?} to {:?}: {}", raw_key, raw_value, e);
                continue;
            }
        }
    }

    settings
}
