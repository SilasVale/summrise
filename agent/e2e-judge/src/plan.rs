//! THE PLAN — WHAT THE DEVICE E2E SUITE RUNS, IN WHAT ORDER, WITH WHICH FIXTURES AND WHICH BUDGETS.
//!
//! **THIS FILE IS THE ANSWER TO "IS THE DRIVER'S SEQUENCING A DECISION?" — AND IT IS YES.** The suite used to
//! hold its own order in an `if (want('terminal')) await sectionTerminal();` ladder at the bottom of
//! `e2e.js`, its own `--only` vocabulary in a `const SECTIONS = [...]`, and its own timing policy in thirty
//! `await sleep(...)` calls and retry counters. Every one of those is a decision in the manifest's sense —
//! *"computes, parses, derives, validates, decides"* — and the one that matters most is the **validation**:
//! `--only governance-typo` used to select nothing and be indistinguishable from success, which is why the
//! guard exists at all. A name that selects nothing is a judgement about configuration, and judgements are
//! this crate's.
//!
//! So the plan is DATA, the way `sweep-plan.mjs` already describes for the design sweeps: *"Everything a
//! payload used to derive for itself (which surfaces are visited, in what order, at which density/path/
//! viewport/theme/mode, which passes run on each, **and the policy caps and floors**) is computed there,
//! once, at EMIT time, and then travels as DATA."* The driver is its interpreter and holds no fixture of its
//! own: the commands it types, the markers it requires, the byte counts it uploads and the polling budgets it
//! spends all arrive from here.
//!
//! Two ways the plan reaches a run, and they carry the SAME bytes:
//!
//! * `summrise-e2e-judge --plan --only governance,runs > plan.json`, then `node e2e.js --plan plan.json`
//!   — the path CI and a developer's box take, where cargo is present.
//! * `summrise-e2e-judge --bundle > e2e.js` — the plan is EMBEDDED in the emitted driver, so the copy a
//!   device fetches over the CDN needs no toolchain. This is the same emit-time shape the sweeps use.

use serde::{Deserialize, Serialize};

/// One section of the suite. The NAME is the `--only` vocabulary and the plan's own order; the variant is
/// what the check modules dispatch on.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Section {
    Terminal,
    File,
    Workflow,
    Panel,
    Mcp,
    Evidence,
    Browser,
    Governance,
    Runs,
}

/// THE CANONICAL ORDER, and the only place it is written down. It is the order the suite has always run in,
/// and it is a decision rather than a consequence: `terminal` is first because everything else needs a PTY to
/// exist, `governance` and `runs` are last because they are the only two that arm a gate or mint a run id.
pub const ALL_SECTIONS: [Section; 9] = [
    Section::Terminal,
    Section::File,
    Section::Workflow,
    Section::Panel,
    Section::Mcp,
    Section::Evidence,
    Section::Browser,
    Section::Governance,
    Section::Runs,
];

impl Section {
    pub fn name(self) -> &'static str {
        match self {
            Section::Terminal => "terminal",
            Section::File => "file",
            Section::Workflow => "workflow",
            Section::Panel => "panel",
            Section::Mcp => "mcp",
            Section::Evidence => "evidence",
            Section::Browser => "browser",
            Section::Governance => "governance",
            Section::Runs => "runs",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        ALL_SECTIONS.into_iter().find(|x| x.name() == s)
    }

    /// THE SECTIONS THAT NEED A BROWSER, and therefore the ones `--no-browser` drops. Named here rather than
    /// derived from a flag on the section, because the set is a decision: `evidence` drives the SPA's rail
    /// over CDP, `panel` reads the SPA's xterm over CDP, `mcp` and `browser` are CDP and Playwright.
    pub fn needs_browser(self) -> bool {
        matches!(
            self,
            Section::Panel | Section::Mcp | Section::Evidence | Section::Browser
        )
    }

    /// **THE ID PREFIX THIS SECTION'S OBSERVATIONS ARE RECORDED UNDER — ONE HALF OF THE CONTRACT BETWEEN THE
    /// TWO LANGUAGES, AND THE OTHER HALF IS IN THE DRIVER.** It is written down here so the judge can tell "the
    /// run never reached this section" from "the run reached it and the judge could not read a single id",
    /// which are the same thing to every check function and completely different to a reader.
    pub fn prefix(self) -> &'static str {
        match self {
            Section::Terminal => "terminal.",
            Section::File => "file.",
            Section::Workflow => "workflow.",
            Section::Panel => "panel.",
            Section::Mcp => "mcp.",
            Section::Evidence => "evidence.",
            Section::Browser => "browser.",
            // The one that is not the section's own name: the driver records governance's observations under
            // `gov.`, which is what the suite's check names have always used.
            Section::Governance => "gov.",
            Section::Runs => "runs.",
        }
    }
}

