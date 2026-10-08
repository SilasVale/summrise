//! THIS CLI'S OWN VERSION, and where it comes from.
//!
//! The TypeScript reads `require("../package.json").version`, and that number is load-bearing in
//! three decisions: `status` prints it as `this CLI:`, `updateWouldNotMove` compares it against the
//! device's release marker, and the `cmd-% version:` claim in the oracle is a claim about THIS
//! release. So the port keeps the SAME single owner — the npm package's manifest — until the
//! cutover landing makes the CLI the deliverable and the version moves to this crate's
//! `Cargo.toml`. Two owners now would be two answers to "what does `summrise --version` print".
//!
//! It is `include_str!` rather than a runtime file read so the number cannot go missing at run
//! time, and so a mismatch is a compile error rather than a wrong answer on a device.

/// The npm package's manifest, verbatim.
pub const PACKAGE_JSON: &str = include_str!("../../summrise-agent-npm/package.json");

/// `package.json`'s `version`, or an empty string when the manifest cannot be parsed (the
/// TypeScript's `String(require(...).version || "")`, which `writeReleaseMarker` skips on).
pub fn package_version() -> String {
    serde_json::from_str::<serde_json::Value>(PACKAGE_JSON)
        .ok()
        .and_then(|j| j.get("version").and_then(|v| v.as_str()).map(String::from))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `cmd-% version:` claim in the oracle is only checkable while the version is a plain
    /// dotted triple. Ported from `cli.test.mjs`: "package.json's version must be a plain semver
    /// for the `cmd-% version:` claim to hold".
    #[test]
    fn package_version_is_a_plain_semver() {
        let v = package_version();
        assert!(
            regex::Regex::new(r"^\d+\.\d+\.\d+$").unwrap().is_match(&v),
            "package.json's version must be a plain semver, got {v:?}"
        );
    }
}
