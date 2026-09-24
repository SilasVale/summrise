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
 * `device-version-rule-check.mjs` holds both to one table. `version` remains the fallback for a device old enough not to send
 * `release` at all.
 */
export function releaseVersion(status: unknown): string {
  const s = (status ?? {}) as { release?: unknown; version?: unknown };
  if (typeof s.release === "string" && s.release) return s.release;
  if (typeof s.version === "string" && s.version) return s.version;
  return "";
}

/** `v1.2.354`, or `v?` when the device reported neither — never a bare "v". */
export function releaseVersionLabel(status: unknown): string {
  return `v${releaseVersion(status) || "?"}`;
}
