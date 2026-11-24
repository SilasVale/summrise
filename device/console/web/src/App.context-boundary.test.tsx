import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { StrictMode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import type { Device } from "./api";
import { resetFleetRecord } from "./useFleetRecord";
import { resetJoinMarks } from "./useJoinMarks";
import { resetMachineDays } from "./useMachineDay";
import { resetMachineRows } from "./useMachineRows";
import { resetLiveToolsCache } from "./useLiveTools";

// App, TerminalWorkspace, api, callTool and all terminal queues/feeds are real. Only xterm's
// renderer and the HTTP boundary are supplied: the assertions inspect real outgoing MCP authority.
const terminals = vi.hoisted(() => ({ instances: [] as {
  output: string[]; input: ((data: string) => void) | null; disposed: boolean;
}[] }));
vi.mock("@xterm/xterm", () => ({ Terminal: class {
  cols = 80; rows = 24; output: string[] = []; input: ((data: string) => void) | null = null;
  disposed = false; options: Record<string, unknown>;
  buffer = { active: { length: 0, cursorX: 0, cursorY: 0, viewportY: 0, baseY: 0 } };
  constructor(options: Record<string, unknown>) { this.options = options; terminals.instances.push(this); }
  open() {} loadAddon() {} focus() {} blur() {} paste() {} clear() {} scrollToBottom() {}
  write(text: string) { this.output.push(text); }
  onData(handler: (data: string) => void) { this.input = handler; return { dispose() {} }; }
  onResize() { return { dispose() {} }; }
  onSelectionChange() { return { dispose() {} }; }
  getSelection() { return ""; }
  attachCustomKeyEventHandler() {}
  dispose() { this.disposed = true; this.input = null; }
} }));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class { fit() {} dispose() {} } }));
vi.mock("@xterm/addon-search", () => ({ SearchAddon: class {
  findNext() { return true; } findPrevious() { return true; } clearDecorations() {} dispose() {}
} }));

const A: Device = { id: "terminal-device-a", name: "workshop", online: true, version: "0.3.42", last_seen_ms: 1 };
const B: Device = { id: "terminal-device-b", name: "attic", online: true, version: "0.3.42", last_seen_ms: 1 };
type Call = { device_id: string; grant?: unknown; request: { params: { name: string; arguments: Record<string, unknown> } } };
let calls: Call[];
let respond: (call: Call) => Promise<Response>;
let listA: unknown[];
let rejectNextARead: boolean;

function json(value: unknown, status = 200) {
  return new Response(JSON.stringify(value), { status, headers: { "content-type": "application/json" } });
}
function tool(value: unknown) {
  return json({ result: { content: [{ type: "text", text: JSON.stringify(value) }] } });
}
function expired() { return json({ error: { code: -32001, message: "The previous grant expired" } }, 403); }
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}
const named = (name: string) => calls.filter((call) => call.request.params.name === name);
const row = (device: Device) => ({ id: "session-1", kind: "pty", goal: { text: device === A ? "A private goal" : "B own goal", by: { kind: "install" }, at_ms: 1 } });
function answer(call: Call): Response {
  const { name, arguments: args } = call.request.params;
  if (name === "terminal.list") return tool(call.device_id === A.id ? listA : [row(B)]);
  if (name === "terminal.read") {
    if (call.device_id === A.id && rejectNextARead) { rejectNextARead = false; return expired(); }
    return tool({ data: args.cursor === 0 ? (call.device_id === A.id ? "A-output\n" : "B-output\n") : "", cursor: 9, alive: true });
  }
  if (name === "terminal.create") return tool({ id: "session-1" });
  if (name === "terminal.resize") return tool({ rows: 24, cols: 80 });
  if (name === "terminal.write") return tool({ written: 1 });
  if (name === "terminal.close") return tool({ closed: true });
  if (name === "terminal.screen") return tool({ data: "", lines: 0 });
  if (name === "runs.get") return tool({ id: args.id, command: "fixture", args: [], cwd: "", exit_code: 0, timed_out: false, duration_ms: 1, started_at_ms: 1, stdout_bytes: 0, stderr_bytes: 0 });
  if (name === "runs.list") return tool([]);
  throw new Error(`Unexpected MCP tool ${name}`);
}

