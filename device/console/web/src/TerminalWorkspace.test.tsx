import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterAll, afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import { TerminalWorkspace } from "./TerminalWorkspace";
import { callTool, ToolError, type Grant } from "./terminal";
import { DEFAULT_FONT_SIZE, FONT_SIZE_KEY, MAX_FONT_SIZE, MIN_FONT_SIZE } from "./terminalFont";
import { restoreGlobal } from "./test/dom";

/// The xterms the component created, in creation order: a font change is asserted on the instance,
/// because that is what the operator's screen shows, and the loaded addons are how a tab proves the
/// search addon belongs to its own terminal.
const mockedXterm = vi.hoisted(() => ({
  instances: [] as { options: Record<string, unknown>; loadAddon: Mock; write: Mock }[]
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
  fireEvent.click(screen.getByRole("button", { name: "+ New" }));
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
  it("disables Rename tab until a tab is open", () => {
    render(<TerminalWorkspace grant={GRANT} />);
    expect(screen.getByRole("button", { name: "Rename tab" })).toBeDisabled();
  });

  it("opens a pty session as a tab named pty 1", async () => {
    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();

    const tab = await screen.findByRole("tab", { name: /pty 1/ });
    expect(tab).toHaveTextContent("pty 1");
    expect(callToolMock).toHaveBeenCalledWith(GRANT, "terminal.create", { kind: "pty" });
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
    expect(screen.getByText('Renamed to "prod db".')).toBeInTheDocument();

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

  it("disables A− at the minimum the layout survives", () => {
    window.localStorage.setItem(FONT_SIZE_KEY, String(MIN_FONT_SIZE));
    render(<TerminalWorkspace grant={GRANT} />);

    expect(screen.getByRole("button", { name: "A−" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "A+" })).toBeEnabled();
  });

  it("disables A+ at the maximum", () => {
    window.localStorage.setItem(FONT_SIZE_KEY, String(MAX_FONT_SIZE));
    render(<TerminalWorkspace grant={GRANT} />);

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
      expect(screen.getByRole("button", { name: "A+" })).toBeEnabled();

      await startPtyTab();
      await screen.findByRole("tab", { name: /pty 1/ });
      await waitFor(() => expect(mockedXterm.instances).toHaveLength(1));
      expect(mockedXterm.instances[0].options.fontSize).toBe(DEFAULT_FONT_SIZE);
    } finally {
      restoreGlobal(window, "localStorage", descriptor);
    }
  });
});

describe("TerminalWorkspace session end", () => {
  it("marks the tab ended with the exit code and asks no further", async () => {
    let reads = 0;
    callToolMock.mockImplementation(async (_grant, name) => {
      if (name === "terminal.create") return JSON.stringify({ id: "t1" });
      if (name === "terminal.resize") return JSON.stringify({ rows: 24, cols: 80 });
      if (name === "terminal.list") return JSON.stringify([{ id: "t1", kind: "pty" }]);
      if (name === "terminal.read") {
        reads += 1;
        return JSON.stringify({ data: "bye\n", cursor: 4, alive: false, exit_code: 7, signal: null });
      }
      throw new Error(`the test did not expect a ${name} call`);
    });

    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();
    const tab = await screen.findByRole("tab", { name: /pty 1/ });

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The session ended with exit code 7. The output above is what it printed."
    );
    // The last bytes are written before the tab is marked, and the output stays on screen.
    await waitFor(() => expect(mockedXterm.instances).toHaveLength(1));
    expect(mockedXterm.instances[0].write).toHaveBeenCalledWith("bye\n");
    expect(tab).toHaveTextContent("pty 1 · ended");

    // The read that discovered the end sets `closed`; the next 250 ms tick must send nothing.
    const before = reads;
    expect(before).toBeGreaterThan(0);
    await new Promise((resolve) => setTimeout(resolve, 600));
    expect(reads).toBe(before);
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
    callToolMock.mockImplementation(async (_grant, name) => {
      if (name === "terminal.create") return JSON.stringify({ id: "t1" });
      if (name === "terminal.resize") return JSON.stringify({ rows: 24, cols: 80 });
      if (name === "terminal.list") return JSON.stringify([{ id: "t1", kind: "pty" }]);
      if (name === "terminal.read") {
        return JSON.stringify({ data: "", cursor: 0, alive: false, exit_code: null, signal: "SIGTERM" });
      }
      throw new Error(`the test did not expect a ${name} call`);
    });

    render(<TerminalWorkspace grant={GRANT} />);
    await startPtyTab();

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The session ended on signal SIGTERM. The output above is what it printed."
    );
  });

  it("says only that the session ended when the device knows neither fact", async () => {
    callToolMock.mockImplementation(async (_grant, name) => {
      if (name === "terminal.create") return JSON.stringify({ id: "t1" });
      if (name === "terminal.resize") return JSON.stringify({ rows: 24, cols: 80 });
      if (name === "terminal.list") return JSON.stringify([{ id: "t1", kind: "pty" }]);
      if (name === "terminal.read") {
        return JSON.stringify({ data: "", cursor: 0, alive: false, exit_code: null, signal: null });
      }
      throw new Error(`the test did not expect a ${name} call`);
    });

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

  it("disables Find until a session is open", () => {
    render(<TerminalWorkspace grant={GRANT} />);
    expect(screen.getByRole("button", { name: "Find" })).toBeDisabled();
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
      matchBackground: "#3a4f63",
      activeMatchBackground: "#b45309"
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
