import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterAll, afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import { TerminalWorkspace } from "./TerminalWorkspace";
import { api } from "./api";
import { callTool, ToolError, type Grant } from "./terminal";
import { DEFAULT_FONT_SIZE, FONT_SIZE_KEY, MAX_FONT_SIZE, MIN_FONT_SIZE } from "./terminalFont";
import { restoreGlobal } from "./test/dom";

/// The xterms the component created, in creation order: a font change is asserted on the instance,
/// because that is what the operator's screen shows, and the loaded addons are how a tab proves the
/// search addon belongs to its own terminal.
const mockedXterm = vi.hoisted(() => ({
  instances: [] as {
    options: Record<string, unknown>;
    loadAddon: Mock;
    write: Mock;
    rows: number;
    cols: number;
    selectionHandler: (() => void) | null;
    keyHandler: ((event: KeyboardEvent) => boolean) | null;
    getSelection: Mock;
    paste: Mock;
  }[]
}));

/// The fit addons, in creation order: a font change refits each xterm into the host it already has.
const mockedFit = vi.hoisted(() => ({
  instances: [] as { fit: Mock }[]
}));

/// The search addons, in creation order: one per tab, so a search is asserted on the tab's own addon.
/// `nextResult`/`previousResult` stand in for "the scrollback has a match", which jsdom cannot have.
const mockedSearch = vi.hoisted(() => ({
  instances: [] as { findNext: Mock; findPrevious: Mock; clearDecorations: Mock }[],
  nextResult: true,
  previousResult: true
}));

vi.mock("@xterm/xterm", () => {
  class Terminal {
    cols = 80;
    rows = 24;
    options: Record<string, unknown>;
    buffer = { active: { length: 0, cursorX: 0, cursorY: 0, viewportY: 0, baseY: 0 } };
    open = vi.fn();
    loadAddon = vi.fn();
    onData = vi.fn(() => ({ dispose: vi.fn() }));
    onResize = vi.fn(() => ({ dispose: vi.fn() }));
    selectionHandler: (() => void) | null = null;
    onSelectionChange = vi.fn((handler: () => void) => {
      this.selectionHandler = handler;
      return { dispose: vi.fn() };
    });
    keyHandler: ((event: KeyboardEvent) => boolean) | null = null;
    attachCustomKeyEventHandler = vi.fn((handler: (event: KeyboardEvent) => boolean) => {
      this.keyHandler = handler;
    });
    getSelection = vi.fn(() => "");
    paste = vi.fn();
    write = vi.fn();
    reset = vi.fn();
    dispose = vi.fn();
    focus = vi.fn();
    clear = vi.fn();

    constructor(options: Record<string, unknown> = {}) {
      this.options = { ...options };
      mockedXterm.instances.push(this);
    }
  }
  return { Terminal };
});

vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit = vi.fn();

    constructor() {
      mockedFit.instances.push(this);
    }
  }
}));

vi.mock("@xterm/addon-search", () => ({
  SearchAddon: class {
    findNext = vi.fn(() => mockedSearch.nextResult);
    findPrevious = vi.fn(() => mockedSearch.previousResult);
    clearDecorations = vi.fn();

    constructor() {
      mockedSearch.instances.push(this);
    }
  }
}));

vi.mock("./terminal", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./terminal")>();
  return { ...actual, callTool: vi.fn() };
});

vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return { ...actual, api: { ...actual.api, connections: vi.fn(async () => []) } };
});

const callToolMock = vi.mocked(callTool);
const GRANT: Grant = { deviceId: "device-1", envelope: {} };

/// jsdom has no `matchMedia` and the workspace asks it for the terminal's theme.
const matchMedia = Object.getOwnPropertyDescriptor(window, "matchMedia");

/// jsdom has no object-URL methods; the export's download needs both, and its revoke is delayed.
const objectUrls = {
  create: Object.getOwnPropertyDescriptor(URL, "createObjectURL"),
  revoke: Object.getOwnPropertyDescriptor(URL, "revokeObjectURL")
};

let created = 0;

beforeEach(() => {
  created = 0;
  mockedXterm.instances.length = 0;
  mockedFit.instances.length = 0;
  mockedSearch.instances.length = 0;
  mockedSearch.nextResult = true;
  mockedSearch.previousResult = true;
  callToolMock.mockReset();
  callToolMock.mockImplementation(async (_grant, name) => {
    if (name === "terminal.create") {
      created += 1;
      return JSON.stringify({ id: `t${created}` });
    }
    if (name === "terminal.read") return JSON.stringify({ data: "", cursor: 0 });
    if (name === "terminal.resize") return JSON.stringify({ rows: 24, cols: 80 });
    if (name === "terminal.list") return JSON.stringify([]);
    if (name === "terminal.close") return JSON.stringify({ closed: true });
    throw new Error(`the test did not expect a ${name} call`);
  });
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    value: vi.fn(() => ({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() }))
  });
  Object.defineProperty(URL, "createObjectURL", { configurable: true, value: vi.fn(() => "blob:test") });
  Object.defineProperty(URL, "revokeObjectURL", { configurable: true, value: vi.fn() });
});

afterEach(() => {
  restoreGlobal(window, "matchMedia", matchMedia);
  vi.restoreAllMocks();
});

afterAll(() => {
  restoreGlobal(URL, "createObjectURL", objectUrls.create);
  restoreGlobal(URL, "revokeObjectURL", objectUrls.revoke);
});

/** Open the pty dialog from the panel head and submit it. */
async function startPtyTab() {
  fireEvent.click(screen.getByRole("button", { name: "+ New session" }));
  const dialog = await screen.findByRole("dialog", { name: "New terminal session" });
  fireEvent.click(within(dialog).getByRole("button", { name: "Open" }));
  await waitFor(() =>
    expect(screen.queryByRole("dialog", { name: "New terminal session" })).not.toBeInTheDocument()
  );
}

function reads(): number {
  return callToolMock.mock.calls.filter(([, name]) => name === "terminal.read").length;
}

