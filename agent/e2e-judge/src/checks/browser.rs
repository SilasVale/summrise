//! §4 `browser` — the bundled-runtime discovery every AI client starts from, the bounded runner's FAIL path,
//! a real Playwright navigation of the embedded view, the SPA's address bar following it, and the focus-trap
//! regression: a bar that is FOCUSED BUT UNTYPED must still follow AI navigation without the operator
//! re-entering the panel.
//!
//! **THE BAR IS POLLED, NOT SLEPT ON, AND THE POLL IS RE-JUDGED HERE.** Right after a Playwright-driven
//! navigation the desktop SPA can answer `Runtime.evaluate` without a `result.value` for a few seconds
//! (Electron CDP quirk, observed ~3 s and fine at ~8 s). The driver's rule is "stop when the answer arrived";
//! the check's rule is "the bar carries this URL", and it replays the recorded series to decide it.

use crate::rows::{js_field, View};
use crate::verdict::{Check, Verdict};

pub fn judge(rows: &View, out: &mut Verdict) {
    let fx = rows.fixtures();

    // ── the bundled runtime ──────────────────────────────────────────────────────────────────────────
    let Some(pi) = rows.got("browser.pw_info").ok() else {
        return;
    };
    let bundled = crate::rows::truthy(crate::rows::field(pi, "pw_dir"))
        && crate::rows::truthy(crate::rows::field(pi, "playwright_core_version"));
    out.push(Check::pass_or_fail(
        "browser pwinfo bundled",
        bundled,
        format!("core={}", js_field(pi, "playwright_core_version")),
    ));

    // ── the FAIL path is bounded, not a hang ─────────────────────────────────────────────────────────
    let Some(fail) = rows.got("browser.script.fail").ok() else {
        return;
    };
    let fout = format!(
        "{}{}",
        fail.get("stdout")
            .and_then(|v| v.as_str())
            .unwrap_or_default(),
        fail.get("stderr")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
    );
    let exit = fail.get("exit_code").and_then(|v| v.as_f64());
    out.push(Check::pass_or_fail(
        "browser_run_script fail path",
        exit != Some(0.0) && fout.contains("FAIL:e2e-intentional"),
        format!("exit={}", js_field(fail, "exit_code")),
    ));

    // ── a real navigation of the embedded view ───────────────────────────────────────────────────────
    let Some(br) = rows.got("browser.script.drive").ok() else {
        return;
    };
    let stdout = br
        .get("stdout")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let exit = br.get("exit_code").and_then(|v| v.as_f64());
    out.push(Check::pass_or_fail(
        "browser_run_script drives view",
        exit == Some(0.0) && stdout.contains(&fx.browser_title_line),
        crate::text::truncate(stdout.trim(), 80),
    ));

    // ── the SPA's address bar follows the navigation ─────────────────────────────────────────────────
    let Some(targets) = rows.got("browser.cdp.targets").ok() else {
        return;
    };
    let no_spa = !targets
        .as_array()
        .map(|a| a.iter().any(|t| js_field(t, "url").contains("/desktop/")))
        .unwrap_or(false);
    if no_spa {
        out.push(Check::fail("browser SPA bar sync", "no desktop SPA target"));
        return;
    }
    let mut state = Bar::default();
    for raw in rows.series_values("browser.bar.poll") {
        let Some(parsed) = parse_bar(raw) else {
            continue;
        };
        if !parsed.url.is_empty() && parsed.url.contains(&fx.browser_bar_fragment) {
            state = parsed;
            break;
        }
        state = parsed;
        if !state.on_browser {
            break;
        }
    }
    if !state.on_browser {
        // The rail is clicked by the DRIVER (it is a platform call); the answer it reads afterwards is the
        // only thing this decides.
        if let Some(s2) = rows.got("browser.bar.after_rail").ok().and_then(parse_bar) {
            state = s2;
        }
    }
    out.push(Check::pass_or_fail(
        "browser SPA bar sync",
        state.url.contains(&fx.browser_bar_fragment),
        format!("bar={}", crate::text::truncate(&state.url, 60)),
    ));

    // ── the focus-trap regression ────────────────────────────────────────────────────────────────────
    let marker = rows
        .got("mint.focus_marker")
        .ok()
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let mut bar_ok = false;
    let mut bar_val = String::new();
    for raw in rows.series_values("browser.trap.bar") {
        let parsed = parse_bar(raw);
        bar_val = parsed.map(|p| p.url).unwrap_or_default();
        if bar_val.contains(&marker) {
            bar_ok = true;
            break;
        }
    }
    out.push(Check::pass_or_fail(
        "browser focus-trap bar follows",
        bar_ok,
        format!("bar={}", crate::text::truncate(&bar_val, 60)),
    ));
}

#[derive(Default, Clone)]
struct Bar {
    on_browser: bool,
    url: String,
}

/// `raw ? JSON.parse(raw) : null` — the SPA answers a JSON string, and a malformed or empty answer is `null`
/// rather than a zero-valued state (which would read as "the bar is there and empty").
fn parse_bar(raw: &serde_json::Value) -> Option<Bar> {
    let text = raw.as_str()?;
    if text.is_empty() {
        return None;
    }
    let parsed: serde_json::Value = serde_json::from_str(text).ok()?;
    Some(Bar {
        on_browser: parsed
            .get("onBrowser")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        url: parsed
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
    })
}
