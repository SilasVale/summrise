//! Where the sheets the mark-coverage note reads actually live.
//!
//! The JavaScript reads them by a path relative to its own module (`new URL("../../gateway/ui/src/styles/",
//! import.meta.url)`) or, for the panel, relative to the PROCESS'S WORKING DIRECTORY
//! (`readFileSync("agent/resources/panel/panel.css")`). Both resolve to the same three files when the tool
//! is run from the repository root, which is what every caller does — the gate scripts `cd` there first.
//!
//! This finds the repository root instead of assuming it, by walking up from the working directory (and
//! then from the executable) until it sees `agent/scripts/lib/design-sweep.mjs`. From the root the walk is
//! a single step, so the file the judge reads is the file the JavaScript read.

use std::path::{Path, PathBuf};

fn walk_up(start: &Path) -> Option<PathBuf> {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        if dir.join("agent/scripts/lib/design-sweep.mjs").is_file() {
            return Some(dir.to_path_buf());
        }
        cur = dir.parent();
    }
    None
}

pub fn repo_root() -> Option<PathBuf> {
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(root) = walk_up(&cwd) {
            return Some(root);
        }
    }
    let exe = std::env::current_exe().ok()?;
    walk_up(exe.parent()?)
}

/// The panel's BUILT sheet. The JS wraps the read in a `try` and judges nothing when it cannot be read, so
/// an unreadable sheet is not an error here either — there is simply no coverage note to write.
pub fn panel_sheet() -> Option<String> {
    std::fs::read_to_string(repo_root()?.join("agent/resources/panel/panel.css")).ok()
}

/// The console's SOURCE sheets, concatenated: its `dist` is a pruned build artifact, so the sources are
/// what the mark-coverage note reads (the same choice `console-marks-check.mjs` makes, for the same reason).
///
/// The JS joins `readdirSync(dir)` in FILESYSTEM order, which is not a stable order across machines; this
/// sorts, so the note's state list cannot depend on which filesystem the judging ran on.
pub fn console_sheets() -> Option<String> {
    let dir = repo_root()?.join("gateway/ui/src/styles");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "css").unwrap_or(false))
        .collect();
    files.sort();
    let mut parts = Vec::new();
    for f in files {
        parts.push(std::fs::read_to_string(&f).ok()?);
    }
    Some(parts.join("\n"))
}

/// The landing's stylesheet. The JS THROWS rather than measuring an empty string, because a note about zero
/// declared families reads exactly like a clean surface — so a missing sheet is reported rather than
/// silently skipped.
pub fn landing_sheet() -> Result<String, String> {
    let root = repo_root().ok_or_else(|| {
        "could not find the repository root, so the landing's stylesheet cannot be read".to_string()
    })?;
    let path = root.join("index/landing/assets/page.css");
    let css = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !css.contains(":root") {
        return Err(
            "the landing's stylesheet was not found — the coverage note would be reading an empty sheet and reporting a clean surface"
                .to_string(),
        );
    }
    Ok(css)
}
