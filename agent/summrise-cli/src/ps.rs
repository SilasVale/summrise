//! The two quoting rules this CLI needs, and the argv frame that replaced a command string.
//!
//! Ported from `agent/summrise-agent-npm/src/summrise.ts` (`psq`, `psArgv`).

/// PowerShell single-quote doubling: the ONLY escaping a single-quoted PS literal needs.
///
/// This is the injection surface for every script this CLI hands to a SYSTEM/admin PowerShell, so it
/// is a decision and not a formatting detail: a path carrying `'` closes the literal it was
/// interpolated into, and everything after it is PowerShell source.
///
/// Oracle: `cli.test.mjs` "psq: PowerShell single-quote doubling (injection surface for SYSTEM task
/// scripts)".
pub fn psq(x: &str) -> String {
    x.replace('\'', "''")
}

/// The argv for a one-shot PowerShell script — **NO SHELL, and that is the point.**
///
/// `shell: true` routed this through cmd.exe, where `"` is a quote TOGGLE and `\"` is not an escape
/// at all, so cmd re-parsed the argument before PowerShell saw it and any character it treats
/// specially became an OPERATOR. Observed on d1: the update receipt contains `1.2.322 -> 1.2.323`,
/// cmd saw the `>` and performed a REDIRECTION — the log line landed as `update requested 1.2.322 -`
/// and a stray zero-byte file named `1.2.323` appeared in the working directory.
///
/// The script is passed VERBATIM as one argument: no quoting, no escaping of any kind. The oracle
/// asserts exactly that (`>` `<` `|` `&` `^` `%` `"` all survive untouched, and no `\"` may appear
/// anywhere in the argv).
///
/// Oracle: `cli.test.mjs` "psArgv: the script is ONE argv element, so no shell can re-parse it".
pub fn ps_argv(script: &str) -> Vec<String> {
    vec![
        "-NoProfile".to_string(),
        "-Command".to_string(),
        script.to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle: cli.test.mjs:47 — "psq: PowerShell single-quote doubling (injection surface for
    /// SYSTEM task scripts)".
    #[test]
    fn psq_doubles_single_quotes() {
        assert_eq!(
            psq("C:\\Program Files\\Summrise\\a'b"),
            "C:\\Program Files\\Summrise\\a''b"
        );
        assert_eq!(psq("/plain/path"), "/plain/path");
        assert_eq!(psq(""), "");
        assert_eq!(psq("'"), "''");
        assert_eq!(psq("a'b'c"), "a''b''c");
    }

    /// Oracle: cli.test.mjs:1732 — "psArgv: the script is ONE argv element, so no shell can
    /// re-parse it".
    #[test]
    fn ps_argv_is_one_verbatim_element() {
        let script = "[$(Get-Date -Format o)] update requested 1.2.322 -> 1.2.323 \" | Out-File 'D:\\Summrise\\logs\\summrise-update.log' -Append; Write-Output \"a<b & c|d ^ e%f\"";
        let argv = ps_argv(script);
        assert_eq!(&argv[..2], ["-NoProfile", "-Command"]);
        assert_eq!(argv.len(), 3, "the whole script is exactly one argument");
        assert_eq!(argv[2], script, "and it is passed VERBATIM");
        for ch in ['>', '<', '|', '&', '^', '%', '"'] {
            assert!(argv[2].contains(ch), "script still carries {ch}");
        }
        assert!(
            !argv.iter().any(|a| a.contains("\\\"")),
            "no cmd-style quote escaping may appear anywhere — that escaping is what made `>` an operator"
        );
    }
}
