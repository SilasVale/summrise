// Shell + DesktopShell tests — the two densities share the same page set;
// DesktopShell must switch between every page (activity included — it is the
// one that works with no session) and the rail must reflect the active page. (Uses mock props; no backend calls.)
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { DesktopShell } from "../DesktopShell";
import { StripMore } from "../TabBar";
import { Shell } from "../Shell";
import type { Session } from "../../hooks/useSessions";
import { callApi } from "../../lib/api";

// Partial mock via importOriginal — see the note in SettingsPage.test.tsx: a
// factory listing only the used exports breaks when a new consumer appears.
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  callApi: vi.fn(() => Promise.resolve({})),
  callTool: vi.fn(() => Promise.resolve({})),
}));

function sessions(): Session[] {
  return [
    {
      sid: "s1",
      label: "shell",
      kind: "pty",
      closed: false,
      savedOnly: false,
      active: true,
      idleMs: 0, commandRunning: false, firstSeenAt: Date.now(),
      closedAt: null,
      heldByHuman: false,
      approvalRequired: false,
      pendingApproval: null,
      approvalGrants: [],
      goal: null,
      plan: [],
    },
  ];
}

const baseProps = {
  sessions: sessions(),
  activeSid: "s1",
  onActivate: vi.fn(),
  onClose: vi.fn(),
  onExport: vi.fn(),
  onViewChange: vi.fn(),
  sessionViews: {} as Record<string, "terminal" | "trajectory">,
  onSetControl: vi.fn(() => Promise.resolve(false)),
  onSetApproval: vi.fn(() => Promise.resolve(false)),
  onDecideApproval: vi.fn(() => Promise.resolve(true)),
  onRevokeGrants: vi.fn(() => Promise.resolve([])),
  onSetGoal: vi.fn(() => Promise.resolve(null)),
  registerWrite: vi.fn(() => vi.fn()),
  plugins: {
    rows: [],
    specLoaded: false,
    loadError: "",
    busy: null,
    log: [],
    start: vi.fn(),
    stop: vi.fn(),
  } as any,
  onNewSession: vi.fn(),
  onConnConnect: vi.fn(() => Promise.resolve("s1")),
  connModal: null,
  onConnClose: vi.fn(),
  status: "",
  sseState: "connected" as "connected" | "down" | "connecting",
  token: "t",
  cmdEvents: { cards: [], events: [], firstSeq: 1 } as any,
};

describe("Shell", () => {
  it("renders three columns for panel density", () => {
    const { container } = render(
      <Shell
        density="panel"
        iconRail={<div>rail</div>}
        contextRail={<div>ctx</div>}
        canvas={<div>canvas</div>}
        statusBar={<div>status</div>}
      />,
    );
    expect(container.querySelector("#app-shell")).toBeTruthy();
    expect(container.querySelector("#icon-rail")).toBeTruthy();
    expect(container.querySelector("#context-rail")).toBeTruthy();
    expect(container.querySelector("#canvas-host")).toBeTruthy();
  });

  it("renders desktop-shell layout for desktop density without context rail", () => {
    const { container } = render(
      <Shell
        density="desktop"
        iconRail={<div>rail</div>}
        canvas={<div>canvas</div>}
      />,
    );
    expect(container.querySelector(".desktop-shell")).toBeTruthy();
    expect(container.querySelector("#context-rail")).toBeNull();
    expect(container.querySelector("#app-shell")).toBeNull();
  });
});

