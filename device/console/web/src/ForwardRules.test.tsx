import { fireEvent, render, screen, within } from "@testing-library/react";
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

/// The tool list this build advertises. `forward_list` present means the machine can carry rules at
/// all — the fact the section gates its first read on.
const liveTools = vi.hoisted(() => ({
  tools: ["forward_list", "forward_probe"] as string[],
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
 * One rule's wire shape, copied from the DEVICE'S OWN ANSWER (the hundred-and-fortieth list).
 *
 * `agent/src/plugins/forward/tools.rs` builds each row as
 * `{"name", "listen", "target", "via", "state": <forward::ForwardState>}`, and the whole answer as
 * `{"rules": [...]}`. The previous fixture here was a bare array of FLAT rows and its doc comment
 * claimed it was "copied from `forward.list`" — it was copied from this page's reader, so the suite
 * was green while the page could not read a device at all.
 *
 * `state` carries no credential: the device stores `auth` inside the rule and `forward_list` reports
 * the row without it, which is why this page can be drawn at all.
 */
type Wire = {
  name: string;
  listen: string;
  target: string;
  via: string;
  state: {
    name: string;
    listen: string;
    target: string;
    via: string;
    listening: boolean;
    /// 0 means the executor has never listened — not 1970.
    since_ms: number;
    attempts: number;
    connections: number;
    last_error: string | null;
  };
};

const LIVE: Wire = {
  name: "console",
  listen: "100.85.163.159:8080",
  target: "10.10.61.83:7738",
  via: "zss@10.10.61.83:22122",
  state: {
    name: "console",
    listen: "100.85.163.159:8080",
    target: "10.10.61.83:7738",
    via: "zss@10.10.61.83:22122",
    listening: true,
    since_ms: 1_000_000,
    attempts: 1,
    connections: 41,
    last_error: null
  }
};

const BROKEN: Wire = {
  name: "office",
  listen: "100.85.163.159:8081",
  target: "10.10.61.83:7736",
  via: "zss@10.10.61.83:22122",
  state: {
    name: "office",
    listen: "100.85.163.159:8081",
    target: "10.10.61.83:7736",
    via: "zss@10.10.61.83:22122",
    listening: false,
    since_ms: 0,
    attempts: 4,
    connections: 0,
    last_error: "connecting to via zss@127.0.0.1:1 failed: Connection refused (os error 111)"
  }
};

/** The device's answer, as its tool builds it. */
const answer = (rows: unknown[]) => JSON.stringify({ rules: rows });

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
    liveTools.tools = ["forward_list", "forward_probe"];
    liveTools.loading = false;
    liveTools.error = null;
  });

  it("reads the machine's rules and says what each one is doing", async () => {
    device(() => answer([LIVE, BROKEN]));
    render(<ForwardRules grant={GRANT} deviceId="device-1" />);

    expect(await screen.findByText("console")).toBeTruthy();
    expect(screen.getByText("office")).toBeTruthy();

    // The state is a WORD, not a colour: a bound rule and a failing one are told apart by reading.
    expect(screen.getByText("Listening")).toBeTruthy();
    expect(screen.getByText("Not listening")).toBeTruthy();

    // The route reads left to right: bound here → reaches this → carried by that.
    expect(
      screen.getByText("100.85.163.159:8080 → 10.10.61.83:7738 via zss@10.10.61.83:22122")
    ).toBeTruthy();

    // **CARRIED, NOT ARRIVED.** The device's count is both directions ended without an error.
    expect(screen.getByText(/41 connections carried/)).toBeTruthy();
    expect(screen.getByText(/4 bind attempts/)).toBeTruthy();

    // A failing rule's own sentence is on the page; a healthy rule has none.
    expect(screen.getByText(BROKEN.state.last_error as string)).toBeTruthy();
    // **THE DEVICE'S TOOL NAME, NOT THIS PAGE'S** (the hundred-and-fortieth list): `forward_list`.
    expect(called("forward_list").length).toBe(1);
  });

  it("says a machine with no rules has none, and where a rule comes from instead of a button", async () => {
    device(() => answer([]));
    render(<ForwardRules grant={GRANT} deviceId="device-1" />);

    expect(await screen.findByText(/No forward rules/)).toBeTruthy();
    // **THE PAGE READS; IT DOES NOT CREATE.** The device registers no `forward_add` (design 0015 D6),
    // so the button that called `forward.add` is gone and the section says where a rule is written.
    expect(screen.queryByRole("button", { name: "Add a rule" })).toBeNull();
    expect(screen.getByText(/forwards\.json/)).toBeTruthy();
    // A catalogue of things is not a state: no Refresh is offered for a list that cannot be read
    // into anything.
    expect(screen.queryByRole("button", { name: "Refresh" })).toBeNull();
  });

  it("offers no way to create or remove a rule, because the device has none", async () => {
    device(() => answer([LIVE]));
    render(<ForwardRules grant={GRANT} deviceId="device-1" />);
    await screen.findByText("console");

    const section = within(screen.getByRole("region", { name: "Forward rules" }));
    expect(section.queryByRole("button", { name: "Add a rule" })).toBeNull();
    expect(section.queryByRole("button", { name: /Remove/ })).toBeNull();
    // Only the read is offered, and it is a read.
    expect(section.getByRole("button", { name: "Refresh" })).toBeTruthy();
  });

  it("says an OLDER build cannot carry rules, instead of a refusal with a Retry", async () => {
    liveTools.tools = ["monitor.list"];
    try {
      device(() => answer([]));
      render(<ForwardRules grant={GRANT} deviceId="device-old" />);

      expect(await screen.findByText("Older build")).toBeTruthy();
      expect(screen.getByText(/does not advertise the forward tools yet/)).toBeTruthy();
      // The proof that this is a sentence and not a failure: nothing was asked.
      expect(called("forward_list").length).toBe(0);
    } finally {
      liveTools.tools = ["forward_list", "forward_probe"];
    }
  });

  it("measures a rule's age on ONE clock, so a device ahead of the console is not a bogus age", async () => {
    // **A DEVICE WHOSE CLOCK IS AHEAD MAKES EVERY RULE LOOK OLDER THAN IT IS.** `since_ms` is the
    // device's; `Date.now()` is the browser's. A time server two minutes apart is not exotic, and
    // the result would be `bound 2 m` the instant a rule binds — a number that reads as a fact.
    const now = Date.now();
    device(() =>
      answer([
        { ...LIVE, state: { ...LIVE.state, since_ms: now + 120_000, listening: true } }, // 2 min AHEAD
        { ...LIVE, name: "behind", state: { ...LIVE.state, since_ms: now - 90_000, listening: true } }
      ])
    );
    render(<ForwardRules grant={GRANT} deviceId="device-1" />);

    await screen.findByText("console");
    // The ahead case states the disagreement rather than inventing a negative age.
    expect(screen.getByText(/this machine's clock is ahead of the console's/)).toBeTruthy();
    // The behind case is a real age, and it is NOT what a skew would have produced.
    expect(screen.getByText(/bound 1 m 30 s/)).toBeTruthy();
    expect(screen.queryByText(/bound 3 m 30 s/)).toBeNull();
  });

  it("reads a rule's never-bound state as never, not as 1970", async () => {
    // **`since_ms: 0` IS THE DEVICE'S "NEVER LISTENED"** (`forward::ForwardState`: "0 when it never has").
    // Read as a timestamp it is 1970, and the page would say a rule has been bound for 56 years.
    device(() => answer([BROKEN]));
    render(<ForwardRules grant={GRANT} deviceId="device-1" />);
    await screen.findByText("office");

    // The state is the word, and the line is the attempts — never an age computed from a 1970 stamp.
    expect(screen.getByText("Not listening")).toBeTruthy();
    expect(screen.getByText(/4 bind attempts/)).toBeTruthy();
    expect(screen.queryByText(/bound /)).toBeNull();
  });

  it("does not ask a machine the fleet says is down, and says why", async () => {
    device(() => answer([]));
    render(<ForwardRules grant={GRANT} deviceId="device-1" deviceOnline={false} />);

    expect(screen.getByText(/The machine is offline/)).toBeTruthy();
    expect(called("forward_list").length).toBe(0);
  });

  it("shows the device's own sentence when the list is refused, and retries", async () => {
    let attempt = 0;
    device(() => {
      attempt += 1;
      if (attempt === 1) throw new ToolError("the device refused the read", 200, -32001, true);
      return answer([LIVE]);
    });
    render(<ForwardRules grant={GRANT} deviceId="device-1" />);

    expect(await screen.findByText("the device refused the read")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("console")).toBeTruthy();
  });

  it("does not read a device's unreadable answer as a machine with no rules", async () => {
    // **A BARE ARRAY IS NOT THIS DEVICE'S ANSWER.** It is what this page used to expect; a reader that
    // accepted it would keep the schema drift invisible, and a reader that treated it as empty would
    // tell an operator their machine carries no rule while it carries one.
    device(() => JSON.stringify([LIVE]));
    render(<ForwardRules grant={GRANT} deviceId="device-1" />);

    expect(await screen.findByText(/answered forward_list without a list/)).toBeTruthy();
    expect(screen.queryByText(/No forward rules/)).toBeNull();
  });

  it("never draws a credential the device did not send", async () => {
    // A device that DID leak an `auth` field would still not have it printed: the page renders the
    // nine fields it knows, not the object it was handed.
    device(() => answer([{ ...LIVE, auth: { kind: "password", password: "hunter2" } }]));
    render(<ForwardRules grant={GRANT} deviceId="device-1" />);
    await screen.findByText("console");

    expect(screen.queryByText("hunter2")).toBeNull();
    expect(
      within(screen.getByRole("region", { name: "Forward rules" })).queryByText(/password/i)
    ).toBeNull();
  });
});
