#!/usr/bin/env node
// Summrise payload face: give the built Electron binary its brand icon and product
// metadata, deterministically, with no wine and no signing.
//
// electron-builder cannot run its rcedit step on this Linux host (`app-builder rcedit`
// answers "wine is required"), so the 2.0.9 payload is built with
// `-c.win.signAndEditExecutable=false` and this script edits the exe afterwards:
//
//   node scripts/pe-face.mjs --exe dist/win-unpacked/SummriseDesktop.exe \
//                            --icon icon.ico --version 2.0.9
//
// The edit replaces the existing icon group (id 1, language 1033) with every image in
// icon.ico and rewrites the VERSIONINFO string table at the language the exe already
// uses. Every other resource entry -- electron-builder's INTEGRITY(ELECTRONASAR), the
// manifest, the cursors -- is carried through untouched; `SquirrelAwareVersion` is an
// existing string value and is preserved by merging, not by replacement.
//
// The output is generated in memory and installed with a temp file + rename, so an
// interruption never leaves a half-faced executable behind. The separate
// `probe-pe-face.mjs` proves the result; this script's own report is not proof.
//
// resedit is pinned exactly (1.7.2): 3.x has a different API.

import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { Data, NtExecutable, NtExecutableResource, Resource } from "resedit";

const ICON_GROUP_ID = 1;
const ICON_GROUP_LANG = 1033;

// The eight named strings the payload carries. `FileVersion`/`ProductVersion` are set
// from --version (plus the fourth, always-zero component) by VersionInfo itself.
const FACTS = {
  ProductName: "SummriseDesktop",
  CompanyName: "Summrise",
  FileDescription: "Summrise Desktop",
  InternalName: "SummriseDesktop",
  OriginalFilename: "SummriseDesktop.exe",
  LegalCopyright: "Summrise",
};

function usage() {
  console.error("usage: pe-face.mjs --exe <SummriseDesktop.exe> --icon <icon.ico> --version <X.Y.Z>");
}

function fail(message) {
  console.error(`pe-face: ${message}`);
  process.exit(1);
}

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "-h" || arg === "--help") {
      usage();
      process.exit(0);
    }
    if (arg !== "--exe" && arg !== "--icon" && arg !== "--version") {
      fail(`unknown argument ${JSON.stringify(arg)}`);
    }
    const value = argv[i + 1];
    if (!value || value.startsWith("--")) fail(`${arg} needs a value`);
    out[arg.slice(2)] = value;
    i += 1;
  }
  if (!out.exe || !out.icon || !out.version) {
    usage();
    fail("--exe, --icon and --version are required");
  }
  return out;
}

function sha256(buffer) {
  return crypto.createHash("sha256").update(buffer).digest("hex");
}

function parseVersion(text) {
  const match = /^(\d{1,5})\.(\d{1,5})\.(\d{1,5})$/.exec(text);
  if (!match) fail(`version must be exactly X.Y.Z (three numbers), got ${JSON.stringify(text)}`);
  const parts = match.slice(1).map(Number);
  if (parts.some((part) => part > 65535)) fail(`version component above 65535: ${text}`);
  return { full: `${parts.join(".")}.0` };
}

const args = parseArgs(process.argv.slice(2));
const { full: fullVersion } = parseVersion(args.version);
const exePath = path.resolve(args.exe);
const iconPath = path.resolve(args.icon);

for (const [label, file] of [["exe", exePath], ["icon", iconPath]]) {
  let stat;
  try {
    stat = fs.statSync(file);
  } catch {
    fail(`missing ${label}: ${file}`);
  }
  if (!stat.isFile()) fail(`${label} is not a file: ${file}`);
  if (stat.size === 0) fail(`${label} is empty: ${file}`);
}

const before = fs.readFileSync(exePath);
const beforeSha = sha256(before);

let executable;
let resources;
try {
  executable = NtExecutable.from(before, { ignoreCert: true });
  resources = NtExecutableResource.from(executable);
} catch (error) {
  fail(`cannot parse ${exePath} as a PE image: ${error.message}`);
}
const entries = resources.entries;

