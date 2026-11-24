#!/usr/bin/env node
// Independent probe of the Summrise payload face.
//
//   node scripts/probe-pe-face.mjs --exe dist/win-unpacked/SummriseDesktop.exe \
//                                  --icon icon.ico --version 2.0.9.0
//
// This script is deliberately not the editor: it parses the PE resource table with
// resedit's reader classes, reads icon.ico's directory itself, and compares the RAW
// bytes of every RT_ICON image with the matching source image. It never asks the
// editor what it wrote, and it understands nothing about pe-face.mjs.
//
// Required facts proved here:
//   * exactly one icon group, id 1, language 1033;
//   * 6/6 icon images byte-equal to icon.ico (count follows the icon file);
//   * the eight VERSIONINFO strings exact, at the exe's one string language;
//   * SquirrelAwareVersion=1 preserved;
//   * electron-builder's INTEGRITY(ELECTRONASAR) entry preserved;
//   * with --baseline, every non-icon/non-version resource entry byte-identical and
//     the exe length unchanged.
//
// Exit 0 = PASS; exit 1 = named mismatches; exit 2 = usage or unreadable input.
// The negative run against the unfaced Electron exe must exit 1.

import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
import { NtExecutable, NtExecutableResource, Resource } from "resedit";

const ICON_GROUP_ID = 1;
const ICON_GROUP_LANG = 1033;

const failures = [];

function usage() {
  console.error(
    "usage: probe-pe-face.mjs --exe <SummriseDesktop.exe> --icon <icon.ico> --version <X.Y.Z.W> [--baseline <unfaced.exe>]",
  );
}

function fatal(message) {
  console.error(`probe: ${message}`);
  process.exit(2);
}

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "-h" || arg === "--help") {
      usage();
      process.exit(0);
    }
    if (arg !== "--exe" && arg !== "--icon" && arg !== "--version" && arg !== "--baseline") {
      fatal(`unknown argument ${JSON.stringify(arg)}`);
    }
    const value = argv[i + 1];
    if (!value || value.startsWith("--")) fatal(`${arg} needs a value`);
    out[arg.slice(2)] = value;
    i += 1;
  }
  if (!out.exe || !out.icon || !out.version) {
    usage();
    fatal("--exe, --icon and --version are required");
  }
  return out;
}

function sha256(buffer) {
  return crypto.createHash("sha256").update(buffer).digest("hex");
}

function readFile(label, file) {
  let stat;
  try {
    stat = fs.statSync(file);
  } catch {
    fatal(`missing ${label}: ${file}`);
  }
  if (!stat.isFile()) fatal(`${label} is not a file: ${file}`);
  return fs.readFileSync(file);
}

function parseVersion(text) {
  const match = /^(\d{1,5})\.(\d{1,5})\.(\d{1,5})\.(\d{1,5})$/.exec(text);
  if (!match) {
    fatal(`version must be exactly X.Y.Z.W (four numbers, e.g. 2.0.9.0), got ${JSON.stringify(text)}`);
  }
  const parts = match.slice(1).map(Number);
  if (parts.some((part) => part > 65535)) fatal(`version component above 65535: ${text}`);
  return parts;
}

function parseIco(buffer, file) {
  if (buffer.length < 6) fatal(`${file} is too short to be an ICO file`);
  const reserved = buffer.readUInt16LE(0);
  const type = buffer.readUInt16LE(2);
  const count = buffer.readUInt16LE(4);
  if (reserved !== 0) fatal(`${file}: ICO reserved field is ${reserved}, expected 0`);
  if (type !== 1) fatal(`${file}: ICO type is ${type}, expected 1 (icon)`);
  if (count < 1) fatal(`${file}: ICO declares no images`);
  if (buffer.length < 6 + count * 16) fatal(`${file}: ICO directory is truncated`);
  const images = [];
  for (let index = 0; index < count; index += 1) {
    const entry = 6 + index * 16;
    const dataSize = buffer.readUInt32LE(entry + 8);
    const dataOffset = buffer.readUInt32LE(entry + 12);
    if (dataSize === 0) fatal(`${file}: image ${index} has zero bytes`);
    if (dataOffset + dataSize > buffer.length) {
      fatal(`${file}: image ${index} runs past the file (${dataOffset}+${dataSize} > ${buffer.length})`);
    }
    images.push({
      width: buffer[entry] === 0 ? 256 : buffer[entry],
      height: buffer[entry + 1] === 0 ? 256 : buffer[entry + 1],
      bitCount: buffer.readUInt16LE(entry + 6),
      data: buffer.subarray(dataOffset, dataOffset + dataSize),
    });
  }
  return images;
}

function check(name, condition, detail) {
  if (condition) return true;
  failures.push(name);
  console.error(`FAIL ${name}: ${detail}`);
  return false;
}

function note(line) {
  console.log(line);
}