/// The plan for one run: which sections, in which order.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Plan {
    pub sections: Vec<Section>,
    pub fixtures: Fixtures,
    pub budgets: Budgets,
}

/// **THE FIXTURES ARE THE TEST'S SUBSTANCE, AND THEY ARE RUST'S.** Every command the suite types, every
/// marker it requires in an answer, every byte count it uploads and every string it asserts on lives here.
/// The driver used to hold them as string literals next to the conditions that read them, which is how a
/// fixture and its expectation could drift apart in one file; now the driver types what it is given and the
/// judge checks against the same record it gave.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Fixtures {
    /// `SUMMRISE_EVIDENCE_DIR`'s default: where the suite's own artifacts land (layout v2, ADR 0008).
    pub evidence_dir: String,
    /// `SUMMRISE_PW_DIR`'s default: the bundled Playwright the `browser_run_script` payloads require.
    pub pw_dir: String,

    // ── terminal ────────────────────────────────────────────────────────────────────────────────────
    /// The command whose OUTPUT is the proof that a session executes.
    pub session_command: String,
    /// The marker that output must contain.
    pub session_marker: String,
    /// A command that does not exist: the shell must survive it and any code reported must be non-zero.
    pub failing_command: String,
    pub background_command: String,
    pub background_marker: String,
    /// A session id that was never opened: `terminal_read` must answer `evicted`, not throw and not empty-ok.
    pub unknown_session: String,
    /// The prefix of the keystroke-path marker, which is suffixed with the run's own timestamp.
    pub write_marker_prefix: String,
    /// **THE TEMPLATES ARE FIXTURES TOO, AND THIS IS WHY THEY ARE NOT STRING CONCATENATION IN THE DRIVER.**
    /// `{marker}` is filled with the value the driver minted, so the command the device receives is data the
    /// judge can read back out of `args` and reason about — a `'Write-Output "' + m + '"'` in the driver would
    /// put the shape of the test in the file that is supposed to have no opinion about it.
    pub write_command_template: String,
    pub panel_command_template: String,
    /// The URL a navigation is aimed at when the marker IS the address: `https://example.com/<marker>`.
    pub marker_url_template: String,

    // ── file ────────────────────────────────────────────────────────────────────────────────────────
    pub transfer_name: String,
    /// The size of ONE uploaded page, and how many of them: the expected file size is their product, and it
    /// is computed here rather than written as a constant the driver would have to keep in step.
    pub transfer_page_bytes: usize,
    pub transfer_pages: u64,
    /// Every byte of the uploaded file, so `file single-read download` can require all of them and not only
    /// the length. 66 is `B`.
    pub transfer_fill: u8,
    pub text_name: String,
    pub text_payload: String,
    pub missing_name: String,

    // ── workflow ────────────────────────────────────────────────────────────────────────────────────
    pub workflow_name: String,
    pub workflow_command: String,
    pub workflow_marker: String,
    pub memory_title: String,
    pub memory_tags: Vec<String>,
    /// The token written into `memory_save`, suffixed with the run's timestamp; its `memory_search`
    /// follow-ups use the recorded value.
    pub memory_token_prefix: String,
    pub memory_update_prefix: String,
    pub memory_list_limit: u64,

    // ── panel ───────────────────────────────────────────────────────────────────────────────────────
    pub panel_marker_prefix: String,

    // ── mcp ─────────────────────────────────────────────────────────────────────────────────────────
    pub mcp_transports: Vec<McpTransport>,
    pub mcp_home_url: String,
    /// Candidate link labels, in the order the snapshot is searched for them — and INDEX-ALIGNED with
    /// `mcp_click_targets`, because which label the click used decides which URL the view is expected to land
    /// on. The driver also builds the injected link out of both, so the link it clicks and the URL the judge
    /// expects cannot drift.
    pub mcp_link_labels: Vec<String>,
    /// URL fragments that count as "the click drove the view".
    pub mcp_click_targets: Vec<String>,

    // ── evidence ────────────────────────────────────────────────────────────────────────────────────
    pub evidence_shot_prefix: String,
    pub evidence_shot_suffix: String,

    // ── browser ─────────────────────────────────────────────────────────────────────────────────────
    pub browser_home_url: String,
    /// The `TITLE=` line the driving payload prints, which is the proof it reached the page.
    pub browser_title_line: String,
    pub browser_bar_fragment: String,
    pub focus_marker_prefix: String,

    // ── governance ──────────────────────────────────────────────────────────────────────────────────
    pub gov_goal: String,
    pub gov_command: String,
    pub gov_intent: String,
    pub gov_considered: Vec<String>,
    pub gov_sibling_command: String,
    /// The command WORD the server is expected to derive the grant from — the first token of `gov_command`.
    pub gov_grant_word: String,

    // ── runs ────────────────────────────────────────────────────────────────────────────────────────
    pub runs_label_prefix: String,
    pub runs_goal: String,
    pub runs_inside_command: String,
    pub runs_loose_command: String,
    pub runs_unknown_id: String,
}

