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

/// **TWO PURE HELPERS FROM `gateway/src/tooling.ts`, EMITTED BACK — `tooling-logic.ts` (2026-10-15).**
///
/// This module was already their Rust home ("TWO PURE HELPERS FROM `gateway/src/tooling.ts`", its own first
/// line); what it lacked was the render, so the console kept its two spellings and nothing compared them. They
/// are the probe's environment-key table and the installer's base64 — the second of which has a CHUNKING rule
/// whose only observable effect is not throwing on a large CLI.
const TS_MODULE: &str = r#"/**
 * tooling-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/tooling.rs` (`render_typescript`). The freshness test there is a byte comparison:
 *
 *     SUMMRISE_REFRESH_TOOLING=1 cargo test --manifest-path gateway/wasm/Cargo.toml tooling::emission
 *
 * `gateway/src/tooling.ts` re-exports this module and keeps the host: the `/api/health` assembly, the
 * `summriseProbe` sequence, the ASSETS read and the installer templates' callers.
 */

/** The worker-level key each channel's probe uses — **A TABLE WITH A DEFAULT**, because an unknown channel
 *  probes the one channel that always has a key rather than nothing at all. */
export const PROBE_ENV_KEYS: Record<string, string> = {
{{ENV_KEYS}}
};

/** `probeEnvKeyName(prefix)` — the key name, or DeepSeek's for an unknown prefix.
 *
 *  **`hasOwnProperty` AGAIN, AND THIS TIME IT MATTERS IN BOTH DIRECTIONS**: the lookup is guarded, so
 *  `"constructor"` gets the DEFAULT rather than `Object.prototype.constructor`'s source, and the default is a
 *  real key rather than null. */
export function probeEnvKeyName(prefix: string): string {
  return Object.prototype.hasOwnProperty.call(PROBE_ENV_KEYS, prefix)
    ? (PROBE_ENV_KEYS[prefix] as string)
    : "DEEPSEEK_API_KEY";
}

/** UTF-8-safe base64: `btoa` is Latin1-only and throws on non-ASCII (the summrise CLI is full of Chinese text),
 *  so the text is encoded to bytes first.
 *
 *  **AND IT IS CHUNKED, WHICH IS A RULE RATHER THAN AN OPTIMISATION**: spreading a big `Uint8Array` into
 *  `String.fromCharCode` blows the argument-length limit (`RangeError`) as the CLI grows, so the bytes are
 *  appended 32k at a time. The OUTPUT is what matters — standard base64 of the UTF-8 encoding, padded. */
export function encodeBase64Utf8(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let bin = "";
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) {
    bin += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(bin);
}

// ── THE TWO INSTALLER TEMPLATES ─────────────────────────────────────────────────────────────────
//
// **THE ONLY PIECE OF `tooling.ts` THAT NEEDED NEW RUST**: every other rule in that file already lived here or
// was corpus-pinned, but the two one-liners the CLI is installed with (`curl|sh` and `irm|iex`) were decided in
// the console alone. They are decisions — what the installer DOES, what it refuses, what it prints — and they
// are rendered back verbatim below.

export function posixInstaller(b64: string) {
  return `#!/bin/sh
set -e
command -v node >/dev/null 2>&1 || { echo "error: Node.js required"; exit 1; }
DEST="\${SUMMRISE_BIN:-$HOME/.local/bin}"
mkdir -p "$DEST"
echo "${b64}" | (base64 -d 2>/dev/null || base64 -D) > "$DEST/summrise"
chmod +x "$DEST/summrise"
echo "installed: $DEST/summrise"
echo "usage: summrise check | summrise use <ds|qw|og|or> | summrise use auto | summrise restore"
`;
}

export function psInstaller(b64: string) {
  return `$ErrorActionPreference = "Stop"
try { node --version | Out-Null } catch { Write-Error "Node.js required"; exit 1 }
$dest = Join-Path $HOME ".local\\bin"
New-Item -ItemType Directory -Force -Path $dest | Out-Null
$script = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String("${b64}"))
Set-Content -Path (Join-Path $dest "summrise") -Value $script -Encoding UTF8 -NoNewline
Set-Content -Path (Join-Path $dest "summrise.cmd") -Value '@echo off\r\nnode "%~dp0summrise" %*' -Encoding ASCII
Write-Host "installed: $dest\\summrise  (command: summrise)"
`;
}
"#;

/// **THE PRODUCER.** The key table is rendered as a literal rather than quoted, so its eight rows come from
/// [`PROBE_ENV_KEYS`] and cannot drift from the lookup that reads them.
pub fn render_typescript() -> String {
    let rows = PROBE_ENV_KEYS
        .iter()
        // **PRETTIER DROPS THE QUOTES ON A KEY THAT IS A PLAIN IDENTIFIER** ("or", "qw", …) and keeps them
        // where they are needed, so the emission has to know the same rule: a generated file that fails
        // `format:check` is a red CI on a tree nobody edited.
        .map(|(prefix, name)| {
            let key = if prefix
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
                && !prefix.chars().next().is_some_and(|c| c.is_ascii_digit())
            {
                prefix.to_string()
            } else {
                ts_string(prefix)
            };
            format!("  {key}: {},", ts_string(name))
        })
        .collect::<Vec<_>>()
        .join("\n");
    TS_MODULE.replace("{{ENV_KEYS}}", &rows)
}

/// A TypeScript string literal for a Rust `&str`.
fn ts_string(raw: &str) -> String {
    format!("\"{}\"", raw.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The committed emission, and the path the refresh command writes.
pub fn typescript_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/tooling-logic.ts")
}

#[cfg(test)]
mod emission {
    use super::*;

    /// **THE FRESHNESS CHECK** — the committed file IS `render_typescript()`'s bytes.
    #[test]
    fn the_typescript_is_what_this_module_emits() {
        let path = typescript_path();
        let rendered = render_typescript();
        if std::env::var("SUMMRISE_REFRESH_TOOLING").is_ok_and(|v| !v.is_empty()) {
            std::fs::write(&path, &rendered).expect("write tooling-logic.ts");
            return;
        }
        let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "{} missing ({e}) — run SUMMRISE_REFRESH_TOOLING=1 cargo test tooling::emission",
                path.display()
            )
        });
        assert_eq!(
            committed, rendered,
            "gateway/src/tooling-logic.ts is not what tooling.rs emits — \
             run SUMMRISE_REFRESH_TOOLING=1 cargo test tooling::emission"
        );
    }

    /// **EIGHT ROWS, AND THE TWO ENDS OF THE TABLE ARE THE ONES THAT MATTER**: the lookup's guard (a prefix
    /// that exists on `Object.prototype` gets the default, not the prototype's value) and the default itself.
    #[test]
    fn the_emission_carries_the_table_and_the_default() {
        let ts = render_typescript();
        assert_eq!(PROBE_ENV_KEYS.len(), 8);
        for (prefix, name) in PROBE_ENV_KEYS.iter() {
            // Prettier's key rule, mirrored: an identifier-safe key is written unquoted.
            assert!(
                ts.contains(&format!("  {prefix}: \"{name}\",")),
                "lost {prefix}"
            );
            assert_eq!(probe_env_key_name(prefix), *name);
        }
        assert_eq!(probe_env_key_name("constructor"), "DEEPSEEK_API_KEY");
        assert_eq!(probe_env_key_name("nope"), "DEEPSEEK_API_KEY");
        assert!(ts.contains("hasOwnProperty"), "the guard is the rule");
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
                _ => continue,
            }
            checked += 1;
        }
        assert!(
            checked >= 15,
            "the tooling corpus shrank to {checked} cases"
        );
    }
}
