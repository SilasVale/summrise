//! EVERY ROUTE THE HEADER NAMES MUST EXIST IN THE TABLE IT POINTS AT.
//!
//! WHY THIS EXISTS (rounds 146-150). `agent/src/web/mod.rs` is the largest file in the agent. Its authority is
//! the table of `Pattern::Exact` / `Pattern::Prefix` rows that `route_of` resolves. Its HEADER said
//! `Routes:` and named TEN, so for as long as anyone read the header as the list, 23 routes were invisible to a
//! reader: `/api/settings`, `/api/monitors` and its three verbs, `/api/logs`, `/api/boots`, `/api/sessions`,
//! `/api/vitals/history`, `/api/update`, `/api/run/mark-exit` — most of what the panel calls. Round 147 made the
//! header say `A SELECTION, NOT THE INVENTORY` and point at the table.
//!
//! SO ONLY ONE DIRECTION REMAINS CHECKABLE, and this is it: a header that NAMES a route the device does not serve
//! sends a reader looking for something that is not there, or "fixing" one that is. The reverse direction is
//! deliberately NOT asserted — the header is a selection by design, and asserting it would make every new table
//! row a header edit.
//!
//! THE HEADER WRITES ITS ROUTES AS PROSE, which is why the extraction is not a single literal match: `GET
//! /panel, /panel/` and `POST /api/plugins/playwright/start|stop` are one line each, and a naive match takes the
//! comma and the pipe literally (rounds 146 and 147 each recorded that mistake). Splitting on `,`/`|`/whitespace
//! is the fix, and a route that still contains `{` or `}` is a template the table spells differently, so it is
//! matched by prefix.
//!
//! MIGRATION-TIME EQUIVALENCE, MEASURED (the same tree, both implementations, 2026-09-28):
//!   * `node scripts/test/http-route-header-check.mjs` → exit 0, stdout 96 bytes, one line:
//!     "http-route-header: ok — 11 header route(s) all resolve against 33 Exact + 5 Prefix pattern(s)",
//!     newline-terminated; stderr 0 bytes.
//!   * this file → the SAME line, byte for byte, from the same scan: 96/96 stdout, 0/0 stderr.
//!   * a planted `GET  /api/does-not-exist` line in the header → BOTH exit 1 with the FULL body (the
//!     verdict line and the one-route list), byte for byte: 177/177 characters on stderr. The verdict
//!     agreed on the first line; the body is what was compared.
//!   * THE CASE THAT MUST NOT BITE, in both: the header is a SELECTION, so a `Pattern::Exact` row the
//!     header never names is NOT an offender — planted `/api/zz-not-in-header` as a real table row, and
//!     both stayed green. That is this gate's own case and not a generic one: it is the direction the
//!     header above says is deliberately unasserted, and the clean tree already exercises it 22 times
//!     (33 rows, 11 named).
//!
//! MUTATION: name a route in the HTTP surface's header that no `Pattern` row resolves (add `GET  /api/does-not-exist`
//!           to the `//!` list above `use axum::body::Body`).
//! RESULT:   fails, printing the whole body the JS prints — "the header names 1 route(s) no Pattern row resolves —
//!           a reader is sent looking for something the device does not serve:" and then "  /api/does-not-exist".
//!           Clean tree passes. THE REVERSE IS DELIBERATELY NOT ASSERTED (round 147).
//!
//! THE FIX THIS PRINTS, and why it is not a sentence of its own: this gate's JS body carries NO fix line, so
//! adding one to the Rust body would break the equivalence the port exists to demonstrate. Where byte-equality
//! and a nicer message conflict, the message loses — a gate that prints a better sentence while reaching a
//! different verdict is worse than the one it replaced, and the byte count is the only thing that proves it did
//! not. The two repairs a reader has — delete the line from the header, or add the `Pattern` row that serves it —
//! are stated HERE, where a reader looks before they look at the output, and the body itself NAMES the offending
//! route(s) rather than reporting that an assertion failed.
//!
//! WHAT IT DOES NOT SEE, stated rather than implied:
//!   * **`Pattern::Under` rows.** There are 3 (one table row, `/api/sessions/`, and two match arms) and the JS
//!     counts only `Exact` and `Prefix`, so a header route served ONLY by an `Under` row would be reported as
//!     unresolved — a FALSE POSITIVE that has never fired, because the header names no such route. It is a
//!     stated limit, not a design: the JS's `covered()` has the same hole and this port did not widen or
//!     narrow it.
//!   * the two `Pattern::Exact(p) =>` MATCH ARMS, which the JS's `Pattern::Exact\("([^"]+)"\)` also skips —
//!     that is why `grep -c 'Pattern::Exact('` answers 35 and this scan answers 33.
//!   * a method mismatch: the header's `GET /api/spec` is checked by PATH only, so a row that serves
//!     `/api/spec` as `POST` would satisfy it.
//!   * `text.indexOf("\nuse ")` with no match is `slice(0, -1)` in JS — everything but the last character.
//!     Mirrored rather than "fixed", because the two implementations must agree on every input, not only the
//!     one this tree happens to have.