describe("TerminalWorkspace tab names", () => {
  it("shows only the action that creates a session until one is open", () => {
    render(<TerminalWorkspace grant={GRANT} />);
    // The session toolbar belongs to a session; at the empty state the page offers the one action
    // that makes one, and no disabled controls pretending to be a toolbar.
    expect(screen.getByRole("button", { name: "+ New session" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Rename tab" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Attach to a session" })).not.toBeInTheDocument();
  });

  it("says the machine is offline before it says anything about sessions", async () => {
    const seen = Date.now() - 5 * 60_000;
    const { container } = render(
      <TerminalWorkspace grant={GRANT} deviceOnline={false} lastSeenMs={seen} />
    );
    // The empty page had one explanation for two causes: "no session" was printed over a machine
    // that could not answer at all. The machine's state is now the page's first sentence.
    const banner = container.querySelector(".offline-notice");
    expect(banner).not.toBeNull();
    expect(banner).toHaveTextContent("This machine is offline.");
    expect(banner).toHaveTextContent(/last seen/i);
    // And the empty state must not promise the action the page has just disabled.
    expect(await screen.findByText(/A shell can start when the machine reconnects/)).toBeInTheDocument();
    expect(screen.queryByText(/New session opens a shell/)).not.toBeInTheDocument();
    // A machine the fleet says is down is not asked: the call cannot reach it, and its failure used
    // to print a second sentence under the banner that already said it.
    expect(callToolMock.mock.calls.filter(([, name]) => name === "terminal.list")).toHaveLength(0);
  });

  it("turns off the action an offline machine cannot satisfy, and says why", () => {
    render(<TerminalWorkspace grant={GRANT} deviceOnline={false} />);
    const start = screen.getByRole("button", { name: "+ New session" });
    expect(start).toBeDisabled();
    expect(start.getAttribute("title")).toMatch(/reconnects/);
    // The list cannot be refreshed from a machine that is not there either.
    expect(screen.getByRole("button", { name: "Refresh" })).toBeDisabled();
  });

  it("does not accuse a machine it knows nothing about", () => {
    const { container } = render(<TerminalWorkspace grant={GRANT} />);
    expect(container.querySelector(".offline-notice")).toBeNull();
    expect(screen.getByRole("button", { name: "+ New session" })).toBeEnabled();
  });

  it("attaches the device's only live session without an Attach click", async () => {
    callToolMock.mockImplementation(async (_grant, name) => {
      if (name === "terminal.list") return JSON.stringify([{ id: "live-1", kind: "pty" }]);
      if (name === "terminal.resize") return JSON.stringify({ rows: 24, cols: 80 });
      if (name === "terminal.read") return JSON.stringify({ data: "", cursor: 0 });
      throw new Error(`the test did not expect a ${name} call`);
    });
    render(<TerminalWorkspace grant={GRANT} />);

    // The session is on screen, not a row with an Attach button: the console's default page should
    // show a shell that is already running; the panel already does the same.
    expect(await screen.findByRole("tab", { name: /pty 1/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Attach" })).not.toBeInTheDocument();
    expect(callToolMock).toHaveBeenCalledWith(GRANT, "terminal.list", {});
  });

  it("attaches the newest session, not the first one the device lists", async () => {
    callToolMock.mockImplementation(async (_grant, name) => {
      if (name === "terminal.list") {
        // The device answers oldest-first: the shell abandoned an hour ago comes before the one an
        // AI client started a minute ago. The page attaches to the head of its own list.
        return JSON.stringify([
          { id: "old", kind: "pty", started_at_ms: 1_000 },
          { id: "new", kind: "pty", started_at_ms: 2_000 }
        ]);
      }
      if (name === "terminal.resize") return JSON.stringify({ rows: 24, cols: 80 });
      if (name === "terminal.read") return JSON.stringify({ data: "", cursor: 0 });
      throw new Error(`the test did not expect a ${name} call`);
    });
    render(<TerminalWorkspace grant={GRANT} />);

    await screen.findByRole("tab", { name: /pty 1/ });
    const reads = callToolMock.mock.calls.filter(([, name]) => name === "terminal.read");
    expect(reads[0][2]).toMatchObject({ id: "new" });
  });

  it("shows the device session behind each tab", async () => {
    callToolMock.mockImplementation(async (_grant, name) => {
      if (name === "terminal.list") {
        return JSON.stringify([{ id: "session-7", kind: "pty", started_at_ms: 1_000 }]);
      }
      if (name === "terminal.resize") return JSON.stringify({ rows: 24, cols: 80 });
      if (name === "terminal.read") return JSON.stringify({ data: "", cursor: 0 });
      throw new Error(`the test did not expect a ${name} call`);
    });
    render(<TerminalWorkspace grant={GRANT} />);

    // "pty 1" is this console's name for the tab; #7 is what the device calls the session, and the
    // Sessions list speaks in those. Without it two open shells are two identical-looking tabs.
    const tab = await screen.findByRole("tab", { name: /pty 1/ });
    expect(tab).toHaveTextContent("#7");
    expect(tab.querySelector(".tab-session")).toHaveAttribute("title", "session-7");
  });

  it("reads the session list again when the tab becomes visible", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await screen.findByText(/No session is open on this device/);
    const lists = () =>
      callToolMock.mock.calls.filter(([, name]) => name === "terminal.list").length;
    const before = lists();

    // Coming back to the tab is the moment the answer matters: a shell may have started meanwhile.
    document.dispatchEvent(new Event("visibilitychange"));

    await waitFor(() => expect(lists()).toBeGreaterThan(before));
  });

  it("finds a session that started while the page was open", async () => {
    vi.useFakeTimers();
    try {
      let listed = JSON.stringify([]);
      callToolMock.mockImplementation(async (_grant, name) => {
        if (name === "terminal.list") return listed;
        if (name === "terminal.resize") return JSON.stringify({ rows: 24, cols: 80 });
        if (name === "terminal.read") return JSON.stringify({ data: "", cursor: 0 });
        throw new Error(`the test did not expect a ${name} call`);
      });
      render(<TerminalWorkspace grant={GRANT} />);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(screen.getByText(/No session is open on this device/)).toBeInTheDocument();

      listed = JSON.stringify([{ id: "late-1", kind: "pty" }]);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(10_000);
      });

      // A page that keeps saying "no session" while a shell runs is the bug this surface exists to
      // avoid: the list was read once, before the shell existed, and no device event announces it.
      expect(screen.getByRole("tab", { name: /pty 1/ })).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("opens a pty session as a tab named pty 1", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();

    const tab = await screen.findByRole("tab", { name: /pty 1/ });
    expect(tab).toHaveTextContent("pty 1");
    expect(callToolMock).toHaveBeenCalledWith(GRANT, "terminal.create", { kind: "pty" });
  });

  it("paints the terminal on the page's canvas, not a black one", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });

    // jsdom loads no sheet, so the seam's fallback is what it sees; the point is that it is the
    // WHITE canvas the canonical palette names, not the old hardcoded #10151c.
    expect(mockedXterm.instances[0].options.theme).toMatchObject({ background: "#ffffff" });
  });

  it("keeps the session's secondary actions in one menu", async () => {
    const { container } = render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });

    const menu = container.querySelector("details.session-menu");
    expect(menu).not.toBeNull();
    expect(menu).not.toHaveAttribute("open");
    expect(screen.getByLabelText("Session tools")).toBeInTheDocument();
    // The actions exist inside the popover, not as a second row of toolbar buttons.
    expect(within(menu as HTMLElement).getByRole("button", { name: "Rename tab" })).toBeInTheDocument();
    expect(within(menu as HTMLElement).getByRole("button", { name: "Export log" })).toBeInTheDocument();
  });

  it("closes the menu once an action opens its dialog", async () => {
    const { container } = render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });
    const menu = container.querySelector("details.session-menu") as HTMLDetailsElement;
    menu.open = true;

    fireEvent.click(within(menu).getByRole("button", { name: "Rename tab" }));

    expect(await screen.findByRole("dialog", { name: "Rename tab" })).toBeInTheDocument();
    expect(menu.open).toBe(false);
  });

  it("renames the active tab and the name survives the next poll", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });

    fireEvent.click(screen.getByRole("button", { name: "Rename tab" }));
    const dialog = await screen.findByRole("dialog", { name: "Rename tab" });
    const input = within(dialog).getByLabelText("Tab name");
    expect(input).toHaveValue("pty 1");

    fireEvent.change(input, { target: { value: "  prod   db " } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));

    expect(await screen.findByRole("tab", { name: /prod db/ })).toHaveTextContent("prod db");
    // The tab's new name IS the confirmation; no sentence repeats it in the page chrome.

    const before = reads();
    await waitFor(() => expect(reads()).toBeGreaterThan(before), { timeout: 2_000 });
    expect(screen.getByRole("tab", { name: /prod db/ })).toBeInTheDocument();
    expect(screen.queryByRole("tab", { name: /pty 1/ })).not.toBeInTheDocument();
  });

  it("refuses an empty name and leaves the title and the dialog alone", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });

    fireEvent.click(screen.getByRole("button", { name: "Rename tab" }));
    const dialog = await screen.findByRole("dialog", { name: "Rename tab" });
    fireEvent.change(within(dialog).getByLabelText("Tab name"), { target: { value: "   " } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));

    expect(within(dialog).getByText("A tab needs a name.")).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: /pty 1/ })).toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Rename tab" })).toBeInTheDocument();
  });

  it("numbers a second tab after the first", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });
    await startPtyTab();

    expect(await screen.findByRole("tab", { name: /pty 2/ })).toHaveTextContent("pty 2");
    expect(screen.getAllByRole("tab")).toHaveLength(2);
  });

  it("offers a generated label again once its tab was renamed", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });

    fireEvent.click(screen.getByRole("button", { name: "Rename tab" }));
    const dialog = await screen.findByRole("dialog", { name: "Rename tab" });
    fireEvent.change(within(dialog).getByLabelText("Tab name"), { target: { value: "prod db" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Save" }));
    await screen.findByRole("tab", { name: /prod db/ });

    await startPtyTab();
    await waitFor(() => expect(screen.getAllByRole("tab")).toHaveLength(2));
    expect(screen.getByRole("tab", { name: /pty 1/ })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: /prod db/ })).toBeInTheDocument();
  });
});

