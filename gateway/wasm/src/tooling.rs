//! TWO PURE HELPERS FROM `gateway/src/tooling.ts`.
//!
//! `probeEnvKeyName` is a table lookup with a DEFAULT (an unknown channel probes DeepSeek's key rather
//! than nothing), and `encodeBase64Utf8` is the base64 the installer scripts embed. Both are pure; the
//! rest of that module reads KV, R2 and upstreams.

/// `PROBE_ENV_KEYS` — the worker-level key each channel's probe uses.
const PROBE_ENV_KEYS: [(&str, &str); 8] = [
    ("or", "OPENROUTER_API_KEY"),
    ("qw", "QWEN_API_KEY"),
    ("nv", "NVAPI_KEY"),
    ("gmi", "GMI_API_KEY"),
    ("cm", "CMD_API_KEY"),
    ("amd", "AMD_API_KEY"),
    ("r4", "R4_API_KEY"),
    ("ds", "DEEPSEEK_API_KEY"),
];

/// `probeEnvKeyName(prefix)` — the key name, or DeepSeek's for an unknown prefix.
///
/// **`hasOwnProperty` AGAIN, AND THIS TIME IT MATTERS IN BOTH DIRECTIONS.** The lookup is guarded, so
/// `"constructor"` gets the DEFAULT rather than `Object.prototype.constructor`'s source; and the default is
/// a real key rather than null, so an unknown channel probes the one channel that always has a key.
pub fn probe_env_key_name(prefix: &str) -> &'static str {
    PROBE_ENV_KEYS
        .iter()
        .find(|(k, _)| *k == prefix)
        .map(|(_, v)| *v)
        .unwrap_or("DEEPSEEK_API_KEY")
}

/// `encodeBase64Utf8(text)` — `btoa` over the UTF-8 bytes, with padding.
///
/// **THE JAVASCRIPT CHUNKS ITS `String.fromCharCode` CALL** (`0x8000` at a time) to keep a large body off
/// the argument stack, and the OUTPUT is what this has to match: standard base64 of the UTF-8 encoding,
/// padded. A `&str` here is already UTF-8, so the encoding step is the bytes.
pub fn encode_base64_utf8(text: &str) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// The three public installer paths — the ONE list the router's arm and its CORS carve-out share.
///
/// It is a function rather than two `matches!` calls because the carve-out in `lib.rs`'s `fetch` entry has to
/// agree with the arm in `route`, and a second spelling of the list is how the two drift.
pub fn is_installer_path(path: &str) -> bool {
    path == "/api/summrise-cli"
        || path == "/api/summrise-install"
        || path == "/api/summrise-install.ps1"
}

/// `posixInstaller(b64)` — the `curl … | sh` one-liner, byte for byte.
///
/// **THE `{{`/`}}` ARE RUST'S, NOT THE SCRIPT'S**: every brace below is doubled because this is a `format!`
/// literal, and the rendered script is the single-brace text the shipping TypeScript produced. The corpus cases
/// in `fixtures/translate-corpus.json` were recorded from that TypeScript with five different payloads (`""`,
/// `"YWJj"`, a 4000-character one), so a transcription that lost a brace or a `$` fails here rather than on a
/// user's machine.
///
/// **AND IT IS A STRING IN RUST NOW RATHER THAN A LINE OF TYPESCRIPT EMITTED BACK.** The emission
/// (`render_typescript`, `tooling-logic.ts`) existed for exactly one reason — the console's own callers needed
/// these bytes — and those callers are deleted with `tooling.ts` (2026-10-17). A generated twin with no consumer
/// is not a twin; it is a second copy waiting to drift.
pub fn posix_installer(b64: &str) -> String {
    format!(
        "#!/bin/sh\nset -e\ncommand -v node >/dev/null 2>&1 || {{ echo \"error: Node.js required\"; exit 1; }}\nDEST=\"${{SUMMRISE_BIN:-$HOME/.local/bin}}\"\nmkdir -p \"$DEST\"\necho \"{}\" | (base64 -d 2>/dev/null || base64 -D) > \"$DEST/summrise\"\nchmod +x \"$DEST/summrise\"\necho \"installed: $DEST/summrise\"\necho \"usage: summrise check | summrise use <ds|qw|og|or> | summrise use auto | summrise restore\"\n",
        b64
    )
}

/// `psInstaller(b64)` — the `irm … | iex` one-liner, byte for byte.
///
/// **THE `\r\n` INSIDE THE SINGLE-QUOTED POWERSHELL STRING IS REAL, AND IT IS THE ONE BYTE A READER WOULD NOT
/// NOTICE OTHERWISE**: the shipping TypeScript wrote `'@echo off\r\nnode "%~dp0summrise" %*'` inside a template
/// literal, so the emitted `.cmd` line ends CR LF — the file it writes is a Windows batch file, and a bare `\n`
/// there is the kind of difference nothing notices until cmd.exe does.
pub fn ps_installer(b64: &str) -> String {
    format!(
        "$ErrorActionPreference = \"Stop\"\ntry {{ node --version | Out-Null }} catch {{ Write-Error \"Node.js required\"; exit 1 }}\n$dest = Join-Path $HOME \".local\\bin\"\nNew-Item -ItemType Directory -Force -Path $dest | Out-Null\n$script = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String(\"{}\"))\nSet-Content -Path (Join-Path $dest \"summrise\") -Value $script -Encoding UTF8 -NoNewline\nSet-Content -Path (Join-Path $dest \"summrise.cmd\") -Value '@echo off\r\nnode \"%~dp0summrise\" %*' -Encoding ASCII\nWrite-Host \"installed: $dest\\summrise  (command: summrise)\"\n",
        b64
    )
}

