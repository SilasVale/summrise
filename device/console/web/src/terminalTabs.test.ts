import { describe, expect, it } from "vitest";
import { nextTabTitle, renameTab, titleFromInput, type Tab } from "./terminalTabs";

function tab(key: string, kind: string, title: string): Tab {
  return { key, session: key, kind, title };
}

describe("nextTabTitle", () => {
  it("starts at one when no tab exists", () => {
    expect(nextTabTitle("pty", [])).toBe("pty 1");
  });

  it("numbers after the labels in use", () => {
    expect(nextTabTitle("pty", [tab("a", "pty", "pty 1"), tab("b", "pty", "pty 2")])).toBe("pty 3");
  });

  it("fills the gap a closed tab leaves", () => {
    expect(nextTabTitle("pty", [tab("a", "pty", "pty 1"), tab("c", "pty", "pty 3")])).toBe("pty 2");
  });

  it("does not offer a label another tab now carries", () => {
    expect(nextTabTitle("pty", [tab("a", "ssh", "pty 1")])).toBe("pty 2");
  });

  it("numbers different kinds independently", () => {
    const tabs = [tab("a", "pty", "pty 1"), tab("b", "serial", "serial 1")];
    expect(nextTabTitle("serial", tabs)).toBe("serial 2");
    expect(nextTabTitle("pty", tabs)).toBe("pty 2");
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
    const first = tab("a", "pty", "pty 1");
    const second = tab("b", "ssh", "pty 2");
    const renamed = renameTab([first, second], "a", "prod db");

    expect(renamed.map((entry) => entry.title)).toEqual(["prod db", "pty 2"]);
    expect(renamed[0]).not.toBe(first);
    expect(renamed[1]).toBe(second);
  });

  it("returns an unchanged copy when the key is absent", () => {
    const tabs = [tab("a", "pty", "pty 1")];
    expect(renameTab(tabs, "missing", "prod db")).toEqual(tabs);
  });
});
