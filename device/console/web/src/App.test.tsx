import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { VITALS_KEY_PREFIX, type VitalsSample } from "./vitals";

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

    expect(document.title).toBe("Sign in · Summrise console");
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
    expect(document.title).toBe("Machines · Summrise console");
    expect(window.location.hash).toBe("#home");
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
    expect(window.location.hash).toBe("#device/files");
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
    expect(window.location.hash).toBe("#run/workshop/r-1");
    window.history.replaceState(null, "", window.location.pathname);
  });

  it("asks the audit for the console's cap, so a full day is the day", async () => {
    await bootAndSignIn();
    // MAX_LOG_LIMIT in device/console/src/http.rs. A shorter window made "Today" mean "the newest
    // slice"; P2-12 raised the fetch to the cap before measuring what 1000 rows cost.
    expect(mocks.audit).toHaveBeenCalledWith(1000);
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
    expect(document.title).toBe("Machines · Summrise console");

    fireEvent.click(screen.getByRole("button", { name: "Activity" }));
    await screen.findByRole("heading", { name: "Activity", level: 1 });
    expect(document.title).toBe("Activity · Summrise console");

    fireEvent.click(screen.getByRole("button", { name: "Device" }));
    expect(document.title).toBe("Terminal · Summrise console");
  });

  it("renders the whole shell: five destinations, one h1, the account menu and the build", async () => {
    const { container } = await bootAndSignIn();

    const rail = screen.getByRole("navigation", { name: "Destinations" });
    const sections = Array.from(container.querySelectorAll<HTMLButtonElement>(".rail-btn"));
    expect(sections.map((section) => section.textContent)).toEqual([
      "Home",
      "Capabilities",
      "Plugins",
      "Activity",
      "Device"
    ]);
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
    // five, and Connect is a verb in the account menu, not a rail seat.
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

  it("opens a tool's sheet from a capabilities deep link", async () => {
    window.history.replaceState(null, "", "#capabilities/files.read");
    // A device-specific catalogue needs a machine; the selection survives a reload.
    localStorage.setItem("summrise.selected.device", workshop.id);
    mocks.liveTools.mockResolvedValue({
      tools: [
        {
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
        }
      ]
    });
    await boot();
    fireEvent.change(screen.getByLabelText("Operator token"), { target: { value: "operator-token" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));

    expect(await screen.findByRole("heading", { name: "Capabilities", level: 1 })).toBeInTheDocument();
    expect(await screen.findByRole("dialog", { name: "files.read" })).toBeInTheDocument();
    expect(window.location.hash).toBe("#capabilities/files.read");
    window.history.replaceState(null, "", window.location.pathname);
  });

  it("opens a tool's sheet when the fragment changes after sign-in", async () => {
    mocks.liveTools.mockResolvedValue({
      tools: [
        {
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
        }
      ]
    });
    await bootAndSignIn();
    // A catalogue belongs to a machine, so pick one the way a person does.
    fireEvent.click(screen.getAllByRole("button", { name: /workshop/ })[0]);
    await screen.findByRole("heading", { name: "Home", level: 1 });

    // The late path: no reload, no click — the fragment changes on a page that is already mounted.
    window.history.replaceState(null, "", "#capabilities/files.read");
    window.dispatchEvent(new Event("hashchange"));

    expect(await screen.findByRole("dialog", { name: "files.read" })).toBeInTheDocument();
    window.history.replaceState(null, "", window.location.pathname);
  });

  it("says so when the URL names a tool the machine does not have", async () => {
    window.history.replaceState(null, "", "#capabilities/not-a-tool");
    localStorage.setItem("summrise.selected.device", workshop.id);
    mocks.liveTools.mockResolvedValue({
      tools: [
        {
          name: "files.read",
          description: "read a file",
          inputSchema: { type: "object", properties: {}, required: [] },
          source: "builtin",
          capability: "files",
          destructive: false
        }
      ]
    });
    await boot();
    fireEvent.change(screen.getByLabelText("Operator token"), { target: { value: "operator-token" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));

    expect(await screen.findByRole("heading", { name: "Capabilities", level: 1 })).toBeInTheDocument();
    // The link is stale: the machine has no such tool. Silence would read as "the console ignored
    // me", so the page says which fact it is.
    expect(await screen.findByText("That tool is not on this machine.")).toBeInTheDocument();
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
    expect(screen.getByText("No calls yet.")).toBeInTheDocument();

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

  it("selects a machine without leaving Home, then opens its terminal from Device", async () => {
    await bootAndSignIn();

    selectDevice("workshop");

    // Selecting stores the machine and stays on Home, which becomes the hub; the header is where
    // the machine is now in context, as one control.
    expect(screen.getByRole("heading", { name: "Home", level: 1 })).toBeInTheDocument();
    expect(within(screen.getByRole("main")).getByText("Running · console v0.3.42")).toBeInTheDocument();
    expect(screen.getByLabelText("Machine: workshop")).toBeInTheDocument();
    expect(localStorage.getItem("summrise.selected.device")).toBe(workshop.id);
    expect(window.location.hash).toBe("#home");

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
    for (const tab of ["Files", "Screen", "Runs", "Machine", "Terminal"]) {
      fireEvent.click(screen.getByRole("button", { name: tab }));
      expect(screen.getByLabelText("Machine: workshop")).toBeInTheDocument();
      expect(screen.getByRole("heading", { name: tab, level: 1 })).toBeInTheDocument();
    }
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
    expect(screen.getByText("No calls yet.")).toBeInTheDocument();
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
