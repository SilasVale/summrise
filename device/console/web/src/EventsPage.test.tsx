import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { EventsPage } from "./EventsPage";
import { ToolError, type Grant } from "./terminal";

const mocks = vi.hoisted(() => ({ callTool: vi.fn() }));

vi.mock("./terminal", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./terminal")>();
  return { ...actual, callTool: mocks.callTool };
});

const grant: Grant = { deviceId: "device-1", envelope: { token: "envelope" } };

const health = {
  status: "ok",
  version: "0.3.44",
  pid: 42,
  poisoned: false
};

describe("EventsPage", () => {
  afterEach(() => {
    mocks.callTool.mockReset();
  });

  it("shows the device's health and the event tail newest first, with each event's own fields", async () => {
    mocks.callTool.mockResolvedValue(
      JSON.stringify({
        state: { schema: 1 },
        health,
        events: [
          { ts_ms: 1, kind: "repaired", from: "baseline" },
          { ts_ms: 2, kind: "state_quarantined" }
        ]
      })
    );
    render(<EventsPage grant={grant} />);

    expect(await screen.findByText("state_quarantined")).toBeInTheDocument();
    const facts = screen.getByLabelText("Device health");
    expect(within(facts).getByText("ok")).toBeInTheDocument();
    expect(within(facts).getByText("0.3.44")).toBeInTheDocument();
    expect(within(facts).getByText("42")).toBeInTheDocument();
    expect(screen.getByText("The device keeps its last ten events.")).toBeInTheDocument();

    const rows = screen.getAllByRole("row");
    expect(within(rows[1]).getByText("state_quarantined")).toBeInTheDocument();
    expect(within(rows[1]).getByText(new Date(2).toLocaleString())).toBeInTheDocument();
    expect(within(rows[2]).getByText("repaired")).toBeInTheDocument();
    expect(within(rows[2]).getByText("from=baseline")).toBeInTheDocument();
    expect(within(rows[2]).getByText(new Date(1).toLocaleString())).toBeInTheDocument();
    expect(mocks.callTool).toHaveBeenCalledWith(grant, "state.get", {});

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await vi.waitFor(() => expect(mocks.callTool).toHaveBeenCalledTimes(2));
  });

  it("shows degraded health and the reason when the document is poisoned, and still shows the events", async () => {
    mocks.callTool.mockResolvedValue(
      JSON.stringify({
        state: null,
        health: {
          ...health,
          status: "degraded",
          poisoned: true,
          reason: "state: the state file is corrupt"
        },
        events: [{ ts_ms: 3, kind: "state_quarantined" }]
      })
    );
    render(<EventsPage grant={grant} />);

    expect(
      await screen.findByText("The device's state document is poisoned, so its health is degraded.")
    ).toBeInTheDocument();
    expect(screen.getByText("state: the state file is corrupt")).toBeInTheDocument();
    expect(screen.getByText("state_quarantined")).toBeInTheDocument();
  });

  it("says the device has recorded no events when the tail is empty", async () => {
    mocks.callTool.mockResolvedValue(
      JSON.stringify({ state: { schema: 1 }, health, events: [] })
    );
    render(<EventsPage grant={grant} />);

    expect(await screen.findByText("The device has recorded no events.")).toBeInTheDocument();
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
    expect(within(screen.getByLabelText("Device health")).getByText("0.3.44")).toBeInTheDocument();
  });

  it("shows the device's sentence when the read fails and invents no rows", async () => {
    mocks.callTool.mockRejectedValue(
      new ToolError("the grant has expired", 403, -32001)
    );
    render(<EventsPage grant={grant} />);

    const failure = await screen.findByText("the grant has expired");
    expect(failure).toHaveClass("error");
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Device health")).not.toBeInTheDocument();
  });

  it("treats a device too old to send events as a device with an empty tail", async () => {
    // The additive field is how the page reaches an older device; the absence is the empty tail,
    // not a crash that would blank the health the device can still report.
    mocks.callTool.mockResolvedValue(JSON.stringify({ state: { schema: 1 }, health }));
    render(<EventsPage grant={grant} />);

    expect(await screen.findByText("The device has recorded no events.")).toBeInTheDocument();
    expect(within(screen.getByLabelText("Device health")).getByText("0.3.44")).toBeInTheDocument();
  });

  it("says the device could not read its event log when events is null", async () => {
    mocks.callTool.mockResolvedValue(
      JSON.stringify({ state: { schema: 1 }, health, events: null })
    );
    render(<EventsPage grant={grant} />);

    expect(await screen.findByText("The device could not read its event log.")).toBeInTheDocument();
    expect(within(screen.getByLabelText("Device health")).getByText("0.3.44")).toBeInTheDocument();
    expect(screen.queryByText("The device has recorded no events.")).not.toBeInTheDocument();
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
  });

  it("renders an object-valued kind as JSON rather than [object Object]", async () => {
    mocks.callTool.mockResolvedValue(
      JSON.stringify({
        state: { schema: 1 },
        health,
        events: [{ ts_ms: 3, kind: { nested: true } }]
      })
    );
    render(<EventsPage grant={grant} />);

    expect(await screen.findByText('{"nested":true}')).toBeInTheDocument();
    expect(screen.queryByText("[object Object]")).not.toBeInTheDocument();
  });

  it("shows an audit line's event as its kind and does not repeat it among the fields", async () => {
    // The device's tool-call lines say `event: "begin"`, not `kind`; the column is the event's
    // label, so the label must not also appear as `event=begin` beside `tool=...`.
    mocks.callTool.mockResolvedValue(
      JSON.stringify({
        state: { schema: 1 },
        health,
        events: [{ ts_ms: 3, event: "begin", tool: "state.get" }]
      })
    );
    render(<EventsPage grant={grant} />);

    expect(await screen.findByText("tool=state.get")).toBeInTheDocument();
    const rows = screen.getAllByRole("row");
    expect(within(rows[1]).getByText("begin")).toBeInTheDocument();
    expect(within(rows[1]).queryByText("event=begin")).not.toBeInTheDocument();
  });

  it("renders an event that is a string as text rather than an object", async () => {
    mocks.callTool.mockResolvedValue(
      JSON.stringify({
        state: { schema: 1 },
        health,
        events: ["a line the device wrote but did not encode as JSON"]
      })
    );
    render(<EventsPage grant={grant} />);

    expect(
      await screen.findByText("a line the device wrote but did not encode as JSON")
    ).toBeInTheDocument();
    expect(screen.queryByText("[object Object]")).not.toBeInTheDocument();
  });
});
