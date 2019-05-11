use crate::context::*;
use crate::types::*;
use crate::util::*;
use jsonrpc_core::{Id, Params};
use lsp_types::notification::*;
use lsp_types::request::*;
use lsp_types::*;
use serde::Deserialize;
use serde_json::{self, Value};
use toml;

fn insert_value<'a, 'b, P>(
    target: &'b mut serde_json::map::Map<String, Value>,
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

pub fn did_change_configuration(params: EditorParams, ctx: &mut Context) {
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

    let params = DidChangeConfigurationParams {
        settings: Value::Object(settings),
    };
    ctx.notify::<DidChangeConfiguration>(params);
}

pub fn workspace_symbol(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = WorkspaceSymbolParams::deserialize(params)
        .expect("Params should follow WorkspaceSymbolParams structure");
    ctx.call::<WorkspaceSymbol, _>(meta, params, move |ctx: &mut Context, meta, result| {
        editor_workspace_symbol(meta, result, ctx)
    });
}

pub fn editor_workspace_symbol(
    meta: EditorMeta,
    result: Option<Vec<SymbolInformation>>,
    ctx: &mut Context,
) {
    if result.is_none() {
        return;
    }
    let result = result.unwrap();
    let content = format_symbol_information(result, ctx);
    let command = format!(
        "lsp-show-workspace-symbol {} {}",
        editor_quote(&ctx.root_path),
        editor_quote(&content),
    );
    ctx.exec(meta, command);
}

#[derive(Deserialize)]
struct EditorExecuteCommand {
    command: String,
    arguments: String,
}

pub fn execute_command(meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = EditorExecuteCommand::deserialize(params)
        .expect("Params should follow ExecuteCommand structure");
    let req_params = ExecuteCommandParams {
        command: params.command,
        // arguments is quoted to avoid parsing issues
        arguments: serde_json::from_str(&params.arguments).unwrap(),
    };
    ctx.call::<ExecuteCommand, _>(meta, req_params, move |_: &mut Context, _, _| ());
}

pub fn apply_edit(id: Id, params: Option<Params>, ctx: &mut Context) {
    let params: ApplyWorkspaceEditParams = params.unwrap().parse().expect("Failed to parse params");
    let meta = ctx.meta_for_session();
    let applied = if let Some(changes) = params.edit.changes {
        for (url, edits) in changes {
            apply_text_edits(&meta, &url, &edits, &ctx);
        }
        true
    } else {
        warn!("kak-lsp doesn't yet support DocumentChanges");
        false
    };
    ctx.reply(
        id,
        Ok(serde_json::to_value(ApplyWorkspaceEditResponse { applied }).unwrap()),
    );
}
