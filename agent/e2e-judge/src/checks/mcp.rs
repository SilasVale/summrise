//! §4c2 `mcp` — `mcp_client_connect` must auto-select the EMBEDDED-VIEW tab so the first `browser_navigate`
//! drives the page the operator watches rather than the desktop SPA, and an INTERACTION (a real snapshot-ref
//! click) must reach it too — for each of the two transports.
//!
//! **THE ARM IS DECIDED HERE, ONCE PER TRANSPORT, AND IT IS A `skip` RATHER THAN A `pass`.** Only the HTTP
//! transport can be on playwright-mcp's PRIVATE-HEADLESS fork: the product probes for the desktop CDP and
//! attaches when it is up, and falls back to a private headless browser when it is not — stdio spawns and owns
//! its own child, so it is always on a browser the panel can see. On that fork "the embedded view followed" is
//! not a contract that can hold, so asserting it asserts a missing PRECONDITION — which is what this check did
//! for eleven rounds. What remains assertable on that arm is that the transport connected, that the tool
//! reported ok, and that the click drove the browser the tool owns.
//!
//! **THE CLICK IS THE HALF THAT NAVIGATION CANNOT COVER.** A tool that navigates but whose clicks never land is
//! a tool an AI cannot use, and every earlier version of this probe only navigated.

use crate::rows::{js_field, View};
use crate::verdict::{Check, Verdict};

pub fn judge(rows: &View, out: &mut Verdict) {
    let transports = rows.fixtures().mcp_transports.clone();
    for t in &transports {
        judge_transport(rows, out, &t.tag);
    }
}

