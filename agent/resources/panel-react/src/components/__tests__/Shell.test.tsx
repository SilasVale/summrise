// Shell pins — ONE shell, two densities (core design §4): BOTH lay out their rails and canvas in a row with
// the status bar as a full-width BOTTOM bar (round-161 fixed the stray column in the panel density; round 265
// gave the desktop density the same slot, because its strip used to be the content card's footer and started
// at the rail's edge — the operator asked why the bar does not reach the window's left). The desktop density
// hides the CONTEXT RAIL only. PAGES/PAGE_LABELS are the shared page contract.
import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { Shell, PAGES, PAGE_LABELS, type Page } from "../Shell";

describe("Shell page contract", () => {
  it("PAGES lists every page with a label — including the one that needs no session", () => {
    expect(PAGES).toEqual([
      "terminal",
      "history",
      "browser",
      "memory",
      "plugins",
      "settings",
    ]);
    const labels = (Object.keys(PAGE_LABELS) as Page[]).map(
      (p) => PAGE_LABELS[p],
    );
    expect(labels).toEqual([
      "Terminal",
      "History",
      "Browser",
      "Memory",
      "Plugins",
      "Settings",
    ]);
    // HISTORY is the one page that answers "what has this device been doing / what did it record"
    // with zero sessions open, so it has to be on the rail. IconRail's PAGE_ICONS is a Record over
    // Page — a page without an icon does not build.
    expect(PAGES).toContain("history");
    // It sits directly under Terminal: it is that page's own history, and the merge of `archive` +
    // `activity` is what put both halves behind one icon.
    expect(PAGES.indexOf("history")).toBe(PAGES.indexOf("terminal") + 1);
  });
});

describe("Shell panel density", () => {
  it("lays out rails + canvas with the status bar as a bottom bar", () => {
    const { container } = render(
      <Shell
        density="panel"
        iconRail={<span>rail</span>}
        contextRail={<span>ctx</span>}
        canvas={<span>canvas</span>}
        statusBar={<span>status</span>}
      />,
    );
    expect(screen.getByText("rail")).toBeTruthy();
    expect(screen.getByText("ctx")).toBeTruthy();
    expect(screen.getByText("canvas")).toBeTruthy();
    const shell = container.querySelector("#app-shell")!;
    const status = screen.getByText("status");
    expect(status.parentElement).toBe(shell);
    expect(status.previousElementSibling?.id).toBe("shell-main");
  });

  it("optional rails omit their hosts", () => {
    const { container } = render(
      <Shell
        density="panel"
        iconRail={<span>rail</span>}
        canvas={<span>canvas</span>}
      />,
    );
    expect(container.querySelector("#context-rail")).toBeNull();
  });
});

describe("Shell desktop density", () => {
  it("hides the context rail but keeps the status bar, as a bottom bar across the window", () => {
    const { container } = render(
      <Shell
        density="desktop"
        iconRail={<span>rail</span>}
        contextRail={<span>ctx</span>}
        canvas={<span>canvas</span>}
        statusBar={<span>status</span>}
      />,
    );
    const shell = container.querySelector(".desktop-shell")!;
    expect(shell).toBeTruthy();
    expect(screen.queryByText("ctx")).toBeNull();
    expect(screen.getByText("canvas")).toBeTruthy();
    // THE BAR IS A CHILD OF THE SHELL, NOT OF THE CANVAS — that is the whole rule: the rail and the canvas
    // share `.desktop-body`, and the bar sits under BOTH, so it spans the window from its left edge.
    const status = screen.getByText("status");
    expect(status.parentElement).toBe(shell);
    expect(status.previousElementSibling?.className).toBe("desktop-body");
    expect(shell.querySelector(".desktop-body .desktop-rail")).toBeTruthy();
    expect(shell.querySelector(".desktop-body .desktop-main")).toBeTruthy();
  });
});
