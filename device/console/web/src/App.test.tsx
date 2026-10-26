import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";

// The shell is driven against a mocked HTTP surface: no request leaves jsdom, and the sign-in state is a
// flag here rather than a server. `api.signIn` flips it, so the following `api.session` reports the
// session the real console would have created.
const mocks = vi.hoisted(() => ({
  session: vi.fn(),
  health: vi.fn(),
  devices: vi.fn(),
  audit: vi.fn(),
  connections: vi.fn(),
  signIn: vi.fn(),
  enroll: vi.fn(),
  rename: vi.fn(),
  remove: vi.fn(),
  tools: vi.fn(),
  grant: vi.fn(),
  tokens: vi.fn(),
  createToken: vi.fn(),
  revokeToken: vi.fn(),
  meshReadiness: vi.fn(),
  setToken: vi.fn()
}));

vi.mock("./api", () => ({
  api: {
    session: mocks.session,
    health: mocks.health,
    devices: mocks.devices,
    audit: mocks.audit,
    connections: mocks.connections,
    signIn: mocks.signIn,
    enroll: mocks.enroll,
    rename: mocks.rename,
    remove: mocks.remove,
    tools: mocks.tools,
    grant: mocks.grant,
    tokens: mocks.tokens,
    createToken: mocks.createToken,
    revokeToken: mocks.revokeToken
  },
  meshReadiness: mocks.meshReadiness,
  setToken: mocks.setToken,
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
// `visible` is the prop App uses to hide it on every page that is not the terminal.
vi.mock("./TerminalWorkspace", () => ({
  TerminalWorkspace: ({ visible }: { visible: boolean }) => (
    <div data-testid="workspace" data-visible={visible ? "yes" : "no"} />
  )
}));

// The chip has its own poll and its own test; here it is a marker whose click must land on the
// Monitors page without touching the grant or the selected device.
vi.mock("./MonitorChip", () => ({
  MonitorChip: ({ onOpen }: { onOpen: () => void }) => (
    <button type="button" onClick={onOpen}>monitor-chip</button>
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

beforeEach(() => {
  signedIn = false;
  mocks.session.mockImplementation(async () =>
    signedIn
      ? { authenticated: true, via: "token" }
      : { authenticated: false, access: { configured: false, header: false } }
  );
  mocks.health.mockResolvedValue({ version: "0.3.42", devices: 2 });
  mocks.devices.mockResolvedValue([workshop, attic]);
  mocks.audit.mockResolvedValue([]);
  mocks.connections.mockResolvedValue([]);
  mocks.signIn.mockImplementation(async () => {
    signedIn = true;
    return { token: "fresh-token", expires_at_ms: Date.now() + 3_600_000 };
  });
  mocks.enroll.mockResolvedValue({ code: "code", expires_at_ms: Date.now() + 60_000 });
  mocks.rename.mockResolvedValue({ id: workshop.id, name: workshop.name });
  mocks.remove.mockResolvedValue(undefined);
  mocks.grant.mockResolvedValue({ grant: {}, signature: "signature" });
  mocks.tokens.mockResolvedValue([]);
  mocks.createToken.mockResolvedValue({ id: "tok-1", token: "srm_token" });
  mocks.revokeToken.mockResolvedValue(undefined);
  // The grant form asks the client for the tool registry; that request is not this test's subject,
  // so the client's answer fails and the form keeps its fallback list.
  mocks.tools.mockRejectedValue(new TypeError("Failed to fetch"));
});

/** Render the page and wait for the signed-out boot to settle on the login form. */
async function boot() {
  const view = render(<App />);
  await screen.findByRole("heading", { name: "Sign in to the console" });
  return view;
}

/** Boot signed out, sign in with the operator token, and wait for the fleet. */
async function bootAndSignIn() {
  const view = await boot();
  fireEvent.change(screen.getByLabelText("Operator token"), { target: { value: "operator-token" } });
  fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
  await screen.findByRole("heading", { name: "Summrise" });
  await screen.findByText("workshop");
  return view;
}

/** Select a device from its card; App switches to the terminal and asks for a grant. */
function selectDevice(name: string) {
  const card = screen.getByText(name).closest("article");
  expect(card).not.toBeNull();
  fireEvent.click(within(card as HTMLElement).getByRole("button", { name: "Work on this" }));
}

describe("App", () => {
  it("boots signed out, signs in with the operator token, and renders the fleet", async () => {
    const { container } = await boot();

    const token = screen.getByLabelText("Operator token");
    expect(token).toHaveAttribute("type", "password");
    expect(screen.getByRole("button", { name: "Sign in" })).toBeDisabled();

    fireEvent.change(token, { target: { value: "operator-token" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));

    await waitFor(() => expect(mocks.signIn).toHaveBeenCalledWith({ token: "operator-token" }));
    expect(mocks.setToken).toHaveBeenCalledWith("fresh-token");

    expect(await screen.findByText("workshop")).toBeInTheDocument();
    expect(screen.getByText("attic")).toBeInTheDocument();
    // The status bar is where the page counts the fleet; the panel's own summary says the same numbers.
    const statusbar = container.querySelector(".statusbar");
    expect(statusbar).not.toBeNull();
    expect(within(statusbar as HTMLElement).getByText("2 devices")).toBeInTheDocument();
    expect(within(statusbar as HTMLElement).getByText("2 online")).toBeInTheDocument();
    expect(within(statusbar as HTMLElement).getByText("signed in with the operator token")).toBeInTheDocument();
  });

  it("renders the whole shell: the eleven rail sections, the brand, the build and the sign-in path", async () => {
    await bootAndSignIn();

    const rail = screen.getByRole("navigation", { name: "Sections" });
    const sections = within(rail).getAllByRole("button");
    expect(sections.map((section) => section.textContent)).toEqual([
      "Devices",
      "Files",
      "Terminal",
      "Browser",
      "Screen",
      "Memory",
      "Monitors",
      "Runs",
      "System",
      "Audit",
      "Events"
    ]);
    expect(within(rail).getByRole("button", { name: "Devices" })).toHaveAttribute("aria-current", "page");

    expect(screen.getByRole("heading", { name: "Summrise" })).toBeInTheDocument();
    expect(screen.getByText("console 0.3.42")).toBeInTheDocument();
    expect(screen.getByText("2 devices")).toBeInTheDocument();
    expect(screen.getByText("signed in with the operator token")).toBeInTheDocument();
  });

  it("navigates the rail to the audit panel and to the memory, screen and events pickers", async () => {
    await bootAndSignIn();

    fireEvent.click(screen.getByRole("button", { name: "Audit" }));
    expect(screen.getByRole("heading", { name: "Audit" })).toBeInTheDocument();
    expect(screen.getByText("No audit lines yet.")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Memory" }));
    expect(screen.getByRole("heading", { name: "Memory" })).toBeInTheDocument();
    expect(screen.getByText(/Memory is kept on the device/)).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Use" })).toHaveLength(2);
    expect(screen.queryByText("No audit lines yet.")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Screen" }));
    expect(screen.getByRole("heading", { name: "Screen" })).toBeInTheDocument();
    expect(
      screen.getByText(
        "The screen belongs to the device's user session, so pick the device whose screen you want to see. The console signs a grant for the capture tool, and the device checks that grant on every call."
      )
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Events" }));
    expect(screen.getByRole("heading", { name: "Events" })).toBeInTheDocument();
    expect(screen.getByText(/The events are the device's own/)).toBeInTheDocument();
    expect(screen.queryByText(/Memory is kept on the device/)).not.toBeInTheDocument();
  });

  it("selects a device and lands on the terminal with that device's grant form", async () => {
    await bootAndSignIn();

    selectDevice("workshop");

    expect(
      screen.getByRole("heading", { name: "Grant what the console may do on workshop" })
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Lifetime (seconds)")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Grant access" })).toBeInTheDocument();
    expect(screen.queryByText("No device selected.")).not.toBeInTheDocument();
    // Files draws the same grant form, so the grant heading alone does not say which page this is.
    const rail = screen.getByRole("navigation", { name: "Sections" });
    expect(within(rail).getByRole("button", { name: "Terminal" })).toHaveAttribute("aria-current", "page");
  });

  it("draws each device page's grant panel with the hint that shipped, and Browser without one", async () => {
    await bootAndSignIn();
    selectDevice("workshop");

    const rail = screen.getByRole("navigation", { name: "Sections" });
    const grantHints: [string, string][] = [
      [
        "Screen",
        "A grant is signed by the console, names the tools and expires. The device checks it on every call. Everything is selected by default: press the button and the screen can be captured."
      ],
      [
        "Memory",
        "A grant is signed by the console, names the tools and expires. The device checks it on every call. Everything is selected by default: press the button and the entries appear."
      ],
      [
        "Events",
        "A grant is signed by the console, names the tools and expires. The device checks it on every call. Everything is selected by default: press the button and the device's health and its last ten events appear."
      ],
      [
        "Files",
        "A grant is signed by the console, names the tools and expires. The device checks it on every call. Everything is selected by default: press the button and the files appear."
      ],
      [
        "Monitors",
        "A grant is signed by the console, names the tools and expires. The device checks it on every call. Everything is selected by default: press the button and the targets appear."
      ],
      [
        "Runs",
        "A grant is signed by the console, names the tools and expires. The device checks it on every call. Everything is selected by default: press the button and the run form appears."
      ],
      [
        "System",
        "A grant is signed by the console, names the tools and expires. The device checks it on every call. Everything is selected by default: press the button and the process list appears."
      ]
    ];
    for (const [section, hint] of grantHints) {
      fireEvent.click(within(rail).getByRole("button", { name: section }));
      expect(
        screen.getByRole("heading", { name: "Grant what the console may do on workshop" })
      ).toBeInTheDocument();
      expect(screen.getByText(hint)).toBeInTheDocument();
    }

    // Browser's grant panel has no hint paragraph; the heading is all it says.
    fireEvent.click(within(rail).getByRole("button", { name: "Browser" }));
    expect(
      screen.getByRole("heading", { name: "Grant what the console may do on workshop" })
    ).toBeInTheDocument();
  });

  it("keeps the workspace mounted, hidden, across a page change", async () => {
    await bootAndSignIn();
    selectDevice("workshop");
    expect(screen.queryByTestId("workspace")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "monitor-chip" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Grant access" }));
    const workspace = await screen.findByTestId("workspace");
    expect(mocks.grant).toHaveBeenCalledWith("dev-workshop-0001", expect.any(Array), 3600, true);
    expect(workspace).toHaveAttribute("data-visible", "yes");

    // The chip appears with the grant, and its click is navigation only: no second grant, same page
    // shell, and the Monitors page itself (the heading alone is also the picker's).
    const chip = await screen.findByRole("button", { name: "monitor-chip" });
    fireEvent.click(chip);
    expect(screen.getByRole("heading", { name: "Monitors" })).toBeInTheDocument();
    expect(
      screen.getByText(
        "The device probes each target on its own interval and keeps the last 60 probes for the summary. A down target is a fact, not a failure of this page: the dot is its newest probe, and the numbers are the device's."
      )
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Host")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add target" })).toBeInTheDocument();
    expect(screen.queryByText(/^Monitors run on the device, so pick one\./)).not.toBeInTheDocument();
    expect(mocks.grant).toHaveBeenCalledTimes(1);

    const rail = screen.getByRole("navigation", { name: "Sections" });
    fireEvent.click(within(rail).getByRole("button", { name: "Audit" }));
    expect(screen.getByText("No audit lines yet.")).toBeInTheDocument();
    // Hidden, not unmounted: the tabs, their cursors and their names live in this subtree.
    expect(screen.getByTestId("workspace")).toBe(workspace);
    expect(workspace).toHaveAttribute("data-visible", "no");
    expect(workspace.closest("section")).toHaveAttribute("hidden");

    fireEvent.click(within(rail).getByRole("button", { name: "Terminal" }));
    expect(screen.getByTestId("workspace")).toBe(workspace);
    expect(workspace).toHaveAttribute("data-visible", "yes");
    expect(workspace.closest("section")).not.toHaveAttribute("hidden");
  });

  it("shows the chip only for the device the grant names", async () => {
    await bootAndSignIn();
    selectDevice("workshop");
    fireEvent.click(screen.getByRole("button", { name: "Grant access" }));
    await screen.findByRole("button", { name: "monitor-chip" });

    // The console moves to another device while the workshop grant is still in force: the chip must
    // not show workshop's monitors under attic, and attic's page asks for its own grant.
    const rail = screen.getByRole("navigation", { name: "Sections" });
    fireEvent.click(within(rail).getByRole("button", { name: "Devices" }));
    selectDevice("attic");

    expect(
      screen.getByRole("heading", { name: "Grant what the console may do on attic" })
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "monitor-chip" })).not.toBeInTheDocument();
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

  it("signs out to the login form and clears the fleet", async () => {
    await bootAndSignIn();
    expect(screen.getByText("workshop")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Sign out" }));

    expect(screen.getByRole("heading", { name: "Sign in to the console" })).toBeInTheDocument();
    expect(screen.queryByText("workshop")).not.toBeInTheDocument();
    expect(screen.queryByText("2 devices")).not.toBeInTheDocument();
    expect(mocks.setToken).toHaveBeenLastCalledWith(null);
  });
});