describe("TerminalWorkspace transcript", () => {
  it("downloads what the device retains without moving the tab's own cursor", async () => {
    const readCursors: number[] = [];
    callToolMock.mockImplementation(async (_grant, name, args) => {
      if (name === "terminal.create") return JSON.stringify({ id: "t1" });
      if (name === "terminal.resize") return JSON.stringify({ rows: 24, cols: 80 });
      if (name === "terminal.list") return JSON.stringify([{ id: "t1", kind: "pty" }]);
      if (name === "terminal.read") {
        readCursors.push(Number(args?.cursor));
        return JSON.stringify({ data: "hello\n", cursor: 6 });
      }
      throw new Error(`the test did not expect a ${name} call`);
    });

    const blobs: Blob[] = [];
    const createObjectURL = vi.fn((blob: Blob) => {
      blobs.push(blob);
      return "blob:transcript";
    });
    Object.defineProperty(URL, "createObjectURL", { configurable: true, value: createObjectURL });
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});

    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });
    // The tab's first poll has already taken its own cursor to 6.
    await waitFor(() => expect(readCursors.length).toBeGreaterThan(0));

    fireEvent.click(screen.getByRole("button", { name: "Export log" }));
    expect(await screen.findByText("Exported 6 bytes.")).toBeInTheDocument();

    expect(createObjectURL).toHaveBeenCalledTimes(1);
    const file = await blobs[0].text();
    expect(file).toContain("Summrise terminal transcript");
    expect(file).toContain("session: t1");
    expect(file).toContain("range: bytes 0..6 of this session's output");
    expect(file).toContain("dropped: none — the whole session is retained");
    expect(file).toContain("---\nhello\n");
    const anchor = click.mock.instances[0] as HTMLAnchorElement;
    expect(anchor.download).toBe("t1.log");

    const afterExport = readCursors.length;
    await waitFor(() => expect(readCursors.length).toBeGreaterThan(afterExport), { timeout: 2_000 });
    expect(readCursors.slice(afterExport).every((cursor) => cursor === 6)).toBe(true);
  });

  it("moves the cursor to the earliest retained byte when the device refuses it as too old", async () => {
    const readCursors: number[] = [];
    callToolMock.mockImplementation(async (_grant, name, args) => {
      if (name === "terminal.create") return JSON.stringify({ id: "t1" });
      if (name === "terminal.resize") return JSON.stringify({ rows: 24, cols: 80 });
      if (name === "terminal.list") return JSON.stringify([{ id: "t1", kind: "pty" }]);
      if (name === "terminal.read") {
        const cursor = Number(args?.cursor);
        readCursors.push(cursor);
        if (cursor < 4096) {
          throw new ToolError(
            "terminal.read: the cursor 0 is older than the retained output, which starts at 4096",
            200,
            null,
            true
          );
        }
        return JSON.stringify({ data: "", cursor: 4096 });
      }
      throw new Error(`the test did not expect a ${name} call`);
    });

    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });

    await waitFor(() => expect(readCursors).toContain(4096), { timeout: 3_000 });
    expect(readCursors[0]).toBe(0);
    expect(callToolMock).toHaveBeenCalledWith(GRANT, "terminal.list", {});
  });

  it("leaves the cursor alone after a transient read failure", async () => {
    const readCursors: number[] = [];
    let attempts = 0;
    callToolMock.mockImplementation(async (_grant, name, args) => {
      if (name === "terminal.create") return JSON.stringify({ id: "t1" });
      if (name === "terminal.resize") return JSON.stringify({ rows: 24, cols: 80 });
      if (name === "terminal.list") return JSON.stringify([{ id: "t1", kind: "pty" }]);
      if (name === "terminal.read") {
        const cursor = Number(args?.cursor);
        readCursors.push(cursor);
        attempts += 1;
        if (attempts === 1) return JSON.stringify({ data: "", cursor: 6 });
        if (attempts === 2) throw new ToolError("Failed to fetch", 0);
        return JSON.stringify({ data: "", cursor: 6 });
      }
      throw new Error(`the test did not expect a ${name} call`);
    });

    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });

    await waitFor(
      () => expect(readCursors.filter((cursor) => cursor === 6).length).toBeGreaterThanOrEqual(2),
      { timeout: 3_000 }
    );
    expect(readCursors.slice(1).every((cursor) => cursor === 6)).toBe(true);
  });

  it("recovers on the next poll when a too-old refusal arrives during a read fault", async () => {
    const readCursors: number[] = [];
    let attempts = 0;
    let releaseTooOld!: () => void;
    const transientFaultSeen = new Promise<void>((resolve) => {
      releaseTooOld = resolve;
    });

    callToolMock.mockImplementation(async (_grant, name, args) => {
      if (name === "terminal.create") return JSON.stringify({ id: "t1" });
      if (name === "terminal.resize") return JSON.stringify({ rows: 24, cols: 80 });
      if (name === "terminal.list") return JSON.stringify([{ id: "t1", kind: "pty" }]);
      if (name === "terminal.read") {
        const cursor = Number(args?.cursor);
        readCursors.push(cursor);
        attempts += 1;
        if (attempts === 1) return JSON.stringify({ data: "", cursor: 6 });
        if (attempts === 2) throw new ToolError("Failed to fetch", 0);
        if (cursor < 4096) {
          // Hold the too-old refusal until the transient fault is on screen: this is the case where
          // `fail` already has a fault to update, not the first-failure path `diagnose` handles.
          await transientFaultSeen;
          throw new ToolError(
            `terminal.read: the cursor ${cursor} is older than the retained output, which starts at 4096`,
            200,
            null,
            true
          );
        }
        return JSON.stringify({ data: "", cursor: 4096 });
      }
      throw new Error(`the test did not expect a ${name} call`);
    });

    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });

    expect(await screen.findByRole("alert")).toHaveTextContent("Failed to fetch");
    releaseTooOld();

    await waitFor(() => expect(readCursors).toContain(4096), { timeout: 3_000 });
    const recoveredAt = readCursors.indexOf(4096);
    expect(recoveredAt).toBeGreaterThan(0);
    expect(readCursors[recoveredAt - 1]).toBe(6);
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
  });
});