/// One MCP transport the autoselect regression drives. The `args` are forwarded to `mcp_client_connect`
/// verbatim, because the two arms differ by exactly them.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct McpTransport {
    pub tag: String,
    pub args: serde_json::Value,
}

/// **THE BUDGETS ARE THE SUITE'S POLICY CAPS AND FLOORS.** Every one of these used to be a literal at its
/// call site: a poll count, a retry count, or a `sleep` whose only justification was "long enough on this
/// device". They are here for two reasons. A number that decides whether evidence EXISTS is a policy, and the
/// sweeps' rule already says policies travel as data. And they are the suite's history of flakiness written
/// in one place — the click poll that had to grow past a fixed 5 s, the shell-prompt gate that replaced a
/// fixed sleep, the mcp view poll that had to outlast a transport switch — so the next person to widen one
/// can see every other one it should stay in proportion with.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Budgets {
    /// Poll until the shell has produced ANY output, i.e. until it has a prompt. A gate, not a sleep: a
    /// fixed 2500 ms was measured failing under load with `state=partial exit=null` and nothing wrong.
    pub prompt_polls: u32,
    pub prompt_interval_ms: u64,
    /// A command that lands while the shell is still settling can come back partial, so try twice.
    pub execute_attempts: u32,
    pub execute_retry_ms: u64,
    /// How long a backgrounded command is given before its output is collected.
    pub background_collect_ms: u64,
    /// How long a keystroke-path write is given to echo back to the screen tail.
    pub screen_settle_ms: u64,

    pub panel_open_settle_ms: u64,
    pub panel_list_settle_ms: u64,
    pub panel_rail_settle_ms: u64,
    pub panel_tab_settle_ms: u64,
    pub panel_xterm_polls: u32,
    pub panel_xterm_interval_ms: u64,

    pub mcp_disconnect_settle_ms: u64,
    /// After a transport switch the embedded view can still show the previous probe's page for seconds.
    pub mcp_view_polls: u32,
    pub mcp_view_interval_ms: u64,
    pub mcp_nav_settle_ms: u64,
    pub mcp_inject_settle_ms: u64,
    /// A click can land while the view is mid-navigation and silently do nothing, so snapshot and click
    /// again — with a FRESH snapshot, because the old refs are stale after a navigation.
    pub mcp_click_attempts: u32,
    pub mcp_click_retry_ms: u64,

    pub evidence_rail_settle_ms: u64,
    pub evidence_shots_settle_ms: u64,
    /// How long the SPA has to act on a rail click before the next read.
    pub spa_rail_settle_ms: u64,

    /// The gap between the Playwright navigation and the first look at the address bar — see the comment on
    /// the default.
    pub browser_bar_settle_ms: u64,
    /// Right after a Playwright-driven navigation the SPA can answer `Runtime.evaluate` without a value for
    /// a few seconds (Electron CDP quirk), so the bar is polled generously rather than slept on once.
    pub browser_bar_polls: u32,
    pub browser_bar_interval_ms: u64,
    pub browser_rail_settle_ms: u64,
    pub browser_trap_polls: u32,
    pub browser_trap_interval_ms: u64,

    /// The gate's pending command is published by the session task, so it is polled for rather than slept on.
    pub gov_pending_polls: u32,
    pub gov_pending_interval_ms: u64,
    /// A granted sibling must run WITHOUT asking. The number is the check: it is a ceiling on a call that
    /// never waits for a person, and the whole approval path is bypassed only when it comes back fast.
    pub gov_sibling_ms: u64,

    /// Closing a CDP WebSocket immediately after `send` can trip Node's `UV_HANDLE_CLOSING` assert on
    /// Windows, so the close is awaited (with a grace period) instead of fired and forgotten.
    pub ws_close_immediate_ms: u64,
    pub ws_close_grace_ms: u64,
    /// The ceiling on one `Runtime.evaluate` round trip over the SPA's CDP socket.
    pub ws_eval_timeout_ms: u64,
}

