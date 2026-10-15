/**
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
  or: "OPENROUTER_API_KEY",
  qw: "QWEN_API_KEY",
  nv: "NVAPI_KEY",
  gmi: "GMI_API_KEY",
  cm: "CMD_API_KEY",
  amd: "AMD_API_KEY",
  r4: "R4_API_KEY",
  ds: "DEEPSEEK_API_KEY",
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