fn judge_transport(rows: &View, out: &mut Verdict, tag: &str) {
    let fx = rows.fixtures();
    let id = |suffix: &str| format!("mcp.{tag}.{suffix}");

    // ── connect ──────────────────────────────────────────────────────────────────────────────────────
    let Some(c) = rows.got(&id("connect")).ok() else {
        return;
    };
    let connected = c.get("status").and_then(|v| v.as_str()) == Some("connected");
    out.push(Check::pass_or_fail(
        format!("mcp {tag} connect"),
        connected,
        match c.get("status") {
            Some(serde_json::Value::String(s)) => s.clone(),
            _ => crate::text::json_trunc(c, 60),
        },
    ));
    if !connected {
        // The probe cannot drive a browser through a connection it does not have. The JS returned here, and
        // so does this: the checks below are not registered at all rather than registered as failures.
        return;
    }

    let marker = rows
        .got(&id("mint"))
        .ok()
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    // ── the first navigation drives the embedded view ────────────────────────────────────────────────
    let Some(nav) = rows.got(&id("navigate")).ok() else {
        return;
    };
    let nav_ok = nav.get("ok").map(crate::rows::truthy).unwrap_or(false);
    out.push(Check::pass_or_fail(
        format!("mcp {tag} navigate ok"),
        nav_ok,
        if crate::rows::truthy(nav) {
            crate::text::json_trunc(nav, 60)
        } else {
            "no result".to_string()
        },
    ));

    // The poll series, replayed: each answer is a CDP target list, and the embedded view is the first target
    // that is not the desktop SPA.
    let mut embedded: Option<serde_json::Value> = None;
    let mut list = serde_json::Value::Null;
    for v in rows.series_values(&id("view")) {
        list = (*v).clone();
        if let Some(t) = non_desktop(v) {
            embedded = Some(t);
        }
    }
    let drove = embedded
        .as_ref()
        .map(|e| js_field(e, "url").contains(&marker))
        .unwrap_or(false);
    let spa_ok = list
        .as_array()
        .map(|a| a.iter().any(|t| js_field(t, "url").contains("/desktop/")))
        .unwrap_or(false);
    let private_arm = tag == "http" && !drove;
    if private_arm {
        out.push(Check::skip(
            format!("mcp {tag} drives embedded view"),
            "private-headless arm (9229 launched without a desktop view): the tool drove its own browser",
        ));
    } else {
        out.push(Check::pass_or_fail(
            format!("mcp {tag} drives embedded view"),
            drove,
            match embedded
                .as_ref()
                .and_then(|e| e.get("url"))
                .and_then(|v| v.as_str())
            {
                Some(u) => crate::text::truncate(u, 60),
                None => "NO VIEW".to_string(),
            },
        ));
    }
    let targets = list.as_array().map(|a| a.len()).unwrap_or(0);
    out.push(Check::pass_or_fail(
        format!("mcp {tag} SPA intact"),
        spa_ok,
        format!("targets={targets}"),
    ));

    // ── the click ────────────────────────────────────────────────────────────────────────────────────
    //
    // Accumulated exactly as the JS accumulated it across attempts: the click is "ok" if ANY attempt's
    // `browser_click` answered ok, and the reference shown in the detail is the LAST one attempted.
    let mut click_ok = false;
    let mut click_ref = "null".to_string();
    let mut want = fx.mcp_click_targets.first().cloned().unwrap_or_default();
    let mut emb2: Option<serde_json::Value> = None;
    for attempt in 0..rows.budgets().mcp_click_attempts {
        let click_id = format!("mcp.{tag}.click.{attempt}");
        if let Some(cl) = rows.got(&click_id).ok() {
            click_ref = cl
                .get("target")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| "null".to_string());
            click_ok = click_ok || cl.get("ok").map(crate::rows::truthy).unwrap_or(false);
        }
        // WHICH LINK THE REF CAME FROM IS THE DRIVER'S REPORT, AND IT DECIDES WHICH URL IS EXPECTED: the
        // injected same-origin link lands on `/inner-click-test`, the external fallback ("Learn more") lands on
        // iana.org. The two arrays are index-aligned for exactly this reason.
        if let Some(label) = rows
            .find(&click_id)
            .and_then(|o| o.args.as_ref())
            .and_then(|a| a.get("label"))
            .and_then(|v| v.as_str())
        {
            if let Some(i) = fx.mcp_link_labels.iter().position(|l| l == label) {
                if let Some(t) = fx.mcp_click_targets.get(i) {
                    want = t.clone();
                }
            }
        }
        for v in rows.series_values(&format!("mcp.{tag}.view2.{attempt}")) {
            if let Some(t) = non_desktop(v) {
                emb2 = Some(t);
            }
        }
        // The second attempt exists because a click can land while the view is mid-navigation and do nothing;
        // if the first one already drove the view there is no second.
        if emb2
            .as_ref()
            .map(|e| drove_target(&js_field(e, "url"), &fx.mcp_click_targets))
            .unwrap_or(false)
        {
            break;
        }
    }
    out.push(Check::pass_or_fail(
        format!("mcp {tag} click learn-more"),
        click_ok,
        format!("ref={click_ref} want={want}"),
    ));
    let click_drove = emb2
        .as_ref()
        .map(|e| drove_target(&js_field(e, "url"), &fx.mcp_click_targets))
        .unwrap_or(false);
    if !click_drove {
        // Geometry triage: DIAGNOSTIC ONLY, never a check. A physical click that misses for viewport reasons
        // looks identical to a broken click path, so the viewport and the link's rect are printed beside the
        // failure — and the instrument now says WHICH of the two indistinguishable states it saw, because
        // `geometry: null` used to mean both "the link is not in the view" and "the probe answered nothing".
        let geom = rows
            .got(&format!("mcp.{tag}.geometry"))
            .ok()
            .map(crate::text::js_string)
            .unwrap_or_else(|| {
                "no geometry recorded (the driver did not reach the triage probe)".to_string()
            });
        out.note(format!("click missed; geometry: {geom}"));
    }
    if private_arm {
        out.push(Check::skip(
            format!("mcp {tag} click drives embedded view"),
            "private-headless arm: the click drove the browser the tool owns, which no embedded view shows",
        ));
    } else {
        out.push(Check::pass_or_fail(
            format!("mcp {tag} click drives embedded view"),
            click_drove,
            match emb2
                .as_ref()
                .and_then(|e| e.get("url"))
                .and_then(|v| v.as_str())
            {
                Some(u) => crate::text::truncate(u, 60),
                None => "NO VIEW".to_string(),
            },
        ));
    }
}

/// `list.find(t => !t.url.includes('/desktop/'))` — the embedded view, as opposed to the SPA itself.
fn non_desktop(list: &serde_json::Value) -> Option<serde_json::Value> {
    list.as_array()?
        .iter()
        .find(|t| !js_field(t, "url").contains("/desktop/"))
        .cloned()
}

/// Did the view reach one of the URLs a driven click is expected to land on?
fn drove_target(url: &str, targets: &[String]) -> bool {
    targets.iter().any(|t| url.contains(t.as_str()))
}