beforeEach(() => {
  calls = []; listA = [row(A)]; rejectNextARead = false; terminals.instances = [];
  respond = async (call) => answer(call);
  localStorage.clear(); localStorage.setItem("summrise.selected.device", A.id);
  window.history.replaceState(null, "", "#device/terminal");
  resetFleetRecord(); resetJoinMarks(); resetMachineDays(); resetMachineRows(); resetLiveToolsCache();
  vi.stubGlobal("EventSource", undefined);
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = new URL(String(input), window.location.origin);
    if (url.pathname === "/mcp") {
      const call = JSON.parse(String(init?.body)) as Call;
      calls.push(call); return respond(call);
    }
    if (url.pathname === "/session") return json({ authenticated: true, via: "token" });
    if (url.pathname === "/fleet") return json([A, B]);
    if (url.pathname === "/health") return json({ version: "0.3.42", devices: 2 });
    if (url.pathname === "/tokens" || url.pathname === "/audit" || url.pathname.endsWith("/connections")) return json([]);
    if (url.pathname === "/audit/day") return json({ client_calls: 0, failed: 0, console: 0, console_failed: 0, poll: 0, poll_failed: 0, unattributed: 0, unattributed_failed: 0 });
    if (url.pathname === "/audit/source") return json({});
    if (url.pathname === "/journal/read") return json({ device: url.searchParams.get("device"), journal: true, oldest_seq: 0, last_seq: 0, rows: [], more: false, read_at_ms: 1, unknown: false });
    if (url.pathname === "/journal/day") return json({ device: url.searchParams.get("device"), journal: true, calls: { console: { calls: 0, failed: 0 }, panel: { calls: 0, failed: 0 }, mesh: { calls: 0, failed: 0 }, loopback: { calls: 0, failed: 0 }, unknown: { calls: 0, failed: 0 } }, read_at_ms: 1, unknown: false });
    throw new Error(`Unexpected HTTP request ${url.pathname}`);
  }));
});
afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); });

async function openA() {
  const view = render(<App />);
  await waitFor(() => expect(terminals.instances[0]?.output).toContain("A-output\n"));
  return view;
}
async function chooseB() {
  const summary = screen.getByLabelText("Machine: workshop");
  fireEvent.click(summary);
  fireEvent.click(within(summary.closest("details")!).getByRole("button", { name: /^attic/ }));
  await waitFor(() => expect(screen.getByLabelText("Machine: attic")).toBeInTheDocument());
  await act(async () => {});
}
async function hash(address: string) {
  await act(async () => {
    window.history.replaceState(null, "", address);
    window.dispatchEvent(new HashChangeEvent("hashchange"));
  });
}
async function complete<T>(pending: ReturnType<typeof deferred<T>>, value: T) {
  await act(async () => { pending.resolve(value); await pending.promise; });
}

