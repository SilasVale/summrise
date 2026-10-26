import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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
  grant: vi.fn(),
  meshReadiness: vi.fn(),
  setToken: vi.fn(),
  getToken: vi.fn(() => "")
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
    grant: mocks.grant
  },
  meshReadiness: mocks.meshReadiness,
  setToken: mocks.setToken,
  getToken: mocks.getToken,
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
  mocks.getToken.mockReturnValue("fresh-token");
  // The grant form asks the console for the tool registry; that request is not this test's subject.
  vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("Failed to fetch")));
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

  it("renders the whole shell: the ten rail sections, the brand, the build and the sign-in path", async () => {
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
      "Audit"
    ]);
    expect(within(rail).getByRole("button", { name: "Devices" })).toHaveAttribute("aria-current", "page");

    expect(screen.getByRole("heading", { name: "Summrise" })).toBeInTheDocument();
    expect(screen.getByText("console 0.3.42")).toBeInTheDocument();
    expect(screen.getByText("2 devices")).toBeInTheDocument();
    expect(screen.getByText("signed in with the operator token")).toBeInTheDocument();
  });

  it("navigates the rail to the audit panel and to the memory picker", async () => {
    await bootAndSignIn();

    fireEvent.click(screen.getByRole("button", { name: "Audit" }));
    expect(screen.getByRole("heading", { name: "Audit" })).toBeInTheDocument();
    expect(screen.getByText("No audit lines yet.")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Memory" }));
    expect(screen.getByRole("heading", { name: "Memory" })).toBeInTheDocument();
    expect(screen.getByText(/Memory is kept on the device/)).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Use" })).toHaveLength(2);
    expect(screen.queryByText("No audit lines yet.")).not.toBeInTheDocument();
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

  it("keeps the workspace mounted, hidden, across a page change", async () => {
    await bootAndSignIn();
    selectDevice("workshop");
    expect(screen.queryByTestId("workspace")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Grant access" }));
    const workspace = await screen.findByTestId("workspace");
    expect(mocks.grant).toHaveBeenCalledWith("dev-workshop-0001", expect.any(Array), 3600, true);
    expect(workspace).toHaveAttribute("data-visible", "yes");

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
