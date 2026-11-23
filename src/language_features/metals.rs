use jsonrpc_core::Params;

use crate::{context::Context, editor_quote, EditorMeta, ServerId, LAST_CLIENT};

/// https://scalameta.org/metals/docs/integrations/new-editor#metalsstatus
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MetalsStatusParams {
    /// The text to display in the status bar.
    pub text: String,
    /// If true, show the status bar.
    pub show: Option<bool>,
    /// If true, hide the status bar.
    pub hide: Option<bool>,
    /// If set, display this message when user hovers over the status bar.
    pub tooltip: Option<String>,
    /// If set, execute this command when the user clicks on the status bar item.
    pub command: Option<String>,
}

/// Maps 'metals/status' to 'lsp-show-message-info'.
/// Doesn't support the 'command' parameter as it's not clear how to action on the notification.
pub fn status(server_id: ServerId, meta: EditorMeta, params: Params, ctx: &mut Context) {
    let params: MetalsStatusParams = params.parse().expect("Failed to parse semhl params");

    let show = params.show.unwrap_or(false) && !params.hide.unwrap_or(false);

    if show {
        let have_client = meta.client.is_some();
        let last_client = LAST_CLIENT.lock().unwrap();
        let client = if have_client {
            ""
        } else {
            &last_client
                .as_ref()
                .map(|client| client.as_str())
                .unwrap_or_default()
        };
        let msg = format!(
            "{}{}",
            params.text,
            params
                .tooltip
                .map(|tooltip| format!(": {}", tooltip))
                .unwrap_or("".to_string())
        );

        ctx.exec(
            meta,
            format!(
                "evaluate-commands -verbatim -try-client '{}' lsp-show-message-info {} {}",
                client,
                editor_quote(&ctx.server(server_id).name),
                editor_quote(&msg)
            ),
        );
    }
}
