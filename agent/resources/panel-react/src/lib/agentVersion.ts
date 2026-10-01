/**
 * Which version to show for a device.
 *
 * THE DEVICE REPORTS TWO, AND THEY ARE NOT INTERCHANGEABLE:
 *
 *   `release`  the npm release — 1.2.x — written by install/update into
 *              `.summrise-release`. THIS is the number that changes per release, and the
 *              only one a user can compare against "is my device up to date?".
 *   `version`  the Cargo protocol version — 1.0.x — from `env!("CARGO_PKG_VERSION")`.
 *              FROZEN: it has not moved in a very long time and is not a release.
 *
 * `DesktopShell` learned this and says so in a comment beside its own copy of the
 * rule; `ConnectCard`'s connection probe kept using `version`. In the desktop shell
 * both are on screen at once — the status strip reading v1.2.354 while Settings
 * reported v1.0.145 for the SAME device, so a user checking whether their update or
 * registration took got two answers.
 *
 * Two copies of a rule is what let them disagree, so the rule lives here now and both
 * callers use it.
 *
 * AND IT IS STILL TWO COPIES, one per package: the gateway decides the same thing for the console in
 * `gateway/src/plugins/mcp.ts` (`wireVersion`), and it met the same defect from the other side — round-304
 * there, a strip reading v1.2.354 here. They cannot share a module, so each names the other and
 * `agent/tests/device_version_rule.rs` holds both to one table. `version` remains the fallback for a device old enough not to send
 * `release` at all.
 *
 * ── RUST SINCE 2026-09-30 (block ②), AND THE GATE FOLLOWED IT ────────────────────────────────────
 *
 * The rule is `agent/resources/panel-logic/src/version.rs` now; the differential is 52 corpus cases with 0
 * divergences and every arm reached, and `lib/agentVersion.test.ts` runs UNCHANGED.
 *
 * `agent/tests/device_version_rule.rs` — which this file used to cite by its old `.mjs` name, a reference
 * that had outlived the file — reads the CRATE for the panel's half now: it still requires the rule to be
 * implemented under a name, still requires BOTH halves to test `release` before `version`, and still
 * requires each side to NAME the other. That last one is why the crate's header names
 * `gateway/src/plugins/mcp.ts`, and why the gateway's comment names the crate.
 */
import { logic } from "../wasm/panelLogic";

export function releaseVersion(status: unknown): string {
  return logic().release_version(status) as string;
}

/** `v1.2.354`, or `v?` when the device reported neither — never a bare "v". */

export function releaseVersionLabel(status: unknown): string {
  return logic().release_version_label(status) as string;
}
