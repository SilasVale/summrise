import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ForwardRules } from "./ForwardRules";
import { callTool, ToolError, type Grant } from "./terminal";

// The section's whole device surface is `callTool`; the real `ToolError` stays so a refusal is the
// class the section actually receives from `./terminal`.
vi.mock("./terminal", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./terminal")>();
  return { ...actual, callTool: vi.fn() };
});

const mocked = vi.mocked(callTool);

const GRANT: Grant = { deviceId: "device-1", envelope: { token: "envelope" } };

/**
 * What this machine's `tools/list` advertises. `forward.list` present means this build can carry
 * rules at all — the fact `ForwardRules` gates its first read on.
 */
const liveTools = vi.hoisted(() => ({
  /// What this machine's `tools/list` advertises. A test sets it BEFORE it renders.
  tools: ["forward.add", "forward.list", "forward.remove"] as string[],
  loading: false,
  error: null as string | null
}));

vi.mock("./useLiveTools", () => ({
  useLiveTools: () => ({
    tools: liveTools.tools.map((name) => ({ name })),
    stale: false,
    error: liveTools.error,
    offline: false,
    loading: liveTools.loading,
    retry: () => {}
  })
}));

/**
 * One rule's wire shape, copied from `forward.list`.
 *
 * **THIS IS DELIBERATELY THE WHOLE LIST SHAPE, WITH NO CREDENTIAL.** The real `forward.list`
 * reports `{name, listen, target, via, listening, since_ms, attempts, connections, last_error}` and
 * NOT the row's `auth`; a fixture that carried one would let a test assert a page that leaks.
 */
type Wire = {
  name: string;
  listen: string;
  target: string;
  via: string;
  listening: boolean;
  since_ms: number | null;
  attempts: number;
  connections: number;
  last_error: string | null;
};

const LIVE: Wire = {
  name: "console",
  listen: "100.85.163.159:8080",
  target: "10.10.61.83:7738",
  via: "zss@10.10.61.83:22122",
  listening: true,
  since_ms: 1_000_000,
  attempts: 1,
  connections: 41,
  last_error: null
};

const BROKEN: Wire = {
  name: "office",
  listen: "100.85.163.159:8081",
  target: "10.10.61.83:7736",
  via: "zss@10.10.61.83:22122",
  listening: false,
  since_ms: null,
  attempts: 4,
  connections: 0,
  last_error: "connecting to via zss@127.0.0.1:1 failed: Connection refused (os error 111)"
};

/** Answer the named tools; an unasked-for call fails the test instead of returning a plausible value. */
function device(answers: (name: string, args: Record<string, unknown>) => string) {
  mocked.mockImplementation(
    async (_grant: Grant, name: string, args: Record<string, unknown>) => answers(name, args)
  );
}

const called = (name: string) => mocked.mock.calls.filter((call) => call[1] === name);

