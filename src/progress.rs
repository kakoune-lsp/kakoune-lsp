use crate::context::Context;
use crate::types::{EditorMeta, EditorParams};
use crate::util::editor_quote;
use jsonrpc_core::Params;
use lsp_types::{
    notification::WorkDoneProgressCancel, NumberOrString, ProgressParams, ProgressParamsValue,
    WorkDoneProgress, WorkDoneProgressBegin, WorkDoneProgressCancelParams,
    WorkDoneProgressCreateParams, WorkDoneProgressEnd,
};
use serde::Deserialize;
use std::collections::hash_map;
use std::time::{self, Duration};

pub fn work_done_progress_cancel(_meta: EditorMeta, params: EditorParams, ctx: &mut Context) {
    let params = WorkDoneProgressCancelParams::deserialize(params).expect("Failed to parse params");
    ctx.notify::<WorkDoneProgressCancel>(params);
}

pub fn work_done_progress_create(
    params: Params,
    ctx: &mut Context,
) -> Result<jsonrpc_core::Value, jsonrpc_core::Error> {
    let WorkDoneProgressCreateParams { token } = params
        .parse()
        .map_err(|_| jsonrpc_core::Error::new(jsonrpc_core::ErrorCode::InvalidParams))?;
    match ctx.work_done_progress.entry(token) {
        hash_map::Entry::Occupied(e) => {
            warn!("Received duplicate ProgressToken '{:?}'", e.key());
        }
        hash_map::Entry::Vacant(e) => {
            e.insert(None);
        }
    };
    Ok(jsonrpc_core::Value::Null)
}

pub fn dollar_progress(meta: EditorMeta, params: Params, ctx: &mut Context) {
    let params: ProgressParams = match params.parse() {
        Ok(params) => params,
        Err(err) => {
            // Workaround: clangd up to version 12 sends us invalid messages.  Avoid panicking so
            // other features keep working. This is fixed by LLVM commit f088af37e6b5 ([clangd]
            // Fix data type of WorkDoneProgressReport::percentage, 2021-05-10).
            warn!("Failed to parse WorkDoneProgressParams params: {}", err);
            return;
        }
    };

    fn handle_progress_command(
        token: &lsp_types::ProgressToken,
        title: &str,
        cancelable: bool,
        message: &Option<String>,
        percentage: &Option<u32>,
        done: bool,
    ) -> String {
        let token = match token {
            NumberOrString::Number(token) => token.to_string(),
            NumberOrString::String(token) => editor_quote(token),
        };
        format!(
            "lsp-handle-progress {} {} {} {} {} {}",
            token,
            editor_quote(title),
            cancelable,
            editor_quote(message.as_deref().unwrap_or_default()),
            editor_quote(&percentage.map(|x| x.to_string()).unwrap_or_default()),
            done,
        )
    }

    let token = &params.token;
    match params.value {
        ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(begin)) => {
            match ctx.work_done_progress.get_mut(&params.token) {
                Some(Some(_)) => {
                    warn!(
                        "Received begin event for already started ProgressToken '{:?}'",
                        token
                    )
                }
                Some(progress @ None) => {
                    let command = handle_progress_command(
                        token,
                        &begin.title,
                        begin.cancellable.unwrap_or(false),
                        &begin.message,
                        &begin.percentage,
                        false,
                    );
                    *progress = Some(begin);
                    ctx.exec(meta, command);
                }
                None => {
                    warn!(
                        "Received begin event for non-existent ProgressToken '{:?}'",
                        token
                    );
                }
            }
        }
        ProgressParamsValue::WorkDone(WorkDoneProgress::Report(report)) => {
            if ctx.work_done_progress_report_timestamp.elapsed() < Duration::from_millis(50) {
                warn!("Progress report arrived too fast, dropping");
                return;
            }
            ctx.work_done_progress_report_timestamp = time::Instant::now();
            match ctx.work_done_progress.get_mut(&params.token) {
                Some(Some(progress)) => {
                    let command = handle_progress_command(
                        token,
                        &progress.title,
                        report.cancellable.unwrap_or(false),
                        &report.message,
                        &report.percentage,
                        false,
                    );
                    progress.cancellable = report.cancellable;
                    progress.message = report.message;
                    progress.percentage = report.percentage;
                    ctx.exec(meta, command);
                }
                Some(None) => {
                    let token = &params.token;
                    warn!(
                        "Received report event for unstarted ProgressToken '{:?}'",
                        token
                    );
                }
                None => {
                    let token = &params.token;
                    warn!(
                        "Received report event for non-existent ProgressToken '{:?}'",
                        token
                    );
                }
            }
        }
        ProgressParamsValue::WorkDone(WorkDoneProgress::End(WorkDoneProgressEnd { message })) => {
            match ctx.work_done_progress.remove(&params.token) {
                Some(Some(WorkDoneProgressBegin { title, .. })) => {
                    let command =
                        handle_progress_command(token, &title, false, &message, &Some(100), true);
                    ctx.exec(meta, command);
                }
                Some(None) => {
                    let token = &params.token;
                    warn!(
                        "Received end event for unstarted ProgressToken '{:?}'",
                        token
                    );
                }
                None => {
                    let token = &params.token;
                    warn!(
                        "Received end event for non-existent ProgressToken '{:?}'",
                        token
                    );
                }
            }
        }
    }
}
