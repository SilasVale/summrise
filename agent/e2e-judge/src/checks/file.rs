//! §2 `file` — stat, a two-page append upload, a single paged raw read back down, the directory listing, a
//! missing path as a data-shaped `ok:false`, and the text (not base64) path.
//!
//! **THE EXPECTED SIZE IS COMPUTED HERE RATHER THAN WRITTEN DOWN.** The suite used to carry `300 * 1024` next
//! to the two `Buffer.alloc(150 * 1024, 'B')` writes that produce it, so the arithmetic existed twice and only
//! one of them was the test. The page size and the page count are the plan's (`transfer_page_bytes`,
//! `transfer_pages`), the product is this crate's, and the read-back check decodes the base64 to require every
//! byte rather than only the length — a transfer that returned the right number of wrong bytes used to pass.

use base64::Engine;

use crate::rows::{rows_list_entries, text_of, View};
use crate::verdict::{Check, Verdict};

pub fn judge(rows: &View, out: &mut Verdict) {
    let fx = rows.fixtures();
    let expected = fx.transfer_page_bytes as u64 * fx.transfer_pages;

    // ── two pages, appended ──────────────────────────────────────────────────────────────────────────
    //
    // AN EMPTY SERIES MEANS THE DRIVER NEVER GOT HERE, not two pages that came back empty. Without this guard a
    // run that aborted before the first write would still emit `file upload 2 pages` as a FAILURE — a check
    // about a call nobody made — and a recording with no observations at all would judge one check, not none.
    let pages: Vec<&serde_json::Value> = rows.series_values("file.write.page");
    if pages.is_empty() {
        return;
    }
    let uploaded = pages.len() as u64 == fx.transfer_pages
        && pages.iter().all(|w| {
            w.get("ok")
                .map(|v| v.as_bool() != Some(false))
                .unwrap_or(true)
        });
    let byte_detail = serde_json::Value::Array(
        pages
            .iter()
            .map(|w| w.get("bytes").cloned().unwrap_or(serde_json::Value::Null))
            .collect(),
    );
    out.push(Check::pass_or_fail(
        "file upload 2 pages",
        uploaded,
        crate::text::json_trunc(&byte_detail, 60),
    ));

    // ── the file on disk is the size both pages add up to ────────────────────────────────────────────
    let Some(st) = rows.got("file.stat").ok() else {
        return;
    };
    let size = st.get("size").and_then(|v| v.as_u64());
    out.push(Check::pass_or_fail(
        "file stat",
        size == Some(expected),
        format!("size={}", crate::rows::js_field(st, "size")),
    ));

    // ── ONE read returns all of it, and every byte is the fill byte ──────────────────────────────────
    let Some(r1) = rows.got("file.read.raw").ok() else {
        return;
    };
    let encoded = r1.get("data").and_then(|v| v.as_str()).unwrap_or_default();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap_or_default();
    let all_fill = bytes.iter().all(|b| *b == fx.transfer_fill);
    out.push(Check::pass_or_fail(
        "file single-read download",
        bytes.len() as u64 == expected && all_fill,
        format!("bytes={}", bytes.len()),
    ));

    // ── the directory listing shows it ───────────────────────────────────────────────────────────────
    let Some(fl) = rows.got("file.list").ok() else {
        return;
    };
    let entries = rows_list_entries(fl);
    let listed = entries.iter().any(|e| {
        e.get("name")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .contains(&fx.transfer_name)
    });
    out.push(Check::pass_or_fail(
        "file list contains upload",
        listed,
        format!("entries={}", entries.len()),
    ));

    // ── a missing path is a data-shaped answer, not a transport error ────────────────────────────────
    let Some(miss) = rows.got("file.stat.missing").ok() else {
        return;
    };
    let refused = miss.get("ok").and_then(|v| v.as_bool()) == Some(false);
    out.push(Check::pass_or_fail(
        "file stat missing ok:false",
        refused,
        crate::text::truncate(
            miss.get("error")
                .and_then(|v| v.as_str())
                .unwrap_or_default(),
            60,
        ),
    ));

    // ── the text path, not only base64 pages ─────────────────────────────────────────────────────────
    let Some(tw) = rows.got("file.text.write").ok() else {
        return;
    };
    out.push(Check::pass_or_fail(
        "file text write",
        tw.get("ok")
            .map(|v| v.as_bool() != Some(false))
            .unwrap_or(true),
        "",
    ));
    let Some(tr) = rows.got("file.text.read").ok() else {
        return;
    };
    let read_back = text_of(tr);
    out.push(Check::pass_or_fail(
        "file text read",
        read_back.contains(&fx.text_payload),
        format!("bytes={}", crate::rows::js_field(tr, "bytes")),
    ));
}