// --- icon group -------------------------------------------------------------------
const groups = Resource.IconGroupEntry.fromEntries(entries);
if (groups.length === 0) fail(`no GROUP_ICON resource in ${exePath}`);
if (groups.length !== 1) fail(`expected exactly 1 GROUP_ICON resource, found ${groups.length}`);
const group = groups[0];
if (Number(group.id) !== ICON_GROUP_ID) fail(`GROUP_ICON id ${group.id}, expected ${ICON_GROUP_ID}`);
if (Number(group.lang) !== ICON_GROUP_LANG) {
  fail(`GROUP_ICON language ${group.lang}, expected ${ICON_GROUP_LANG}`);
}

const iconBytes = fs.readFileSync(iconPath);
let iconFile;
try {
  iconFile = Data.IconFile.from(iconBytes);
} catch (error) {
  fail(`cannot parse ${iconPath} as an ICO file: ${error.message}`);
}
const icons = iconFile.icons.map((item) => item.data);
if (icons.length === 0) fail(`no images in ${iconPath}`);

// --- VERSIONINFO ------------------------------------------------------------------
const versions = Resource.VersionInfo.fromEntries(entries);
if (versions.length === 0) fail(`no VERSIONINFO resource in ${exePath}`);
if (versions.length !== 1) fail(`expected exactly 1 VERSIONINFO resource, found ${versions.length}`);
const versionInfo = versions[0];
const languages = versionInfo.getAllLanguagesForStringValues();
if (languages.length === 0) fail(`VERSIONINFO has no string-table language in ${exePath}`);
if (languages.length !== 1) {
  fail(`VERSIONINFO has ${languages.length} string-table languages, expected exactly 1`);
}
const language = languages[0];
const existingStrings = versionInfo.getStringValues(language);

try {
  versionInfo.setStringValues(language, { ...existingStrings, ...FACTS });
  versionInfo.setFileVersion(fullVersion, Number(language.lang));
  versionInfo.setProductVersion(fullVersion, Number(language.lang));
  versionInfo.outputToResourceEntries(entries);
} catch (error) {
  fail(`cannot rewrite VERSIONINFO: ${error.message}`);
}

// --- icon images ------------------------------------------------------------------
try {
  Resource.IconGroupEntry.replaceIconsForResource(entries, ICON_GROUP_ID, ICON_GROUP_LANG, icons);
} catch (error) {
  fail(`cannot replace the icon group: ${error.message}`);
}

// --- write ------------------------------------------------------------------------
let after;
try {
  resources.outputResource(executable);
  after = Buffer.from(executable.generate());
} catch (error) {
  fail(`cannot generate the faced exe: ${error.message}`);
}
if (after.length === 0) fail("generated exe is empty");

const afterSha = sha256(after);
if (afterSha === beforeSha) fail("generated exe is byte-identical to the input; nothing changed");

const dir = path.dirname(exePath);
const temp = path.join(dir, `.${path.basename(exePath)}.face-${process.pid}.tmp`);
const mode = fs.statSync(exePath).mode & 0o777;
try {
  fs.writeFileSync(temp, after, { mode });
  fs.renameSync(temp, exePath);
} catch (error) {
  try {
    fs.unlinkSync(temp);
  } catch {
    // The temp file may never have been created; the rename failure is the report.
  }
  fail(`cannot write ${exePath}: ${error.message}`);
}

const integrity = entries.filter((entry) => entry.type === "INTEGRITY");
const preserved = integrity.length === 1 ? `${integrity[0].id}/${integrity[0].lang}` : `count=${integrity.length}`;

console.log(
  [
    `pe-face: ${exePath}`,
    `size=${before.length}->${after.length}`,
    `sha256=${afterSha}`,
    `icon=${iconPath}`,
    `icon_sha256=${sha256(iconBytes)}`,
    `images=${icons.length}`,
    `group=${ICON_GROUP_ID}/${ICON_GROUP_LANG}`,
    `version=${fullVersion}`,
    `version_lang=${language.lang}/${language.codepage}`,
    `squirrel=${existingStrings.SquirrelAwareVersion ?? "absent"}`,
    `integrity=${preserved}`,
  ].join(" "),
);