const args = parseArgs(process.argv.slice(2));
const versionParts = parseVersion(args.version);
const exePath = path.resolve(args.exe);
const iconPath = path.resolve(args.icon);
const baselinePath = args.baseline ? path.resolve(args.baseline) : null;

const exeBytes = readFile("exe", exePath);
const iconBytes = readFile("icon", iconPath);
const baselineBytes = baselinePath ? readFile("baseline", baselinePath) : null;

note(`exe ${exePath} size=${exeBytes.length} sha256=${sha256(exeBytes)}`);
note(`icon ${iconPath} size=${iconBytes.length} sha256=${sha256(iconBytes)}`);
const sourceImages = parseIco(iconBytes, iconPath);
note(`icon images=${sourceImages.length}`);
sourceImages.forEach((image, index) => {
  note(`  source ${index} ${image.width}x${image.height} ${image.bitCount}bpp bytes=${image.data.length} sha256=${sha256(image.data)}`);
});

let executable;
let resources;
try {
  executable = NtExecutable.from(exeBytes, { ignoreCert: true });
  resources = NtExecutableResource.from(executable);
} catch (error) {
  fatal(`cannot parse ${exePath} as a PE image: ${error.message}`);
}
const entries = resources.entries;

// --- icon group -------------------------------------------------------------------
const groups = Resource.IconGroupEntry.fromEntries(entries);
check("GROUP_ICON count", groups.length === 1, `expected exactly 1, got ${groups.length}`);
const group = groups[0];
if (group) {
  check("GROUP_ICON id", Number(group.id) === ICON_GROUP_ID, `expected ${ICON_GROUP_ID}, got ${group.id}`);
  check(
    "GROUP_ICON language",
    Number(group.lang) === ICON_GROUP_LANG,
    `expected ${ICON_GROUP_LANG}, got ${group.lang}`,
  );
  check(
    "GROUP_ICON image count",
    group.icons.length === sourceImages.length,
    `expected ${sourceImages.length} (icon.ico), got ${group.icons.length}`,
  );

  const iconEntries = entries.filter((entry) => entry.type === 3);
  check(
    "RT_ICON count",
    iconEntries.length === sourceImages.length,
    `expected ${sourceImages.length} (icon.ico), got ${iconEntries.length}`,
  );

  const pairs = Math.min(group.icons.length, sourceImages.length);
  for (let index = 0; index < pairs; index += 1) {
    const groupIcon = group.icons[index];
    const source = sourceImages[index];
    const groupWidth = groupIcon.width === 0 ? 256 : groupIcon.width;
    const groupHeight = groupIcon.height === 0 ? 256 : groupIcon.height;
    check(
      `image ${index} dimensions`,
      groupWidth === source.width && groupHeight === source.height,
      `group says ${groupWidth}x${groupHeight}, icon.ico says ${source.width}x${source.height}`,
    );
    check(
      `image ${index} dataSize`,
      groupIcon.dataSize === source.data.length,
      `group says ${groupIcon.dataSize} B, icon.ico says ${source.data.length} B`,
    );
    const matches = iconEntries.filter(
      (entry) => Number(entry.id) === Number(groupIcon.iconID) && Number(entry.lang) === Number(group.lang),
    );
    if (!check(`image ${index} resource`, matches.length === 1, `iconID ${groupIcon.iconID} matched ${matches.length} RT_ICON entries`)) {
      continue;
    }
    const bytes = Buffer.from(matches[0].bin);
    const equal = bytes.length === source.data.length && bytes.equals(source.data);
    check(
      `image ${index} bytes`,
      equal,
      `exe sha256=${sha256(bytes)} (${bytes.length} B), icon.ico sha256=${sha256(source.data)} (${source.data.length} B)`,
    );
    if (equal) {
      note(`  image ${index} ${source.width}x${source.height} bytes=${bytes.length} match sha256=${sha256(bytes)}`);
    }
  }
}

// --- VERSIONINFO ------------------------------------------------------------------
const expectedStrings = {
  ProductName: "SummriseDesktop",
  CompanyName: "Summrise",
  FileDescription: "Summrise Desktop",
  InternalName: "SummriseDesktop",
  OriginalFilename: "SummriseDesktop.exe",
  LegalCopyright: "Summrise",
  FileVersion: args.version,
  ProductVersion: args.version,
};

