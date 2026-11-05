import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { HttpError } from "./api";
import { VITALS_KEY_PREFIX, type VitalsSample } from "./vitals";
import { resetLiveToolsCache } from "./useLiveTools";

// The shell is driven against a mocked HTTP surface: no request leaves jsdom, and the sign-in state is a
// flag here rather than a server. `api.signIn` flips it, so the following `api.session` reports the
// session the real console would have created.
const mocks = vi.hoisted(() => ({
  session: vi.fn(),
  health: vi.fn(),
  devices: vi.fn(),
  audit: vi.fn(),
  denials: vi.fn(),
  connections: vi.fn(),
  signIn: vi.fn(),
  logout: vi.fn(),
  enroll: vi.fn(),
  rename: vi.fn(),
  remove: vi.fn(),
  tools: vi.fn(),
  liveTools: vi.fn(),
  grant: vi.fn(),
  tokens: vi.fn(),
  createToken: vi.fn(),
  revokeToken: vi.fn(),
  meshReadiness: vi.fn(),
}));

vi.mock("./api", () => ({
  api: {
    session: mocks.session,
    health: mocks.health,
    devices: mocks.devices,
    audit: mocks.audit,
    denials: mocks.denials,
    connections: mocks.connections,
    signIn: mocks.signIn,
    logout: mocks.logout,
    enroll: mocks.enroll,
    rename: mocks.rename,
    remove: mocks.remove,
    tools: mocks.tools,
    liveTools: mocks.liveTools,
    grant: mocks.grant,
    tokens: mocks.tokens,
    createToken: mocks.createToken,
    revokeToken: mocks.revokeToken
  },
  meshReadiness: mocks.meshReadiness,
  HttpError: class HttpError extends Error {
    constructor(
      readonly status: number,
      readonly code: string
    ) {
      super(code);
    }
  },
  NoAnswer: class NoAnswer extends Error {}
}));

// The workspace owns xterm and the tabs, and this file is about the shell around it. The stub is a
// marker, so "a page change did not remount it" is a node-identity assertion rather than a guess;
// `visible` is the prop App uses to hide it on every tab that is not the terminal.
vi.mock("./TerminalWorkspace", () => ({
  TerminalWorkspace: ({ visible }: { visible: boolean }) => (
    <div data-testid="workspace" data-visible={visible ? "yes" : "no"} />
  )
}));

// The Tools tab reads the plugin list through the device's own `plugins.list`; that transport is
// not this file's subject. The list it answers with is.
const pluginMocks = vi.hoisted(() => ({ list: vi.fn() }));
vi.mock("./pluginApi", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./pluginApi")>();
  return { ...actual, listPlugins: pluginMocks.list };
});

let signedIn = false;

const workshop = {
  id: "dev-workshop-0001",
  name: "workshop",
  online: true,
  version: "0.3.42",
  last_seen_ms: Date.now() - 15_000
};
const attic = {
  id: "dev-attic-0002",
  name: "attic",
  online: true,
  version: "0.3.42",
  last_seen_ms: Date.now() - 30_000
};

// One permanent token as `GET /tokens` lists it (no value), and the audit line that names its id.
const tokenId = "a".repeat(64);
const shortTokenId = "a".repeat(12) + "…";
const laptop = {
  id: tokenId,
  name: "laptop-dsh",
  device_id: workshop.id,
  tools: ["tools/list"],
  allow_destructive: false,
  created_at_ms: 1
};
const tokenCall = {
  ts_ms: 1,
  kind: "mcp",
  device_id: workshop.id,
  tool: "tools/list",
  detail: null,
  status: 200,
  token_id: tokenId
};

/** One built-in and the plugin tool that makes the plugin section supported. */
const filesTool = {
  name: "files.read",
  description: "read a file",
  inputSchema: {
    type: "object",
    properties: { path: { type: "string" } },
    required: ["path"]
  },
  source: "builtin",
  capability: "files",
  destructive: false
};
const pluginTool = {
  name: "plugins.list",
  description: "list the plugins",
  inputSchema: { type: "object", properties: {} },
  source: "builtin",
  capability: "plugins",
  destructive: false
};
const helloPlugin = {
  id: "com.example.hello",
  namespace: "hello",
  name: "Hello",
  version: "0.1.0",
  description: "Two friendly tools",
  state: "running",
  tools: 2,
  path: "C:\\dev\\plugins\\hello",
  last_error: null
};

/**
 * The browser's own `EventSource`, replaced with one the test drives.
 *
 * The live stream is a real connection in production and an `EventSource` the page never sees in
 * jsdom; without a stand-in `useLiveActivity` reports "offline" and a test cannot deliver a line.
 */
class TestEventSource {
  static instances: TestEventSource[] = [];
  listeners = new Map<string, (event: MessageEvent) => void>();
  closed = false;
  onerror: ((event: Event) => void) | null = null;
  constructor(public url: string) {
    TestEventSource.instances.push(this);
  }
  addEventListener(type: string, listener: (event: MessageEvent) => void) {
    this.listeners.set(type, listener);
  }
  close() {
    this.closed = true;
  }
  emit(type: string, data: unknown) {
    this.listeners.get(type)?.({ data: JSON.stringify(data) } as MessageEvent);
  }
}