describe("DesktopShell", () => {
  it("switches between every page via the rail", () => {
    render(<DesktopShell {...baseProps} />);
    // Default page: Terminal (the header title marks the current page).
    expect(
      document.querySelector(".desktop-header-title")?.textContent,
    ).toContain("Terminal");
    // Every page: the desktop density must expose the same page set as the panel's rail
    // (Shell.PAGES is the shared contract), and HISTORY must mount here — the device's record is
    // the one thing reachable with no live session at all. It is ONE page now (round 31 merged
    // `archive` + `activity`), which is why this list is shorter than it was.
    for (const label of [
      "History",
      "Browser",
      "Memory",
      "Plugins",
      "Settings",
    ]) {
      fireEvent.click(screen.getByTitle(label));
      expect(
        document.querySelector(".desktop-header-title")?.textContent,
      ).toContain(label);
    }
  });

  it("shows a single New menu instead of four scattered buttons (stage-l)", () => {
    render(<DesktopShell {...baseProps} />);
    expect(document.querySelectorAll(".desktop-new .btn-new").length).toBe(1);
    // The old four-button row is gone.
    expect(document.querySelectorAll(".desktop-new .btn-ghost").length).toBe(0);
  });

  it("RENDERS the view App owns — the shadow copy is what made Ctrl+Shift+Y a no-op", () => {
    // `sessionViews` is a PROP, not local state. When this shell kept its own copy,
    // App's copy was write-only: the shortcut and PathView's "jump to step" both wrote
    // it and NOTHING rendered from it. This test fails the moment a local copy returns,
    // because a local copy cannot see this prop.
    const { container } = render(
      <DesktopShell {...baseProps} sessionViews={{ s1: "trajectory" }} />,
    );
    // `sessionView === "trajectory"` hides the terminal container and mounts the
    // trajectory body — that is the observable effect of the prop being READ.
    expect(
      container.querySelector("#desktop-term-container")?.className,
      "the terminal container must be hidden while the trajectory view is active",
    ).toBe("hidden");
    // And the view switch reflects the SAME value rather than the shell's own idea of the view
    // (`role="tab"` + `aria-selected`, per ViewSwitch). It lives in the terminal CONTROL BAR since round
    // 169, not in the header row — this query deliberately does not care WHERE, only that the value is read.
    const selected = [
      ...container.querySelectorAll('.desktop-view-switch [role="tab"]'),
    ]
      .filter((b) => b.getAttribute("aria-selected") === "true")
      .map((b) => b.textContent?.trim());
    expect(selected).toEqual(["Trajectory"]);
  });

  it("shows the device state from sseState (and never a false IDLE)", () => {
    // The dot carries the DEVICE state now (off / idle / working), not just
    // connectivity — see useDeviceActivity. A dropped stream must not read as a
    // healthy idle machine, so "down" has its own state rather than merely
    // lacking the ok class.
    const { container } = render(<DesktopShell {...baseProps} />);
    const dot = () =>
      container
        .querySelector(".desktop-rail-status .mark")!
        .getAttribute("data-live");
    expect(dot()).toBe("idle");
    const { container: c2 } = render(
      <DesktopShell {...baseProps} sseState="down" />,
    );
    const dot2 = () =>
      c2.querySelector(".desktop-rail-status .mark")!.getAttribute("data-live");
    expect(dot2()).toBe("off");
  });

  it("renders the SSH connection modal when connModal is set (regression: desktop shell had no ConnModal mount — SSH/Serial buttons were dead)", () => {
    const { container } = render(
      <DesktopShell {...baseProps} connModal="ssh" />,
    );
    expect(container.querySelector("#conn-modal")).toBeTruthy();
    expect(container.querySelector(".modal-card h2")?.textContent).toBe(
      "New SSH",
    );
    const { container: c2 } = render(
      <DesktopShell {...baseProps} connModal="serial" />,
    );
    expect(c2.querySelector(".modal-card h2")?.textContent).toBe("New Serial");
  });

  it("status strip shows vitals from /api/status polling (stage-n vitals)", async () => {
    (callApi as any).mockResolvedValueOnce({
      version: "1.0.145",
      uptime_secs: 95,
      live_sessions: 1,
      cpu_pct: 12.4,
      mem_pct: 48.9,
    });
    render(<DesktopShell {...baseProps} />);
    const el = await waitFor(() => {
      const node = document.querySelector(".desktop-status-msg");
      expect(node?.textContent).toContain("CPU");
      return node!;
    });
    expect(el.textContent).toContain("1 session");
    expect(el.textContent).toContain("v1.0.145");
    expect(el.textContent).toContain("up 1m 35s");
    expect(el.textContent).toContain("CPU 12%");
    expect(el.textContent).toContain("MEM 49%");
  });

  it("status strip prefers the npm release field over the Cargo protocol version", async () => {
    (callApi as any).mockResolvedValueOnce({
      version: "1.0.145",
      release: "1.2.304",
    });
    render(<DesktopShell {...baseProps} />);
    await waitFor(() =>
      expect(
        document.querySelector(".desktop-status-msg")?.textContent,
      ).toContain("v1.2.304"),
    );
    expect(
      document.querySelector(".desktop-status-msg")?.textContent,
    ).not.toContain("v1.0.145");
  });

  it("status strip omits vitals when the fields are absent (graceful degradation)", async () => {
    (callApi as any).mockResolvedValueOnce({ version: "1.0.145" });
    render(<DesktopShell {...baseProps} />);
    await waitFor(() =>
      expect(
        document.querySelector(".desktop-status-msg")?.textContent,
      ).toContain("v1.0.145"),
    );
    expect(
      document.querySelector(".desktop-status-msg")?.textContent,
    ).not.toContain("CPU");
    expect(
      document.querySelector(".desktop-status-msg")?.textContent,
    ).not.toContain("MEM");
  });

  it("desktop strip: repeated labels are disambiguated, like the panel's", () => {
    // ROUND 170'S DEFECT, PINNED. The label disambiguation went into TabBar (round 167) and the desktop
    // renders its OWN tabs, so a rendered page showed `d1, serial:COM4, d1, …` — the same defect the
    // operator reported, still on screen in this density. Two sessions with one label is the whole test.
    const two = sessions();
    const dup = { ...two[0], sid: "s2", active: false };
    render(<DesktopShell {...baseProps} sessions={[two[0], dup]} />);
    const names = [...document.querySelectorAll(".dtab-name")].map((e) => e.textContent);
    expect(names.length).toBeGreaterThan(1);
    expect(new Set(names).size, `labels must be distinguishable, got ${JSON.stringify(names)}`).toBe(names.length);
  });

  it("desktop strip: the fade flag is NOT always on when the tabs fit", () => {
    // THE SILENT BREAK. `useStripOverflow` began returning { overflowing, hidden } in round 168; this file
    // kept using it as a boolean, so the flag became permanently truthy and the fade would never go away.
    // TypeScript does not error on truthiness and no test mounted this strip — this is that test. In jsdom
    // nothing is laid out, so scrollWidth === clientWidth and the honest answer is "no overflow".
    render(<DesktopShell {...baseProps} />);
    expect(document.querySelector(".desktop-tabs")?.getAttribute("data-more")).toBeNull();
  });

  it("desktop strip: does NOT carry the view switch (it is a session control)", () => {
    // Round 169's decision, guarded in the OTHER strip as well — the panel's TabBar test covers its own.
    const { container } = render(<DesktopShell {...baseProps} />);
    expect(container.querySelector(".desktop-tabs .desktop-view-switch")).toBeNull();
  });

  it("StripMore: says how many, and says nothing at zero", () => {
    // The chip's own contract, since jsdom cannot lay out a strip for the hook to count.
    const { container, rerender } = render(<StripMore n={3} />);
    expect(container.textContent).toBe("+3");
    rerender(<StripMore n={0} />);
    expect(container.textContent).toBe("");
    rerender(<StripMore n={1} />);
    expect(container.querySelector(".tab-more")?.getAttribute("title")).toMatch(/1 more session —/);
  });
});