mod common;

use std::fmt::Write as _;

const SRC: &str = "agent/src/web/mod.rs";

struct Scan {
    named: Vec<String>,
    exact: Vec<String>,
    prefix: Vec<String>,
}

/// THE TWO STREAMS, SEPARATED, because the JS separates them: the success line goes to stdout and the
/// failure body to stderr. A port that merged them would compare equal line by line and differ the moment
/// anyone redirected one of them.
///
/// `stderr` carries NO trailing newline: `panic!` supplies it, and the JS's `console.error` does the same,
/// so the two bodies are byte-equal once the panic framing is stripped.
struct Streams {
    stdout: String,
    stderr: String,
    failed: bool,
}

/// `text.slice(0, text.indexOf("\nuse "))`.
fn head_of(text: &str) -> String {
    match text.find("\nuse ") {
        Some(i) => text[..i].to_string(),
        // `indexOf` answering -1 makes `slice(0, -1)` drop the LAST CHARACTER. Mirrored (see the header).
        None => {
            let c: Vec<char> = text.chars().collect();
            c[..c.len().saturating_sub(1)].iter().collect()
        }
    }
}

/// `body.split(/[,\s|]+/).filter(Boolean)` — split on RUNS of comma, pipe or whitespace, and drop empties.
fn split_class(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in body.chars() {
        if c == ',' || c == '|' || common::is_js_space(c) {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// `/^\/\/!\s+(?:GET|POST|DELETE|PUT|ANY)\s+(.+)$/` against `line.trim()`, then the `→` tail dropped and the
/// body split — the whole of step 1.
fn named_routes(head: &str) -> Vec<String> {
    let mut named = Vec::new();
    for line in head.split('\n') {
        let t: Vec<char> = line
            .trim_matches(|c| common::is_js_space(c))
            .chars()
            .collect();
        if t.len() < 4 || t[0] != '/' || t[1] != '/' || t[2] != '!' {
            continue;
        }
        let bang = 3;
        let after_ws = common::skip_ws(&t, bang);
        if after_ws == bang {
            continue; // `\s+` needs at least one
        }
        let rest: String = t[after_ws..].iter().collect();
        for method in ["DELETE", "POST", "GET", "PUT", "ANY"] {
            if !rest.starts_with(method) {
                continue;
            }
            // the second `\s+` is measured in the ORIGINAL character space, so re-index from `after_ws`
            let m_end = after_ws + method.len();
            let body_start = common::skip_ws(&t, m_end);
            if body_start == m_end {
                break; // `\s+` failed, and no other method can match here
            }
            let body: String = t[body_start..].iter().collect();
            if body.is_empty() {
                break; // `(.+)` needs at least one
            }
            let body = body.split('→').next().unwrap_or("").to_string();
            for piece in split_class(&body) {
                if piece.starts_with('/') {
                    named.push(piece);
                }
            }
            break;
        }
    }
    named
}

/// `[...text.matchAll(/Pattern::<kind>\("([^"]+)"\)/g)]` — the needle is built from `kind`, so the two calls
/// cannot drift from the JS's two patterns. `[^"]+` requires a non-empty value and `"\)` must follow, which
/// is why a match arm (`Pattern::Exact(p) =>`) is not a row.
fn pattern_rows(text: &str, kind: &str) -> Vec<String> {
    let needle = format!("Pattern::{kind}(\"");
    let mut out = Vec::new();
    let mut from = 0usize;
    while from <= text.len() {
        let Some(rel) = text[from..].find(&needle) else {
            break;
        };
        let start = from + rel + needle.len();
        let tail = &text[start..];
        match tail.find('"') {
            Some(q) if q > 0 && tail[q + 1..].starts_with(')') => {
                out.push(tail[..q].to_string());
                from = start + q + 2; // past `")`
            }
            // No match at this position: the engine advances one character. `start > from` always
            // (the needle is non-empty), so this terminates.
            _ => from = start + 1,
        }
    }
    out
}

/// `route.replace(/\/$/, "")` — ONE trailing slash, and only if there is one.
fn strip_trailing_slash(route: &str) -> String {
    match route.strip_suffix('/') {
        Some(s) => s.to_string(),
        None => route.to_string(),
    }
}

/// `route.replace(/\{.*\}$/, "")` — the LEFTMOST `{`, and only when the route ends with `}`.
fn strip_template_tail(route: &str) -> String {
    if route.ends_with('}') {
        if let Some(i) = route.find('{') {
            return route[..i].to_string();
        }
    }
    route.to_string()
}

/// The JS's `covered()` — the four arms, in the JS's order, because `||` short-circuits there and a reader
/// comparing the two should find them in the same order here.
fn covered(route: &str, exact: &[String], prefix: &[String]) -> bool {
    exact.iter().any(|e| *e == strip_trailing_slash(route))
        || exact.iter().any(|e| *e == route)
        || prefix.iter().any(|p| route.starts_with(p.as_str()))
        || prefix
            .iter()
            .any(|p| p.starts_with(strip_template_tail(route).as_str()))
}

fn scan(text: &str) -> Scan {
    Scan {
        named: named_routes(&head_of(text)),
        exact: pattern_rows(text, "Exact"),
        prefix: pattern_rows(text, "Prefix"),
    }
}

fn report(s: &Scan) -> Streams {
    // THE VACUITY CHECKS COME FIRST, in the JS's order: a scan that reads nothing must not pass by
    // reporting zero offenders, which is the failure mode this repository has recorded seven times.
    if s.named.is_empty() {
        return Streams {
            stdout: String::new(),
            stderr: "http-route-header: no routes found in the header — did its shape change? The check cannot pass vacuously.".to_string(),
            failed: true,
        };
    }
    if s.exact.is_empty() {
        return Streams {
            stdout: String::new(),
            stderr: "http-route-header: no Pattern rows found — the table moved or was renamed, and this check cannot pass vacuously.".to_string(),
            failed: true,
        };
    }
    let missing: Vec<&String> = s
        .named
        .iter()
        .filter(|r| !covered(r, &s.exact, &s.prefix))
        .collect();
    if !missing.is_empty() {
        let mut stderr = format!(
            "http-route-header: the header names {} route(s) no Pattern row resolves — a reader is sent looking for something the device does not serve:",
            missing.len()
        );
        for r in &missing {
            let _ = write!(stderr, "\n  {r}");
        }
        return Streams {
            stdout: String::new(),
            stderr,
            failed: true,
        };
    }
    Streams {
        stdout: format!(
            "http-route-header: ok — {} header route(s) all resolve against {} Exact + {} Prefix pattern(s)\n",
            s.named.len(),
            s.exact.len(),
            s.prefix.len()
        ),
        stderr: String::new(),
        failed: false,
    }
}

#[test]
fn the_header_names_only_routes_the_table_resolves() {
    let out = report(&scan(&common::read(SRC)));
    print!("{}", out.stdout);
    if out.failed {
        panic!("{}", out.stderr);
    }
}

// ── the unit cases: the JS's own edges, each one measured rather than assumed ─────────────────────────

/// THE CASE THAT MUST NOT BITE, and it is this gate's own: the header is a SELECTION, so a table row it
/// never names is not an offender. The reverse direction is deliberately unasserted (round 147).
#[test]
fn a_table_row_the_header_never_names_is_not_an_offender() {
    let text = "//!   GET  /api/spec → plugin spec\nuse x;\n\
                Route { pattern: Pattern::Exact(\"/api/spec\") },\n\
                Route { pattern: Pattern::Exact(\"/api/zz-not-in-header\") },\n";
    let out = report(&scan(text));
    assert!(!out.failed, "{}", out.stderr);
    assert!(out.stdout.contains("1 header route(s)"), "{}", out.stdout);
    assert!(out.stdout.contains("2 Exact + 0 Prefix"), "{}", out.stdout);
}

/// `GET /panel, /panel/` and `.../start|stop` are ONE LINE each, and both spellings must survive the split.
/// Round 146 and 147 each recorded a scan that took the comma and the pipe literally.
#[test]
fn the_comma_and_the_pipe_are_separators_not_part_of_a_route() {
    let text = "//!   GET  /panel, /panel/   → panel\n\
                //!   POST /api/plugins/playwright/start|stop → start/stop\nuse x;\n\
                Route { pattern: Pattern::Exact(\"/panel\") },\n\
                Route { pattern: Pattern::Exact(\"/api/plugins/playwright/start\") },\n";
    let s = scan(text);
    assert_eq!(
        s.named,
        vec![
            "/panel".to_string(),
            "/panel/".to_string(),
            "/api/plugins/playwright/start".to_string()
        ],
        "`stop` has no leading slash and is dropped, exactly as the JS's `startsWith(\"/\")` drops it"
    );
    assert!(!report(&s).failed);
}

/// A template the table spells differently is matched by prefix: `/api/tools/{name}` against
/// `Pattern::Prefix("/api/tools/")`.
#[test]
fn a_template_route_is_matched_by_its_prefix() {
    // The `Exact` row is not decoration: the JS refuses a scan that found no Exact row at all, BEFORE it
    // looks at any route, so a fixture without one measures the vacuity check instead of this rule.
    let text = "//!   POST /api/tools/{name} → dispatch\nuse x;\n\
                Route { pattern: Pattern::Exact(\"/api/spec\") },\n\
                Route { pattern: Pattern::Prefix(\"/api/tools/\") },\n";
    let out = report(&scan(text));
    assert!(!out.failed, "{}", out.stderr);
    assert!(out.stdout.contains("1 Exact + 1 Prefix"), "{}", out.stdout);
}

/// The failure BODY, not the verdict: the count, the reason and the route list, in the JS's own words.
#[test]
fn the_failure_body_names_the_route_and_why_it_matters() {
    let text = "//!   GET  /api/does-not-exist → nothing serves this\nuse x;\n\
                Route { pattern: Pattern::Exact(\"/api/spec\") },\n";
    let out = report(&scan(text));
    assert!(out.failed);
    assert_eq!(
        out.stderr,
        "http-route-header: the header names 1 route(s) no Pattern row resolves — a reader is sent \
         looking for something the device does not serve:\n  /api/does-not-exist"
    );
    assert!(out.stdout.is_empty(), "the failure body is stderr only");
}

/// A scan that reads nothing must not pass vacuously — both halves, and the message says which shape moved.
#[test]
fn a_scan_that_reads_nothing_refuses_rather_than_passing() {
    let no_routes = report(&scan("//! nothing here\nuse x;\nPattern::Exact(\"/a\"),\n"));
    assert!(no_routes.failed);
    assert!(
        no_routes.stderr.contains("no routes found in the header"),
        "{}",
        no_routes.stderr
    );

    let no_rows = report(&scan("//!   GET  /api/spec → x\nuse x;\n"));
    assert!(no_rows.failed);
    assert!(
        no_rows.stderr.contains("no Pattern rows found"),
        "{}",
        no_rows.stderr
    );
}

/// The header's own shape: a match arm is not a row, and `indexOf` with no `\nuse ` is `slice(0, -1)`.
#[test]
fn match_arms_are_not_rows_and_the_head_falls_back_the_way_the_js_does() {
    let s = scan("Pattern::Exact(p) => path == *p,\nPattern::Exact(\"/api/spec\"),\n");
    assert_eq!(s.exact, vec!["/api/spec".to_string()]);

    let head = head_of("no marker here");
    assert_eq!(
        head, "no marker her",
        "slice(0, -1) drops the last character"
    );
}