/// The three public installer routes' bytes, from the CLI asset (or its absence).
///
/// **THE 200 ARM'S `Access-Control-Allow-Origin: *` IS FORCED, AND THAT IS THE SOURCE'S OWN DECISION RATHER
/// THAN A PORT ARTIFACT**: `index.ts` returns these three directly, BEFORE the dispatch's `withCors`, with the
/// comment *"Genuitinely-public installer payloads (curl|sh / irm|iex — CORS-irrelevant non-browser clients):
/// KEEP the ACAO:* wildcard so any browser-hosted install helper keeps working. No session, no secret."* A
/// `withCors` stamp would DELETE that wildcard for any origin the allowlist does not know, which is why
/// `lib.rs`'s door skips its stamp for this family and the handler carries the whole header set.
///
/// **THE 404 ARM IS THE OPPOSITE — IT IS STAMPED**, because the source's `jsonError` path falls through the
/// dispatch's `withCors` like every other refusal: `cors` is `cors::cors_headers_for(origin, host, configured)`,
/// which adds nothing for a request with no permitted origin and is therefore exactly the recorded bytes.
pub fn installer_response(
    asset: Option<&str>,
    path: &str,
    cors: Vec<(String, String)>,
) -> crate::responses::Built {
    use crate::responses::{cors_headers, Built};
    let Some(cli) = asset else {
        let mut built =
            crate::responses::json_error(404, "summrise CLI not found", "not_found_error");
        built.headers.extend(cors);
        return built;
    };
    let body = if path == "/api/summrise-cli" {
        cli.to_string()
    } else if path == "/api/summrise-install" {
        posix_installer(&encode_base64_utf8(cli))
    } else {
        ps_installer(&encode_base64_utf8(cli))
    };
    let mut headers = vec![(
        "Content-Type".to_string(),
        "text/plain; charset=utf-8".to_string(),
    )];
    headers.extend(cors_headers());
    headers.push(("Access-Control-Allow-Origin".to_string(), "*".to_string()));
    Built {
        status: 200,
        headers,
        body,
    }
}

#[cfg(test)]
mod oracle_corpus {
    //! `fixtures/translate-corpus.json`'s `tooling` cases were produced by the SHIPPING TypeScript
    //! (`gateway/wasm/oracle-translate.mjs`), and this replays them.
    use super::*;

    fn corpus() -> serde_json::Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/fixtures/translate-corpus.json"
        );
        let text = std::fs::read_to_string(path).expect("the corpus is committed");
        serde_json::from_str(&text).expect("the corpus parses")
    }

    #[test]
    fn every_tooling_helper_matches_the_shipping_typescript() {
        let doc = corpus();
        let mut checked = 0;
        for case in doc["cases"].as_array().expect("cases") {
            let func = case["fn"].as_str().unwrap_or("?");
            let name = case["name"].as_str().unwrap_or("?");
            let input = &case["input"];
            let want = &case["expected"]["value"];
            match func {
                "probe_env_key_name" => {
                    let got = probe_env_key_name(input.as_str().unwrap_or(""));
                    assert_eq!(
                        &serde_json::Value::String(got.to_string()),
                        want,
                        "{func} / {name}"
                    );
                }
                "encode_base64_utf8" => {
                    let got = encode_base64_utf8(input.as_str().unwrap_or(""));
                    assert_eq!(&serde_json::Value::String(got), want, "{func} / {name}");
                }
                // THE TWO INSTALLER TEMPLATES, recorded from the shipping `tooling.ts` before it was
                // deleted — same instrument as the two helpers above, and the reason the port's braces and
                // its CR LF are pinned rather than eyeballed.
                "posix_installer" => {
                    let got = posix_installer(input.as_str().unwrap_or(""));
                    assert_eq!(&serde_json::Value::String(got), want, "{func} / {name}");
                }
                "ps_installer" => {
                    let got = ps_installer(input.as_str().unwrap_or(""));
                    assert_eq!(&serde_json::Value::String(got), want, "{func} / {name}");
                }
                _ => continue,
            }
            checked += 1;
        }
        assert!(
            checked >= 25,
            "the tooling corpus shrank to {checked} cases"
        );
    }

    /// **THE TWO COVERAGE CHECKS THE DELETED `health.test.mjs` OWNED, MOVED RATHER THAN DROPPED.**
    ///
    /// They were the only tests that compared the probe's key TABLE against the rest of the product, and they
    /// are not byte comparisons: a corpus cannot see them, because both are statements about what is MISSING. A
    /// channel with no row does not fail — it silently probes with DeepSeek's key, which is the failure mode
    /// SOLID round-53 and round-59 named.
    ///
    /// **AND THEY READ THE PRODUCT'S OWN TABLES RATHER THAN A LIST SPELLED HERE**: `HEALTH_CHANNELS` is the
    /// health route's card table (`lib.rs`) and `user_key_names()` is derived from `byok.rs`'s channel table,
    /// so a new channel or a new managed key reaches these assertions without anyone remembering to add it.
    #[test]
    fn every_channel_probes_with_its_own_key_and_every_managed_key_is_probeable() {
        for (id, _) in crate::registry::HEALTH_CHANNELS.iter() {
            if *id == "og" {
                continue;
            }
            if *id == "ds" {
                assert_eq!(probe_env_key_name(id), "DEEPSEEK_API_KEY");
                continue;
            }
            assert_ne!(
                probe_env_key_name(id),
                "DEEPSEEK_API_KEY",
                "{id}/ must resolve via its own row, not the default — a channel with no row spends the \
                 wrong worker key on every probe while every gate stays green"
            );
        }
        let tabled: Vec<&str> = PROBE_ENV_KEYS.iter().map(|(_, v)| *v).collect();
        for name in crate::user_store::user_key_names() {
            assert!(
                tabled.contains(&name) || name == "OPENCODE_GO_API_KEY",
                "managed key {name} has no probe path — `summrise check` would be blind for its channel"
            );
        }
    }
}

