// UpdateCard — "is this device current, and can I do something about it?"
//
// This is the one card in Settings that CHANGES the device, so its states are what these
// tests pin — and every one of them is a way an update surface misleads an operator:
//
//   * an unreachable release server must NOT read as "up to date" (unknown is not no);
//   * a purely local install says so, and offers no button;
//   * a rollback pin is reported, because the device would REFUSE an update the card
//     otherwise offers;
//   * the button POSTs the SAME tool an AI calls, once, and only after a confirm;
//   * a swap is RECOGNISED by watching the release change (nothing can report "done" —
//     the agent dies mid-update), and the panel says what it is now running.
import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { UpdateCard, parseUpdateStatus, type UpdateStatus, checkedAge, parseAttempt, attemptAge } from "../UpdateCard";
import { callApi } from "../../lib/api";

vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  callApi: vi.fn(),
}));
const mockCallApi = callApi as unknown as ReturnType<typeof vi.fn>;

const status = (over: Partial<UpdateStatus> = {}): UpdateStatus => ({
  current: "1.2.369",
  channel: "https://agent.saisi.online",
  latest: "1.2.369",
  updateAvailable: false,
  pinnedTo: null,
  busy: false,
  checkedAt: null,
  lastAttempt: null,
  error: null,
  ...over,
});

it("the card states WHEN the device last asked, from the device's own checked_at", async () => {
  mockCallApi.mockResolvedValue({
    ok: true,
    current: "1.2.403",
    channel: "stable",
    latest: "1.2.433",
    update_available: true,
    pinned_to: null,
    busy: false,
    error: null,
    checked_at: 1_700_000_000_000,
  });
  const { container } = render(
    <UpdateCard
      status={parseUpdateStatus({
        ok: true,
        current: "1.2.403",
        channel: "stable",
        latest: "1.2.433",
        update_available: true,
        checked_at: 1_700_000_000_000,
      })}
      refresh={vi.fn(async () => {})}
      // 40 seconds after the check: the age is in SECONDS, because that is the unit that matters while an
      // update may be in flight.
      nowMs={1_700_000_040_000}
    />,
  );
  const age = container.querySelector(".update-checked");
  expect(age, "the age of the device's answer must be on the card").toBeTruthy();
  expect(age!.textContent).toBe("checked 40s ago");
  expect(age!.getAttribute("title") || "").toMatch(/last asked its channel at/);
});

it("a device that never checked says NOTHING rather than an age", () => {
  // The rule the vitals and boot records follow: absent, never zero. A missing `checked_at` renders no line —
  // "checked 56 years ago" would be a claim about a device that simply has not answered that question.
  expect(checkedAge(null, Date.now())).toBeNull();
  expect(checkedAge(0, Date.now()), "zero is not a time").toBeNull();
  expect(parseUpdateStatus({ checked_at: "1700000000000" }).checkedAt, "a STRING is not epoch ms").toBeNull();
  expect(parseUpdateStatus({ checked_at: -5 }).checkedAt).toBeNull();
  expect(checkedAge(1_700_000_000_000, 1_700_000_040_000)).toBe("checked 40s ago");
  expect(checkedAge(1_700_000_000_000, 1_700_000_600_000)).toBe("checked 10m ago");
  expect(checkedAge(1_700_000_000_000, 1_700_010_000_000)).toBe("checked 3h ago");
});

it("the age of an ACT reads as an age, not as a reading", () => {
  // Two formatters on purpose: `checkedAge` is the age of a READING ("checked 2m ago") and `attemptAge` the age of
  // an ACT ("10m ago"), because the sentence around the second already says what happened and repeating the reading
  // verb there would describe the wrong event. A record with no usable time must still produce a sentence.
  expect(attemptAge(1_700_000_000_000, 1_700_000_040_000)).toBe("40s ago");
  expect(attemptAge(1_700_000_000_000, 1_700_000_600_000)).toBe("10m ago");
  expect(attemptAge(1_700_000_000_000, 1_700_010_000_000)).toBe("3h ago");
  expect(attemptAge(null, Date.now())).toBe("at an unknown time");
  expect(attemptAge(0, Date.now())).toBe("at an unknown time");
});