describe("TerminalWorkspace text size", () => {
  afterEach(() => {
    window.localStorage.removeItem(FONT_SIZE_KEY);
  });

  it("opens a tab at the default size", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });

    await waitFor(() => expect(mockedXterm.instances).toHaveLength(1));
    expect(mockedXterm.instances[0].options.fontSize).toBe(DEFAULT_FONT_SIZE);
  });

  it("sizes every open terminal in place and remembers the choice", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });
    await startPtyTab();
    await waitFor(() => expect(mockedXterm.instances).toHaveLength(2));
    await waitFor(() => expect(mockedFit.instances).toHaveLength(2));
    const fitsBefore = mockedFit.instances.map((instance) => instance.fit.mock.calls.length);

    fireEvent.click(screen.getByRole("button", { name: "A+" }));

    // Every open tab is refitted into the host it already has, and none of them is recreated.
    expect(mockedFit.instances.map((instance) => instance.fit.mock.calls.length)).toEqual(
      fitsBefore.map((count) => count + 1)
    );
    expect(mockedXterm.instances.map((instance) => instance.options.fontSize)).toEqual([14, 14]);
    expect(window.localStorage.getItem(FONT_SIZE_KEY)).toBe("14");
    expect(screen.getByRole("button", { name: "A+" })).toHaveAttribute(
      "title",
      "Larger terminal text (now 14)"
    );
  });

  it("creates the next tab at the size already chosen", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });
    fireEvent.click(screen.getByRole("button", { name: "A+" }));

    await startPtyTab();
    await waitFor(() => expect(mockedXterm.instances).toHaveLength(2));
    expect(mockedXterm.instances[1].options.fontSize).toBe(14);
  });

  it("reads the remembered size before the first tab exists", async () => {
    window.localStorage.setItem(FONT_SIZE_KEY, "18");

    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });

    await waitFor(() => expect(mockedXterm.instances).toHaveLength(1));
    expect(mockedXterm.instances[0].options.fontSize).toBe(18);
    expect(screen.getByRole("button", { name: "A+" })).toHaveAttribute(
      "title",
      "Larger terminal text (now 18)"
    );
  });

  it("disables A− at the minimum the layout survives", async () => {
    window.localStorage.setItem(FONT_SIZE_KEY, String(MIN_FONT_SIZE));
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });

    expect(screen.getByRole("button", { name: "A−" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "A+" })).toBeEnabled();
  });

  it("disables A+ at the maximum", async () => {
    window.localStorage.setItem(FONT_SIZE_KEY, String(MAX_FONT_SIZE));
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });

    expect(screen.getByRole("button", { name: "A+" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "A−" })).toBeEnabled();
  });

  it("renders and opens a tab when the browser refuses storage access", async () => {
    // A browser with cookies or site data blocked throws on the property itself, not on getItem: the
    // workspace must still render, and the size falls back to the default for this visit.
    const descriptor = Object.getOwnPropertyDescriptor(window, "localStorage");
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      get() {
        throw new Error("The browser blocked storage.");
      }
    });
    try {
      render(<TerminalWorkspace grant={GRANT} />);
      await startPtyTab();
      await screen.findByRole("tab", { name: /pty 1/ });
      expect(screen.getByRole("button", { name: "A+" })).toBeEnabled();
      await waitFor(() => expect(mockedXterm.instances).toHaveLength(1));
      expect(mockedXterm.instances[0].options.fontSize).toBe(DEFAULT_FONT_SIZE);
    } finally {
      restoreGlobal(window, "localStorage", descriptor);
    }
  });
});

