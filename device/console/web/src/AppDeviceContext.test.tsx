import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterAll, afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import type { Device } from "./api";
import { resetFleetRecord } from "./useFleetRecord";
import { resetJoinMarks } from "./useJoinMarks";
import { resetMachineDays } from "./useMachineDay";
import { resetMachineRows } from "./useMachineRows";
import { resetLiveToolsCache } from "./useLiveTools";

// Xterm probes optional canvas while importing the real hidden workspace. jsdom has no canvas.
const canvasContext = vi.hoisted(() => {
  const descriptor = Object.getOwnPropertyDescriptor(HTMLCanvasElement.prototype, "getContext")!;
  Object.defineProperty(HTMLCanvasElement.prototype, "getContext", { configurable: true, value: () => null });
  return descriptor;
});
afterAll(() => Object.defineProperty(HTMLCanvasElement.prototype, "getContext", canvasContext));

// App, ScreenPage, BrowserPane, api and callTool remain real. Only HTTP/MCP responses are supplied.
const MACHINE_A: Device = { id: "device-context-a", name: "workshop", online: true, version: "0.3.42", last_seen_ms: 1 };
const MACHINE_B: Device = { id: "device-context-b", name: "attic", online: true, version: "0.3.42", last_seen_ms: 1 };
type ToolRequest = { device_id: string; grant?: unknown; request: { params: { name: string } } };
let calls: ToolRequest[];
let fleetReads: number;

function json(value: unknown) {
  return new Response(JSON.stringify(value), { status: 200, headers: { "content-type": "application/json" } });
}

beforeEach(() => {
  calls = [];
  fleetReads = 0;
  localStorage.clear();
  localStorage.setItem("summrise.selected.device", MACHINE_A.id);
  window.history.replaceState(null, "", "#device/screen/browser");
  resetFleetRecord();
  resetJoinMarks();
  resetMachineDays();
  resetMachineRows();
  resetLiveToolsCache();
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = new URL(String(input), window.location.origin);
    if (url.pathname === "/mcp") {
      const body = JSON.parse(String(init?.body)) as ToolRequest;
      calls.push(body);
      const tool = body.request.params.name;
      const result = tool === "browser.list"
        ? [{ id: `page-${body.device_id}`, title: body.device_id === MACHINE_A.id ? "Workshop page" : "Attic page", url: `https://${body.device_id}.test/`, type: "page" }]
        : tool === "browser.shutdown" ? { stopped: true }
        : tool === "terminal.list" ? [] : null;
      if (result === null) throw new Error(`Unexpected MCP tool: ${tool}`);
      return json({ result: { content: [{ type: "text", text: JSON.stringify(result) }] } });
    }
    if (url.pathname === "/session") return json({ authenticated: true, via: "token" });
    if (url.pathname === "/fleet") {
      fleetReads++;
      return json([{ ...MACHINE_A, last_seen_ms: fleetReads }, { ...MACHINE_B, last_seen_ms: fleetReads }]);
    }
    if (url.pathname === "/health") return json({ version: "0.3.42", devices: 2 });
    if (url.pathname === "/tokens" || url.pathname === "/audit" || url.pathname.endsWith("/connections")) return json([]);
    if (url.pathname === "/audit/day") return json({ client_calls: 0, failed: 0, console: 0, console_failed: 0, poll: 0, poll_failed: 0, unattributed: 0, unattributed_failed: 0 });
    if (url.pathname === "/audit/source") return json({});
    if (url.pathname === "/journal/read") return json({ device: url.searchParams.get("device"), journal: true, oldest_seq: 0, last_seq: 0, rows: [], more: false, read_at_ms: Date.now(), unknown: false });
    if (url.pathname === "/journal/day") return json({ device: url.searchParams.get("device"), journal: true, calls: { console: { calls: 0, failed: 0 }, panel: { calls: 0, failed: 0 }, mesh: { calls: 0, failed: 0 }, loopback: { calls: 0, failed: 0 }, unknown: { calls: 0, failed: 0 } }, read_at_ms: Date.now(), unknown: false });
    throw new Error(`Unexpected HTTP request: ${url.pathname}`);
  }));
});

afterEach(() => {
  vi.unstubAllGlobals();
});

async function stopWorkshopBrowser() {
  const view = render(<App />);
  await screen.findByRole("option", { name: /Workshop page/ });
  fireEvent.click(screen.getByRole("button", { name: "Stop browser" }));
  await screen.findByText(/^The browser is stopped\./);
  expect(calls.filter((call) => call.request.params.name === "browser.shutdown").map((call) => call.device_id)).toEqual([MACHINE_A.id]);
  return view;
}

describe("App machine credentials at the Screen Browser boundary", () => {
  it("never restarts stopped A when the header changes the active Browser machine to B", async () => {
    await stopWorkshopBrowser();
    const cut = calls.length;
    const summary = screen.getByLabelText("Machine: workshop");
    fireEvent.click(summary);
    const picker = summary.closest("details")!;
    fireEvent.click(within(picker).getByRole("button", { name: /^attic/ }));
    await act(async () => {});
    const lists = calls.slice(cut).filter((call) => call.request.params.name === "browser.list");
    expect(lists.map((call) => call.device_id)).toEqual([MACHINE_B.id]);
    expect(lists.every((call) => !("grant" in call))).toBe(true);
    expect(await screen.findByRole("option", { name: /Attic page/ })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: /Workshop page/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Browser" })).toHaveAttribute("aria-pressed", "true");
  });

  it("keeps the stopped browser stopped across a same-machine fleet poll", async () => {
    const intervals = vi.spyOn(window, "setInterval");
    await stopWorkshopBrowser();
    const cut = calls.length;
    const readCount = fleetReads;
    const poll = intervals.mock.calls.find(([, delay]) => delay === 5000);
    expect(poll).toBeDefined();
    await act(async () => { (poll![0] as () => void)(); });
    expect(fleetReads).toBeGreaterThan(readCount);
    expect(calls.slice(cut).filter((call) => call.request.params.name === "browser.list")).toEqual([]);
    expect(screen.getByText(/^The browser is stopped\./)).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: /Workshop page/ })).not.toBeInTheDocument();
    intervals.mockRestore();
  });
});
