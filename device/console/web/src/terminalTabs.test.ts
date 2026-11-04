import { describe, expect, it } from "vitest";
import { renameTab, tabLabel, titleFromInput, type Tab } from "./terminalTabs";

function tab(key: string, kind: string, title: string | null, startedAtMs?: number): Tab {
  return { key, session: key, kind, title, startedAtMs };
}

const NOW = Date.UTC(2026, 10, 5, 12, 0, 0);
const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

describe("tabLabel", () => {
  it("says the kind and the age for a tab the operator has not named", () => {
    expect(tabLabel(tab("a", "pty", null, NOW - 12 * MINUTE), NOW)).toBe("pty · 12m");
    expect(tabLabel(tab("b", "ssh", null, NOW - 2 * HOUR), NOW)).toBe("ssh · 2h");
    expect(tabLabel(tab("c", "serial", null, NOW - 40_000), NOW)).toBe("serial · 40s");
  });

  it("keeps a name the operator typed, exactly", () => {
    expect(tabLabel(tab("a", "pty", "prod db", NOW - DAY), NOW)).toBe("prod db");
  });

  it("says only the kind when the session's start is unknown", () => {
    // A list that did not carry `started_at_ms` is not a reason to invent an age.
    expect(tabLabel(tab("a", "pty", null), NOW)).toBe("pty");
  });
});

describe("titleFromInput", () => {
  it("trims and collapses whitespace", () => {
    expect(titleFromInput("  prod   db  ")).toBe("prod db");
  });

  it("refuses a name that is only whitespace", () => {
    expect(titleFromInput("   ")).toBeNull();
  });

  it("caps a long name at 60 characters", () => {
    expect(titleFromInput("x".repeat(70))).toBe("x".repeat(60));
  });

  it("counts an emoji as one character when capping", () => {
    expect(titleFromInput("😀".repeat(40))).toBe("😀".repeat(40));
  });
});

describe("renameTab", () => {
  it("replaces only the named tab and keeps the other objects identical", () => {
    const first = tab("a", "pty", null, NOW);
    const second = tab("b", "ssh", "staging", NOW);
    const renamed = renameTab([first, second], "a", "prod db");

    expect(renamed.map((entry) => entry.title)).toEqual(["prod db", "staging"]);
    expect(renamed[0]).not.toBe(first);
    expect(renamed[1]).toBe(second);
  });

  it("returns an unchanged copy when the key is absent", () => {
    const tabs = [tab("a", "pty", null, NOW)];
    expect(renameTab(tabs, "missing", "prod db")).toEqual(tabs);
  });
});