describe("ForwardRules", () => {
  beforeEach(() => {
    mocked.mockReset();
    liveTools.tools = ["forward.add", "forward.list", "forward.remove"];
    liveTools.loading = false;
    liveTools.error = null;
  });

  it("reads the machine's rules and says what each one is doing", async () => {
    device(() => JSON.stringify([LIVE, BROKEN]));
    render(<ForwardRules grant={GRANT} deviceId="device-1" />);

    expect(await screen.findByText("console")).toBeTruthy();
    expect(screen.getByText("office")).toBeTruthy();

    // The state is a WORD, not a colour: a bound rule and a failing one are told apart by reading.
    expect(screen.getByText("Listening")).toBeTruthy();
    expect(screen.getByText("Not listening")).toBeTruthy();

    // The route reads left to right: bound here → reaches this → carried by that.
    expect(screen.getByText("100.85.163.159:8080 → 10.10.61.83:7738 via zss@10.10.61.83:22122")).toBeTruthy();

    // **CARRIED, NOT ARRIVED.** The device's count is both directions ended without an error.
    expect(screen.getByText(/41 connections carried/)).toBeTruthy();
    expect(screen.getByText(/4 bind attempts/)).toBeTruthy();

    // A failing rule's own sentence is on the page; a healthy rule has none.
    expect(screen.getByText(BROKEN.last_error as string)).toBeTruthy();
    expect(called("forward.list").length).toBe(1);
  });

  it("says a machine with no rules has none, and offers the act that adds one", async () => {
    device(() => "[]");
    render(<ForwardRules grant={GRANT} deviceId="device-1" />);

    expect(await screen.findByText(/No forward rules/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Add a rule" })).toBeTruthy();
    // A catalogue of things is not a state: no Refresh is offered for a list that cannot be read
    // into anything.
    expect(screen.queryByRole("button", { name: "Refresh" })).toBeNull();
  });

  it("says an OLDER build cannot carry rules, instead of a refusal with a Retry", async () => {
    // **A MACHINE THAT DOES NOT ADVERTISE THE TOOLS ANSWERS `unknown tool: forward.list`, AND A
    // RETRY FOR THAT CANNOT SUCCEED.** The section checks the same `tools/list` Plugins checks,
    // BEFORE the read, so the operator gets a sentence about the machine rather than a failure
    // about the console.
    liveTools.tools = ["monitor.list"];
    try {
      device(() => "[]");
      render(<ForwardRules grant={GRANT} deviceId="device-old" />);

    expect(await screen.findByText("Older build")).toBeTruthy();
    expect(screen.getByText(/does not advertise the forward tools yet/)).toBeTruthy();
    // The proof that this is a sentence and not a failure: nothing was asked.
      expect(called("forward.list").length).toBe(0);
    } finally {
      liveTools.tools = ["forward.add", "forward.list", "forward.remove"];
    }
  });

  it("does not ask a machine the fleet says is down, and says why", async () => {
    device(() => "[]");
    render(<ForwardRules grant={GRANT} deviceId="device-1" deviceOnline={false} />);

    expect(screen.getByText(/The machine is offline/)).toBeTruthy();
    expect(called("forward.list").length).toBe(0);
  });

  it("shows the device's own sentence when the list is refused, and retries", async () => {
    let attempt = 0;
    device(() => {
      attempt += 1;
      if (attempt === 1) throw new ToolError("the device refused the read", 200, -32001, true);
      return JSON.stringify([LIVE]);
    });
    render(<ForwardRules grant={GRANT} deviceId="device-1" />);

    expect(await screen.findByText("the device refused the read")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("console")).toBeTruthy();
  });

    it("refuses a wildcard listen before it asks the device", async () => {
    device(() => JSON.stringify([]));
    render(<ForwardRules grant={GRANT} deviceId="device-1" />);
    fireEvent.click(await screen.findByRole("button", { name: "Add a rule" }));

    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "wide" } });
    fireEvent.change(screen.getByLabelText("Listen (this machine)"), {
      target: { value: "0.0.0.0:8080" }
    });
    fireEvent.change(screen.getByLabelText("Target (the far side dials)"), {
      target: { value: "10.10.61.83:7738" }
    });
    fireEvent.change(screen.getByLabelText("Via (the SSH connection)"), {
      target: { value: "zss@10.10.61.83:22" }
    });
    fireEvent.change(screen.getByLabelText("Password the connection uses"), { target: { value: "hunter2" } });
    fireEvent.click(screen.getByRole("button", { name: "Add rule" }));

    // The device refuses this too; the form refusing it means a person reads the reason in the
    // place they typed the value, not in a round trip.
    expect(await screen.findByText(/A wildcard listen is refused/)).toBeTruthy();
    expect(called("forward.add").length).toBe(0);
  });

  it("adds a rule with the credential typed here, and does not keep it", async () => {
    device((name) => (name === "forward.list" ? "[]" : JSON.stringify(LIVE)));
    render(<ForwardRules grant={GRANT} deviceId="device-1" />);
    fireEvent.click(await screen.findByRole("button", { name: "Add a rule" }));

    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "console" } });
    fireEvent.change(screen.getByLabelText("Listen (this machine)"), {
      target: { value: "100.85.163.159:8080" }
    });
    fireEvent.change(screen.getByLabelText("Target (the far side dials)"), {
      target: { value: "10.10.61.83:7738" }
    });
    fireEvent.change(screen.getByLabelText("Via (the SSH connection)"), {
      target: { value: "zss@10.10.61.83:22122" }
    });
    fireEvent.change(screen.getByLabelText("Password the connection uses"), { target: { value: "hunter2" } });
    fireEvent.click(screen.getByRole("button", { name: "Add rule" }));

    await waitFor(() => expect(called("forward.add").length).toBe(1));
    const args = called("forward.add")[0][2];
    expect(args).toMatchObject({
      name: "console",
      listen: "100.85.163.159:8080",
      target: "10.10.61.83:7738",
      via: "zss@10.10.61.83:22122",
      auth: { kind: "password", password: "hunter2" }
    });
    // **ABSENT, NOT EMPTY.** The device refuses an empty `host_key`; leaving the field blank must
    // mean "the device's known_hosts decides", which is the key not being there at all.
    expect("host_key" in (args as Record<string, unknown>)).toBe(false);

    // The sheet closes on success and the list is re-read.
    await waitFor(() => expect(screen.queryByLabelText("Password the connection uses")).toBeNull());
    await waitFor(() => expect(called("forward.list").length).toBe(2));
  });

  it("sends a key path as a key, and refuses a relative one here", async () => {
    device((name) => (name === "forward.list" ? "[]" : JSON.stringify(LIVE)));
    render(<ForwardRules grant={GRANT} deviceId="device-1" />);
    fireEvent.click(await screen.findByRole("button", { name: "Add a rule" }));

    fireEvent.click(screen.getByLabelText("Key file"));
    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "console" } });
    fireEvent.change(screen.getByLabelText("Listen (this machine)"), {
      target: { value: "100.85.163.159:8080" }
    });
    fireEvent.change(screen.getByLabelText("Target (the far side dials)"), {
      target: { value: "10.10.61.83:7738" }
    });
    fireEvent.change(screen.getByLabelText("Via (the SSH connection)"), {
      target: { value: "zss@10.10.61.83:22122" }
    });
    fireEvent.change(screen.getByLabelText("Key path (on this machine)"), {
      target: { value: "id_ed25519" }
    });
    fireEvent.change(screen.getByLabelText("Host key fingerprint (optional)"), {
      target: { value: "SHA256:abc" }
    });
    fireEvent.click(screen.getByRole("button", { name: "Add rule" }));

    // A relative path resolves against whatever the SERVICE is running from, which is not where a
    // key was put — so the rule would fail at boot, with nobody there to read it.
    expect(await screen.findByText(/must be absolute/)).toBeTruthy();
    expect(called("forward.add").length).toBe(0);

    fireEvent.change(screen.getByLabelText("Key path (on this machine)"), {
      target: { value: "/home/zss/.ssh/id_ed25519" }
    });
    fireEvent.click(screen.getByRole("button", { name: "Add rule" }));
    await waitFor(() => expect(called("forward.add").length).toBe(1));
    expect(called("forward.add")[0][2]).toMatchObject({
      auth: { kind: "key", path: "/home/zss/.ssh/id_ed25519" },
      host_key: "SHA256:abc"
    });
  });

  it("removes a rule only after naming it, and reports `false` as a present answer", async () => {
    device((name) => (name === "forward.list" ? JSON.stringify([LIVE]) : '{"removed":true}'));
    render(<ForwardRules grant={GRANT} deviceId="device-1" />);
    await screen.findByText("console");

    fireEvent.click(screen.getByRole("button", { name: "Remove the forward rule console" }));
    // The confirm names the rule, the address it stops carrying, and the connections it has carried.
    expect(screen.getByText(/Remove console?/)).toBeTruthy();
    expect(screen.getByText(/Connections through 100.85.163.159:8080/)).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Remove" }));
    await waitFor(() => expect(called("forward.remove").length).toBe(1));
    expect(called("forward.remove")[0][2]).toEqual({ name: "console" });
    // The list is re-read after the act, so the row that was removed is gone by the device's answer.
    await waitFor(() => expect(called("forward.list").length).toBe(2));
  });

  it("shows the device's sentence when a removal is refused, and leaves the row standing", async () => {
    device((name) => {
      if (name === "forward.list") return JSON.stringify([LIVE]);
      throw new ToolError("the rule is still there", 200, -32002, true);
    });
    render(<ForwardRules grant={GRANT} deviceId="device-1" />);
    fireEvent.click(await screen.findByRole("button", { name: "Remove the forward rule console" }));
    fireEvent.click(screen.getByRole("button", { name: "Remove" }));

    expect(await screen.findByText("the rule is still there")).toBeTruthy();
    // The confirm stays open on a refusal: a sheet that closed would read as success.
    expect(screen.getByText(/Remove console?/)).toBeTruthy();
    expect(screen.getByText("console")).toBeTruthy();
  });

  it("does not read a device's unreadable answer as a machine with no rules", async () => {
    device(() => "not json");
    render(<ForwardRules grant={GRANT} deviceId="device-1" />);

    // **AN EMPTY LIST AND AN UNREADABLE ANSWER ARE DIFFERENT FACTS.** Drawing the second as the
    // first would tell an operator their machine carries no rule while it carries one.
    expect(await screen.findByText(/answered forward\.list without a list/)).toBeTruthy();
    expect(screen.queryByText(/No forward rules/)).toBeNull();
  });

  it("never draws a credential the device did not send", async () => {
    // A device that DID leak an `auth` field would still not have it printed: the page renders the
    // nine fields it knows, not the object it was handed.
    device(() => JSON.stringify([{ ...LIVE, auth: { kind: "password", password: "hunter2" } }]));
    render(<ForwardRules grant={GRANT} deviceId="device-1" />);
    await screen.findByText("console");

    expect(screen.queryByText("hunter2")).toBeNull();
    expect(within(screen.getByRole("region", { name: "Forward rules" })).queryByText(/password/i)).toBeNull();
  });
});
