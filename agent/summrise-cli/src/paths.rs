//! WHERE THINGS LIVE: the install dir, the data dir, and everything derived from them.
//!
//! Ported from `resolveDir` / `resolveDataDir` in `agent/summrise-agent-npm/src/summrise.ts`.
//! The registry is the SINGLE SOURCE OF TRUTH for both: the Rust agent's `paths.rs` reads the same
//! two values, so a CLI that resolved them differently would print the default install dir and
//! "panel: (not installed)" for an install that succeeded somewhere else.

use crate::host::Host;
use std::path::{Path, PathBuf};

/// The registry key both sides read.
pub const REG_KEY: &str = "HKLM\\SOFTWARE\\Summrise\\Agent";

/// The default install dir, used when neither the env var nor the registry answers.
pub const DEFAULT_INSTALL_DIR: &str = "C:\\Program Files\\Summrise";

/// `REG_SZ\s+(.+)` on the line that names the value — the one place the `reg query` text is parsed.
///
/// Split out of the host so the PARSING is testable and so a host that answers with real `reg`
/// output and a fake that answers with a literal take the same path.
pub fn parse_reg_sz(text: &str, name: &str) -> Option<String> {
    let line = text.lines().find(|l| l.contains(name))?;
    let idx = line.find("REG_SZ")?;
    let v = line[idx + "REG_SZ".len()..].trim();
    if v.is_empty() {
        None
    } else {
        Some(v.to_string())
    }
}

/// `$env:SUMMRISE_AGENT_DIR` → `HKLM\SOFTWARE\Summrise\Agent\InstallDir` → the default.
///
/// No legacy directory probing: the installer/setup always writes the registry, and every command
/// resolves the install dir through this one function.
pub fn resolve_dir(host: &dyn Host) -> String {
    if let Some(v) = host.env("SUMMRISE_AGENT_DIR") {
        if !v.is_empty() {
            return v;
        }
    }
    host.reg_read("InstallDir")
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| DEFAULT_INSTALL_DIR.to_string())
}

/// `HKLM\SOFTWARE\Summrise\Agent\DataDir` → `%ProgramData%\Summrise`.
///
/// Runtime logs and evidence live there, never in program files.
pub fn resolve_data_dir(host: &dyn Host) -> String {
    if let Some(v) = host.reg_read("DataDir").filter(|s| !s.trim().is_empty()) {
        return v.trim().to_string();
    }
    let root = host
        .env("ProgramData")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "C:\\ProgramData".to_string());
    format!("{root}\\Summrise")
}

/// Every path the CLI derives from the two roots (layout v2, ADR 0008).
///
/// The install ROOT keeps only the service exe (+ its transient `.new`/`.old`) and the NSIS
/// uninstaller; everything else lives in one of the three subdirectories. Leaf names are unchanged
/// — Electron packaging and task arguments are rename-sensitive; only the parent moves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub dir: String,
    pub etc_dir: String,
    pub components_dir: String,
    pub scripts_dir: String,
    pub exe_dst: String,
    /// The launcher lives in `scripts\` BECAUSE IT DERIVES ITS OWN LOG FROM THERE: it walks one
    /// directory up from its own exe to find the install root and appends `logs\launcher.log`, so a
    /// copy placed anywhere else would write its failures somewhere nobody looks.
    pub launcher_dst: String,
    pub data_dir: String,
    pub logs_dir: String,
    pub cfg_file: String,
    pub hostname_file: String,
    pub desk_dir: String,
    pub pw_dir: String,
}

impl Layout {
    pub fn resolve(host: &dyn Host) -> Layout {
        let dir = resolve_dir(host);
        let data_dir = resolve_data_dir(host);
        Layout::from_roots(&dir, &data_dir)
    }

    /// The same layout from two already-resolved roots — the seam every test drives, so a case can
    /// name `D:\Summrise` without a host at all.
    pub fn from_roots(dir: &str, data_dir: &str) -> Layout {
        let etc_dir = format!("{dir}\\etc");
        let components_dir = format!("{dir}\\components");
        let scripts_dir = format!("{dir}\\scripts");
        Layout {
            dir: dir.to_string(),
            etc_dir: etc_dir.clone(),
            components_dir: components_dir.clone(),
            scripts_dir: scripts_dir.clone(),
            exe_dst: format!("{dir}\\summrise-agent.exe"),
            launcher_dst: format!("{scripts_dir}\\summrise-launch.exe"),
            data_dir: data_dir.to_string(),
            logs_dir: format!("{data_dir}\\logs"),
            cfg_file: format!("{etc_dir}\\config.yaml"),
            hostname_file: format!("{etc_dir}\\summrise-agent.hostname"),
            desk_dir: format!("{components_dir}\\summrise-desktop-electron"),
            pw_dir: format!("{components_dir}\\playwright"),
        }
    }