describe("TerminalWorkspace session end", () => {
  /**
   * Answer the workspace's terminal calls, serving `answer` to every read.
   *
   * Returns the read count: a tab that discovered its session ended must stop asking, and the
   * count is how a test sees that.
   */
  function serveRead(answer: Record<string, unknown>): () => number {
    let reads = 0;
    callToolMock.mockImplementation(async (_grant, name) => {
      if (name === "terminal.create") return JSON.stringify({ id: "t1" });
      if (name === "terminal.resize") return JSON.stringify({ rows: 24, cols: 80 });
      if (name === "terminal.list") return JSON.stringify([{ id: "t1", kind: "pty" }]);
      if (name === "terminal.read") {
        reads += 1;
        return JSON.stringify(answer);
      }
      throw new Error(`the test did not expect a ${name} call`);
    });
    return () => reads;
  }

  it("marks the tab ended with the exit code and asks no further", async () => {
    const readCount = serveRead({
      data: "bye\n",
      cursor: 4,
      alive: false,
      exit_code: 7,
      signal: null
    });

    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    const tab = await screen.findByRole("tab", { name: /pty 1/ });

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The session ended with exit code 7. The output above is what it printed."
    );
    // The last bytes still reach the terminal: output is queued for the next frame, so the write
    // lands just after the mark rather than before it — and it is not dropped.
    await waitFor(() => expect(mockedXterm.instances).toHaveLength(1));
    await waitFor(() => expect(mockedXterm.instances[0].write).toHaveBeenCalledWith("bye\n"));
    // One mark: the suffix ends the label, and the old "closed" chip is not drawn beside it.
    expect(tab).toHaveTextContent(/pty 1 · ended/);
    expect(within(tab).queryByText("closed")).not.toBeInTheDocument();

    // The read that discovered the end sets `closed`; the next 250 ms tick must send nothing.
    const before = readCount();
    expect(before).toBeGreaterThan(0);
    await new Promise((resolve) => setTimeout(resolve, 600));
    expect(readCount()).toBe(before);
  });

  it("falls back when an older device rejects the wait_ms field", async () => {
    let reads = 0;
    callToolMock.mockImplementation(async (_grant, name, args) => {
      if (name === "terminal.create") return JSON.stringify({ id: "t1" });
      if (name === "terminal.resize") return JSON.stringify({ rows: 24, cols: 80 });
      if (name === "terminal.list") return JSON.stringify([]);
      if (name === "terminal.read") {
        reads += 1;
        if (reads === 1) {
          throw new ToolError("terminal.read: the arguments do not match the tool's input schema.", 400);
        }
        // A device answers from the cursor it was given: the retry starts at 0 and every later read
        // starts at 5, so the one line is handed over once.
        const cursor = (args as { cursor?: number } | undefined)?.cursor ?? 0;
        return cursor === 0
          ? JSON.stringify({ data: "hello", cursor: 5, alive: true })
          : JSON.stringify({ data: "", cursor, alive: true });
      }
      throw new Error(`the test did not expect a ${name} call with ${JSON.stringify(args)}`);
    });

    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();

    await waitFor(() => expect(mockedXterm.instances[0]?.write).toHaveBeenCalledWith("hello"));
    const calls = callToolMock.mock.calls.filter(([, name]) => name === "terminal.read");
    expect(calls[0][2]).toMatchObject({ wait_ms: 15_000 });
    expect(calls[1][2]).toEqual({ id: "t1", cursor: 0 });
    expect(screen.queryByText(/Reading this session failed/)).not.toBeInTheDocument();
  });

  it("does not stack reads while one answer is slow", async () => {
    let reads = 0;
    let release: (value: string) => void = () => {};
    callToolMock.mockImplementation(async (_grant, name) => {
      if (name === "terminal.create") return JSON.stringify({ id: "slow-1" });
      if (name === "terminal.resize") return JSON.stringify({ rows: 24, cols: 80 });
      if (name === "terminal.list") return JSON.stringify([]);
      if (name === "terminal.read") {
        reads += 1;
        return new Promise<string>((resolve) => {
          release = resolve;
        });
      }
      throw new Error(`the test did not expect a ${name} call`);
    });

    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await waitFor(() => expect(reads).toBe(1), { timeout: 2_000 });

    // Two interval ticks pass while the first read is still unanswered; a second read at the same
    // cursor would duplicate output (or reorder it), which is what "not live" looked like.
    await new Promise((resolve) => setTimeout(resolve, 650));
    expect(reads).toBe(1);

    release(JSON.stringify({ data: "", cursor: 0 }));
  });

  it("leaves a tab alone when the device does not say whether it is alive", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    const tab = await screen.findByRole("tab", { name: /pty 1/ });

    // The default answer is the older device's `{data, cursor}`; it must not mark the tab.
    await waitFor(() => expect(reads()).toBeGreaterThan(0), { timeout: 2_000 });
    await new Promise((resolve) => setTimeout(resolve, 400));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(tab).not.toHaveTextContent("ended");
  });

  it("says which signal ended the session", async () => {
    serveRead({ data: "", cursor: 0, alive: false, exit_code: null, signal: "SIGTERM" });

    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The session ended on signal SIGTERM. The output above is what it printed."
    );
  });

  it("says only that the session ended when the device knows neither fact", async () => {
    serveRead({ data: "", cursor: 0, alive: false, exit_code: null, signal: null });

    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The session ended. The output above is what it printed."
    );
  });

  it("clears a read fault on a live answer that says alive", async () => {
    let attempts = 0;
    callToolMock.mockImplementation(async (_grant, name) => {
      if (name === "terminal.create") return JSON.stringify({ id: "t1" });
      if (name === "terminal.resize") return JSON.stringify({ rows: 24, cols: 80 });
      if (name === "terminal.list") return JSON.stringify([{ id: "t1", kind: "pty" }]);
      if (name === "terminal.read") {
        attempts += 1;
        if (attempts <= 2) throw new ToolError("Failed to fetch", 0);
        return JSON.stringify({ data: "", cursor: 0, alive: true, exit_code: null, signal: null });
      }
      throw new Error(`the test did not expect a ${name} call`);
    });

    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();

    expect(await screen.findByRole("alert")).toHaveTextContent("Failed to fetch");
    await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument(), {
      timeout: 3_000
    });
  });
});

