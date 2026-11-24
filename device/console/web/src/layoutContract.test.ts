// A3 light source and semantic contracts. Values come from the accepted design.
// Exact selector/condition reads do not simulate browser specificity, layout or paint.
// Rendered geometry, actual painted AA and platform behavior have separate gates.
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createElement, Fragment, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { within } from "@testing-library/react";
import ts from "typescript";
import { PageHeading } from "./PageHeading";
import { DevicePageShell } from "./DevicePageShell";
import { DevicePicker } from "./DevicePicker";
import { Rail, PAGE_LABELS, DEVICE_TABS } from "./Rail";
import type { Device } from "./api";

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.resolve(here, "../../../..");
const contractPath = path.join(repo, "docs/superpowers/specs/layout-contract.json");
const readRepo = (name: string) => readFileSync(path.join(repo, name), "utf8");
const readSource = (name: string) => readFileSync(path.join(here, name), "utf8");
const css = () => readSource("app.css");
const tokens = () => readSource("tokens.css");

type Contract = {
  version: "A3-light";
  ground: { light: string; surface: string; dark: null; decorativeGradients: boolean };
  palette: { ink: string; muted: string; faint: string; accent: string; danger: string;
    semantic: string[]; accentFamily: { hover: string; onSoft: string; washValue: string };
    sidebar: { surface: string; ink: string; muted: string; selected: string; onSelected: string; focus: string } };
  page: { max: number; pad: number; phonePad: number; gap: number; boxSizing: string; workspace: string };
  header: { height: number; background: string; borderBottom: number };
  rail: { console: number; icon: number; compactMin: number; compactMax: number; phoneMax: number; phoneMode: string; deviceSections: number };
  navigation: { global: string[]; contextual: string; overviewLabel: string; deviceSections: string[] };
  title: { size: number; line: number; weight: number };
  section: { h2: { size: number; line: number; weight: number }; detail: { size: number; line: number; weight: number }; label: { size: number; line: number; weight: number } };
  metric: { size: number; line: number; weight: number };
  text: Record<"body" | "status" | "metadata", { size: number; line: number }>;
  head: { gap: number; align: string; maxHeadsPerPage: number; pageLevel: number; toolLevel: number; subtitleSlot: string };
  space: { scale: number[]; controlH: number; controlPadY: number; target: number; targetTouch: number };
  card: { radius: number; pad: number; gap: number };
  radius: { page: Record<"card" | "row" | "control" | "mark", number>; overlay: { sheet: number; dialog: number }; pill: number };
  hairline: { line: string; strong: string; soft: string; stateMarksCarryOwnColour: boolean; stateMarksCarryWord: boolean };
  leading: { body: number; tight: number; mark: number };
  measure: { prose: string; codeExempt: boolean };
  elevation: { flat: string; pane: string; raised: string; recess: string; recessTint: number; cardTint: number };
  empty: { wayBack: boolean; inCardSurface: string; inCardBorder: number; inCardShadow: string };
  loading: { rows: number; role: string; labelRequired: boolean };
  motion: { reducedMotion: string; cap: string; namedAnimationsStop: boolean; interactionMs: number };
  accessibility: { textContrast: number; controlContrast: number; focusWidth: number; hiddenIsAuthoritative: boolean; forcedColors: string };
  theme: { decides: string; override: boolean };
};
const contract = JSON.parse(readFileSync(contractPath, "utf8")) as Contract;

const PHONE = ["@media (max-width: 639px)"];
const COMPACT = ["@media (min-width: 640px) and (max-width: 1100px)"];
const REDUCE = ["@media (prefers-reduced-motion: reduce)"];
const FORCED = ["@media (forced-colors: active)"];
function normalize(value: string): string {
  let result = "", quote = "", escaped = false, space = false;
  for (const c of value.trim()) {
    if (quote) {
      result += c;
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === quote) quote = "";
    } else if (/\s/.test(c)) space = true;
    else {
      if (space && result) result += " ";
      space = false; result += c;
      if (c === '"' || c === "'") quote = c;
    }
  }
  return result;
}
const literalSelector = (selector: string) => normalize(selector.split("\\.").join(".").split("\\*").join("*"));
type Declaration = { name: string; value: string };
type SheetRule = { selectors: string[]; conditions: string[]; declarations: Declaration[]; at: number };

/** CSS comments cannot terminate a quoted value. Positions survive comment removal. */
function stripCssComments(source: string): string {
  let result = "", quote = "", escaped = false;
  for (let at = 0; at < source.length; at += 1) {
    const c = source[at];
    if (quote) {
      result += c;
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === quote) quote = "";
    } else if (c === '"' || c === "'") { quote = c; result += c; }
    else if (c === "/" && source[at + 1] === "*") {
      const end = source.indexOf("*/", at + 2);
      if (end < 0) throw new Error("Unterminated CSS comment");
      result += source.slice(at, end + 2).replace(/[^\n]/g, " "); at = end + 1;
    } else result += c;
  }
  if (quote) throw new Error("Unterminated CSS string");
  return result;
}

/** Split outside functional values, attribute selectors and quoted strings. */
function splitCss(value: string, separator: string): string[] {
  const parts: string[] = [];
  let start = 0, depth = 0, quote = "", escaped = false;
  for (let at = 0; at < value.length; at += 1) {
    const c = value[at];
    if (quote) {
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === quote) quote = "";
    } else if (c === '"' || c === "'") quote = c;
    else if (c === "(" || c === "[") depth += 1;
    else if (c === ")" || c === "]") { depth -= 1; if (depth < 0) throw new Error("Unbalanced CSS value"); }
    else if (c === separator && depth === 0) { parts.push(value.slice(start, at).trim()); start = at + 1; }
  }
  if (quote || depth !== 0) throw new Error("Unbalanced CSS value");
  parts.push(value.slice(start).trim());
  return parts.filter(Boolean);
}

