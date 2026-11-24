/**
 * Whether the running version is numerically older than the staged installer's.
 *
 * The fleet's only cue about an update is this comparison, so it may not guess. A version is a
 * sequence of numbers separated by dots — `0.3.9` is older than `0.3.10`, which a string compare
 * would order the other way — and anything else (a null, an empty value, a pre-release suffix) is
 * not a comparable version. The caller gets `false` for an unknown side, which draws no cue: the
 * row says "behind X" only when it can prove both versions.
 *
 * Missing trailing zeroes are equal (`0.3.284` and `0.3.284.0` are the same release), so the shorter
 * side is padded before the walk.
 */
export function versionBelow(
  running: string | null | undefined,
  installer: string | null | undefined
): boolean {
  return compareVersions(running, installer) === -1;
}

/** Which side of the staged release a running build is on, or null when either is no version. */
export type VersionRelation = "current" | "behind" | "ahead";

/**
 * The one answer the Machine tab's update card gives: is this machine current with the release?
 *
 * `versionBelow` answers only "behind", which the fleet's attention cue needs and nothing else can
 * spell. The Machine tab also has to say "ahead" — the measured 9VH ran 0.3.292 while the release
 * was 0.3.291 — and "current" when the two name the same release, trailing zeroes included. An
 * unknown side answers `null` and the card says the release is unknown rather than inventing one.
 */
export function versionRelation(
  running: string | null | undefined,
  release: string | null | undefined
): VersionRelation | null {
  const comparison = compareVersions(running, release);
  if (comparison === null) return null;
  return comparison === 0 ? "current" : comparison < 0 ? "behind" : "ahead";
}

/** -1, 0 or 1 for two dotted versions; null when either side is not one. */
function compareVersions(
  left: string | null | undefined,
  right: string | null | undefined
): -1 | 0 | 1 | null {
  const a = parseVersion(left);
  const b = parseVersion(right);
  if (a === null || b === null) return null;
  const length = Math.max(a.length, b.length);
  for (let index = 0; index < length; index += 1) {
    const leftPart = a[index] ?? 0;
    const rightPart = b[index] ?? 0;
    if (leftPart !== rightPart) return leftPart < rightPart ? -1 : 1;
  }
  return 0;
}

/**
 * Whether two dotted versions name the same release.
 *
 * The update story compares what the running process records with the version the updater says it
 * applied; those are two fields written by two builds of the device. `0.3.284` and `0.3.284.0` are
 * the same release, and a value that is not a version proves nothing about another value — so an
 * unknown side answers false and the caller draws no "this is the running one" claim.
 */
export function versionEqual(
  left: string | null | undefined,
  right: string | null | undefined
): boolean {
  const a = parseVersion(left);
  const b = parseVersion(right);
  if (a === null || b === null) return false;
  const length = Math.max(a.length, b.length);
  for (let index = 0; index < length; index += 1) {
    if ((a[index] ?? 0) !== (b[index] ?? 0)) return false;
  }
  return true;
}

/** The numeric parts of a dotted version, or null when the value is not one. */
function parseVersion(value: string | null | undefined): number[] | null {
  if (typeof value !== "string" || value.trim() === "") return null;
  const parts = value.trim().split(".");
  const numbers: number[] = [];
  for (const part of parts) {
    if (!/^\d+$/.test(part)) return null;
    const number = Number(part);
    if (!Number.isSafeInteger(number)) return null;
    numbers.push(number);
  }
  return numbers;
}

/**
 * **ONE MACHINE HAS ONE CURRENCY — AND ONE PLACE DEFINES IT (the thirty-fourth list).** Home's
 * attention block and the machine's own Machine tab used to compute their target separately: Home
 * took the newer of the fleet's newest build and the staged installer, the tab took the installer
 * alone, so one machine read `behind 0.3.292` in Needs attention and `current (the release)` on its
 * own page in one visit. These two functions are the single definition both callers use.
 *
 * `releaseTarget` answers the newer of the two; `from` says which number it is, so a sentence can
 * name its subject in words instead of saying "the release" about a file the fleet has outrun.
 */
export type Release = { version: string; from: "fleet" | "installer" };

/** The newest build any machine reports, or null when none reported one. */
export function fleetNewestVersion(
  devices: readonly { version?: string | null }[]
): string | null {
  let newest: string | null = null;
  for (const device of devices) {
    const version = device.version;
    if (!version) continue;
    if (newest === null || versionBelow(newest, version)) newest = version;
  }
  return newest;
}

/** The build machines are measured against: the newer of their own newest and the installer. */
export function releaseTarget(
  devices: readonly { version?: string | null }[],
  installer: { available?: boolean; version?: string | null } | null
): Release | null {
  const newest = fleetNewestVersion(devices);
  const staged = installer?.available ? (installer.version ?? null) : null;
  if (newest === null) return staged === null ? null : { version: staged, from: "installer" };
  if (staged === null) return { version: newest, from: "fleet" };
  return versionBelow(newest, staged)
    ? { version: staged, from: "installer" }
    : { version: newest, from: "fleet" };
}