const versions = Resource.VersionInfo.fromEntries(entries);
check("VERSIONINFO count", versions.length === 1, `expected exactly 1, got ${versions.length}`);
const versionInfo = versions[0];
if (versionInfo) {
  const languages = versionInfo.getAllLanguagesForStringValues();
  check("VERSIONINFO string languages", languages.length === 1, `expected exactly 1, got ${languages.length}`);
  const language = languages[0] ?? { lang: 0, codepage: 0 };
  note(`VERSIONINFO lang=${language.lang} codepage=${language.codepage}`);
  const values = versionInfo.getStringValues(language);
  for (const [key, expected] of Object.entries(expectedStrings)) {
    const actual = values[key];
    check(
      `VERSIONINFO ${key}`,
      actual === expected,
      `expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`,
    );
    if (actual === expected) note(`  ${key}=${actual}`);
  }
  check(
    "VERSIONINFO SquirrelAwareVersion",
    values.SquirrelAwareVersion === "1",
    `expected "1", got ${JSON.stringify(values.SquirrelAwareVersion)}`,
  );
  if (values.SquirrelAwareVersion === "1") note("  SquirrelAwareVersion=1 (preserved)");

  const fixed = versionInfo.fixedInfo;
  const [major, minor, micro, revision] = versionParts;
  const expectedMS = ((major << 16) | minor) >>> 0;
  const expectedLS = ((micro << 16) | revision) >>> 0;
  check(
    "VS_FIXEDFILEINFO fileVersion",
    fixed.fileVersionMS === expectedMS && fixed.fileVersionLS === expectedLS,
    `expected ${args.version} (MS=${expectedMS}, LS=${expectedLS}), got MS=${fixed.fileVersionMS}, LS=${fixed.fileVersionLS}`,
  );
  check(
    "VS_FIXEDFILEINFO productVersion",
    fixed.productVersionMS === expectedMS && fixed.productVersionLS === expectedLS,
    `expected ${args.version} (MS=${expectedMS}, LS=${expectedLS}), got MS=${fixed.productVersionMS}, LS=${fixed.productVersionLS}`,
  );
}

// --- preservation -----------------------------------------------------------------
const integrity = entries.filter((entry) => entry.type === "INTEGRITY");
check("INTEGRITY count", integrity.length === 1, `expected exactly 1, got ${integrity.length}`);
if (integrity[0]) {
  const entry = integrity[0];
  check("INTEGRITY id", entry.id === "ELECTRONASAR", `expected "ELECTRONASAR", got ${JSON.stringify(entry.id)}`);
  check("INTEGRITY language", Number(entry.lang) === 1033, `expected 1033, got ${entry.lang}`);
  let body = null;
  try {
    body = JSON.parse(Buffer.from(entry.bin).toString("utf8"));
  } catch (error) {
    check("INTEGRITY body", false, `not JSON: ${error.message}`);
  }
  if (body) {
    const ok =
      Array.isArray(body) &&
      body.length === 1 &&
      body[0]?.file === "resources/app.asar" &&
      body[0]?.alg === "SHA256" &&
      typeof body[0]?.value === "string" &&
      /^[0-9a-f]{64}$/.test(body[0].value);
    check("INTEGRITY body", ok, `expected the resources/app.asar SHA256 entry, got ${JSON.stringify(body)}`);
    if (ok) note(`  INTEGRITY ELECTRONASAR resources/app.asar SHA256 ${body[0].value}`);
  }
}

if (baselinePath) {
  let baselineResources;
  try {
    baselineResources = NtExecutableResource.from(NtExecutable.from(baselineBytes, { ignoreCert: true }));
  } catch (error) {
    fatal(`cannot parse baseline ${baselinePath} as a PE image: ${error.message}`);
  }
  check(
    "size unchanged",
    exeBytes.length === baselineBytes.length,
    `baseline ${baselineBytes.length} B, faced ${exeBytes.length} B`,
  );

  // The face changes only the icon group, its RT_ICON images and VERSIONINFO; every
  // other resource entry must be the same bytes under the same (type, id, language).
  const changedType = (entry) => entry.type === 3 || entry.type === 14 || entry.type === 16;
  const keyOf = (entry) => `${String(entry.type)}|${String(entry.id)}|${String(entry.lang)}`;
  const baselineMap = new Map(baselineResources.entries.filter((e) => !changedType(e)).map((e) => [keyOf(e), e]));
  const facedMap = new Map(entries.filter((e) => !changedType(e)).map((e) => [keyOf(e), e]));
  let preserved = 0;
  for (const [key, before] of baselineMap) {
    const after = facedMap.get(key);
    if (!after) {
      check(`preserved ${key}`, false, "missing from the faced exe");
      continue;
    }
    const equal = Buffer.from(after.bin).equals(Buffer.from(before.bin));
    check(
      `preserved ${key}`,
      equal,
      `bytes differ (baseline ${before.bin.byteLength} B, faced ${after.bin.byteLength} B)`,
    );
    if (equal) preserved += 1;
  }
  for (const key of facedMap.keys()) {
    if (!baselineMap.has(key)) check(`preserved ${key}`, false, "resource not present in the baseline");
  }
  note(`preserved ${preserved}/${baselineMap.size} non-icon, non-version resource entries byte-identical to baseline`);
}

if (failures.length > 0) {
  console.error(`PROBE FAIL (${failures.length} mismatches)`);
  process.exit(1);
}
console.log(`PROBE PASS exe_size=${exeBytes.length} exe_sha256=${sha256(exeBytes)}`);