/// **THE INSTALLER'S FUNCTION, NOT ONLY ITS BYTES — THE JAVASCRIPT TEST THIS REPLACES.**
///
/// `gateway/test/install-chain.test.mjs` ran the `curl|sh` chain end to end: it took the served artifact,
/// built the script, executed it under `/bin/sh`, and asserted the installed file was byte-identical to the
/// artifact and executable. That test's subject was the INSTALLER, and the installer is Rust now, so the check
/// moves here rather than being deleted with its file — a byte comparison against a recording cannot tell you
/// that the script RUNS.
///
/// **IT USES THE REAL ARTIFACT** (`gateway/public/summrise`, through `include_str!`), which makes this the
/// second half of the drift gate: the route serves the bytes on disk, and those bytes are what an installed
/// CLI is. **WHAT IT DOES NOT DO, SAID RATHER THAN IMPLIED**: it does not run the CLI afterwards (the deleted
/// test asserted the installed copy answers a refused gateway) — that is a test of the CLI's own argument
/// handling, not of this template, and the CLI has its own suite.
#[cfg(all(test, unix))]
mod installer_runs {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// The committed CLI artifact — the same bytes `ASSETS` serves in production.
    const CLI: &str = include_str!("../../public/summrise");

    #[test]
    fn the_posix_installer_installs_the_artifact_byte_for_byte_and_executably() {
        let dir = std::env::temp_dir().join(format!("summrise-inst-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temp dir");
        let script = dir.join("install.sh");
        std::fs::write(&script, posix_installer(&encode_base64_utf8(CLI))).expect("the script");

        let out = std::process::Command::new("/bin/sh")
            .arg(&script)
            .env("SUMMRISE_BIN", &dir)
            .output()
            .expect("sh runs");
        assert!(
            out.status.success(),
            "the installer exited {:?}: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );

        let installed = dir.join("summrise");
        let bytes = std::fs::read(&installed).expect("the CLI is installed");
        assert_eq!(
            String::from_utf8_lossy(&bytes),
            CLI,
            "the decoded payload must be the artifact byte for byte"
        );
        // **THE OWNER'S BIT, NOT ALL THREE — MEASURED 2026-10-10 UNDER `umask 077`**: `chmod +x` is itself
        // umask-sensitive, so asserting 0o111 would assert the AMBIENT UMASK rather than the installer.
        let mode = std::fs::metadata(&installed)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o100,
            0o100,
            "the installed binary must be owner-executable"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_posix_installer_refuses_a_host_with_no_node() {
        let dir = std::env::temp_dir().join(format!("summrise-nonode-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a temp dir");
        let script = dir.join("install.sh");
        std::fs::write(&script, posix_installer(&encode_base64_utf8(CLI))).expect("the script");

        // The script's own `command -v node` is what must fail; /bin/sh is spawned by absolute path so the
        // stripped PATH cannot stop the spawn itself.
        let out = std::process::Command::new("/bin/sh")
            .arg(&script)
            .env("PATH", "/nonexistent")
            .env("SUMMRISE_BIN", &dir)
            .output()
            .expect("sh runs");
        assert_eq!(out.status.code(), Some(1), "a node-less host exits 1");
        // THE `echo` GOES TO STDOUT — measured by this test's first run on the earlier base, which asserted
        // stderr and read an empty string while the exit code was already right.
        assert!(
            String::from_utf8_lossy(&out.stdout).contains("Node.js required"),
            "the refusal names what is missing: {:?}",
            String::from_utf8_lossy(&out.stdout)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