describe("TerminalWorkspace search", () => {
  /** Open the bar from the actions row and hand back its input. */
  function openSearch(): HTMLElement {
    fireEvent.click(screen.getByRole("button", { name: "Find" }));
    return screen.getByPlaceholderText("Find in this session");
  }

  it("offers Find only once a session is open", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    expect(screen.queryByRole("button", { name: "Find" })).not.toBeInTheDocument();
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });
    expect(screen.getByRole("button", { name: "Find" })).toBeEnabled();
  });

  it("finds forward on Enter and backward on Shift+Enter, decorating the matches", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });
    await waitFor(() => expect(mockedSearch.instances).toHaveLength(1));
    const addon = mockedSearch.instances[0];
    // The addon is loaded on this tab's own xterm, so it searches that tab's scrollback and no other.
    expect(mockedXterm.instances[0].loadAddon.mock.calls.map(([loaded]) => loaded)).toContain(addon);

    const input = openSearch();
    expect(input).toHaveFocus();
    // An empty term is not a search: both directions wait for one.
    expect(screen.getByRole("button", { name: "Next" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Previous" })).toBeDisabled();

    fireEvent.change(input, { target: { value: "error" } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(addon.findNext).toHaveBeenCalledTimes(1);
    expect(addon.findNext.mock.calls[0][0]).toBe("error");
    // Without decorations the addon selects a match but draws nothing, and clearing draws nothing too.
    const next = addon.findNext.mock.calls[0][1];
    expect(next?.decorations).toMatchObject({
      matchBackground: "rgba(191,58,10,.18)",
      activeMatchBackground: "#fff0e6"
    });
    expect(next?.caseSensitive).toBeUndefined();

    fireEvent.keyDown(input, { key: "Enter", shiftKey: true });
    expect(addon.findPrevious).toHaveBeenCalledTimes(1);
    expect(addon.findPrevious.mock.calls[0][0]).toBe("error");
    expect(addon.findPrevious.mock.calls[0][1]?.decorations).toBeDefined();
    expect(addon.findPrevious.mock.calls[0][1]?.caseSensitive).toBeUndefined();

    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    expect(addon.findNext).toHaveBeenCalledTimes(2);
    fireEvent.click(screen.getByRole("button", { name: "Previous" }));
    expect(addon.findPrevious).toHaveBeenCalledTimes(2);
  });

  it("says No match. on a miss and clears the sentence on the next hit", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });
    const input = openSearch();
    fireEvent.change(input, { target: { value: "error" } });

    mockedSearch.nextResult = false;
    fireEvent.keyDown(input, { key: "Enter" });
    expect(await screen.findByText("No match.")).toBeInTheDocument();

    mockedSearch.nextResult = true;
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(screen.queryByText("No match.")).not.toBeInTheDocument());
  });

  it("searches while the operator types, without waiting for Enter", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });
    const input = openSearch();
    const addon = mockedSearch.instances[0];

    vi.useFakeTimers();
    try {
      fireEvent.change(input, { target: { value: "line 199" } });
      act(() => {
        vi.advanceTimersByTime(200);
      });
      // A field that waits for Enter reads as broken; the term searches itself after the pause.
      expect(addon.findNext).toHaveBeenCalledWith("line 199", expect.anything());
    } finally {
      vi.useRealTimers();
    }
  });

  it("clears the decorations of the tab it searched when the bar closes", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });
    await waitFor(() => expect(mockedSearch.instances).toHaveLength(1));
    const addon = mockedSearch.instances[0];

    const input = openSearch();
    fireEvent.change(input, { target: { value: "error" } });
    fireEvent.keyDown(input, { key: "Enter" });
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(screen.queryByPlaceholderText("Find in this session")).not.toBeInTheDocument();
    expect(addon.clearDecorations).toHaveBeenCalledTimes(1);

    const again = openSearch();
    fireEvent.keyDown(again, { key: "Escape" });
    expect(screen.queryByPlaceholderText("Find in this session")).not.toBeInTheDocument();
    expect(addon.clearDecorations).toHaveBeenCalledTimes(2);
  });

  it("closes with Escape from a button in the bar, not only from the field", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });
    await waitFor(() => expect(mockedSearch.instances).toHaveLength(1));
    const addon = mockedSearch.instances[0];

    const input = openSearch();
    fireEvent.change(input, { target: { value: "error" } });
    fireEvent.keyDown(input, { key: "Enter" });

    const close = screen.getByRole("button", { name: "Close" });
    close.focus();
    expect(close).toHaveFocus();
    fireEvent.keyDown(close, { key: "Escape" });

    expect(screen.queryByPlaceholderText("Find in this session")).not.toBeInTheDocument();
    expect(addon.clearDecorations).toHaveBeenCalledTimes(1);
  });

  it("toggles the bar back out of the way from Find", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });

    openSearch();
    fireEvent.click(screen.getByRole("button", { name: "Find" }));
    expect(screen.queryByPlaceholderText("Find in this session")).not.toBeInTheDocument();
  });

  it("clears the decorations of the tab it leaves and searches the one that is active", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 2/ });
    await waitFor(() => expect(mockedSearch.instances).toHaveLength(2));
    const [first, second] = mockedSearch.instances;

    fireEvent.click(screen.getByRole("tab", { name: /pty 1/ }));
    const input = openSearch();
    fireEvent.change(input, { target: { value: "error" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(first.findNext).toHaveBeenCalledTimes(1);

    // A miss belongs to the tab that was searched, not to the one the operator switches to.
    mockedSearch.nextResult = false;
    fireEvent.keyDown(input, { key: "Enter" });
    expect(await screen.findByText("No match.")).toBeInTheDocument();
    mockedSearch.nextResult = true;

    const clearedBefore = first.clearDecorations.mock.calls.length;
    fireEvent.click(screen.getByRole("tab", { name: /pty 2/ }));
    expect(first.clearDecorations.mock.calls.length).toBe(clearedBefore + 1);
    expect(screen.queryByText("No match.")).not.toBeInTheDocument();
    // The bar stayed open, so the next step is the newly active tab's search.
    fireEvent.keyDown(input, { key: "Enter" });
    expect(second.findNext).toHaveBeenCalledTimes(1);
    expect(second.findNext.mock.calls[0][0]).toBe("error");
    expect(first.findNext).toHaveBeenCalledTimes(2);
  });

  it("never asks the device anything", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await screen.findByRole("tab", { name: /pty 1/ });
    const before = callToolMock.mock.calls.length;

    const input = openSearch();
    fireEvent.change(input, { target: { value: "error" } });
    fireEvent.keyDown(input, { key: "Enter" });
    fireEvent.keyDown(input, { key: "Enter", shiftKey: true });
    fireEvent.click(screen.getByRole("button", { name: "Next" }));
    fireEvent.click(screen.getByRole("button", { name: "Close" }));

    // No await between the bar's actions and this assertion, so the tab's poll cannot have run: every
    // call the workspace made was there before the search started.
    expect(callToolMock.mock.calls.length).toBe(before);
  });
});