impl Default for Fixtures {
    fn default() -> Self {
        let evidence_dir = "C:\\ProgramData\\Summrise\\pwout".to_string();
        Self {
            evidence_dir,
            pw_dir: "D:\\Summrise\\components\\playwright".to_string(),

            session_command: "Write-Output E2E-SESSION-OK".to_string(),
            session_marker: "E2E-SESSION-OK".to_string(),
            failing_command: "summrise-e2e-no-such-command".to_string(),
            background_command: "Start-Sleep -Seconds 2; Write-Output E2E-BG-DONE".to_string(),
            background_marker: "E2E-BG-DONE".to_string(),
            unknown_session: "no-such-session-e2e".to_string(),
            write_marker_prefix: "E2E-WRITE-".to_string(),
            write_command_template: "Write-Output \"{marker}\"\r\n".to_string(),
            panel_command_template: "Write-Output \"{marker}\"".to_string(),
            marker_url_template: "https://example.com/{marker}".to_string(),

            transfer_name: "e2e_suite_transfer.bin".to_string(),
            transfer_page_bytes: 150 * 1024,
            transfer_pages: 2,
            transfer_fill: 66,
            text_name: "e2e_suite_text.txt".to_string(),
            text_payload: "E2E-TEXT-OK".to_string(),
            missing_name: "e2e_no_such_file_xyz".to_string(),

            workflow_name: "e2e_wf.json".to_string(),
            workflow_command: "echo E2E-WF-1".to_string(),
            workflow_marker: "E2E-WF-1".to_string(),
            memory_title: "E2E suite marker".to_string(),
            memory_tags: vec!["e2e".to_string(), "suite".to_string()],
            memory_token_prefix: "toolchain-".to_string(),
            memory_update_prefix: "toolchain-upd-".to_string(),
            memory_list_limit: 50,

            panel_marker_prefix: "PANEL-VIS-".to_string(),

            mcp_transports: vec![
                McpTransport {
                    tag: "stdio".to_string(),
                    args: serde_json::json!({ "name": "pw", "transport": "stdio" }),
                },
                McpTransport {
                    tag: "http".to_string(),
                    args: serde_json::json!({
                        "name": "pw-http",
                        "transport": "http",
                        "url": "http://127.0.0.1:9229/mcp",
                    }),
                },
            ],
            mcp_home_url: "https://example.com/".to_string(),
            mcp_link_labels: vec!["E2E-INNER-LINK".to_string(), "Learn more".to_string()],
            mcp_click_targets: vec!["inner-click-test".to_string(), "iana.org".to_string()],

            evidence_shot_prefix: "e2e_evidence_".to_string(),
            evidence_shot_suffix: ".png".to_string(),

            browser_home_url: "https://www.example.com".to_string(),
            browser_title_line: "TITLE=Example Domain".to_string(),
            browser_bar_fragment: "example.com".to_string(),
            focus_marker_prefix: "focus-trap-".to_string(),

            gov_goal: "provision the ONU at 0/1 on VLAN 100".to_string(),
            gov_command: "echo governed".to_string(),
            gov_intent: "confirm the session is responsive before changing anything".to_string(),
            gov_considered: vec![
                "skip the check".to_string(),
                "reopen the session instead".to_string(),
            ],
            gov_sibling_command: "echo granted-sibling".to_string(),
            gov_grant_word: "echo".to_string(),

            runs_label_prefix: "e2e-run-".to_string(),
            runs_goal: "prove run identity end to end".to_string(),
            runs_inside_command: "echo inside-the-run".to_string(),
            runs_loose_command: "echo loose-work".to_string(),
            runs_unknown_id: "run-does-not-exist".to_string(),
        }
    }
}