    pub fn config_path(&self) -> PathBuf {
        PathBuf::from(&self.cfg_file)
    }

    pub fn busy_notes(&self) -> &'static str {
        "the marker path and the registry echo both derive from this one struct"
    }
}

/// `<base>\<leaf>` — THE ONE WAY THIS CRATE BUILDS A WINDOWS PATH OUT OF TWO STRINGS.
///
/// **NOT `Path::join`, AND THIS IS NOT A STYLE CHOICE.** `Path::join` uses the HOST's separator, so
/// the same decision run on Linux produced `D:\Summrise/components/cloudflared.exe` — a path no
/// Windows device has, and one no test could assert against the strings the CLI prints. Every path
/// this product derives is a WINDOWS path, because the product runs on Windows only, so the
/// separator is a constant of the product rather than a fact about the machine the test runs on.
///
/// It is a `PathBuf` rather than a `String` because that is what [`crate::host::Host`] takes.
pub fn win_join(base: &str, leaf: &str) -> PathBuf {
    PathBuf::from(format!("{base}\\{leaf}"))
}

/// The release marker `agent_update` reads as the LOCAL version, and `/api/status` serves as
/// `release`.
pub fn release_marker_path(install_dir: &str) -> PathBuf {
    // `Path::join` HERE, deliberately, and the difference from `win_join` is the REAL filesystem:
    // `win_join` is for paths that are printed or handed to PowerShell, where a Windows separator is
    // the product's contract. This one is handed to the filesystem, and on Windows `Path::join` IS a
    // backslash — while forcing one would make the path unopenable on the Linux hosts this crate's
    // `RealHost` tests run on. (`FakeHost` normalises separators, so a fixture spelled either way
    // resolves to the same file.)
    Path::new(install_dir).join("etc").join(".summrise-release")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeHost;

    /// Oracle: cli.test.mjs:3164 — "setup must not overwrite a remapped DataDir with the literal
    /// default".
    ///
    /// `resolveDataDir()` is registry-first and the data dir IS its answer, so a device whose data
    /// dir was remapped carries that path. `setup` used to write the LITERAL DEFAULT back to the
    /// registry while creating the tree at the resolved path: the tree landed at the remapped path
    /// and the registry then named the default, which is the path the AGENT reads. The data splits.
    #[test]
    fn a_remapped_data_dir_is_not_replaced_by_the_literal_default() {
        let host = FakeHost::new()
            .with_env("ProgramData", "C:\\ProgramData")
            .with_reg("DataDir", "E:\\SummriseData")
            .with_reg("InstallDir", "D:\\Summrise");
        let layout = Layout::resolve(&host);
        assert_eq!(
            layout.data_dir, "E:\\SummriseData",
            "the registry is the single source of truth"
        );
        assert_eq!(layout.logs_dir, "E:\\SummriseData\\logs");
        // No remap at all is the only case where the default applies.
        let plain = Layout::resolve(&FakeHost::new().with_env("ProgramData", "C:\\ProgramData"));
        assert_eq!(plain.data_dir, "C:\\ProgramData\\Summrise");
        assert_eq!(plain.dir, DEFAULT_INSTALL_DIR);
    }

    /// The env var wins over the registry, which is the documented resolution order (C1).
    #[test]
    fn the_env_var_beats_the_registry_for_the_install_dir() {
        let host = FakeHost::new()
            .with_env("SUMMRISE_AGENT_DIR", "X:\\Override")
            .with_reg("InstallDir", "D:\\Summrise");
        assert_eq!(resolve_dir(&host), "X:\\Override");
    }

    /// `REG_SZ\s+(.+)` on the line that names the value, and nothing else — a `reg query` output
    /// carries a blank line, a header and the key path, any of which a looser parse would take.
    #[test]
    fn reg_sz_parsing_takes_the_named_value_only() {
        let out = "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Summrise\\Agent\r\n    DataDir    REG_SZ    E:\\SummriseData\r\n    InstallDir    REG_SZ    D:\\Summrise\r\n\r\n";
        assert_eq!(
            parse_reg_sz(out, "DataDir").as_deref(),
            Some("E:\\SummriseData")
        );
        assert_eq!(
            parse_reg_sz(out, "InstallDir").as_deref(),
            Some("D:\\Summrise")
        );
        assert_eq!(parse_reg_sz(out, "Missing"), None);
        assert_eq!(parse_reg_sz("", "DataDir"), None);
    }
}
