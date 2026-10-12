/**
 * settings-logic.ts — GENERATED. DO NOT EDIT BY HAND.
 *
 * Emitted by `gateway/wasm/src/settings_logic.rs` (`render_typescript`). The freshness test in that module is a
 * byte comparison against this file, so a hand edit fails there with the text and one command to fix it:
 *
 *     SUMMRISE_REFRESH_SETTINGS=1 cargo test --manifest-path gateway/wasm/Cargo.toml settings_logic
 *
 * The decisions live in Rust; this is their emission, which is why the manifest classifies it GENERATED rather
 * than LOGIC — and why `npm test` can still run it.
 */

/** null = off, everything else passes through. */
export function normalizeSetting(v: string | null | undefined): string | null {
  if (v !== null && v !== undefined && (v === "0" || v === "false")) return null;
  return (v ?? null) as string | null;
}

/** Off is null, undefined, the empty string, "0" and "false". **NOT `normalizeSetting(v) !== null`.** */
export function globalSettingEnabled(v: string | null | undefined): boolean {
  return !(v === null || v === undefined || v === "" || v === "0" || v === "false");
}

/** The string to persist, or null for "delete the key" (an explicit OFF shadows the Worker var). */
export function canonicalSetting(value: unknown): string | null {
  if (value === null || value === undefined || value === "") return null;
  const s = String(value);
  return s === "1" || s === "true" || value === true ? "1" : "0";
}