it("states the last update the DEVICE launched, from its own record", () => {
  // The fact that answers "did my click do anything". It comes from the device (`record_update_attempt` at the
  // moment the swap script is handed to WMI) rather than from a reading of summrise-update.log, which two programs
  // write and which says nothing at all about whether the swap STARTED.
  const { container } = render(
    <UpdateCard
      status={status({
        lastAttempt: { atMs: 1_700_000_000_000, from: "1.2.403", to: "1.2.435" },
      })}
      refresh={vi.fn(async () => {})}
      nowMs={1_700_000_600_000}
    />,
  );
  const line = container.querySelector(".update-attempt");
  expect(line, "the launch record must be on the card").toBeTruthy();
  expect(line!.textContent).toContain("1.2.403");
  expect(line!.textContent).toContain("1.2.435");
  expect(container.querySelector(".update-attempt-age")!.textContent).toBe("10m ago");
  expect(line!.getAttribute("data-to")).toBe("1.2.435");
});

it("a device that never launched an update says nothing about one", () => {
  const { container } = render(
    <UpdateCard status={status({})} refresh={vi.fn(async () => {})} />,
  );
  expect(container.querySelector(".update-attempt")).toBeNull();
});

it("HALF a launch record is not a record: the two ends of the wire are checked independently", () => {
  // The device refuses a body without `at_ms` (its own test says so); this refuses one too, because the two ends
  // drift independently and half a record renders as "updated from ? to ? at Invalid Date".
  expect(parseAttempt(null)).toBeNull();
  expect(parseAttempt({})).toBeNull();
  expect(parseAttempt({ at_ms: 0, from: "1.2.1", to: "1.2.2" })).toBeNull();
  expect(parseAttempt({ at_ms: 1_700_000_000_000 })).toBeNull();
  expect(parseAttempt({ at_ms: 1_700_000_000_000, from: "1.2.1", to: "1.2.2" })).toEqual({
    atMs: 1_700_000_000_000,
    from: "1.2.1",
    to: "1.2.2",
  });
  // A record with only a `from` is still worth showing — the build it came FROM is the half an operator needs to
  // know what they are running now.
  expect(parseAttempt({ at_ms: 1_700_000_000_000, from: "1.2.1" })).toEqual({
    atMs: 1_700_000_000_000,
    from: "1.2.1",
    to: "",
  });
});

const card = (over: Partial<UpdateStatus> = {}, extra: Record<string, unknown> = {}) =>
  render(
    <UpdateCard
      status={status(over)}
      refresh={vi.fn(async () => {})}
      runningRelease={(extra.runningRelease as string) ?? "1.2.369"}
    />,
  );

describe("parseUpdateStatus", () => {
  it("reads the device's answer and treats a missing field as unknown, not as false-y truth", () => {
    const s = parseUpdateStatus({
      ok: true,
      current: "1.2.369",
      channel: "https://agent.saisi.online",
      latest: "1.2.370",
      update_available: true,
      pinned_to: "1.2.368",
      busy: false,
      error: null,
    });
    expect(s.updateAvailable).toBe(true);
    expect(s.pinnedTo).toBe("1.2.368");
    expect(s.error).toBeNull();
    expect(parseUpdateStatus(null).current).toBe("");
    expect(parseUpdateStatus({ update_available: "yes" }).updateAvailable).toBe(false);
  });
});