impl Default for Budgets {
    fn default() -> Self {
        Self {
            prompt_polls: 12,
            prompt_interval_ms: 1000,
            execute_attempts: 2,
            execute_retry_ms: 3000,
            background_collect_ms: 4000,
            screen_settle_ms: 3000,

            panel_open_settle_ms: 2500,
            panel_list_settle_ms: 3000,
            panel_rail_settle_ms: 1500,
            panel_tab_settle_ms: 2500,
            panel_xterm_polls: 6,
            panel_xterm_interval_ms: 2000,

            mcp_disconnect_settle_ms: 1500,
            mcp_view_polls: 15,
            mcp_view_interval_ms: 1000,
            mcp_nav_settle_ms: 3000,
            mcp_inject_settle_ms: 1000,
            mcp_click_attempts: 2,
            mcp_click_retry_ms: 3000,

            evidence_rail_settle_ms: 2000,
            evidence_shots_settle_ms: 2000,
            spa_rail_settle_ms: 500,

            // The gap between the Playwright navigation and the first look at the address bar. Long, and
            // deliberately so: the Electron CDP quirk is a delay in the SPA's own answer rather than a stale
            // value, so the FIRST read is the one most likely to come back without a `result.value`.
            browser_bar_settle_ms: 4000,
            browser_bar_polls: 8,
            browser_bar_interval_ms: 2000,
            browser_rail_settle_ms: 3000,
            browser_trap_polls: 8,
            browser_trap_interval_ms: 2000,

            gov_pending_polls: 25,
            gov_pending_interval_ms: 200,
            gov_sibling_ms: 5000,

            ws_close_immediate_ms: 100,
            ws_close_grace_ms: 3000,
            ws_eval_timeout_ms: 15000,
        }
    }
}

impl Default for Plan {
    fn default() -> Self {
        Self {
            sections: ALL_SECTIONS.to_vec(),
            fixtures: Fixtures::default(),
            budgets: Budgets::default(),
        }
    }
}

/// What `--only` and `--no-browser` mean, and the one error that must not be soft. An unknown name is a
/// CONFIGURATION ERROR rather than a filter that matches nothing: CI runs `--only governance,runs`, so a
/// renamed section would leave the step "passing" while testing nothing at all. The message and the exit code
/// are the ones the JS driver carried, moved here with the decision.
pub fn plan_from_cli(only: Option<&str>, no_browser: bool) -> Result<Plan, String> {
    let mut plan = Plan::default();
    if let Some(only) = only {
        let asked: Vec<&str> = only
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if asked.is_empty() {
            return Err("--only was given with no section names".to_string());
        }
        let unknown: Vec<&str> = asked
            .iter()
            .copied()
            .filter(|s| Section::parse(s).is_none())
            .collect();
        if !unknown.is_empty() {
            return Err(format!(
                "unknown --only section(s): {}\nknown sections: {}",
                unknown.join(", "),
                ALL_SECTIONS
                    .iter()
                    .map(|s| s.name())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        // THE ORDER IS THE PLAN'S, NOT THE CALLER'S. `--only runs,governance` selects the same two sections
        // as `--only governance,runs` and runs them in the plan's order; a caller cannot reorder the suite by
        // reordering a flag, which is what keeping the ladder in the driver used to allow.
        let wanted: Vec<Section> = asked.iter().filter_map(|s| Section::parse(s)).collect();
        plan.sections.retain(|s| wanted.contains(s));
    }
    if no_browser {
        plan.sections.retain(|s| !s.needs_browser());
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_plan_is_every_section_in_the_canonical_order() {
        let p = Plan::default();
        assert_eq!(p.sections, ALL_SECTIONS.to_vec());
        assert_eq!(p.sections.len(), 9);
    }

    #[test]
    fn only_selects_a_subset_but_never_reorders_it() {
        let p = plan_from_cli(Some("runs,governance"), false).unwrap();
        assert_eq!(p.sections, vec![Section::Governance, Section::Runs]);
    }

    #[test]
    fn an_unknown_section_is_a_configuration_error_naming_the_known_ones() {
        let e = plan_from_cli(Some("governance,governance-typo"), false).unwrap_err();
        assert!(e.contains("governance-typo"), "{e}");
        assert!(e.contains("known sections:"), "{e}");
        assert!(e.contains("terminal"), "{e}");
    }

    #[test]
    fn only_with_no_names_is_refused_rather_than_selecting_nothing() {
        let e = plan_from_cli(Some("  ,, "), false).unwrap_err();
        assert!(e.contains("no section names"), "{e}");
    }

    #[test]
    fn no_browser_drops_exactly_the_four_sections_that_drive_cdp() {
        let p = plan_from_cli(None, true).unwrap();
        assert_eq!(
            p.sections,
            vec![
                Section::Terminal,
                Section::File,
                Section::Workflow,
                Section::Governance,
                Section::Runs
            ]
        );
    }

    #[test]
    fn the_plan_round_trips_through_the_json_the_driver_consumes() {
        let p = plan_from_cli(Some("governance,runs"), true).unwrap();
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("\"governance\""), "{json}");
        let back: Plan = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }
}