describe("TerminalWorkspace recovery", () => {
  it("shows a preset's open failure even with no tab open", async () => {
    const connections = vi.mocked(api.connections);
    connections.mockResolvedValueOnce([
      { id: "c1", name: "local shell", kind: "pty" }
    ] as never);
    callToolMock.mockImplementation(async (_grant, name) => {
      if (name === "terminal.list") return JSON.stringify([]);
      if (name === "terminal.create") throw new ToolError("device disconnected", 502);
      throw new Error(`the test did not expect a ${name} call`);
    });

    render(<TerminalWorkspace grant={GRANT} />);
    fireEvent.click(await screen.findByText("Saved connections (1)"));
    fireEvent.click(screen.getByRole("button", { name: "Open" }));

    expect(await screen.findByText("Could not open: device disconnected")).toBeInTheDocument();
    expect(screen.queryByRole("tab")).not.toBeInTheDocument();
  });

  it("keeps the tab's close control out of the tab button", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    const close = screen.getByRole("button", { name: "Close pty 1" });
    expect(close.tagName).toBe("BUTTON");
    expect(close.closest("button")).toBe(close);
    expect(screen.getByRole("tab", { name: /pty 1/ }).contains(close)).toBe(false);
  });

  it("asks before closing a live session, and keeps it when the operator declines", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();

    fireEvent.click(screen.getByRole("button", { name: "Close pty 1" }));
    const dialog = await screen.findByRole("dialog", { name: "Close pty 1?" });
    // The old sentence said the process keeps running while the only button ended it. Both facts
    // are now named, and the safe one is the primary action.
    expect(within(dialog).getByText(/leaves the shell running on the machine/)).toBeInTheDocument();
    expect(within(dialog).getByText(/stops the shell/)).toBeInTheDocument();
    expect(callToolMock.mock.calls.some(([, name]) => name === "terminal.close")).toBe(false);

    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog", { name: "Close pty 1?" })).not.toBeInTheDocument();
    expect(callToolMock.mock.calls.some(([, name]) => name === "terminal.close")).toBe(false);
    expect(screen.getByRole("tab", { name: /pty 1/ })).toBeInTheDocument();
  });

  it("detaches without ending the shell, and offers the session again", async () => {
    // The device keeps listing the session after the page detaches: that is what detach means.
    callToolMock.mockImplementation(async (_grant, name) => {
      if (name === "terminal.list") {
        return JSON.stringify([{ id: "t1", kind: "pty", started_at_ms: 1_000 }]);
      }
      if (name === "terminal.create") return JSON.stringify({ id: "t1" });
      if (name === "terminal.resize") return JSON.stringify({ rows: 24, cols: 80 });
      if (name === "terminal.read") return JSON.stringify({ data: "", cursor: 0 });
      throw new Error(`the test did not expect a ${name} call`);
    });
    render(<TerminalWorkspace grant={GRANT} />);
    await screen.findByRole("tab", { name: /pty 1/ });

    fireEvent.click(screen.getByRole("button", { name: "Close pty 1" }));
    const dialog = await screen.findByRole("dialog", { name: "Close pty 1?" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Detach" }));

    // Detach is a view change: the device is never asked to end anything.
    expect(callToolMock.mock.calls.some(([, name]) => name === "terminal.close")).toBe(false);
    expect(screen.queryByRole("tab", { name: /pty 1/ })).not.toBeInTheDocument();
    // The device still lists it, and the page offers it again instead of hiding it.
    expect(await screen.findByRole("button", { name: "Attach" })).toBeInTheDocument();
  });

  it("does not take another shell's screen after a detach", async () => {
    callToolMock.mockImplementation(async (_grant, name) => {
      if (name === "terminal.list") {
        return JSON.stringify([
          { id: "new", kind: "pty", started_at_ms: 2_000 },
          { id: "older", kind: "pty", started_at_ms: 1_000 }
        ]);
      }
      if (name === "terminal.resize") return JSON.stringify({ rows: 24, cols: 80 });
      if (name === "terminal.read") return JSON.stringify({ data: "", cursor: 0 });
      throw new Error(`the test did not expect a ${name} call`);
    });
    render(<TerminalWorkspace grant={GRANT} />);
    await screen.findByRole("tab", { name: /pty 1/ });

    fireEvent.click(screen.getByRole("button", { name: "Close pty 1" }));
    fireEvent.click(
      within(await screen.findByRole("dialog", { name: "Close pty 1?" })).getByRole("button", {
        name: "Detach"
      })
    );

    // The other shell was already listed; a detach is not a request to show the next one.
    expect(screen.queryByRole("tab")).not.toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "Attach" })).toBeInTheDocument();
  });

  it("attaches another session from the Sessions menu without closing the first", async () => {    callToolMock.mockImplementation(async (_grant, name) => {
      if (name === "terminal.list") {
        return JSON.stringify([
          { id: "new", kind: "pty", started_at_ms: Date.now() - 60_000 },
          { id: "older", kind: "pty", started_at_ms: Date.now() - 5 * 60_000 }
        ]);
      }
      if (name === "terminal.resize") return JSON.stringify({ rows: 24, cols: 80 });
      if (name === "terminal.read") return JSON.stringify({ data: "", cursor: 0 });
      throw new Error(`the test did not expect a ${name} call`);
    });
    render(<TerminalWorkspace grant={GRANT} />);
    await screen.findByRole("tab", { name: /pty 1/ });

    // The second tab does not exist yet; the menu names the one the device still has.
    fireEvent.click(screen.getByLabelText("Session tools"));
    fireEvent.click(await screen.findByRole("button", { name: "Sessions (1)" }));
    const dialog = await screen.findByRole("dialog", { name: "Sessions on this device" });
    expect(within(dialog).getByText("older")).toBeInTheDocument();
    // The list is about "which one is recent"; the age is what answers it, not a wall-clock stamp.
    expect(within(dialog).getByText(/opened 5m \d\ds ago/)).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Attach" }));

    expect(await screen.findByRole("tab", { name: /pty 2/ })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: /pty 1/ })).toBeInTheDocument();
    expect(callToolMock.mock.calls.some(([, name]) => name === "terminal.close")).toBe(false);
  });

  it("keeps the tab and says so when the device will not close the session", async () => {
    const base = callToolMock.getMockImplementation();
    callToolMock.mockImplementation(async (grant, name, args) => {
      if (name === "terminal.close") throw new ToolError("device disconnected", 502);
      return base!(grant, name, args);
    });

    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    fireEvent.click(screen.getByRole("button", { name: "Close pty 1" }));

    // Ending a session asks first, and says what the destructive button does.
    const dialog = await screen.findByRole("dialog", { name: "Close pty 1?" });
    expect(within(dialog).getByText(/stops the shell/)).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "End session" }));

    expect(await screen.findByText(/Could not close pty 1: device disconnected/)).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: /pty 1/ })).toBeInTheDocument();
  });
});

