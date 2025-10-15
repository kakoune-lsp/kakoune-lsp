use crate::context::*;
use crate::position::*;
use crate::types::*;
use indoc::formatdoc;
use itertools::Itertools;
use lsp_types::request::*;
use lsp_types::*;
use url::Url;

pub fn text_document_selection_range(
    meta: EditorMeta,
    params: SelectionRangePositionParams,
    ctx: &mut Context,
) {
    let selections: Vec<KakouneRange> = params
        .selections_desc
        .iter()
        .map(|desc| parse_kakoune_range(desc).0)
        .collect();

    let is_cursor_left_of_anchor = params.position == selections[0].start;

    let document = match ctx.documents.get(&meta.buffile) {
        Some(document) => document,
        None => {
            let err = format!("Missing document for {}", &meta.buffile);
            ctx.show_error(meta, err);
            return;
        }
    };
    let req_params = ctx
        .servers(&meta)
        .map(|(server_id, server_settings)| {
            let cursor_positions = selections
                .iter()
                .map(|range| {
                    let cursor = if is_cursor_left_of_anchor {
                        &range.start
                    } else {
                        &range.end
                    };
                    kakoune_position_to_lsp(cursor, &document.text, server_settings.offset_encoding)
                })
                .collect();

            (
                server_id,
                vec![SelectionRangeParams {
                    text_document: TextDocumentIdentifier {
                        uri: Url::from_file_path(&meta.buffile).unwrap(),
                    },
                    positions: cursor_positions,
                    work_done_progress_params: WorkDoneProgressParams::default(),
                    partial_result_params: PartialResultParams::default(),
                }],
            )
        })
        .collect();
    ctx.call::<SelectionRangeRequest, _>(
        meta,
        RequestParams::Each(req_params),
        move |ctx: &mut Context, meta, results| {
            let result = match results.into_iter().find(|(_, v)| v.is_some()) {
                Some(result) => result,
                None => (meta.servers[0], None),
            };

            editor_selection_range(result, selections, is_cursor_left_of_anchor, meta, ctx);
        },
    );
}

fn editor_selection_range(
    result: (ServerId, Option<Vec<SelectionRange>>),
    selections: Vec<KakouneRange>,
    is_cursor_left_of_anchor: bool,
    meta: EditorMeta,
    ctx: &mut Context,
) {
    let (server_id, result) = result;
    let selection_ranges = match result {
        Some(selection_ranges) => selection_ranges,
        None => return,
    };

    let document = match ctx.documents.get(&meta.buffile) {
        Some(document) => document,
        None => {
            let err = format!("Missing document for {}", &meta.buffile);
            ctx.show_error(meta, err);
            return;
        }
    };

    let server = ctx.server(server_id);

    // We get a list of ranges of parent nodes for each Kakoune selection.  The UI wants to
    // select parent nodes of all Kakoune selections at once.  This means we want to have a
    // list where each entry updates all selections.  As first step, convert to a matrix where
    // the first dimension is the parent index, and the second dimension is the Kakoune selection.
    let mut transposed_selection_ranges: Vec<Vec<Option<KakouneRange>>> = Vec::new();
    for (sel_idx, sel_range) in selection_ranges.iter().enumerate() {
        let mut cur = sel_range;
        let mut i = 0;
        loop {
            let range = {
                let range =
                    lsp_range_to_kakoune(&cur.range, &document.text, server.offset_encoding);
                if is_cursor_left_of_anchor {
                    KakouneRange {
                        start: range.end,
                        end: range.start,
                    }
                } else {
                    range
                }
            };
            if i == transposed_selection_ranges.len() {
                transposed_selection_ranges.push(vec![None; selection_ranges.len()]);
            }
            transposed_selection_ranges[i][sel_idx] = Some(range);
            i += 1;
            match cur.parent.as_deref() {
                Some(parent) => cur = parent,
                None => break,
            }
        }
    }

    let transposed_selection_ranges = transposed_selection_ranges
        .iter()
        .map(|sel_ranges| {
            format!(
                "'{}'",
                &sel_ranges
                    .iter()
                    .filter_map(|s| s.map(|s| ForwardKakouneRange(s).to_string()))
                    .join(" ")
            )
        })
        .join(" ");

    fn contains(haystack: &KakouneRange, needle: &KakouneRange) -> bool {
        haystack.start <= needle.start && haystack.end >= needle.end
    }

    // Find an interesting range to select initially. We use the smallest one that goes beyond
    // the main selection. We only consider the main selection here and hope that the index
    // works well for other selections too.
    let index_of_next_bigger_range = {
        let mut cur = &selection_ranges[0];
        let mut i = 0;
        loop {
            let range = lsp_range_to_kakoune(&cur.range, &document.text, server.offset_encoding);
            // Found a range that exceeds the main selection's range.
            if !contains(&selections[0], &range) {
                break i;
            }
            match cur.parent.as_deref() {
                Some(parent) => cur = parent,
                None => break i,
            }
            i += 1;
        }
    };

    let command = formatdoc!(
        "evaluate-commands -client {} %[
             set-option window lsp_selection_ranges {}
             lsp-selection-range-show
             lsp-selection-range-select {}
         ]",
        meta.client.as_ref().unwrap(),
        &transposed_selection_ranges,
        index_of_next_bigger_range + 1,
    );
    ctx.exec(meta, command);
}