describe("UpdateCard", () => {
  beforeEach(() => mockCallApi.mockReset());

  it("says which build is running when it is the latest", () => {
    const { container } = card();
    expect(container.textContent).toContain("running 1.2.369");
    expect(container.textContent).toContain("latest is 1.2.369");
    // Up to date still offers the REPAIR action, and says what it is for.
    expect(screen.getByRole("button").textContent).toContain("Reinstall 1.2.369");
    expect(container.textContent).toContain("repair path");
  });

  it("offers the newer build when there is one", () => {
    const { container } = card({ latest: "1.2.370", updateAvailable: true });
    expect(container.textContent).toContain("1.2.370 available");
    expect(screen.getByRole("button").textContent).toContain("Update to 1.2.370");
  });

  it("never shows 'up to date' when the release server did not answer", () => {
    const { container } = card({ latest: null, error: "release server unreachable: timeout" });
    expect(container.querySelector('[data-kind="unreachable"]')).not.toBeNull();
    expect(container.textContent).toContain("unknown");
    // No action is offered against an unknown channel state either.
    expect(container.querySelector(".update-actions")).toBeNull();
  });

  it("reports a purely local install as a supported way to run, with no button", () => {
    const { container } = card({ channel: null, latest: null });
    expect(container.textContent).toContain("No update channel is configured");
    expect(container.querySelector(".update-actions")).toBeNull();
    expect(container.querySelector('[data-kind="unreachable"]')).toBeNull();
  });

  it("names a rollback pin instead of offering an update the device would refuse", () => {
    const { container } = card({ latest: "1.2.370", updateAvailable: false, pinnedTo: "1.2.368" });
    expect(container.querySelector('[data-kind="pinned"]')!.textContent).toContain("1.2.368");
    expect(container.textContent).toContain("will refuse");
  });

  it("will not offer a second update while one is in flight", () => {
    const { container } = card({ latest: "1.2.370", updateAvailable: true, busy: true });
    expect(container.querySelector('[data-kind="busy"]')).not.toBeNull();
    expect(container.querySelector(".update-actions")).toBeNull();
  });

  it("asks before it acts, and then calls the SAME tool an AI would", async () => {
    mockCallApi.mockResolvedValue({ ok: true, result: { status: "upgrading", remote: "1.2.370" } });
    card({ latest: "1.2.370", updateAvailable: true });

    fireEvent.click(screen.getByRole("button", { name: /Update to 1.2.370/ }));
    // The confirm step states the cost BEFORE anything happens.
    expect(screen.getByText(/will restart the agent/)).toBeTruthy();
    expect(mockCallApi).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: /Yes, update to 1.2.370/ }));
    await waitFor(() => expect(mockCallApi).toHaveBeenCalledTimes(1));
    const [path, init] = mockCallApi.mock.calls[0];
    expect(path).toBe("/api/tools/agent_update");
    expect(init.method).toBe("POST");
    // Not forced: an update to a NEWER build must still respect the device's own pin logic.
    expect(JSON.parse(init.body)).toEqual({});
  });

  it("forces only for a REINSTALL, which is what force means", async () => {
    mockCallApi.mockResolvedValue({ ok: true, result: { status: "upgrading", remote: "1.2.369" } });
    card();
    fireEvent.click(screen.getByRole("button", { name: /Reinstall/ }));
    fireEvent.click(screen.getByRole("button", { name: /Yes, reinstall/ }));
    await waitFor(() => expect(mockCallApi).toHaveBeenCalledTimes(1));
    expect(JSON.parse(mockCallApi.mock.calls[0][1].body)).toEqual({ force: true });
  });

  it("recognises the swap by watching the release change, and says what it runs now", async () => {
    mockCallApi.mockResolvedValue({ ok: true, result: { status: "upgrading", remote: "1.2.370" } });
    const refresh = vi.fn(async () => {});
    const { rerender } = render(
      <UpdateCard
        status={status({ latest: "1.2.370", updateAvailable: true })}
        refresh={refresh}
        runningRelease="1.2.369"
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /Update to 1.2.370/ }));
    fireEvent.click(screen.getByRole("button", { name: /Yes, update to 1.2.370/ }));
    await waitFor(() => expect(screen.getByText(/reconnects on the new build/)).toBeTruthy());

    // The agent dies mid-update, so nothing reports completion — the RELEASE does.
    rerender(
      <UpdateCard
        status={status({ current: "1.2.370", latest: "1.2.370" })}
        refresh={refresh}
        runningRelease="1.2.370"
      />,
    );
    await waitFor(() => expect(screen.getByText(/now running 1.2.370/)).toBeTruthy());
    expect(refresh).toHaveBeenCalled();
  });

  it("reports a refused call instead of pretending it worked", async () => {
    mockCallApi.mockResolvedValue({ ok: false, error: "device is pinned" });
    const { container } = card({ latest: "1.2.370", updateAvailable: true });
    fireEvent.click(screen.getByRole("button", { name: /Update to 1.2.370/ }));
    fireEvent.click(screen.getByRole("button", { name: /Yes, update to/ }));
    await waitFor(() => expect(container.querySelector(".update-state.is-error")).not.toBeNull());
    expect(container.textContent).toContain("device is pinned");
  });
});