describe("TerminalWorkspace display", () => {
  it("builds the overview ruler the search's match colours are drawn on", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    // xterm only instantiates the ruler renderer when a width is configured; without this the
    // matchOverviewRuler colours go nowhere and a long scrollback has no map of its hits.
    expect(mockedXterm.instances[0].options.overviewRuler).toEqual({ width: 14 });
  });

  it("allows the proposed API the search addon's decorations need", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    // findNext() calls registerDecoration, and xterm refuses that without this flag: the search
    // reported a hit and then threw, so nothing scrolled and no sentence said why. Seen in a real
    // browser as "Uncaught Error: You must set the allowProposedApi option to true".
    expect(mockedXterm.instances[0].options.allowProposedApi).toBe(true);
  });

  it("keeps twenty thousand lines of scrollback", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    // xterm's default is 1000 lines: a few minutes of a chatty shell, and no way back to what a
    // build printed. The legacy panel kept 20 000 and this page follows it.
    expect(mockedXterm.instances[0].options.scrollback).toBe(20_000);
  });

  it("copies a selection as soon as it is made", async () => {
    const clipboard = {
      writeText: vi.fn(async () => {}),
      readText: vi.fn(async () => "")
    };
    const descriptor = Object.getOwnPropertyDescriptor(navigator, "clipboard");
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: clipboard });
    try {
      render(<TerminalWorkspace grant={GRANT} />);
      await startPtyTab();
      const term = mockedXterm.instances[0];
      term.getSelection.mockReturnValue("Get-Date");
      term.selectionHandler?.();
      expect(clipboard.writeText).toHaveBeenCalledWith("Get-Date");
    } finally {
      if (descriptor) Object.defineProperty(navigator, "clipboard", descriptor);
      else delete (navigator as { clipboard?: unknown }).clipboard;
    }
  });

  it("pastes on right-click instead of opening the browser menu", async () => {
    const clipboard = {
      writeText: vi.fn(async () => {}),
      readText: vi.fn(async () => "Get-Date\r")
    };
    const descriptor = Object.getOwnPropertyDescriptor(navigator, "clipboard");
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: clipboard });
    try {
      const { container } = render(<TerminalWorkspace grant={GRANT} />);
      await startPtyTab();
      const host = container.querySelector(".term-host");
      expect(host).not.toBeNull();
      fireEvent.contextMenu(host!);
      await waitFor(() => expect(mockedXterm.instances[0].paste).toHaveBeenCalledWith("Get-Date\r"));
    } finally {
      if (descriptor) Object.defineProperty(navigator, "clipboard", descriptor);
      else delete (navigator as { clipboard?: unknown }).clipboard;
    }
  });

  it("follows a scheme change while the tab is open", async () => {
    const handlers: (() => void)[] = [];
    const media = {
      matches: false,
      addEventListener: vi.fn((_event: string, handler: () => void) => handlers.push(handler)),
      removeEventListener: vi.fn()
    };
    const matchDescriptor = Object.getOwnPropertyDescriptor(window, "matchMedia");
    const styleDescriptor = Object.getOwnPropertyDescriptor(window, "getComputedStyle");
    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      value: vi.fn(() => media)
    });
    // jsdom has no stylesheet, and the page's palette arrives as CSS tokens: this stands one in and
    // flips it with the scheme, so the assertion is about the terminal re-reading the page.
    Object.defineProperty(window, "getComputedStyle", {
      configurable: true,
      value: vi.fn(() => ({
        getPropertyValue: (name: string) =>
          media.matches && name === "--term-canvas" ? "#131418" : ""
      }))
    });
    try {
      render(<TerminalWorkspace grant={GRANT} />);
      await startPtyTab();
      // The xterm instance is created in an effect after the pty answers; reading it without a wait
      // was a race that failed about one run in three.
      await waitFor(() => expect(mockedXterm.instances[0]).toBeDefined());
      expect(mockedXterm.instances[0].options.theme).toMatchObject({ background: "#ffffff" });

      media.matches = true;
      handlers.forEach((handler) => handler());

      await waitFor(() =>
        expect(mockedXterm.instances[0].options.theme).toMatchObject({ background: "#131418" })
      );
    } finally {
      restoreGlobal(window, "matchMedia", matchDescriptor);
      restoreGlobal(window, "getComputedStyle", styleDescriptor);
    }
  });

  it("refits and tells the device when the window changes", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    await waitFor(() => expect(mockedFit.instances[0]).toBeDefined());

    // A real fit changes the grid; the jsdom one is a stub, so the test moves it.
    mockedFit.instances[0].fit.mockImplementation(() => {
      mockedXterm.instances[0].rows = 40;
      mockedXterm.instances[0].cols = 120;
    });
    window.dispatchEvent(new Event("resize"));

    await waitFor(() =>
      expect(callToolMock).toHaveBeenCalledWith(GRANT, "terminal.resize", {
        id: "t1",
        rows: 40,
        cols: 120
      })
    );
  });
});

describe("TerminalWorkspace shortcuts", () => {
  it("opens the scrollback search on Ctrl+F instead of handing the key to the shell", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();

    const handler = mockedXterm.instances[0].keyHandler;
    expect(handler).not.toBeNull();
    const event = {
      type: "keydown",
      key: "f",
      ctrlKey: true,
      metaKey: false,
      shiftKey: false,
      altKey: false,
      preventDefault: vi.fn()
    } as unknown as KeyboardEvent;

    expect(handler!(event)).toBe(false);
    expect(event.preventDefault).toHaveBeenCalled();
    expect(await screen.findByLabelText("Find in this session")).toBeInTheDocument();
  });

  it("leaves every other key to the shell", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();

    const handler = mockedXterm.instances[0].keyHandler!;
    const event = {
      type: "keydown",
      key: "c",
      ctrlKey: true,
      metaKey: false,
      shiftKey: false,
      altKey: false,
      preventDefault: vi.fn()
    } as unknown as KeyboardEvent;

    expect(handler(event)).toBe(true);
    expect(event.preventDefault).not.toHaveBeenCalled();
  });
});
