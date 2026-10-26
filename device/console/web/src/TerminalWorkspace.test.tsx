import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TerminalWorkspace } from "./TerminalWorkspace";
import { callTool, type Grant } from "./terminal";

vi.mock("@xterm/xterm", () => {
  class Terminal {
    cols = 80;
    rows = 24;
    options: Record<string, unknown> = {};
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
  }
  return { Terminal };
});

vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit = vi.fn();
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

let created = 0;

beforeEach(() => {
  created = 0;
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
});

afterEach(() => {
  if (matchMedia) Object.defineProperty(window, "matchMedia", matchMedia);
  else Reflect.deleteProperty(window, "matchMedia");
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
