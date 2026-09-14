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
import { UpdateCard, parseUpdateStatus, type UpdateStatus } from "../UpdateCard";
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
  error: null,
  ...over,
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