/** Locate a block opening without borrowing a brace inside a selector's string. */
function openingBrace(source: string, from: number, to: number): number {
  let quote = "", escaped = false, depth = 0;
  for (let at = from; at < to; at += 1) {
    const c = source[at];
    if (quote) {
      if (escaped) escaped = false;
      else if (c === "\\") escaped = true;
      else if (c === quote) quote = "";
    } else if (c === '"' || c === "'") quote = c;
    else if (c === "(" || c === "[") depth += 1;
    else if (c === ")" || c === "]") depth -= 1;
    else if (c === "{" && depth === 0) return at;
  }
  throw new Error(`CSS has no opening brace at ${from}`);
}

const sheetCache = new Map<string, SheetRule[]>();
function sheetRules(source: string): SheetRule[] {
  const cached = sheetCache.get(source);
  if (cached) return cached;
  const clean = stripCssComments(source), result: SheetRule[] = [];
  const walk = (from: number, to: number, conditions: string[]) => {
    let at = from;
    while (at < to) {
      while (at < to && /\s/.test(clean[at])) at += 1;
      if (at === to) break;
      const opening = openingBrace(clean, at, to);
      const heading = normalize(clean.slice(at, opening));
      let cursor = opening + 1, depth = 1, quote = "", escaped = false;
      while (cursor < to && depth > 0) {
        const c = clean[cursor];
        if (quote) {
          if (escaped) escaped = false;
          else if (c === "\\") escaped = true;
          else if (c === quote) quote = "";
        } else if (c === '"' || c === "'") quote = c;
        else if (c === "{") depth += 1;
        else if (c === "}") depth -= 1;
        cursor += 1;
      }
      if (depth !== 0) throw new Error(`CSS has no matching closing brace for ${heading}`);
      if (heading.startsWith("@")) walk(opening + 1, cursor - 1, [...conditions, heading]);
      else {
        const declarations = splitCss(clean.slice(opening + 1, cursor - 1), ";").map((value) => {
          const colon = value.indexOf(":");
          if (colon < 1) throw new Error(`Invalid CSS declaration: ${value}`);
          return { name: value.slice(0, colon).trim(), value: value.slice(colon + 1).trim() };
        });
        result.push({ selectors: splitCss(heading, ",").map(normalize), conditions, declarations, at });
      }
      at = cursor;
    }
  };
  walk(0, clean.length, []);
  sheetCache.set(source, result);
  return result;
}
function matchingRules(source: string, selector: string, conditions: string[] = []): SheetRule[] {
  const wanted = literalSelector(selector);
  return sheetRules(source).filter((rule) => rule.selectors.includes(wanted)
    && JSON.stringify(rule.conditions) === JSON.stringify(conditions));
}