// Device session ids are local counters: both fixtures intentionally own a different session-1.
describe("App physical Terminal boundary with colliding session ids", () => {
  it("replaces A's local host, cursor, tab title and find draft when B owns the same session id", async () => {
    const view = await openA();
    const host = view.container.querySelector(".term-host");
    const first = terminals.instances[0];
    fireEvent.click(screen.getByLabelText("Session tools"));
    fireEvent.click(screen.getByRole("button", { name: "Rename tab" }));
    fireEvent.change(screen.getByLabelText("Tab name"), { target: { value: "A private title" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    fireEvent.click(screen.getByRole("button", { name: "Find" }));
    fireEvent.change(screen.getByLabelText("Find in this session"), { target: { value: "A private draft" } });
    await chooseB();
    expect(view.container.querySelector(".term-host")).not.toBe(host);
    expect(first.disposed).toBe(true);
    expect(terminals.instances).toHaveLength(2);
    expect(screen.queryByRole("tab", { name: /A private title/ })).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Find in this session")).not.toBeInTheDocument();
    await waitFor(() => expect(terminals.instances[1].output).toEqual(["B-output\n"]));
    expect(named("terminal.read").find((call) => call.device_id === B.id)?.request.params.arguments).toEqual({ id: "session-1", cursor: 0, wait_ms: 15_000 });
    expect(named("terminal.create")).toHaveLength(0);
    expect(named("terminal.write")).toHaveLength(0);
    expect(named("terminal.close")).toHaveLength(0);
    await act(async () => { terminals.instances[1].input?.("B command\r"); });
    expect(named("terminal.write").map((call) => [call.device_id, call.request.params.arguments])).toEqual([[B.id, { id: "session-1", data: "B command\r" }]]);
  });

  it("does not apply a pending A list to the current B session's goal or available sessions", async () => {
    const pending = deferred<Response>();
    respond = async (call) => call.device_id === A.id && call.request.params.name === "terminal.list" ? pending.promise : answer(call);
    render(<App />);
    await waitFor(() => expect(named("terminal.list").some((call) => call.device_id === A.id)).toBe(true));
    await chooseB();
    await waitFor(() => expect(screen.getByText("B own goal")).toBeInTheDocument());
    await complete(pending, tool([row(A), { id: "A-private-session", kind: "ssh" }]));
    expect(screen.queryByText("A private goal")).not.toBeInTheDocument();
    expect(screen.getByText("B own goal")).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText("Session tools"));
    fireEvent.click(screen.getByRole("button", { name: /^Sessions/ }));
    expect(screen.queryByText("A-private-session")).not.toBeInTheDocument();
    expect(named("terminal.close")).toHaveLength(0);
  });

  it("does not attach a delayed A create result to B after a real device deep link", async () => {
    listA = [];
    const pending = deferred<Response>();
    respond = async (call) => call.device_id === A.id && call.request.params.name === "terminal.create" ? pending.promise : answer(call);
    render(<App />);
    await waitFor(() => expect(named("terminal.list").some((call) => call.device_id === A.id)).toBe(true));
    fireEvent.click(screen.getByRole("button", { name: "New session" }));
    fireEvent.click(within(screen.getByRole("dialog", { name: "New terminal session" })).getByRole("button", { name: "Open" }));
    await waitFor(() => expect(named("terminal.create")).toHaveLength(1));
    await hash("#run/" + B.id + "/fixture-run");
    await hash("#device/terminal");
    await waitFor(() => expect(screen.getByText("B own goal")).toBeInTheDocument());
    await complete(pending, tool({ id: "A-created-session" }));
    expect(screen.queryByRole("tab", { name: /A-created/ })).not.toBeInTheDocument();
    expect(named("terminal.read").filter((call) => call.device_id === B.id).every((call) => call.request.params.arguments.id === "session-1")).toBe(true);
    expect(named("terminal.create").map((call) => call.device_id)).toEqual([A.id]);
    expect(named("terminal.close")).toHaveLength(0);
  });

  it("cancels A's queued input without replaying it under B when an in-flight A write completes", async () => {
    const pending = deferred<Response>();
    respond = async (call) => call.device_id === A.id && call.request.params.name === "terminal.write" ? pending.promise : answer(call);
    await openA();
    await act(async () => { terminals.instances[0].input?.("A sent\r"); terminals.instances[0].input?.("A queued\r"); });
    expect(named("terminal.write").map((call) => call.device_id)).toEqual([A.id]);
    await chooseB();
    await act(async () => { terminals.instances.at(-1)?.input?.("B own\r"); });
    await complete(pending, tool({ written: 7 }));
    expect(named("terminal.write").map((call) => [call.device_id, call.request.params.arguments])).toEqual([
      [A.id, { id: "session-1", data: "A sent\r" }],
      [B.id, { id: "session-1", data: "B own\r" }]
    ]);
    expect(named("terminal.close")).toHaveLength(0);
  });

  it("does not let a pending A read refusal mark B's current credentials stale", async () => {
    const pending = deferred<Response>();
    respond = async (call) => call.device_id === A.id && call.request.params.name === "terminal.read" ? pending.promise : answer(call);
    render(<App />);
    await waitFor(() => expect(named("terminal.read").some((call) => call.device_id === A.id)).toBe(true));
    await chooseB();
    await complete(pending, expired());
    expect(screen.queryByRole("button", { name: "Reconnect" })).not.toBeInTheDocument();
    await waitFor(() => expect(terminals.instances.at(-1)?.output).toContain("B-output\n"));
    expect(named("terminal.close")).toHaveLength(0);
  });

  it("does not let A's pending diagnostic probe mark B's current credentials stale", async () => {
    const pending = deferred<Response>();
    let aLists = 0;
    respond = async (call) => {
      if (call.device_id === A.id && call.request.params.name === "terminal.read") return json({ error: { code: -32000, message: "temporary transport failure" } }, 503);
      if (call.device_id === A.id && call.request.params.name === "terminal.list" && ++aLists > 1) return pending.promise;
      return answer(call);
    };
    render(<App />);
    await waitFor(() => expect(aLists).toBeGreaterThan(1));
    await chooseB();
    await complete(pending, expired());
    expect(screen.queryByRole("button", { name: "Reconnect" })).not.toBeInTheDocument();
    await waitFor(() => expect(terminals.instances.at(-1)?.output).toContain("B-output\n"));
    expect(named("terminal.close")).toHaveLength(0);
  });

  it("restores current-device refusal notification after StrictMode effect replay while ignoring a disposed device's refusal", async () => {
    const intervals = vi.spyOn(window, "setInterval");
    const pending = deferred<Response>();
    let rejectB = false;
    respond = async (call) => {
      if (call.device_id === A.id && call.request.params.name === "terminal.read") return pending.promise;
      if (call.device_id === B.id && call.request.params.name === "terminal.read" && rejectB) {
        rejectB = false;
        return expired();
      }
      return answer(call);
    };
    render(<StrictMode><App /></StrictMode>);
    await waitFor(() => expect(named("terminal.read").some((call) => call.device_id === A.id)).toBe(true));
    await chooseB();
    await waitFor(() => expect(terminals.instances.at(-1)?.output).toContain("B-output\n"));
    await complete(pending, expired());
    expect(screen.queryByRole("button", { name: "Reconnect" })).not.toBeInTheDocument();
    rejectB = true;
    const poll = intervals.mock.calls.filter(([, delay]) => delay === 250).at(-1);
    expect(poll).toBeDefined();
    await act(async () => { (poll![0] as () => void)(); });
    expect(await screen.findByRole("button", { name: "Reconnect" })).toBeInTheDocument();
    expect(named("terminal.close")).toHaveLength(0);
  });

  it("retains the same-device host, find draft and cursor across global routes, fleet refresh and explicit grant renewal", async () => {
    const intervals = vi.spyOn(window, "setInterval");
    const view = await openA();
    const host = view.container.querySelector(".term-host"), first = terminals.instances[0];
    fireEvent.click(screen.getByRole("button", { name: "Find" }));
    fireEvent.change(screen.getByLabelText("Find in this session"), { target: { value: "kept on A" } });
    await hash("#home");
    expect(view.container.querySelector(".term-host")).toBe(host);
    expect(view.container.querySelector(".terminal-page")).toHaveAttribute("hidden");
    await hash("#machines");
    expect(view.container.querySelector(".term-host")).toBe(host);
    await hash("#device/terminal");
    expect(screen.getByLabelText("Find in this session")).toHaveValue("kept on A");
    const fleet = intervals.mock.calls.find(([, delay]) => delay === 5000);
    expect(fleet).toBeDefined();
    await act(async () => { (fleet![0] as () => void)(); });
    expect(view.container.querySelector(".term-host")).toBe(host);
    rejectNextARead = true;
    const poll = intervals.mock.calls.filter(([, delay]) => delay === 250).at(-1);
    expect(poll).toBeDefined();
    await act(async () => { (poll![0] as () => void)(); });
    fireEvent.click(await screen.findByRole("button", { name: "Reconnect" }));
    expect(view.container.querySelector(".term-host")).toBe(host);
    expect(terminals.instances).toHaveLength(1);
    expect(first.disposed).toBe(false);
    expect(screen.getByLabelText("Find in this session")).toHaveValue("kept on A");
    expect(named("terminal.read").slice(1).every((call) => call.request.params.arguments.cursor === 9)).toBe(true);
    expect(named("terminal.create")).toHaveLength(0);
    expect(named("terminal.close")).toHaveLength(0);
  });
});
