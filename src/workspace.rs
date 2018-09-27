use context::*;
use languageserver_types::DidChangeConfigurationParams;
use serde_json;
use toml;
use types::*;

use languageserver_types::notification::{self, Notification};

fn insert_value<'a, 'b, P>(
    target: &'b mut serde_json::map::Map<String, serde_json::Value>,
    mut path: P,
    local_key: String,
    value: serde_json::Value,
) -> Result<(), String>
    where P: Iterator<Item=&'a str>,
    P: 'a
{
    match path.next() {
        Some(key) => {
            let mut maybe_new_target = target
                .entry(key)
                .or_insert_with(|| serde_json::Value::Object(
                    serde_json::Map::new()
                )).as_object_mut();

            if maybe_new_target.is_none() {
                return Err(format!(
                    "Expected path {:?} to be object, found {:?}",
                    key,
                    &maybe_new_target,
                ));
            }

            insert_value(
                maybe_new_target.unwrap(),
                path,
                local_key,
                value,
            )
        }
        None => {
            match target.insert(local_key, value) {
                Some(old_value) => Err(format!("Replaced old value: {:?}", old_value)),
                None => Ok(()),
            }
        }
    }
}

pub fn did_change_configuration(
    params: EditorParams,
    _meta: &EditorMeta,
    ctx: &mut Context,
) {
    let default_settings = toml::value::Table::new();

    let raw_settings = params
        .as_table()
        .and_then(|t| t.get("settings"))
        .and_then(|val| val.as_table())
        .unwrap_or_else(|| &default_settings);

    let mut settings = serde_json::Map::new();

    for (raw_key, raw_value) in raw_settings.iter() {
        let mut key_parts = raw_key.split('.');
        let local_key = match key_parts.next_back() {
            Some(name) => name,
            None => {
                warn!("Got a setting with an empty local name: {:?}", raw_key);
                continue
            }
        };

        let value: serde_json::Value = match raw_value.clone().try_into() {
            Ok(value) => value,
            Err(e) => {
                warn!("Could not convert setting {:?} to JSON: {}", raw_value, e,);
                continue
            }
        };

        match insert_value(&mut settings, key_parts, local_key.into(), value) {
            Ok(_) => (),
            Err(e) => {
                warn!("Could not set {:?} to {:?}: {}", raw_key, raw_value, e);
                continue
            }
        }
    }

    let params = DidChangeConfigurationParams {
        settings: serde_json::Value::Object(settings)
    };
    ctx.notify(notification::DidChangeConfiguration::METHOD.into(), params);
}