/** Same-selector source order and importance; this is not a specificity/inheritance engine. */
function properties(source: string, selector: string, conditions: string[] = []): Map<string, string> {
  const found = matchingRules(source, selector, conditions);
  if (found.length === 0) throw new Error(`No rule for ${selector} in ${conditions.join(" / ") || "base"}`);
  const answer = new Map<string, { value: string; important: boolean }>();
  const assign = (name: string, value: string, important: boolean) => {
    if (answer.get(name)?.important && !important) return;
    answer.delete(name); answer.set(name, { value, important });
  };
  for (const rule of found) for (const declaration of rule.declarations) {
    const { name } = declaration;
    const important = /\s*!important\s*$/.test(declaration.value);
    const plain = declaration.value.replace(/\s*!important\s*$/, "").trim();
    if (answer.get(name)?.important && !important) continue;
    const prefixes: Record<string, string[]> = {
      padding: ["padding-"], margin: ["margin-"], border: ["border-"],
      background: ["background-"], font: ["font-", "line-height"],
      gap: ["row-gap", "column-gap"], flex: ["flex-grow", "flex-shrink", "flex-basis"]
    };
    for (const prefix of prefixes[name] ?? []) for (const [key, previous] of answer) {
      if (key.startsWith(prefix) && !(name === "border" && key.includes("radius"))
        && (important || !previous.important)) answer.delete(key);
    }
    assign(name, declaration.value, important);
    if (name === "padding" || name === "margin") {
      const sizes = splitCss(plain, " ");
      if (sizes.length > 4) throw new Error(`Invalid ${name} shorthand`);
      const [a, b = a, c = a, d = b] = sizes;
      for (const [side, size] of [["top", a], ["right", b], ["bottom", c], ["left", d]]) {
        assign(`${name}-${side}`, size + (important ? " !important" : ""), important);
      }
    }
  }
  return new Map([...answer].map(([name, declaration]) => [name, declaration.value]));
}
function valueFor(source: string, selector: string, property: string, conditions: string[] = []): string {
  const value = properties(source, selector, conditions).get(property);
  if (value === undefined) throw new Error(`${selector} has no ${property}`);
  return value;
}
function declared(source: string, name: string): string {
  const match = new RegExp(`--${name}\\s*:\\s*([^;]+);`).exec(stripCssComments(source));
  if (!match) throw new Error(`Missing --${name}`);
  return match[1].trim();
}
function resolveRole(source: string, name: string, seen = new Set<string>()): string {
  if (seen.has(name)) throw new Error(`Cyclic role --${name}`);
  seen.add(name);
  const value = declared(source, name), alias = /^var\(--([\w-]+)\)$/.exec(value);
  return alias ? resolveRole(source, alias[1], seen) : value;
}
function tokenPx(source: string, role: string): number {
  const value = resolveRole(source, role);
  if (!/^[0-9.]+px$/.test(value)) throw new Error(`--${role} is not a pixel role`);
  return Number(value.slice(0, -2));
}
function lengthFor(source: string, selector: string, property: string, conditions: string[] = []): number {
  const value = valueFor(source, selector, property, conditions), role = /^var\(--([\w-]+)\)$/.exec(value);
  if (role) return tokenPx(tokens(), role[1]);
  if (!/^[0-9.]+px$/.test(value)) throw new Error(`${selector} ${property} is not a scalar length`);
  return Number(value.slice(0, -2));
}
function rgbValue(source: string, value: string, seen = new Set<string>()): number[] {
  const role = /^var\(--([\w-]+)\)$/.exec(value);
  if (role) return rgbRole(source, role[1], seen);
  let hex = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(value)?.[1];
  if (hex) {
    if (hex.length === 3) hex = [...hex].map((c) => c + c).join("");
    return [0, 2, 4].map((at) => parseInt(hex!.slice(at, at + 2), 16));
  }
  const mix = /^color-mix\(in srgb, var\(--([\w-]+)\) ([\d.]+)%, var\(--([\w-]+)\)\)$/.exec(value);
  if (!mix) throw new Error(`Unsupported measurable color: ${value}`);
  const weight = Number(mix[2]) / 100;
  if (weight < 0 || weight > 1) throw new Error("Invalid color weight");
  const a = rgbRole(source, mix[1], new Set(seen)), b = rgbRole(source, mix[3], new Set(seen));
  return a.map((channel, at) => channel * weight + b[at] * (1 - weight));
}
function rgbRole(source: string, role: string, seen = new Set<string>()): number[] {
  if (seen.has(role)) throw new Error(`Cyclic color --${role}`);
  seen.add(role);
  return rgbValue(source, declared(source, role), seen);
}
function contrast(a: number[], b: number[]): number {
  const luminance = (rgb: number[]) => rgb.map((channel) => {
    const value = channel / 255;
    return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  }).reduce((sum, value, at) => sum + value * [0.2126, 0.7152, 0.0722][at], 0);
  const first = luminance(a), second = luminance(b);
  return (Math.max(first, second) + 0.05) / (Math.min(first, second) + 0.05);
}
function rustConstant(source: string, name: string): string {
  const match = new RegExp(`const ${name}: &str = r(#+)"([\\s\\S]*?)"\\1;`).exec(source);
  if (!match) throw new Error(`Missing Rust raw constant ${name}`);
  return match[2];
}
function assertSharedStyleOrder(source: string): void {
  const body = /pub fn style\([^)]*\)\s*->\s*String\s*\{([\s\S]*?)\n\}/.exec(source)?.[1];
  if (!body) throw new Error("Missing shared style function");
  const positions = ["css.push_str(TOKENS)", "css.push_str(COMPONENTS)", "css.push_str(components)"].map((part) => body.indexOf(part));
  if (positions.some((at) => at < 0)) throw new Error("Missing shared style layer");
  expect(positions[0]).toBeLessThan(positions[1]);
  expect(positions[1]).toBeLessThan(positions[2]);
}
function jsxElements(file: string, tag: string): ts.JsxOpeningLikeElement[] {
  const source = ts.createSourceFile(file, readSource(file), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX);
  const found: ts.JsxOpeningLikeElement[] = [];
  const walk = (node: ts.Node) => {
    if ((ts.isJsxOpeningElement(node) || ts.isJsxSelfClosingElement(node)) && node.tagName.getText(source) === tag) found.push(node);
    ts.forEachChild(node, walk);
  };
  walk(source); return found;
}
function classNames(node: ts.JsxOpeningLikeElement): string[] {
  const attr = node.attributes.properties.find((item) => ts.isJsxAttribute(item) && item.name.getText() === "className");
  return attr && ts.isJsxAttribute(attr) && attr.initializer && ts.isStringLiteral(attr.initializer)
    ? attr.initializer.text.split(/\s+/) : [];
}
function headingLevel(node: ts.JsxOpeningLikeElement): number | undefined {
  const attr = node.attributes.properties.find((item) => ts.isJsxAttribute(item) && item.name.getText() === "level");
  if (!attr || !ts.isJsxAttribute(attr) || !attr.initializer) return undefined;
  const initial = attr.initializer;
  return ts.isJsxExpression(initial) && initial.expression && ts.isNumericLiteral(initial.expression) ? Number(initial.expression.text) : undefined;
}
function dom(node: ReactNode): HTMLElement {
  const container = document.createElement("div");
  container.innerHTML = renderToStaticMarkup(node);
  return container;
}
const machine: Device = { id: "layout-fixture", name: "Fixture machine", online: true, version: "1", last_seen_ms: null };
function deviceDom(hidden = false): HTMLElement {
  return dom(createElement(DevicePageShell, {
    tab: "terminal", onTab: () => {}, devices: [machine], chosen: machine,
    onChoose: () => {}, onConnect: () => {}, hidden,
    children: () => createElement(Fragment, null,
      createElement(PageHeading, { title: "Terminal", level: 2 }),
      createElement("section", { className: "terminal-page", hidden: true }, "Mounted terminal fixture"))
  }));
}
const localStyles = ["styles/fleet-overview.css", "styles/activity.css", "styles/records.css", "styles/workspaces.css", "styles/tools-machine.css"];