beforeEach(() => {
  localStorage.clear();
  // The catalogue cache is module-level and survives a test; a test that sets the fixture must not
  // read the previous one's empty list.
  resetLiveToolsCache();
  TestEventSource.instances = [];
  vi.stubGlobal("EventSource", TestEventSource);
  // The fragment is the page's address; a test that leaves Device open must not decide where the
  // next one boots.
  window.history.replaceState(null, "", window.location.pathname);
  signedIn = false;
  mocks.session.mockImplementation(async () =>
    signedIn
      ? { authenticated: true, via: "token" }
      : { authenticated: false, access: { configured: false, header: false } }
  );
  mocks.health.mockResolvedValue({ version: "0.3.42", devices: 2 });
  mocks.devices.mockResolvedValue([workshop, attic]);
  mocks.audit.mockResolvedValue([]);
  mocks.denials.mockResolvedValue([]);
  mocks.connections.mockResolvedValue([]);
  mocks.signIn.mockImplementation(async () => {
    signedIn = true;
    return { token: "fresh-token", expires_at_ms: Date.now() + 3_600_000 };
  });
  mocks.logout.mockResolvedValue(undefined);
  mocks.enroll.mockResolvedValue({ code: "code", expires_at_ms: Date.now() + 60_000 });
  mocks.rename.mockResolvedValue({ id: workshop.id, name: workshop.name });
  mocks.remove.mockResolvedValue(undefined);
  mocks.grant.mockResolvedValue({ grant: {}, signature: "signature" });
  mocks.tokens.mockResolvedValue([]);
  mocks.createToken.mockResolvedValue({ id: "tok-1", token: "srm_" + "a".repeat(64) });
  mocks.revokeToken.mockResolvedValue(undefined);
  // The Connect sheet asks the client for the tool registry; that request is not this test's subject,
  // so the client's answer fails and the sheet keeps its fallback list.
  mocks.tools.mockRejectedValue(new TypeError("Failed to fetch"));
  // The hub reads the selected machine's live catalogue. An empty list is a real answer; these tests
  // are about the shell, not about what the device advertises.
  mocks.liveTools.mockResolvedValue({ tools: [] });
  pluginMocks.list.mockReset();
  pluginMocks.list.mockResolvedValue([]);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

/** Render the page and wait for the signed-out boot to settle on the login form. */
async function boot() {
  const view = render(<App />);
  await screen.findByRole("heading", { name: "Sign in to the console" });
  return view;
}

/** Boot signed out, sign in with the operator token, and wait for the fleet on Home. */
async function bootAndSignIn() {
  const view = await boot();
  fireEvent.change(screen.getByLabelText("Operator token"), { target: { value: "operator-token" } });
  fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
  await screen.findByRole("heading", { name: "Machines", level: 1 });
  await screen.findByText("2 machines · 2 online");
  return view;
}

/** A promise whose settlement the test decides, for asserting the state in between. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/** Select a machine from its fleet card; Home stays Home, as the hub's first screen. */
function selectDevice(name: string) {
  const label = screen.getAllByText(name).find((node) => node.closest("article"));
  const card = label?.closest("article") ?? null;
  expect(card).not.toBeNull();
  fireEvent.click(within(card as HTMLElement).getByRole("button", { name: new RegExp(`^Open `) }));
}

/** Open the account menu, where version, console URL, docs and sign out live now. */
function openAccount() {
  fireEvent.click(screen.getByLabelText("Account menu"));
}

describe("App", () => {
  it("boots signed out, signs in with the operator token, and renders the fleet on Home", async () => {
    const { container } = await boot();

    // The title is an effect's write, not part of the render the boot waited for: wait for it.
    await waitFor(() => expect(document.title).toBe("Sign in · Summrise console"));
    const token = screen.getByLabelText("Operator token");
    expect(token).toHaveAttribute("type", "password");
    expect(screen.getByRole("button", { name: "Sign in" })).toBeDisabled();

    fireEvent.change(token, { target: { value: "operator-token" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));

    await waitFor(() => expect(mocks.signIn).toHaveBeenCalledWith({ token: "operator-token" }));
    // The session is the console's cookie, set by the answer; the page keeps nothing and learns it
    // is signed in by re-reading /session.

    expect((await screen.findAllByText("workshop")).length).toBeGreaterThan(0);
    expect(screen.getAllByText("attic").length).toBeGreaterThan(0);
    expect(screen.getByRole("link", { name: "Skip to content" })).toHaveAttribute("href", "#main");
    expect(screen.getByRole("main")).toHaveAttribute("id", "main");
    // THE STATUS STRIP IS DELETED. The fleet count is Home's job now, and there is no bar below the
    // canvas repeating facts that already have a home.
    expect(container.querySelector(".statusbar")).toBeNull();
    expect(screen.getByText("2 machines · 2 online")).toBeInTheDocument();
    // The tab title and the address bar are both effects over the route the page has just drawn.
    await waitFor(() => expect(document.title).toBe("Machines · Summrise console"));
    await waitFor(() => expect(window.location.hash).toBe("#home"));
  });

  it("opens the Device surface named in the URL after sign-in, through the legacy alias", async () => {
    window.history.replaceState(null, "", "#files");
    await boot();
    fireEvent.change(screen.getByLabelText("Operator token"), { target: { value: "operator-token" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));

    // Three seconds, not the default one: the suite runs 42 files across workers, and this test waits
    // for a whole sign-in — session, fleet, audit and tokens — before the surface named in the URL
    // can render. It flaked once in a full run and never alone; a load-induced timeout is the shape.
    expect(
      await screen.findByRole("heading", { name: "Files", level: 1 }, { timeout: 3000 })
    ).toBeInTheDocument();
    // The address bar is rewritten by an effect after the route renders; a synchronous read raced it.
    await waitFor(() => expect(window.location.hash).toBe("#device/files"));
    window.history.replaceState(null, "", window.location.pathname);
  });

  it("opens Runs on boot when the URL names a run", async () => {
    window.history.replaceState(null, "", "#run/workshop/r-1");
    await boot();
    fireEvent.change(screen.getByLabelText("Operator token"), { target: { value: "operator-token" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));

    expect(
      await screen.findByRole("heading", { name: "Runs", level: 1 }, { timeout: 3000 })
    ).toBeInTheDocument();
    await waitFor(() => expect(window.location.hash).toBe("#run/workshop/r-1"));
    window.history.replaceState(null, "", window.location.pathname);
  });

  it("asks the audit for the console's cap, so a full day is the day", async () => {
    await bootAndSignIn();
    // MAX_LOG_LIMIT in device/console/src/http.rs. A shorter window made "Today" mean "the newest
    // slice"; P2-12 raised the fetch to the cap before measuring what 1000 rows cost.
    expect(mocks.audit).toHaveBeenCalledWith(1000);
  });

  it("states the day's count in the Activity header, and says whose failures it counted", async () => {
    // Six rows: two client-proven (one by its caller, one only by its token), one console read and
    // one unattributed row, each with a failure beside it. The header leads with the AI's failure;
    // the console's is named apart, because "failed" beside "client calls" must be the client's —
    // the measured header counted all 55 and read as a failure rate for the AI that was 18 rows
    // wrong.
    const midnight = new Date();
    midnight.setHours(0, 0, 0, 0);
    const at = (seconds: number) => midnight.getTime() + seconds * 1_000;
    mocks.audit.mockResolvedValue([
      {
        ts_ms: at(60),
        kind: "mcp",
        device_id: workshop.id,
        token_id: null,
        tool: "files.read",
        detail: null,
        status: 200,
        caller: "client",
        sentence: "Read a file"
      },
      {
        ts_ms: at(50),
        kind: "mcp",
        device_id: workshop.id,
        token_id: tokenId,
        tool: "runs.exec",
        detail: "run r-1 exited 1 in 42 ms",
        status: 424,
        sentence: "Ran a command"
      },
      {
        ts_ms: at(40),
        kind: "mcp",
        device_id: workshop.id,
        token_id: null,
        tool: "terminal.list",
        detail: null,
        status: 200,
        caller: "console",
        sentence: "Listed the terminal sessions"
      },
      {
        ts_ms: at(30),
        kind: "mcp",
        device_id: workshop.id,
        token_id: null,
        tool: "terminal.read",
        detail: "terminal disconnected",
        status: 424,
        caller: "console",
        sentence: "Read the terminal"
      },
      {
        ts_ms: at(20),
        kind: "mcp",
        device_id: workshop.id,
        token_id: null,
        tool: "files.write",
        detail: null,
        status: 200,
        sentence: "Wrote a file"
      },
      {
        ts_ms: at(10),
        kind: "mcp",
        device_id: workshop.id,
        token_id: null,
        tool: "files.move",
        detail: "jsonrpc error -32002",
        status: 200,
        sentence: "Moved a file"
      }
    ]);
    await bootAndSignIn();
    fireEvent.click(screen.getByRole("button", { name: "Activity" }));

    expect(
      await screen.findByText(
        "Today 2 client calls · 1 failed · 1 console failure · 2 unattributed (1 failed)"
      )
    ).toBeInTheDocument();

    // Activity is the audit: every caller is drawn by default, and the hub's console-reads fold is
    // not offered here — its Clients only chip is the way to narrow, and only when chosen.
    await waitFor(() => expect(document.querySelectorAll(".timeline-row")).toHaveLength(6));
    expect(screen.queryByRole("button", { name: /^Console reads ·/ })).toBeNull();
    expect(screen.getByRole("button", { name: "Clients only · 2" })).toHaveAttribute(
      "aria-pressed",
      "false"
    );

    // The chip draws the client subset; the header then reads the same subset the rows show — the
    // hub's sentence, with no console failure and no unknown caller in it.
    fireEvent.click(screen.getByRole("button", { name: "Clients only · 2" }));
    expect(await screen.findByText("Today 2 client calls · 1 failed")).toBeInTheDocument();
    await waitFor(() => expect(document.querySelectorAll(".timeline-row")).toHaveLength(2));
    expect(screen.getByText("Read a file")).toBeInTheDocument();
    expect(screen.getByText("Ran a command")).toBeInTheDocument();
    expect(screen.queryByText("Listed the terminal sessions")).toBeNull();
    expect(screen.queryByText("Moved a file")).toBeNull();
  });

  it("follows the fragment when the address bar changes, not only at first render", async () => {
    await bootAndSignIn();
    expect(screen.getByRole("heading", { name: "Machines", level: 1 })).toBeInTheDocument();

    // A hash change is what Back, Forward and a pasted link produce. Nothing fired a click here.
    window.history.replaceState(null, "", "#device/files");
    window.dispatchEvent(new Event("hashchange"));
    expect(await screen.findByRole("heading", { name: "Files", level: 1 })).toBeInTheDocument();

    window.history.replaceState(null, "", "#activity");
    window.dispatchEvent(new Event("hashchange"));
    expect(await screen.findByRole("heading", { name: "Activity", level: 1 })).toBeInTheDocument();

    // A fragment the app cannot read leaves the view alone rather than guessing at it.
    window.history.replaceState(null, "", "#not-a-page");
    window.dispatchEvent(new Event("hashchange"));
    expect(screen.getByRole("heading", { name: "Activity", level: 1 })).toBeInTheDocument();
  });

  it("names the current destination in the tab title", async () => {
    await bootAndSignIn();
    await waitFor(() => expect(document.title).toBe("Machines · Summrise console"));

    fireEvent.click(screen.getByRole("button", { name: "Activity" }));
    await screen.findByRole("heading", { name: "Activity", level: 1 });
    await waitFor(() => expect(document.title).toBe("Activity · Summrise console"));

    fireEvent.click(screen.getByRole("button", { name: "Device" }));
    await waitFor(() => expect(document.title).toBe("Terminal · Summrise console"));

    fireEvent.click(screen.getByRole("button", { name: "Tools" }));
    await waitFor(() => expect(document.title).toBe("Tools · Summrise console"));
  });

  it("renders the whole shell: three destinations, one h1, the account menu and the build", async () => {
    const { container } = await bootAndSignIn();

    const rail = screen.getByRole("navigation", { name: "Destinations" });
    const sections = Array.from(container.querySelectorAll<HTMLButtonElement>(".rail-btn"));
    expect(sections.map((section) => section.textContent)).toEqual([
      "Home",
      "Activity",
      "Device"
    ]);
    // The two retired destinations are a tab inside Device now, not rail seats of their own.
    expect(within(rail).queryByRole("button", { name: "Capabilities" })).toBeNull();
    expect(within(rail).queryByRole("button", { name: "Plugins" })).toBeNull();
    expect(within(rail).getByRole("button", { name: "Home" })).toHaveAttribute(
      "aria-current",
      "page"
    );
    expect(screen.getAllByRole("heading", { level: 1 })).toHaveLength(1);
    expect(screen.getByRole("heading", { name: "Machines", level: 1 })).toBeInTheDocument();
    expect(screen.getByLabelText("All machines")).toBeInTheDocument();

    openAccount();
    expect(screen.getByText("0.3.42")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sign out" })).toBeInTheDocument();
    // No standalone Devices or Connect destination survives the fold: the nav's own items are the
    // three, and Connect is a verb in the account menu, not a rail seat.
    const items = container.querySelectorAll<HTMLButtonElement>(".rail-items .rail-btn");
    expect(Array.from(items).map((item) => item.textContent)).not.toContain("Devices");
    expect(Array.from(items).map((item) => item.textContent)).not.toContain("Connect");
  });

  it("lists the machines and Add a machine in the one header control", async () => {
    await bootAndSignIn();

    fireEvent.click(screen.getByLabelText("All machines"));
    const menu = screen.getByLabelText("Machines");
    expect(within(menu).getByRole("button", { name: "All machines" })).toBeInTheDocument();
    expect(within(menu).getByRole("button", { name: /workshop/ })).toBeInTheDocument();
    expect(within(menu).getByRole("button", { name: /attic/ })).toBeInTheDocument();
    fireEvent.click(within(menu).getByRole("button", { name: "Add a machine" }));
    await waitFor(() => expect(mocks.enroll).toHaveBeenCalledTimes(1));
  });

  it("opens a tool's sheet from a capabilities deep link, inside the Tools tab", async () => {
    window.history.replaceState(null, "", "#capabilities/files.read");
    // A device-specific catalogue needs a machine; the selection survives a reload.
    localStorage.setItem("summrise.selected.device", workshop.id);
    mocks.liveTools.mockResolvedValue({
      tools: [filesTool]
    });
    await boot();
    fireEvent.change(screen.getByLabelText("Operator token"), { target: { value: "operator-token" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));

    expect(await screen.findByRole("heading", { name: "Tools", level: 1 })).toBeInTheDocument();
    expect(await screen.findByRole("dialog", { name: "files.read" })).toBeInTheDocument();
    // The old fragment keeps working, and the address bar is rewritten as the tab's own link.
    await waitFor(() => expect(window.location.hash).toBe("#device/tools/files.read"));
    window.history.replaceState(null, "", window.location.pathname);
  });

  it("opens a tool's sheet when the fragment changes after sign-in", async () => {
    mocks.liveTools.mockResolvedValue({
      tools: [filesTool]
    });
    await bootAndSignIn();
    // A catalogue belongs to a machine, so pick one the way a person does.
    fireEvent.click(screen.getAllByRole("button", { name: /workshop/ })[0]);
    await screen.findByRole("heading", { name: "Home", level: 1 });

    // The late path: no reload, no click — the fragment changes on a page that is already mounted.
    window.history.replaceState(null, "", "#capabilities/files.read");
    window.dispatchEvent(new Event("hashchange"));

    expect(await screen.findByRole("heading", { name: "Tools", level: 1 })).toBeInTheDocument();
    expect(await screen.findByRole("dialog", { name: "files.read" })).toBeInTheDocument();
    await waitFor(() => expect(window.location.hash).toBe("#device/tools/files.read"));
    window.history.replaceState(null, "", window.location.pathname);
  });

  it("says so when the URL names a tool the machine does not have", async () => {
    window.history.replaceState(null, "", "#capabilities/not-a-tool");
    localStorage.setItem("summrise.selected.device", workshop.id);
    mocks.liveTools.mockResolvedValue({
      tools: [filesTool]
    });
    await boot();
    fireEvent.change(screen.getByLabelText("Operator token"), { target: { value: "operator-token" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));

    expect(await screen.findByRole("heading", { name: "Tools", level: 1 })).toBeInTheDocument();
    // The link is stale: the machine has no such tool. Silence would read as "the console ignored
    // me", so the page says which fact it is.
    expect(await screen.findByText("That tool is not on this machine.")).toBeInTheDocument();
    await waitFor(() => expect(window.location.hash).toBe("#device/tools/not-a-tool"));
    window.history.replaceState(null, "", window.location.pathname);
  });

  it("scopes Activity to the chosen machine", async () => {
    mocks.audit.mockResolvedValue([
      {
        ts_ms: Date.now(),
        kind: "tool",
        device_id: workshop.id,
        token_id: null,
        tool: "terminal.create",
        detail: null,
        status: 200
      }
    ]);
    await bootAndSignIn();
    selectDevice("workshop");
    fireEvent.click(screen.getByRole("button", { name: "Activity" }));
    // Activity opens on the timeline; the machine scope is the table's, one tab away.
    fireEvent.click(screen.getByRole("tab", { name: "Table" }));

    const machine = await screen.findByRole("button", { name: "This machine" });
    expect(machine).toBeEnabled();
    expect(machine).toHaveAttribute("aria-pressed", "true");
    // The header's machine menu also carries an All machines item; the Activity scope is the one
    // with a pressed state.
    const all = screen
      .getAllByRole("button", { name: "All machines" })
      .find((button) => button.hasAttribute("aria-pressed"));
    expect(all).toHaveAttribute("aria-pressed", "false");

    // The timeline cannot draw the table's scope toggle; the chip for the chosen machine is what
    // carries the same filter back to it.
    fireEvent.click(screen.getByRole("tab", { name: "Timeline" }));
    const chips = screen.getByRole("group", { name: "Filter by machine" });
    expect(within(chips).getByRole("button", { name: "workshop" })).toHaveAttribute(
      "aria-pressed",
      "true"
    );
  });

  it("reads the scope's machine window and widens to the fleet in one request", async () => {
    await bootAndSignIn();
    fireEvent.click(screen.getByRole("button", { name: "Activity" }));
    await screen.findByRole("heading", { name: "Activity", level: 1 });

    // Select the header's machine from Activity itself. The hub is not mounted here, so the read
    // that names it can only be the Activity page's own window.
    fireEvent.click(screen.getByLabelText("All machines"));
    const menu = screen.getByLabelText("Machines");
    fireEvent.click(within(menu).getByRole("button", { name: /workshop/ }));
    await waitFor(() => expect(mocks.audit).toHaveBeenLastCalledWith(1000, workshop.id));

    fireEvent.click(screen.getByRole("tab", { name: "Table" }));
    const scope = screen.getByRole("group", { name: "Audit scope" });
    const readsBefore = mocks.audit.mock.calls.length;
    fireEvent.click(within(scope).getByRole("button", { name: "All machines" }));
    await waitFor(() => expect(mocks.audit).toHaveBeenCalledTimes(readsBefore + 1));
    // One request for the switch: the fleet read, not the machine read plus a fleet read.
    expect(mocks.audit).toHaveBeenLastCalledWith(1000);
  });

  it("names the machine whose window was cut, and the fleet when the window is the fleet's", async () => {
    // More than the page's 1000-line cap, so the shown window is cut and must say so.
    const window = Array.from({ length: 1001 }, (_, index) => ({
      ...tokenCall,
      ts_ms: Date.now() - index * 1000,
      tool: `tool.${index}`,
      sentence: `Call ${index}`
    }));
    mocks.audit.mockResolvedValue(window);
    await bootAndSignIn();
    selectDevice("workshop");
    fireEvent.click(screen.getByRole("button", { name: "Activity" }));

    expect(
      await screen.findByText(
        "Showing the newest 1000 lines from workshop; older ones are not on this page."
      )
    ).toBeInTheDocument();

    // The table reads the same fact: its notice names the same window.
    fireEvent.click(screen.getByRole("tab", { name: "Table" }));
    expect(
      screen.getByText(
        "Showing the newest 1000 lines from workshop; older ones are not on this page."
      )
    ).toBeInTheDocument();

    const scope = screen.getByRole("group", { name: "Audit scope" });
    fireEvent.click(within(scope).getByRole("button", { name: "All machines" }));
    expect(
      await screen.findByText(
        "Showing the newest 1000 lines; older ones are not on this page."
      )
    ).toBeInTheDocument();
    expect(screen.queryByText(/from workshop/)).toBeNull();
  });

  it("says the window's limit at exactly 1000 lines, and not at 999", async () => {
    // The live read returned exactly the requested window. `capped` is false because the page
    // dropped nothing; the sentence still has to name the limit, in both views.
    const full = Array.from({ length: 1000 }, (_, index) => ({
      ...tokenCall,
      ts_ms: Date.now() - index * 1000,
      tool: `tool.${index}`,
      sentence: `Call ${index}`
    }));
    mocks.audit.mockResolvedValue(full);
    await bootAndSignIn();
    fireEvent.click(screen.getByRole("button", { name: "Activity" }));

    expect(
      await screen.findByText("Showing the newest 1000 lines — the window's limit.")
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Table" }));
    expect(
      screen.getByText("Showing the newest 1000 lines — the window's limit.")
    ).toBeInTheDocument();

    // One line fewer is not the limit: the same page must not keep claiming it.
    mocks.audit.mockResolvedValue(full.slice(0, 999));
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() =>
      expect(
        screen.queryByText("Showing the newest 1000 lines — the window's limit.")
      ).toBeNull()
    );
  });

  it("reads a chip's own window instead of filtering another machine's, and says it is reading", async () => {
    const atticLine = {
      ...tokenCall,
      ts_ms: Date.now() - 2_000,
      device_id: attic.id,
      token_id: null,
      tool: "files.list",
      sentence: "Listed the attic"
    };
    const workshopLine = {
      ...tokenCall,
      ts_ms: Date.now(),
      tool: "files.read",
      sentence: "Read the workshop's notes"
    };
    // The fleet window names only the attic; the chosen machine's window holds its own line. The
    // workshop row's appearance therefore proves the machine read landed. The attic still has a
    // chip: a chip is a scope, not only a filter over the lines in hand.
    mocks.audit.mockImplementation(async (_limit: number, device?: string) =>
      device === workshop.id ? [workshopLine] : [atticLine]
    );
    await bootAndSignIn();
    selectDevice("workshop");
    fireEvent.click(screen.getByRole("button", { name: "Activity" }));
    expect(await screen.findByText("Read the workshop's notes")).toBeInTheDocument();

    const answer = deferred<unknown[]>();
    mocks.audit.mockReturnValueOnce(answer.promise);
    const chips = screen.getByRole("group", { name: "Filter by machine" });
    const readsBefore = mocks.audit.mock.calls.length;
    fireEvent.click(within(chips).getByRole("button", { name: "attic" }));

    // The chip is a scope: it asks the console for the attic's window rather than filtering the
    // workshop's window in the page.
    await waitFor(() => expect(mocks.audit).toHaveBeenCalledTimes(readsBefore + 1));
    expect(mocks.audit).toHaveBeenLastCalledWith(1000, attic.id);
    // Meanwhile the rows in hand belong to the workshop; drawing them under the attic chip would be
    // an answer from the wrong window.
    expect(screen.getByText("Reading attic's window…")).toBeInTheDocument();
    expect(screen.queryByText("Read the workshop's notes")).toBeNull();

    await act(async () => {
      answer.resolve([{ ...atticLine, ts_ms: Date.now() }]);
    });
    expect(await screen.findByText("Listed the attic")).toBeInTheDocument();
    expect(screen.queryByText("Reading attic's window…")).toBeNull();
  });

  it("reads the machine's window on the page's read and after a dropped frame", async () => {
    mocks.audit.mockResolvedValue([
      { ...tokenCall, ts_ms: Date.now(), sentence: "A call on the chosen machine" }
    ]);
    await bootAndSignIn();
    selectDevice("workshop");
    fireEvent.click(screen.getByRole("button", { name: "Activity" }));
    await screen.findByRole("heading", { name: "Activity", level: 1 });
    const source = TestEventSource.instances[TestEventSource.instances.length - 1];
    act(() => {
      source.emit("snapshot", { lines: [] });
    });

    // Refresh and the five-second tick are the same `load`; both read the scope's window.
    const beforeRefresh = mocks.audit.mock.calls.length;
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() => expect(mocks.audit).toHaveBeenCalledTimes(beforeRefresh + 1));
    expect(mocks.audit).toHaveBeenLastCalledWith(1000, workshop.id);

    // The lag reread goes through the same decision: the hole is in this machine's window.
    const beforeLag = mocks.audit.mock.calls.length;
    act(() => {
      source.emit("lagged", { dropped: 2 });
    });
    await waitFor(() => expect(mocks.audit).toHaveBeenCalledTimes(beforeLag + 1));
    expect(mocks.audit).toHaveBeenLastCalledWith(1000, workshop.id);
  });

  it("gives the timeline chips and a search, and keeps the six filters on the table", async () => {
    mocks.tokens.mockResolvedValue([laptop]);
    mocks.audit.mockResolvedValue([
      { ...tokenCall, ts_ms: Date.now(), sentence: "Read the workshop's notes" },
      {
        ...tokenCall,
        ts_ms: Date.now() - 1_000,
        device_id: attic.id,
        token_id: null,
        tool: "files.list",
        sentence: "Listed the attic"
      }
    ]);
    await bootAndSignIn();
    fireEvent.click(screen.getByRole("button", { name: "Activity" }));
    await screen.findByRole("heading", { name: "Activity", level: 1 });

    // The timeline: one search and one chip per machine in the lines, All machines included.
    expect(screen.getByRole("searchbox", { name: "Search activity" })).toBeInTheDocument();
    const chips = screen.getByRole("group", { name: "Filter by machine" });
    expect(within(chips).getByRole("button", { name: "workshop" })).toBeInTheDocument();
    expect(within(chips).getByRole("button", { name: "attic" })).toBeInTheDocument();
    expect(within(chips).getByRole("button", { name: "All machines" })).toBeInTheDocument();

    // The table's own controls are not on this view.
    expect(screen.queryByRole("group", { name: "Audit scope" })).toBeNull();
    expect(screen.queryByLabelText("Time")).toBeNull();
    expect(screen.queryByLabelText("Device")).toBeNull();
    expect(screen.queryByLabelText("Token")).toBeNull();
    expect(screen.queryByLabelText("Status")).toBeNull();

    // The table still has all six.
    fireEvent.click(screen.getByRole("tab", { name: "Table" }));
    expect(screen.getByRole("group", { name: "Audit scope" })).toBeInTheDocument();
    expect(screen.getByLabelText("Time")).toBeInTheDocument();
    expect(screen.getByLabelText("Device")).toBeInTheDocument();
    expect(screen.getByLabelText("Token")).toBeInTheDocument();
    expect(screen.getByLabelText("Tool or detail")).toBeInTheDocument();
    expect(screen.getByLabelText("Status")).toBeInTheDocument();
  });

  it("carries the timeline's search to the table and back", async () => {
    mocks.audit.mockResolvedValue([{ ...tokenCall, ts_ms: Date.now() }]);
    await bootAndSignIn();
    fireEvent.click(screen.getByRole("button", { name: "Activity" }));
    await screen.findByRole("heading", { name: "Activity", level: 1 });

    fireEvent.change(screen.getByRole("searchbox", { name: "Search activity" }), {
      target: { value: "tools/list" }
    });
    expect(screen.getByText("tools/list")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("tab", { name: "Table" }));
    expect(screen.getByLabelText("Tool or detail")).toHaveValue("tools/list");

    fireEvent.change(screen.getByLabelText("Tool or detail"), {
      target: { value: "files.read" }
    });
    fireEvent.click(screen.getByRole("tab", { name: "Timeline" }));
    expect(screen.getByRole("searchbox", { name: "Search activity" })).toHaveValue("files.read");
  });

  it("reflects the table's device filter in the timeline's machine chip", async () => {
    mocks.audit.mockResolvedValue([
      { ...tokenCall, ts_ms: Date.now(), sentence: "Read the workshop's notes" },
      {
        ...tokenCall,
        ts_ms: Date.now() - 1_000,
        device_id: attic.id,
        token_id: null,
        tool: "files.list",
        sentence: "Listed the attic"
      }
    ]);
    await bootAndSignIn();
    fireEvent.click(screen.getByRole("button", { name: "Activity" }));
    await screen.findByRole("heading", { name: "Activity", level: 1 });

    fireEvent.click(screen.getByRole("tab", { name: "Table" }));
    fireEvent.change(screen.getByLabelText("Device"), { target: { value: attic.id } });
    fireEvent.click(screen.getByRole("tab", { name: "Timeline" }));

    const chips = screen.getByRole("group", { name: "Filter by machine" });
    expect(within(chips).getByRole("button", { name: "attic" })).toHaveAttribute(
      "aria-pressed",
      "true"
    );
    expect(screen.getByText("Listed the attic")).toBeInTheDocument();
    expect(screen.queryByText("Read the workshop's notes")).not.toBeInTheDocument();

    // And the other way: a chip is the table's device filter when the views change back.
    fireEvent.click(within(chips).getByRole("button", { name: "workshop" }));
    fireEvent.click(screen.getByRole("tab", { name: "Table" }));
    expect(screen.getByLabelText("Device")).toHaveValue(workshop.id);
  });

  it("navigates the rail to Activity and to the Device picker", async () => {
    await bootAndSignIn();

    fireEvent.click(screen.getByRole("button", { name: "Activity" }));
    expect(screen.getByRole("heading", { name: "Activity", level: 1 })).toBeInTheDocument();
    // The empty sentence below is the table's; Activity opens on the timeline, so open the table.
    fireEvent.click(screen.getByRole("tab", { name: "Table" }));
    expect(screen.getByText(/No calls yet\./)).toBeInTheDocument();
    expect(
      screen.getByText(/straight to a machine's own endpoint is not/)
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Device" }));
    expect(screen.getByRole("heading", { name: "Terminal", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Choose a machine" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Screen" }));
    expect(screen.getByRole("heading", { name: "Screen", level: 1 })).toBeInTheDocument();
    expect(screen.queryByText("No calls yet.")).not.toBeInTheDocument();
  });

  it("opens the Tokens section of the Connect sheet from the account menu", async () => {
    await bootAndSignIn();

    openAccount();
    fireEvent.click(screen.getByRole("button", { name: "Tokens" }));

    const dialog = await screen.findByRole("dialog", { name: "Tokens" });
    // The account menu opens the fleet scope: no machine is chosen, so the form can mint only a
    // token for all machines.
    expect(within(dialog).getByRole("button", { name: "Create token" })).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "all machines" })).toHaveAttribute(
      "aria-pressed",
      "true"
    );
    expect(within(dialog).getByText(/spans the fleet/)).toBeInTheDocument();
  });

  it("opens the Tokens section on the token a record names", async () => {
    mocks.audit.mockResolvedValue([tokenCall]);
    mocks.tokens.mockResolvedValue([laptop]);
    await bootAndSignIn();

    fireEvent.click(screen.getByRole("button", { name: "Activity" }));
    await screen.findByRole("heading", { name: "Activity", level: 1 });
    fireEvent.click(screen.getByRole("tab", { name: "Table" }));

    const row = (await screen.findByText("tools/list")).closest("tr");
    expect(row).not.toBeNull();
    fireEvent.click(within(row as HTMLElement).getByRole("button", { name: /Show the record for/ }));
    fireEvent.click(screen.getByRole("button", { name: "Manage" }));

    const dialog = await screen.findByRole("dialog", { name: "Tokens" });
    expect(await within(dialog).findByText("laptop-dsh")).toBeInTheDocument();
  });

  it("selects a machine without leaving Home, then opens its terminal from Device", async () => {
    await bootAndSignIn();

    selectDevice("workshop");

    // Selecting stores the machine and stays on Home, which becomes the hub; the header is where
    // the machine is now in context, as one control.
    expect(screen.getByRole("heading", { name: "Home", level: 1 })).toBeInTheDocument();
    expect(within(screen.getByRole("main")).getByText("Running")).toBeInTheDocument();
    expect(screen.getByLabelText("Machine: workshop")).toBeInTheDocument();
    expect(localStorage.getItem("summrise.selected.device")).toBe(workshop.id);
    await waitFor(() => expect(window.location.hash).toBe("#home"));

    fireEvent.click(screen.getByRole("button", { name: "Device" }));
    const workspace = await screen.findByTestId("workspace");
    expect(workspace).toHaveAttribute("data-visible", "yes");
    expect(screen.getByRole("heading", { name: "Terminal", level: 1 })).toBeInTheDocument();
    expect(screen.queryByText(/Grant what the console may do/)).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Lifetime (seconds)")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Terminal" })).toHaveAttribute(
      "aria-current",
      "page"
    );
  });

  it("keeps the selected machine visible on every Device tab", async () => {
    await bootAndSignIn();
    selectDevice("workshop");
    fireEvent.click(screen.getByRole("button", { name: "Device" }));

    expect(screen.getByLabelText("Machine: workshop")).toBeInTheDocument();
    for (const tab of ["Files", "Screen", "Runs", "Tools", "Machine", "Terminal"]) {
      fireEvent.click(screen.getByRole("button", { name: tab }));
      expect(screen.getByLabelText("Machine: workshop")).toBeInTheDocument();
      expect(screen.getByRole("heading", { name: tab, level: 1 })).toBeInTheDocument();
    }
  });

  it("shows the machine's version once per surface: the header chip, and not the page again", async () => {
    await bootAndSignIn();
    selectDevice("workshop");
    fireEvent.click(screen.getByRole("button", { name: "Device" }));
    await screen.findByRole("heading", { name: "Terminal", level: 1 });

    // The header is the one copy. The shell and the Machine tab draw no second one.
    const header = screen.getByLabelText("Machine: workshop");
    expect(header.textContent?.match(/0\.3\.42/g)).toHaveLength(1);
    for (const tab of ["Files", "Screen", "Runs", "Tools", "Machine"]) {
      fireEvent.click(screen.getByRole("button", { name: tab }));
      expect(screen.getByLabelText("Machine: workshop").textContent?.match(/0\.3\.42/g)).toHaveLength(1);
      expect(within(screen.getByRole("main")).queryByText(/0\.3\.42/)).toBeNull();
    }
  });

  it("answers what the machine can do on one Tools tab: the run form, the catalogue, the plugins", async () => {
    mocks.liveTools.mockResolvedValue({ tools: [filesTool, pluginTool] });
    pluginMocks.list.mockResolvedValue([helloPlugin]);
    await bootAndSignIn();
    selectDevice("workshop");

    fireEvent.click(screen.getByRole("button", { name: "Device" }));
    fireEvent.click(screen.getByRole("button", { name: "Tools" }));

    expect(await screen.findByRole("heading", { name: "Tools", level: 1 })).toBeInTheDocument();
    // ONE ANSWER IN THREE SECTIONS: the exec form that used to lead the Runs page comes first, then
    // the catalogue and the plugins it makes possible.
    expect(
      screen.getAllByRole("heading", { level: 2 }).map((heading) => heading.textContent)
    ).toEqual(["Run a command on this machine", "Tools on this machine", "Plugins"]);
    expect(await screen.findByRole("button", { name: /files\.read/ })).toBeInTheDocument();
    expect(await screen.findByText("Hello")).toBeInTheDocument();
    expect(screen.getByLabelText("Command")).toBeInTheDocument();
    await waitFor(() => expect(window.location.hash).toBe("#device/tools"));
  });

  it("opens the machine's Tools tab from the hub's counts line", async () => {
    mocks.liveTools.mockResolvedValue({ tools: [filesTool, pluginTool] });
    pluginMocks.list.mockResolvedValue([helloPlugin]);
    await bootAndSignIn();
    selectDevice("workshop");

    const counts = await screen.findByRole("button", {
      name: /2 tools · 2 capabilities · 1 plugin/
    });
    fireEvent.click(counts);

    expect(await screen.findByRole("heading", { name: "Tools", level: 1 })).toBeInTheDocument();
    expect(await screen.findByRole("heading", { name: "Tools on this machine", level: 2 })).toBeInTheDocument();
    expect(await screen.findByRole("heading", { name: "Plugins", level: 2 })).toBeInTheDocument();
    await waitFor(() => expect(window.location.hash).toBe("#device/tools"));
  });

  // Each retired link is its own test: the two used to share one loop, and the second boot racing
  // the first unmount made the pair flake about once in ten runs.
  async function retiredLinkWithMachine(hash: string) {
    signedIn = false;
    window.history.replaceState(null, "", hash);
    localStorage.setItem("summrise.selected.device", workshop.id);
    mocks.liveTools.mockResolvedValue({ tools: [filesTool, pluginTool] });
    pluginMocks.list.mockResolvedValue([helloPlugin]);
    const view = await boot();
    fireEvent.change(screen.getByLabelText("Operator token"), {
      target: { value: "operator-token" }
    });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    return view;
  }

  it("lands the retired #capabilities link on Tools when a machine is selected", async () => {
    const view = await retiredLinkWithMachine("#capabilities");
    expect(
      await screen.findByRole("heading", { name: "Tools", level: 1 }, { timeout: 3000 })
    ).toBeInTheDocument();
    await waitFor(() => expect(window.location.hash).toBe("#device/tools"));
    view.unmount();
  });

  it("lands the retired #plugins link on Tools when a machine is selected", async () => {
    const view = await retiredLinkWithMachine("#plugins");
    expect(
      await screen.findByRole("heading", { name: "Tools", level: 1 }, { timeout: 3000 })
    ).toBeInTheDocument();
    await waitFor(() => expect(window.location.hash).toBe("#device/tools"));
    view.unmount();
  });

  async function retiredLinkWithoutMachine(hash: string) {
    signedIn = false;
    window.history.replaceState(null, "", hash);
    const view = await boot();
    fireEvent.change(screen.getByLabelText("Operator token"), {
      target: { value: "operator-token" }
    });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    return view;
  }

  it("lands the retired #capabilities link on Home when no machine is selected", async () => {
    const view = await retiredLinkWithoutMachine("#capabilities");
    expect(
      await screen.findByRole("heading", { name: "Machines", level: 1 }, { timeout: 3000 })
    ).toBeInTheDocument();
    await waitFor(() => expect(window.location.hash).toBe("#home"));
    view.unmount();
  });

  it("lands the retired #plugins link on Home when no machine is selected", async () => {
    const view = await retiredLinkWithoutMachine("#plugins");
    expect(
      await screen.findByRole("heading", { name: "Machines", level: 1 }, { timeout: 3000 })
    ).toBeInTheDocument();
    await waitFor(() => expect(window.location.hash).toBe("#home"));
    view.unmount();
  });

  it("says an offline machine is offline once on the Tools tab", async () => {
    mocks.devices.mockResolvedValue([{ ...workshop, online: false }, attic]);
    // The console's answer for a machine that is not connected: the read fails, it does not say
    // "this build advertises no tools" — that would be the older-build state instead.
    mocks.liveTools.mockRejectedValue(new HttpError(503, "device_offline"));
    await boot();
    fireEvent.change(screen.getByLabelText("Operator token"), { target: { value: "operator-token" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    // A whole sign-in is session, fleet, audit and tokens; the default one second has flaked in a
    // full run before (see the legacy-alias test above).
    await screen.findByText("2 machines · 1 online", {}, { timeout: 3000 });
    selectDevice("workshop");

    fireEvent.click(screen.getByRole("button", { name: "Device" }));
    fireEvent.click(screen.getByRole("button", { name: "Tools" }));
    expect(await screen.findByRole("heading", { name: "Tools", level: 1 })).toBeInTheDocument();

    // The shell's one notice; neither section repeats the machine's state.
    expect(screen.getAllByText(/This machine is offline/)).toHaveLength(1);
    expect(screen.queryByText(/not connected/)).toBeNull();
    // Nor does the empty section pretend the machine is an older build with no tools.
    expect(screen.getByText("The console has no tool list from this machine yet.")).toBeInTheDocument();
    expect(screen.getByText("The console has no plugin list from this machine yet.")).toBeInTheDocument();
  });

  it("keeps the workspace mounted, hidden, across a destination change", async () => {
    await bootAndSignIn();
    selectDevice("workshop");
    fireEvent.click(screen.getByRole("button", { name: "Device" }));

    const workspace = await screen.findByTestId("workspace");
    expect(mocks.grant).not.toHaveBeenCalled();
    expect(workspace).toHaveAttribute("data-visible", "yes");

    fireEvent.click(screen.getByRole("button", { name: "Activity" }));
    // The empty sentence below is the table's; Activity opens on the timeline, so open the table.
    fireEvent.click(screen.getByRole("tab", { name: "Table" }));
    expect(screen.getByText("No calls match the machine workshop.")).toBeInTheDocument();
    // Hidden, not unmounted: the tabs, their cursors and their names live in this subtree.
    expect(screen.getByTestId("workspace")).toBe(workspace);
    expect(workspace).toHaveAttribute("data-visible", "no");
    expect(workspace.closest(".terminal-page")).toHaveAttribute("hidden");

    fireEvent.click(screen.getByRole("button", { name: "Device" }));
    expect(screen.getByTestId("workspace")).toBe(workspace);
    expect(workspace).toHaveAttribute("data-visible", "yes");
    expect(workspace.closest(".terminal-page")).not.toHaveAttribute("hidden");
  });

  it("clears its poll when the page unmounts", async () => {
    const startPoll = vi.spyOn(window, "setInterval");
    const clearInterval = vi.spyOn(window, "clearInterval");
    const { unmount } = await bootAndSignIn();

    // Testing Library's `waitFor` polls on a short interval too; only the page's own 5-second timer is
    // the poll this test is about.
    const poll = startPoll.mock.calls.findIndex(([, delay]) => delay === 5000);
    expect(poll).toBeGreaterThanOrEqual(0);
    const timer = startPoll.mock.results[poll].value;

    unmount();

    expect(clearInterval).toHaveBeenCalledWith(timer);
  });

  it("follows the edge's sign-out when the console names one", async () => {
    // Behind Cloudflare Access the console cookie is only half the identity; the answer names the
    // edge's logout and the page must follow it, or the next request signs the operator back in.
    const assign = vi.fn();
    const original = window.location;
    Object.defineProperty(window, "location", {
      configurable: true,
      value: { ...original, assign }
    });
    try {
      mocks.logout.mockResolvedValue({
        access_logout: "https://edge.example/cdn-cgi/access/logout"
      });
      await bootAndSignIn();
      openAccount();
      fireEvent.click(screen.getByRole("button", { name: "Sign out" }));
      await waitFor(() =>
        expect(assign).toHaveBeenCalledWith("https://edge.example/cdn-cgi/access/logout")
      );
    } finally {
      Object.defineProperty(window, "location", { configurable: true, value: original });
      mocks.logout.mockResolvedValue(undefined);
    }
  });

  it("signs out on the server and clears the fleet", async () => {
    await bootAndSignIn();
    expect(screen.getAllByText("workshop").length).toBeGreaterThan(0);

    openAccount();
    fireEvent.click(screen.getByRole("button", { name: "Sign out" }));

    expect(mocks.logout).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("heading", { name: "Sign in to the console" })).toBeInTheDocument();
    expect(screen.queryByText("workshop")).not.toBeInTheDocument();
    expect(screen.queryByText("2 machines · 2 online")).not.toBeInTheDocument();
  });

  it("signs a proxy identity out of the tab without offering a token it cannot use", async () => {
    mocks.session.mockImplementation(async () => ({
      authenticated: true,
      via: "proxy",
      email: "13122231591@163.com (DESKTOP-9VH92BJ)"
    }));
    render(<App />);
    await screen.findByRole("heading", { name: "Machines", level: 1 });

    openAccount();
    fireEvent.click(screen.getByRole("button", { name: "Sign out" }));

    // The identity is the network's; a local sign-out cannot end it, and the token form would be a
    // dead end for an operator who never had one. The page says both and offers the way back.
    expect(mocks.logout).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("heading", { name: "Signed out of this tab" })).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "Sign in to the console" })
    ).not.toBeInTheDocument();
    expect(screen.getByText(/tailnet login/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Load the console again" })).toBeInTheDocument();
    expect(screen.getByLabelText("Operator token")).toBeInTheDocument();
  });

  it("clears the page even when the logout request is rejected", async () => {
    mocks.logout.mockRejectedValue(new Error("offline"));
    await bootAndSignIn();
    selectDevice("workshop");

    openAccount();
    fireEvent.click(screen.getByRole("button", { name: "Sign out" }));

    // The page forgets the session it was showing; a server that is slow, down or refusing must not
    // keep the operator on a screen they signed out of. The grant, the selected device and the page
    // go with it — the cookie is the server's to end, and the page does not wait for that.
    expect(mocks.logout).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("heading", { name: "Sign in to the console" })).toBeInTheDocument();
    expect(screen.queryByText("workshop")).not.toBeInTheDocument();
    expect(screen.queryByText("2 machines · 2 online")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "Grant what the console may do on workshop" })
    ).not.toBeInTheDocument();
    expect(document.body.textContent).not.toMatch(/srm_|fresh-token/);
  });

  it("names the token behind an audited call from the list it fetched beside the fleet", async () => {
    mocks.tokens.mockResolvedValue([laptop]);
    mocks.audit.mockResolvedValue([tokenCall]);
    await bootAndSignIn();

    fireEvent.click(screen.getByRole("button", { name: "Activity" }));
    // Activity opens on the timeline; the token column is the table's.
    fireEvent.click(screen.getByRole("tab", { name: "Table" }));
    expect(within(await screen.findByRole("table")).getByText("laptop-dsh")).toBeInTheDocument();
    expect(screen.queryByText(shortTokenId)).not.toBeInTheDocument();
    expect(mocks.tokens).toHaveBeenCalled();
  });

  it("survives a rejected token fetch: the fleet and audit render, on ids and All tokens", async () => {
    mocks.tokens.mockRejectedValue(new Error("tokens_unavailable"));
    mocks.audit.mockResolvedValue([tokenCall]);
    await bootAndSignIn();

    // The list is a convenience: losing it leaves the page working and the table on record ids.
    expect(screen.getAllByRole("article")).toHaveLength(2);
    fireEvent.click(screen.getByRole("button", { name: "Activity" }));
    // Activity opens on the timeline; the token column is the table's.
    fireEvent.click(screen.getByRole("tab", { name: "Table" }));
    expect(within(await screen.findByRole("table")).getByText(shortTokenId)).toBeInTheDocument();
    expect(screen.queryByText("tokens_unavailable")).not.toBeInTheDocument();

    const select = screen.getByLabelText("Token");
    // The filter still works and still matches the cell, on the short id, because that is all the page
    // knows once the list is gone. Nothing is named, and nothing is blank.
    expect(within(select).getAllByRole("option").map((option) => option.textContent)).toEqual([
      "All tokens",
      shortTokenId
    ]);
    expect(within(screen.getByRole("table")).getByText(shortTokenId)).toBeInTheDocument();
  });

  it("samples host facts into the browser-local history, per device", async () => {
    const gib = 1024 ** 3;
    mocks.devices.mockResolvedValue([
      {
        ...workshop,
        host: {
          memory: { total_bytes: 8 * gib, available_bytes: 4 * gib },
          disks: [{ name: "/", total_bytes: 100 * gib, free_bytes: 25 * gib }]
        }
      },
      attic
    ]);
    const prior: VitalsSample[] = [
      {
        ts_ms: Date.now() - 60_000,
        memory_available_bytes: 4 * gib,
        memory_total_bytes: 8 * gib,
        disk_name: "/",
        disk_free_bytes: 25 * gib,
        disk_total_bytes: 100 * gib
      }
    ];
    localStorage.setItem(VITALS_KEY_PREFIX + workshop.id, JSON.stringify(prior));

    await bootAndSignIn();

    // One sample was already in this browser; the refresh appended this poll's, so the history has
    // the two points the Machine page draws. The device without host facts has no key at all.
    const stored = JSON.parse(
      localStorage.getItem(VITALS_KEY_PREFIX + workshop.id) ?? "[]"
    ) as VitalsSample[];
    expect(stored).toHaveLength(2);
    expect(stored[1].ts_ms).toBeGreaterThan(prior[0].ts_ms);
    expect(localStorage.getItem(VITALS_KEY_PREFIX + attic.id)).toBeNull();

    // A second refresh inside the 30 s window moves the fleet, not the history: there is no new
    // interval, and the stored timestamp is the only cadence. Activity's Refresh is the page's read.
    fireEvent.click(screen.getByRole("button", { name: "Activity" }));
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() => expect(mocks.devices).toHaveBeenCalledTimes(2));
    expect(JSON.parse(localStorage.getItem(VITALS_KEY_PREFIX + workshop.id) ?? "[]")).toHaveLength(2);
  });

  it("rereads the audit after a dropped frame, and says so until the answer lands", async () => {
    await bootAndSignIn();
    fireEvent.click(screen.getByRole("button", { name: "Activity" }));
    await screen.findByRole("heading", { name: "Activity", level: 1 });

    const source = TestEventSource.instances[TestEventSource.instances.length - 1];
    act(() => {
      source.emit("snapshot", { lines: [] });
    });
    expect(
      await screen.findByText("Following the console's audit as it is written.")
    ).toBeInTheDocument();

    // The reread is the audit window, not a new kind of read: same limit the page already asks for.
    const answer = deferred<unknown[]>();
    const readsBefore = mocks.audit.mock.calls.length;
    mocks.audit.mockReturnValueOnce(answer.promise);

    act(() => {
      source.emit("lagged", { dropped: 3 });
    });

    // The number and the reread are one fact: the page may not say it is following a record with a
    // known hole in it.
    expect(
      await screen.findByText("3 lines were dropped while this tab was behind; rereading")
    ).toBeInTheDocument();
    await waitFor(() => expect(mocks.audit).toHaveBeenCalledTimes(readsBefore + 1));
    expect(mocks.audit).toHaveBeenLastCalledWith(1000);
    expect(screen.queryByText(/Following the console's audit/)).toBeNull();

    // A line arriving after the hole is the stream resuming; it is not the missing lines.
    act(() => {
      source.emit("activity", {
        ts_ms: Date.now(),
        kind: "mcp",
        status: 200,
        ok: true,
        sentence: "Read `notes.txt`",
        tool: "files.read"
      });
    });
    expect(
      screen.getByText("3 lines were dropped while this tab was behind; rereading")
    ).toBeInTheDocument();
    expect(mocks.audit).toHaveBeenCalledTimes(readsBefore + 1);

    // The answer lands, the hole is read back, and only now is the page following again.
    await act(async () => {
      answer.resolve([]);
    });
    expect(
      await screen.findByText("Following the console's audit as it is written.")
    ).toBeInTheDocument();
    expect(screen.queryByText(/were dropped while this tab was behind/)).toBeNull();
  });

  it("keeps the newest audit read when the poll and a lag reread overlap", async () => {
    await bootAndSignIn();
    fireEvent.click(screen.getByRole("button", { name: "Activity" }));
    await screen.findByRole("heading", { name: "Activity", level: 1 });
    const source = TestEventSource.instances[TestEventSource.instances.length - 1];
    act(() => {
      source.emit("snapshot", { lines: [] });
    });

    // Two reads of the same growing file can be in flight: the five-second poll's and the reread a
    // drop started. The file only gains lines, so the read that STARTED later is the fresher answer.
    const polled = deferred<unknown[]>();
    const reread = deferred<unknown[]>();
    const readsBefore = mocks.audit.mock.calls.length;
    mocks.audit.mockReturnValueOnce(polled.promise).mockReturnValueOnce(reread.promise);

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() => expect(mocks.audit).toHaveBeenCalledTimes(readsBefore + 1));
    act(() => {
      source.emit("lagged", { dropped: 1 });
    });
    await waitFor(() => expect(mocks.audit).toHaveBeenCalledTimes(readsBefore + 2));

    await act(async () => {
      reread.resolve([{ ...tokenCall, ts_ms: 2, sentence: "The reread's line" }]);
    });
    expect(await screen.findByText("The reread's line")).toBeInTheDocument();

    // The poll's older answer lands after it; it must not reopen the hole the reread just filled.
    await act(async () => {
      polled.resolve([{ ...tokenCall, ts_ms: 1, sentence: "The poll's older line" }]);
    });
    await waitFor(() => expect(screen.queryByText("The poll's older line")).toBeNull());
    expect(screen.getByText("The reread's line")).toBeInTheDocument();
  });

  it("counts the lines that arrived while Activity was paused", async () => {
    await bootAndSignIn();
    fireEvent.click(screen.getByRole("button", { name: "Activity" }));
    await screen.findByRole("heading", { name: "Activity", level: 1 });

    const source = TestEventSource.instances[TestEventSource.instances.length - 1];
    expect(source).toBeDefined();

    fireEvent.click(screen.getByRole("button", { name: "Pause" }));
    // The subheading says the page is paused, not that it is still following the stream.
    expect(screen.getByText(/Paused — the stream is still running/)).toBeInTheDocument();

    // One call arrives after the freeze. Its console clock is the fact the count reads, and it is
    // strictly after the moment of pausing (the two events can land in the same millisecond).
    act(() => {
      source.emit("activity", {
        ts_ms: Date.now() + 5_000,
        kind: "mcp",
        status: 200,
        ok: true,
        sentence: "Read `notes.txt`",
        tool: "files.read"
      });
    });

    const resume = await screen.findByRole("button", { name: "Resume (1 new)" });
    expect(resume).toBeInTheDocument();

    fireEvent.click(resume);
    // Resuming shows the line the frozen list held back.
    expect(await screen.findByText("Read `notes.txt`")).toBeInTheDocument();
  });

  it("counts a line the fetched window slid in, not the list's length", async () => {
    // The window that fills the list is `/audit`, and it slides: one old line leaves while one new
    // line arrives, so the list's length does not move even though a call is new.
    mocks.audit.mockResolvedValue([{ ...tokenCall, ts_ms: 1 }]);
    await bootAndSignIn();
    fireEvent.click(screen.getByRole("button", { name: "Activity" }));
    await screen.findByRole("heading", { name: "Activity", level: 1 });

    fireEvent.click(screen.getByRole("button", { name: "Pause" }));
    mocks.audit.mockResolvedValue([
      {
        ...tokenCall,
        ts_ms: Date.now() + 5_000,
        tool: "files.read",
        detail: null,
        status: 200
      }
    ]);
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));

    expect(await screen.findByRole("button", { name: "Resume (1 new)" })).toBeInTheDocument();
  });
});
