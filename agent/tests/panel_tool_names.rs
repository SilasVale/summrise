// panel_tool_names — EVERY TOOL THE PANEL CALLS BY NAME MUST BE ONE THE AGENT DECLARES.
//
// **THE PANEL AND THE AGENT ARE TWO PROCESSES THAT AGREE BY SPELLING.** `callTool("terminal_read", …)` in
// `agent/resources/panel-react/src` travels to the agent's MCP surface, which resolves the name against its own
// `ToolDef` table; a name that is not there is not a compile error anywhere — it is a failed call in the UI, in
// production, on a device. **A RENAME ON ONE SIDE IS INVISIBLE ON THE OTHER**, which is the same shape as the
// console/worker drift this repository's emission pattern exists to prevent, and this is the cheap half of it.
//
// The check is a VOCABULARY scan rather than a call into the server: `DeviceServer` needs a live `AppState`, and
// what is being compared is the set of names the two sides SPELL. The declaration shape is stable and one-line
// requirements make it easy to keep honest — `ToolDef::new(` immediately followed by the name literal (measured
// 2026-10-15: every declaration in `agent/src/plugins` is that shape, and a one-line `ToolDef::new("x"` form
// appears nowhere).
//
// MUTATION: `callTool("terminal_read", …)` -> `callTool("terminal_readd", …)` in
//           `panel-react/src/components/TerminalPane.tsx`.
// RESULT:   FAILED, 0 passed / 1 failed, with the name, its file and line, and the nearest declared names —
//           `terminal_readd  (.../TerminalPane.tsx:183)` and `terminal_readd -> terminal_read, terminal_env,
//           terminal_plan`. Measured 2026-10-15. **AND THE FIRST ATTEMPT AT THIS MUTATION NEVER REACHED THIS TEST**:
//           `agent/build.rs` refused the build with "STALE panel bundle — resources/panel-react/src is newer than
//           resources/panel/", so the mutation had to preserve the source file's mtime to get past a DIFFERENT
//           gate that was doing its job. The bundle then turned out to be REPRODUCIBLE: rebuilding it changed
//           nothing in git.
//
// AND THE FLOOR IS THE OTHER HALF: a scan that found NO names on either side would pass silently, so both sides
// are floored (`PANEL_FLOOR`, `AGENT_FLOOR`) — the same rule `script-syntax.bash` and the umask loop carry.
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const CRATE: &str = env!("CARGO_MANIFEST_DIR");

fn repo() -> PathBuf {
    Path::new(CRATE)
        .parent()
        .expect("the crate lives one level below the repo root")
        .to_path_buf()
}

/// Every `.ts`/`.tsx` file under `dir`, recursively, skipping build output and test directories.
fn ts_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            if name == "node_modules" || name == "dist" || name == "__tests__" {
                continue;
            }
            ts_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "ts" || e == "tsx") {
            out.push(path);
        }
    }
}

/// Every `callTool("name"` (and the `callTool(\n "name"` form) in one panel file.
fn called_names(src: &str, file: &Path, into: &mut Vec<(String, String)>) {
    let mut rest = src;
    while let Some(at) = rest.find("callTool(") {
        let after = &rest[at + "callTool(".len()..];
        if let Some(quote) = after.find('"') {
            let tail = &after[quote + 1..];
            if let Some(end) = tail.find('"') {
                let name = &tail[..end];
                // Only a bare first argument is a tool name; `callTool(variable, …)` is not this test's business.
                if !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
                {
                    let line = src[..at].lines().count();
                    into.push((name.to_string(), format!("{}:{line}", file.display())));
                }
            }
        }
        rest = after;
    }
}

const AGENT_FLOOR: usize = 30;
const PANEL_FLOOR: usize = 10;

#[test]
fn every_tool_the_panel_calls_is_one_the_agent_declares() {
    let root = repo();

    // **THE DECLARATIONS COME FROM `vocabulary::TOOL_NAMES`, NOT FROM A SECOND SCAN.** That module is the single
    // source (it emits `contract.gen.ts`, which `callTool`'s `ToolName` union is generated from), and its own test
    // scans `agent/src` for `ToolDef::new("<name>"` and fails by name when the two disagree. Scanning again here
    // would be the second copy this whole exercise is about.
    let declared: BTreeSet<String> = summrise_agent::vocabulary::TOOL_NAMES
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    assert!(
        declared.len() >= AGENT_FLOOR,
        "only {} tool(s) in vocabulary::TOOL_NAMES (floor {AGENT_FLOOR})",
        declared.len()
    );

    let mut panel_files = Vec::new();
    ts_files(
        &root.join("agent/resources/panel-react/src"),
        &mut panel_files,
    );
    let mut called = Vec::new();
    for file in &panel_files {
        let src = fs::read_to_string(file).unwrap_or_default();
        called_names(&src, file, &mut called);
    }
    assert!(
        called.len() >= PANEL_FLOOR,
        "only {} callTool(\"…\") site(s) found in the panel — the scan is looking at the wrong shape (floor {PANEL_FLOOR})",
        called.len()
    );

    let mut unknown: Vec<String> = Vec::new();
    for (name, where_) in &called {
        if !declared.contains(name) {
            unknown.push(format!("  {name}  ({where_})"));
        }
    }
    assert!(
        unknown.is_empty(),
        "the panel calls {} tool(s) the agent does not declare — a rename on one side is a failed call in the UI:\n{}\n\nnearest declared names:\n{}",
        unknown.len(),
        unknown.join("\n"),
        nearest(&declared, &unknown)
    );
}

/// The declared names closest to each unknown one — because the realistic failure is a typo, and a list of one is
/// not a useful failure message.
fn nearest(declared: &BTreeSet<String>, unknown: &[String]) -> String {
    let mut out = Vec::new();
    for line in unknown {
        let name = line.split_whitespace().next().unwrap_or("");
        let mut best: Vec<(usize, &String)> =
            declared.iter().map(|d| (distance(d, name), d)).collect();
        best.sort();
        let picks: Vec<String> = best.iter().take(3).map(|(_, d)| (*d).clone()).collect();
        out.push(format!("  {name} -> {}", picks.join(", ")));
    }
    out.join("\n")
}

/// Levenshtein distance, iterative and small: the vocabulary here is tool names.
fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}