describe("A3 light layout contract", () => {
  it("records the independent A3 hierarchy and its audit-consumer field", () => {
    expect(contract).toMatchObject({ version: "A3-light", page: { max: 1440, pad: 32, phonePad: 16 },
      title: { size: 28, line: 36, weight: 650 }, section: { h2: { size: 18, line: 26, weight: 600 } },
      metric: { size: 32, line: 40, weight: 650 }, space: { controlH: 32, target: 32, targetTouch: 40 },
      head: { maxHeadsPerPage: 1, pageLevel: 1, toolLevel: 2 },
      accessibility: { textContrast: 4.5, controlContrast: 3, focusWidth: 2 }, elevation: { cardTint: 1.072 } });
    expect(contract.text).toEqual({ body: { size: 14, line: 21 }, status: { size: 13, line: 19.5 }, metadata: { size: 12, line: 18 } });
  });

  it("uses the canonical prefix in Console and the current Device source chain", () => {
    const shared = readRepo("device/ui/src/lib.rs"), canonical = rustConstant(shared, "TOKENS");
    expect(tokens()).toBe("/* Generated by scripts/generate-tokens.sh from device/ui/src/lib.rs. Do not edit. */\n" + canonical);
    assertSharedStyleOrder(shared);
    expect(() => assertSharedStyleOrder(shared.replace("css.push_str(TOKENS);", ""))).toThrow("Missing shared style layer");
    expect(readRepo("device/device/src/panel.rs")).toContain("summrise_ui::style(COMPONENTS)");
    expect(readRepo("device/device/src/activity.rs")).toContain("summrise_ui::style(STYLES)");
    expect(() => rustConstant("", "TOKENS")).toThrow("Missing Rust raw constant TOKENS");
  });

  it("keeps exact selectors and conditions isolated and fails closed on missing declarations", () => {
    expect(valueFor(css(), ".page", "width")).toBe("100%");
    const fixture = '.one { padding: 2px 4px; } .two { gap: 8px; } @media (max-width: 639px) { .one { gap: 12px; } }';
    expect(valueFor(fixture, ".one", "padding-left")).toBe("4px");
    expect(valueFor(fixture, ".one", "gap", PHONE)).toBe("12px");
    expect(() => valueFor(fixture, ".one", "gap")).toThrow(".one has no gap");
    expect(() => valueFor(fixture, ".absent", "gap")).toThrow("No rule for .absent");
    expect(() => sheetRules(".broken { gap: 8px;")).toThrow("no matching closing brace");
    expect(() => sheetRules(".broken { invalid; }")).toThrow("Invalid CSS declaration");
  });

  it("reads shorthand order and importance without resetting border radius", () => {
    const fixture = '.one { padding-left: 1px !important; padding: 2px 4px; border-radius: 12px; border-left-color: red; border: 1px solid black; }';
    expect(valueFor(fixture, ".one", "padding-left")).toBe("1px !important");
    expect(valueFor(fixture, ".one", "padding-right")).toBe("4px");
    expect(valueFor(fixture, ".one", "border-radius")).toBe("12px");
    expect(properties(fixture, ".one").has("border-left-color")).toBe(false);
    expect(valueFor(css() + '\n[hidden] { display: grid; }', "[hidden]", "display")).toBe("none !important");
  });

  it("splits selector lists outside functions and quoted delimiters", () => {
    const fixture = ':is(.first, .second), .third[data-mark="{"] { content: "} /* text */"; padding: calc(2px + 4px); }';
    expect(valueFor(fixture, ":is(.first, .second)", "content")).toBe('"} /* text */"');
    expect(valueFor(fixture, '.third[data-mark="{"]', "padding")).toBe("calc(2px + 4px)");
    const quoted = '.label[data-name="two  spaces"] { padding: 14px; }';
    expect(valueFor(quoted, '.label[data-name="two  spaces"]', "padding")).toBe("14px");
    expect(matchingRules(quoted, '.label[data-name="two spaces"]')).toHaveLength(0);
    expect(() => sheetRules("/* unfinished")).toThrow("Unterminated CSS comment");
    expect(() => splitCss("calc(2px + 4px", ";")).toThrow("Unbalanced CSS value");
  });

  it("records source order rather than merging a conditional fold into base", () => {
    const before = '@media (max-width: 639px) { .field { flex: 1 1 100%; } } .field { flex: 1 1 20rem; }';
    expect(valueFor(before, ".field", "flex")).toBe("1 1 20rem");
    expect(valueFor(before, ".field", "flex", PHONE)).toBe("1 1 100%");
    expect(matchingRules(before, ".field", PHONE)[0].at).toBeLessThan(matchingRules(before, ".field")[0].at);
    expect(matchingRules(css(), ".page", PHONE)[0].at).toBeGreaterThan(matchingRules(css(), ".page")[0].at);
  });

  it("has one light palette with readable semantic and sidebar roles", () => {
    const source = tokens();
    for (const [role, value] of Object.entries({ bg: contract.ground.light, panel: contract.ground.surface,
      ink: contract.palette.ink, muted: contract.palette.muted, faint: contract.palette.faint,
      accent: contract.palette.accent, danger: contract.palette.danger, line: contract.hairline.line,
      "line-strong": contract.hairline.strong })) expect(resolveRole(source, role), role).toBe(value);
    for (const [role, value] of Object.entries({ sidebar: contract.palette.sidebar.surface, "sidebar-ink": contract.palette.sidebar.ink,
      "sidebar-muted": contract.palette.sidebar.muted, "sidebar-selected": contract.palette.sidebar.selected, "sidebar-focus": contract.palette.sidebar.focus })) {
      expect(rgbRole(source, role), role).toEqual(rgbValue(source, value));
    }
    expect(contract.ground.dark).toBeNull();
    expect(contract.theme).toEqual({ decides: "single-light", override: false });
    expect(declared(source, "font-serif")).toBe("var(--font)");
    expect(source).not.toMatch(/prefers-color-scheme|data-theme/);
  });

  it("keeps large reading planes light and free of decorative gradients", () => {
    expect(contract.ground.decorativeGradients).toBe(false);
    for (const selector of ["body", "main", ".workspace-frame", ".workspace-sidebar", ".workspace-topbar", ".page", ".card"]) {
      const rule = properties(css(), selector);
      expect([...rule].filter(([name]) => name === "background" || name === "background-image").some(([, value]) => value.includes("gradient(")), selector).toBe(false);
    }
    expect(valueFor(css(), ".workspace-sidebar", "background")).toBe("var(--sidebar)");
    expect(valueFor(css(), ".workspace-topbar", "background")).toBe("var(--panel)");
    expect(valueFor(css(), ".workspace-sidebar", "border-right")).toBe("1px solid var(--line)");
  });

  it("checks static text, ANSI, focus and meaningful-border contrast pairs", () => {
    const pairs = [["ink", "bg"], ["muted", "bg"], ["faint", "bg"], ["faint", "surface-chip"],
      ["accent-fg", "accent-solid"], ["accent-on-soft", "accent-soft"], ["ok", "ok-soft"], ["warn", "warn-soft"], ["danger", "danger-soft"],
      ["warn", "surface-recessed"], ["danger", "surface-recessed"], ["sidebar-ink", "sidebar"], ["sidebar-muted", "sidebar"], ["accent-on-soft", "sidebar-selected"]];
    for (const role of ["black", "white", "red", "green", "yellow", "blue", "magenta", "cyan"]) pairs.push([`term-${role}`, "term-canvas"]);
    for (const [ink, surface] of pairs) expect(contrast(rgbRole(tokens(), ink), rgbRole(tokens(), surface)), `${ink} on ${surface}`).toBeGreaterThanOrEqual(contract.accessibility.textContrast);
    for (const [ink, surface] of [["focus-ring", "panel"], ["focus-ring", "bg"], ["line-strong", "panel"], ["line-strong", "bg"], ["sidebar-focus", "sidebar-selected"]])
      expect(contrast(rgbRole(tokens(), ink), rgbRole(tokens(), surface)), `${ink} on ${surface}`).toBeGreaterThanOrEqual(contract.accessibility.controlContrast);
    expect(() => rgbRole(':root { --bad: oklch(0.5 0.2 20); }', "bad")).toThrow("Unsupported measurable color");
    expect(() => resolveRole(':root { --a: var(--b); --b: var(--a); }', "a")).toThrow("Cyclic role");
  });

  it("preserves readable metadata when a record is selected", () => {
    for (const selector of [".timeline-row.is-selected", ".audit-table .audit-row.is-selected"]) {
      const wash = rgbValue(tokens(), valueFor(css(), selector, "background"));
      expect(contrast(rgbRole(tokens(), "faint"), wash), selector).toBeGreaterThanOrEqual(contract.accessibility.textContrast);
      expect(valueFor(css(), selector, "box-shadow")).toContain("var(--accent)");
      expect(valueFor(css(), selector, "outline", FORCED)).toBe("2px solid Highlight");
    }
  });

  it("draws the fluid overview frame and gives dense workspaces available width", () => {
    expect(valueFor(css(), "*", "box-sizing")).toBe(contract.page.boxSizing);
    expect(valueFor(css(), ".page", "width")).toBe("100%");
    expect(lengthFor(css(), ".page", "max-width")).toBe(contract.page.max);
    expect(lengthFor(css(), ".page", "padding")).toBe(contract.page.pad);
    expect(lengthFor(css(), ".page", "gap")).toBe(contract.page.gap);
    expect(lengthFor(css(), ".page", "padding", PHONE)).toBe(contract.page.phonePad);
    expect(valueFor(css(), 'main[data-ui-density="workspace"] .page', "max-width")).toBe("none");
    expect(valueFor(css(), ".device-content > .page", "padding")).toBe("0");
  });

  it("separates global navigation from a white utility header at desktop and tablet", () => {
    expect(valueFor(css(), ".workspace-frame", "grid-template-columns")).toBe(`${contract.rail.console}px minmax(0, 1fr)`);
    expect(lengthFor(css(), ".workspace-sidebar", "width")).toBe(contract.rail.console);
    expect(valueFor(css(), ".workspace-frame", "grid-template-columns", COMPACT)).toBe(`${contract.rail.icon}px minmax(0, 1fr)`);
    expect(lengthFor(css(), ".workspace-sidebar", "width", COMPACT)).toBe(contract.rail.icon);
    expect(lengthFor(css(), ".topbar-inner", "height")).toBe(contract.header.height);
    expect(valueFor(css(), ".rail", "width")).toBe("100%");
    expect(valueFor(css(), ".rail", "padding")).toBe("0");
    expect(contract.rail.phoneMax + 1).toBe(contract.rail.compactMin);
  });

  it("uses a phone drawer instead of carrying a bottom navigation row", () => {
    expect(contract.rail.phoneMode).toBe("labelled-drawer");
    expect(valueFor(css(), ".workspace-frame", "grid-template-areas", PHONE)).toBe('"header" "alerts" "main"');
    expect(valueFor(css(), ".workspace-sidebar", "position", PHONE)).toBe("fixed");
    expect(valueFor(css(), ".workspace-sidebar", "transform", PHONE)).toBe("translateX(-100%)");
    expect(valueFor(css(), '.workspace-frame[data-nav-open="true"] .workspace-sidebar', "transform", PHONE)).toBe("none");
    expect(valueFor(css(), ".workspace-nav-layer", "display")).toBe("contents");
    expect(valueFor(css(), ".workspace-nav-backdrop", "position", PHONE)).toBe("fixed");
  });

  it("renders four global destinations and a separate selected-device context", () => {
    expect(Object.keys(PAGE_LABELS).sort()).toEqual([...contract.navigation.global, contract.navigation.contextual].sort());
    expect(PAGE_LABELS.home).toBe(contract.navigation.overviewLabel);
    expect([...DEVICE_TABS]).toEqual(contract.navigation.deviceSections);
    const container = dom(createElement(Rail, { page: "activity", onPage: () => {}, machineName: machine.name! }));
    const nav = within(container).getByRole("navigation", { name: "Destinations" });
    expect(within(nav).getByRole("button", { name: "Activity" }).getAttribute("aria-current")).toBe("page");
    for (const name of ["Overview", "Machines", "Activity", "Access", "Device"]) expect(within(nav).getByRole("button", { name })).toBeTruthy();
  });

  it("renders page and tool headings as native semantic levels", () => {
    const container = dom(createElement(Fragment, null,
      createElement(PageHeading, { title: "Workspace overview", description: "A readable introduction" }),
      createElement(PageHeading, { title: "Tool section", level: 2 })));
    expect(within(container).getByRole("heading", { level: 1, name: "Workspace overview" })).toBeTruthy();
    expect(within(container).getByRole("heading", { level: 2, name: "Tool section" })).toBeTruthy();
    expect(within(container).getAllByRole("heading", { level: 1 })).toHaveLength(contract.head.maxHeadsPerPage);
  });

  it("keeps the selected machine as h1 and its working tool as h2", () => {
    const container = deviceDom();
    expect(within(container).getAllByRole("heading", { level: 1 })).toHaveLength(1);
    expect(within(container).getByRole("heading", { level: 1, name: machine.name! })).toBeTruthy();
    expect(within(container).getByRole("heading", { level: 2, name: "Terminal" })).toBeTruthy();
    expect(within(container).getByRole("navigation", { name: "Machine sections" })).toBeTruthy();
    expect(valueFor(css(), ".device-layout", "grid-template-columns")).toBe(`${contract.rail.deviceSections}px minmax(0, 1fr)`);
    expect(valueFor(css(), ".device-nav", "flex-direction", PHONE)).toBe("row");
  });

  it("makes tool callsites request the section level under the machine heading", () => {
    for (const file of ["OverviewPage.tsx", "TerminalWorkspace.tsx", "ScreenPage.tsx", "BrowserPane.tsx", "FilesPage.tsx", "RunsPage.tsx", "MemoryPage.tsx", "ToolsTab.tsx", "SystemPage.tsx"]) {
      const headings = jsxElements(file, "PageHeading");
      expect(headings.length, file).toBeGreaterThan(0);
      for (const heading of headings) expect(headingLevel(heading), file).toBe(contract.head.toolLevel);
    }
    for (const file of ["HomePage.tsx", "App.tsx"]) for (const heading of jsxElements(file, "PageHeading")) expect(headingLevel(heading) ?? 1, file).toBe(contract.head.pageLevel);
  });

  it("draws distinct page, section, metric, body, status and metadata type roles", () => {
    for (const [role, wanted] of [["fs-xl", contract.title.size], ["fs-lg", contract.section.h2.size], ["fs-metric", contract.metric.size],
      ["fs-md", contract.text.body.size], ["fs-base", contract.text.status.size], ["fs-sm", contract.text.metadata.size]] as const) expect(tokenPx(tokens(), role), role).toBe(wanted);
    expect(Number(declared(tokens(), "fw-display"))).toBe(contract.title.weight);
    expect(Number(declared(tokens(), "fw-semi"))).toBe(contract.section.h2.weight);
    expect(tokenPx(tokens(), "lh-page")).toBe(contract.title.line);
    expect(tokenPx(tokens(), "lh-section")).toBe(contract.section.h2.line);
    expect(tokenPx(tokens(), "lh-metric")).toBe(contract.metric.line);
    expect(valueFor(css(), ".page-title", "font-size")).toBe("var(--fs-xl)");
    expect(valueFor(css(), '.page-head[data-heading-level="2"] .page-title', "font-size")).toBe("var(--fs-lg)");
    expect(valueFor(css(), ".metric-value", "font-size")).toBe("var(--fs-metric)");
    expect(valueFor(css(), "body", "font")).toBe("var(--fs-md) / var(--lh-body) var(--font)");
  });

  it("keeps prose measured and code/output exempt from a prose width", () => {
    expect(declared(tokens(), "measure")).toBe(contract.measure.prose);
    expect(contract.measure.codeExempt).toBe(true);
    for (const selector of [".hint", ".page-description", ".activity-status"]) expect(valueFor(css(), selector, "max-width")).toBe("var(--measure)");
    expect(properties(css(), ".evidence-output").get("max-width")).not.toBe("var(--measure)");
    for (const [role, wanted] of Object.entries(contract.leading)) expect(Number(declared(tokens(), `lh-${role}`))).toBe(wanted);
  });

  it("uses the named type and spacing roles across owned local work planes", () => {
    for (const step of contract.space.scale) expect(tokens()).toMatch(new RegExp(`--sp-[0-9-]+:\\s*${step}px`));
    for (const name of ["app.css", ...localStyles]) for (const rule of sheetRules(readSource(name))) for (const declaration of rule.declarations) {
      if (declaration.name !== "font-size") continue;
      const role = /^var\(--(fs-[\w-]+)\)$/.exec(declaration.value);
      expect(role, `${name} / ${rule.selectors} / ${declaration.value}`).not.toBeNull();
      expect(tokenPx(tokens(), role![1])).toBeGreaterThanOrEqual(contract.text.metadata.size);
    }
    expect(valueFor(css(), ".rail-label", "letter-spacing")).toBe("var(--tracking-none)");
    expect(properties(css(), "body").has("letter-spacing")).toBe(false);
  });

  it("keeps desktop targets and phone menu/control targets separate from glyph geometry", () => {
    expect(tokenPx(tokens(), "control-h")).toBe(contract.space.controlH);
    expect(tokenPx(tokens(), "control-pad-y")).toBe(contract.space.controlPadY);
    expect(valueFor(css(), "button", "min-height")).toBe("var(--control-h)");
    expect(valueFor(css(), ".check", "min-height")).toBe("var(--control-h)");
    for (const selector of ["main button", 'main input:not([type="checkbox"]):not([type="radio"])', ".machine-menu > summary", ".machine-pop button", ".account-pop button", ".account-pop a", ".account > summary"])
      expect(lengthFor(css(), selector, "min-height", PHONE), selector).toBe(contract.space.targetTouch);
    expect(contract.space.target).toBe(32);
  });

  it("preserves native focus rings, system focus and a missing-consumer counterexample", () => {
    const assertFocus = (source: string) => {
      for (const tag of ["button", "a", "input", "select", "textarea", "summary"]) {
        expect(valueFor(source, `${tag}:focus-visible`, "outline")).toBe(`${contract.accessibility.focusWidth}px solid var(--focus-ring)`);
        expect(valueFor(source, `${tag}:focus-visible`, "outline-offset")).toBe("2px");
        expect(valueFor(source, `${tag}:focus-visible`, "outline", FORCED)).toBe("2px solid CanvasText");
      }
    };
    const source = css();
    assertFocus(source);
    const native = new Set(["button", "a", "input", "select", "textarea", "summary"].map((tag) => `${tag}:focus-visible`));
    const missingInput = sheetRules(source).flatMap((rule) => {
      const selectors = rule.selectors.filter((selector) => native.has(selector)
        && !(selector === "input:focus-visible" && rule.conditions.length === 0));
      if (selectors.length === 0) return [];
      const body = `${selectors.join(", ")} { ${rule.declarations.map((d) => `${d.name}: ${d.value};`).join(" ")} }`;
      return [rule.conditions.reduceRight((nested, condition) => `${condition} { ${nested} }`, body)];
    }).join("\n");
    expect(matchingRules(missingInput, "input:focus-visible")).toHaveLength(0);
    expect(matchingRules(missingInput, "input:focus-visible", FORCED)).toHaveLength(1);
    expect(() => assertFocus(missingInput)).toThrow("No rule for input:focus-visible in base");
  });

  it("keeps hidden content authoritative while its mounted content survives", () => {
    expect(contract.accessibility.hiddenIsAuthoritative).toBe(true);
    expect(valueFor(css(), "[hidden]", "display")).toBe("none !important");
    for (const selector of [".terminal-page[hidden]", ".terminal-panel[hidden]"]) expect(valueFor(css(), selector, "display")).toBe("none");
    const container = deviceDom(true);
    expect(container.querySelector(".device-page")?.hasAttribute("hidden")).toBe(true);
    expect(container.querySelector(".terminal-page")?.textContent).toBe("Mounted terminal fixture");
    expect(within(container).queryAllByRole("heading", { level: 1 })).toHaveLength(0);
  });

  it("gives the terminal remaining height without introducing a nested page gutter", () => {
    for (const selector of [".device-layout", ".device-content"]) expect(valueFor(css(), selector, "min-height")).toBe("0");
    expect(valueFor(css(), 'main.terminal-main:has(.device-page .terminal-page:not([hidden]))', "grid-template-rows")).toBe("minmax(0, 1fr)");
    expect(valueFor(css(), '.device-page:has(.terminal-page:not([hidden]))', "grid-template-rows")).toBe("auto minmax(0, 1fr)");
    expect(valueFor(css(), ".device-content > .page", "padding")).toBe("0");
    expect(valueFor(readSource("styles/workspaces.css"), ".terminal-workspace .terminal-main-pane", "min-height")).toBe("0");
  });

  it("keeps overview metrics and the main/aside work planes distinct", () => {
    expect(splitCss(valueFor(css(), ".dashboard-grid", "grid-template-columns"), " ")).toHaveLength(2);
    for (const selector of [".dashboard-main", ".dashboard-aside"]) expect(valueFor(css(), selector, "min-width")).toBe("0");
    expect(valueFor(css(), ".metric-value", "font-variant-numeric")).toBe("tabular-nums");
    expect(jsxElements("HomePage.tsx", "PageHeading")).toHaveLength(1);
    for (const [tag, plane] of [["div", "dashboard-main"], ["aside", "dashboard-aside"]])
      expect(jsxElements("HomePage.tsx", tag).some((node) => classNames(node).includes(plane)), plane).toBe(true);
    expect(jsxElements("HomePage.tsx", "dl").some((node) => classNames(node).includes("metric-grid"))).toBe(true);
  });

  it("lets local task layouts refine work planes instead of applying global card geometry", () => {
    const records = readSource("styles/records.css");
    for (const selector of [".files-page .files-workarea", ".runs-page .runs-workarea", ".memory-panel .memory-workarea"]) {
      expect(splitCss(valueFor(records, selector, "grid-template-columns"), " ").length, selector).toBeGreaterThan(1);
      const narrow = sheetRules(records).find((rule) => rule.selectors.includes(selector) && rule.conditions.length > 0
        && rule.declarations.some((d) => d.name === "grid-template-columns" && d.value === "minmax(0, 1fr)"));
      expect(narrow, `${selector} has a local narrow fallback`).toBeTruthy();
    }
    const activity = readSource("styles/activity.css");
    expect(valueFor(activity, ".activity-layout > .activity-record", "padding")).toBe("0");
    expect(valueFor(activity, ".activity-layout > .activity-inspector", "padding")).toBe("0");
    const workspaces = readSource("styles/workspaces.css");
    for (const selector of [".terminal-workspace .terminal-layout", ".browser-pane .browser-workarea", ".screen-workspace .screen-capture-workarea"]) {
      expect(sheetRules(workspaces).some((rule) => rule.selectors.includes(selector) && rule.conditions.some((condition) => condition.startsWith("@container"))
        && rule.declarations.some((d) => d.name === "grid-template-columns" && splitCss(d.value, " ").length > 1)), selector).toBe(true);
    }
  });

  it("keeps the Machines inventory responsive to its own containing box", () => {
    const condition = ["@container fleet-table (max-width: 639px)"];
    expect(valueFor(css(), ".fleet-table-wrap", "container")).toBe("fleet-table / inline-size");
    expect(valueFor(css(), ".fleet-table .fleet-row", "display", condition)).toBe("grid");
    expect(matchingRules(css(), ".fleet-table .fleet-row", PHONE)).toHaveLength(0);
    expect(valueFor(readSource("styles/fleet-overview.css"), ".home-page .fleet-table", "table-layout", condition)).toBe("auto");
    // The fixed action clearance is geometry; no blanket spacing scan treats it as a list tick.
  });

  it("uses the page card role while allowing in-card empties without another surface", () => {
    expect(lengthFor(css(), ".card", "padding")).toBe(contract.card.pad);
    expect(lengthFor(css(), ".card", "gap")).toBe(contract.card.gap);
    expect(lengthFor(css(), ".card", "border-radius")).toBe(contract.card.radius);
    expect(valueFor(css(), ".empty-state.in-card", "background")).toBe(contract.empty.inCardSurface);
    expect(valueFor(css(), ".empty-state.in-card", "border")).toBe(String(contract.empty.inCardBorder));
    expect(valueFor(css(), ".empty-state.in-card", "box-shadow")).toBe(contract.empty.inCardShadow);
    expect(contract.empty.wayBack).toBe(true);
  });

  it("retains named radius and neutral separator/control-boundary roles", () => {
    for (const [role, wanted] of [["radius", contract.radius.page.card], ["radius-sm", contract.radius.page.row], ["radius-sm2", contract.radius.page.control], ["radius-xs", contract.radius.page.mark], ["radius-lg", contract.radius.overlay.sheet], ["radius-pill", contract.radius.pill]] as const)
      expect(tokenPx(tokens(), role), role).toBe(wanted);
    expect(declared(tokens(), "line")).toBe(contract.hairline.line);
    expect(declared(tokens(), "line-strong")).toBe(contract.hairline.strong);
    expect(declared(tokens(), "line-soft")).toBe(contract.hairline.soft);
    expect(valueFor(css(), "button", "border")).toBe("1px solid var(--line-strong)");
    expect(valueFor(css(), ".card", "border")).toBe("1px solid var(--line)");
  });

  it("uses named elevations and state marks without an exact shadow-count budget", () => {
    for (const name of ["app.css", ...localStyles]) for (const rule of sheetRules(readSource(name))) for (const declaration of rule.declarations) {
      if (declaration.name !== "box-shadow") continue;
      expect(declaration.value === "none" || declaration.value.includes("var(--") || /^0 0 0 [0-9]+px? transparent$/.test(declaration.value)
        || declaration.value === "0 0 0 0 transparent", `${name}: ${declaration.value}`).toBe(true);
    }
    expect(declared(tokens(), "surface-recessed")).toBe(contract.elevation.recess);
    expect(contrast(rgbRole(tokens(), "surface-recessed"), rgbRole(tokens(), "panel"))).toBeGreaterThanOrEqual(contract.elevation.recessTint);
    expect(css()).toContain(`var(${contract.elevation.pane})`);
  });

  it("announces loading, preserves failed-read recovery and does not call an unread fleet empty", () => {
    const reading = dom(createElement(DevicePicker, { devices: [], onChoose: () => {}, fleetRead: { state: "reading" } }));
    const status = within(reading).getByRole(contract.loading.role);
    expect(status.getAttribute("aria-label")?.trim().length).toBeGreaterThan(0);
    expect(status.querySelectorAll(".skeleton-row")).toHaveLength(contract.loading.rows);
    expect(within(reading).queryByRole("heading")).toBeNull();
    const failed = dom(createElement(DevicePicker, { devices: [], onChoose: () => {}, fleetRead: { state: "failed", reason: "Fixture unavailable" }, onRetry: () => {} }));
    expect(within(failed).getByRole("status")).toBeTruthy();
    expect(within(failed).getByRole("button", { name: "Retry" })).toBeTruthy();
    const ready = dom(createElement(DevicePicker, { devices: [], onChoose: () => {}, fleetRead: { state: "ready" } }));
    expect(within(ready).getByRole("heading", { level: 2 })).toBeTruthy();
    expect(within(ready).queryByRole("status")).toBeNull();
  });

  it("carries state words beside decorative marks", () => {
    expect(contract.hairline.stateMarksCarryOwnColour).toBe(true);
    expect(contract.hairline.stateMarksCarryWord).toBe(true);
    const container = dom(createElement(DevicePicker, { devices: [machine, { ...machine, id: "offline-fixture", name: "Offline fixture", online: false }], onChoose: () => {} }));
    expect([...container.querySelectorAll(".picker-state")].map((node) => node.textContent)).toEqual(["online", "offline"]);
    for (const mark of container.querySelectorAll(".dot")) expect(mark.getAttribute("aria-hidden")).toBe("true");
  });

  it("honors reduced motion through the shared blanket and named state animations", () => {
    expect(declared(tokens(), "dur-interaction")).toBe(`${contract.motion.interactionMs}ms`);
    for (const selector of ["*", "*::before", "*::after"]) {
      expect(valueFor(css(), selector, "transition-duration", REDUCE)).toBe(`${contract.motion.cap} !important`);
      expect(valueFor(css(), selector, "animation-duration", REDUCE)).toBe(`${contract.motion.cap} !important`);
      expect(valueFor(css(), selector, "animation-iteration-count", REDUCE)).toBe("1 !important");
      expect(valueFor(css(), selector, "scroll-behavior", REDUCE)).toBe("auto !important");
    }
    for (const selector of [".sheet", ".live-entry", ".live-state.is-active .live-dot", ".live-running-entry .live-running-dot"])
      expect(valueFor(css(), selector, "animation", REDUCE)).toBe("none");
    expect(contract.motion).toMatchObject({ reducedMotion: "blanket", namedAnimationsStop: true });
  });
});
