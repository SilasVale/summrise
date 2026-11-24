import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ServicesWatched } from "./ServicesWatched";
import { callTool, type Grant } from "./terminal";

vi.mock("./terminal", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./terminal")>();
  return { ...actual, callTool: vi.fn() };
});

const mocked = vi.mocked(callTool);
const GRANT: Grant = { deviceId: "device-1", envelope: { token: "envelope" } };

/// The device's answer (`monitor_list` in `agent/src/plugins/monitor/tools.rs`): a target and `monitor::summary_of`'s payload,
/// where every fact is `null` until a probe exists.
const UP = {
  id: "9vh-console",
  host: "127.0.0.1",
  port: 19000,
  path: "/health",
  summary: {
    probes: 40,
    up: 39,
    down: 1,
    up_pct: 97.5,
    up_now: true,
    since_ms: 1_762_000_000_000,
    latency: { min_ms: 1, max_ms: 9, last_ms: 2 },
    drops: 1,
    last_status: 200,
    last_expect_ok: true
  }
};

const answer = (targets: unknown[]) => JSON.stringify({ ok: true, interval_secs: 15, targets });

beforeEach(() => { mocked.mockReset(); });

describe("ServicesWatched — what the device probes", () => {
  it("draws a target with its address, its state word, and the summary's numbers", async () => {
    mocked.mockResolvedValue(answer([UP]));
    render(<ServicesWatched grant={GRANT} />);
    await waitFor(() => expect(screen.getByText("Up")).toBeInTheDocument());
    expect(mocked).toHaveBeenCalledWith(GRANT, "monitor.list", {});
    expect(screen.getByText("127.0.0.1:19000/health")).toBeInTheDocument();
    expect(screen.getByText("98% of 40 probes")).toBeInTheDocument();
    expect(screen.getByText("1 drop")).toBeInTheDocument();
    expect(screen.getByText("HTTP 200")).toBeInTheDocument();
    expect(screen.getByText(/probed every 15 s/)).toBeInTheDocument();
  });

  it("reads the BARE ARRAY the shipped device answers with, and draws it as empty", async () => {
    // Measured on the live fleet 2026-11-12: `monitor.list` on 0.3.292 answers `[]`, not `{ok, targets}`, and the page said
    // "the machine answered monitor.list without a list" for a machine with nothing to watch.
    mocked.mockResolvedValue("[]");
    render(<ServicesWatched grant={GRANT} />);
    await waitFor(() =>
      expect(screen.getByText("This machine watches no services.")).toBeInTheDocument()
    );
  });

  it("draws an empty watch list as a sentence, not a blank section", async () => {
    mocked.mockResolvedValue(answer([]));
    render(<ServicesWatched grant={GRANT} />);
    await waitFor(() =>
      expect(screen.getByText("This machine watches no services.")).toBeInTheDocument()
    );
  });

  it("never calls an unprobed target DOWN — a null is its own word", async () => {
    mocked.mockResolvedValue(
      answer([
        {
          id: "new",
          host: "example.test",
          port: 443,
          path: null,
          summary: { probes: 0, up: 0, down: 0, up_pct: null, up_now: null, since_ms: null, drops: null, last_status: null }
        }
      ])
    );
    render(<ServicesWatched grant={GRANT} />);
    await waitFor(() => expect(screen.getByText("Not probed")).toBeInTheDocument());
    expect(screen.queryByText("Down")).toBeNull();
    expect(screen.queryByText("0% of 0 probes")).toBeNull();
  });

  it("says a refused read in the device's words and offers the retry", async () => {
    mocked.mockResolvedValueOnce(JSON.stringify({ ok: false, error: "the watch list is unreadable" }));
    render(<ServicesWatched grant={GRANT} />);
    await waitFor(() =>
      expect(screen.getByText(/the watch list is unreadable/)).toBeInTheDocument()
    );
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  it("adds a watch through the device, and says where the watch lives", async () => {
    mocked.mockResolvedValueOnce(answer([])).mockResolvedValueOnce(JSON.stringify({ ok: true }));
    mocked.mockResolvedValue(answer([UP]));
    render(<ServicesWatched grant={GRANT} />);
    await waitFor(() =>
      expect(screen.getByText("This machine watches no services.")).toBeInTheDocument()
    );
    fireEvent.change(screen.getByLabelText("Host to watch"), { target: { value: "127.0.0.1" } });
    fireEvent.change(screen.getByLabelText("Port to watch"), { target: { value: "19000" } });
    fireEvent.click(screen.getByRole("button", { name: "Start watching" }));
    await waitFor(() =>
      expect(mocked).toHaveBeenCalledWith(GRANT, "monitor.add", { kind: "tcp", host: "127.0.0.1", port: 19000, interval_ms: 15_000 })
    );
    await waitFor(() => expect(screen.getByText(/the machine probes it every 15 s/)).toBeInTheDocument());
  });

  it("asks before it stops a watch, and names what stopping discards", async () => {
    mocked.mockResolvedValue(answer([UP]));
    render(<ServicesWatched grant={GRANT} />);
    await waitFor(() => expect(screen.getByText("Up")).toBeInTheDocument());
    fireEvent.click(screen.getByRole("button", { name: "Stop watching" }));
    expect(screen.getByText(/Its series is discarded/)).toBeInTheDocument();
    expect(mocked).not.toHaveBeenCalledWith(GRANT, "monitor.remove", expect.anything());
    mocked.mockResolvedValueOnce(JSON.stringify({ ok: true }));
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    await waitFor(() => expect(mocked).toHaveBeenCalledWith(GRANT, "monitor.remove", { id: UP.id }));
  });

  it("takes a sample NOW through monitor.probe and re-reads the list", async () => {
    mocked.mockResolvedValue(answer([UP]));
    render(<ServicesWatched grant={GRANT} />);
    await waitFor(() => expect(screen.getByText("Up")).toBeInTheDocument());
    mocked.mockResolvedValueOnce(JSON.stringify({ ts: 1_762_000_000_000, ok: true, latency_ms: 3 }));
    fireEvent.click(screen.getByRole("button", { name: "Probe now" }));
    await waitFor(() => expect(mocked).toHaveBeenCalledWith(GRANT, "monitor.probe", { id: UP.id }));
    // and the row is the DEVICE's summary again, not a guess by the page
    await waitFor(() => expect(mocked.mock.calls.filter(([, n]) => n === "monitor.list").length).toBeGreaterThan(1));
  });

  it.each([
    { initialState: "Up", upPct: 50, drops: 1, window: "50% up in the window" },
    { initialState: "Not probed", upPct: 0, drops: 0, window: "0% up in the window" }
  ])(
    "reloads the device's Down summary after a real down sample replaces $initialState",
    async ({ initialState, upPct, drops, window }) => {
      const target = {
        id: "127.0.0.1:19000/health",
        kind: "http",
        host: "127.0.0.1",
        port: 19000,
        path: "/health",
        expect: null,
        interval_ms: 15_000,
        summary: {
          up_now: initialState === "Up" ? true : null,
          since_ms: initialState === "Up" ? 1_762_000_000_000 : null,
          up_pct: initialState === "Up" ? 100 : null,
          drops: initialState === "Up" ? 0 : null,
          last_status: initialState === "Up" ? 200 : null
        }
      };
      const down = {
        ...target,
        summary: { up_now: false, since_ms: 1_762_000_001_000, up_pct: upPct, drops, last_status: null }
      };
      mocked
        .mockResolvedValueOnce(JSON.stringify([target]))
        .mockResolvedValueOnce('{"ts":1762000001000,"ok":false,"latency_ms":null}')
        .mockResolvedValueOnce(JSON.stringify([down]));
      render(<ServicesWatched grant={GRANT} />);
      await screen.findByText(initialState);

      const probe = screen.getByRole("button", { name: "Probe now" });
      fireEvent.click(probe);
      fireEvent.click(probe);

      await screen.findByText("Down");
      expect(screen.getByText(window)).toBeInTheDocument();
      if (initialState === "Up") expect(screen.getByText("1 drop")).toBeInTheDocument();
      else expect(screen.queryByText(/\d+ drops?/)).not.toBeInTheDocument();
      expect(screen.queryByText("HTTP 200")).not.toBeInTheDocument();
      expect(screen.queryByText("Up")).not.toBeInTheDocument();
      expect(screen.queryByText("Not probed")).not.toBeInTheDocument();
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Probe now" })).toBeEnabled();
      expect(mocked.mock.calls).toEqual([
        [GRANT, "monitor.list", {}],
        [GRANT, "monitor.probe", { id: target.id }],
        [GRANT, "monitor.list", {}]
      ]);
    }
  );

  it("shows an explicit probe refusal without re-reading or replacing the last summary", async () => {
    mocked
      .mockResolvedValueOnce(answer([UP]))
      .mockResolvedValueOnce('{"ok":false,"error":"target is no longer watched"}');
    render(<ServicesWatched grant={GRANT} />);
    await screen.findByText("Up");

    fireEvent.click(screen.getByRole("button", { name: "Probe now" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("target is no longer watched");
    expect(screen.getByText("Up")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Probe now" })).toBeEnabled();
    expect(mocked.mock.calls).toEqual([
      [GRANT, "monitor.list", {}],
      [GRANT, "monitor.probe", { id: UP.id }]
    ]);
  });

  it("drops an unfinished watch read when the machine goes offline", async () => {
    let finish!: (value: string) => void;
    mocked.mockReturnValue(new Promise<string>((resolve) => { finish = resolve; }));
    const view = render(<ServicesWatched grant={GRANT} online />);
    expect(mocked).toHaveBeenCalledWith(GRANT, "monitor.list", {});

    view.rerender(<ServicesWatched grant={GRANT} online={false} />);
    await act(async () => { finish(answer([UP])); });

    expect(screen.queryByText("127.0.0.1:19000/health")).not.toBeInTheDocument();
    expect(screen.getByText(/offline, so its watch list cannot be read/)).toBeInTheDocument();
    expect(mocked).toHaveBeenCalledTimes(1);
  });

  it.each(["monitor.add", "monitor.probe", "monitor.remove"])(
    "does not apply a late %s or reread the previous grant after switching machines",
    async (operation) => {
      const nextGrant: Grant = { deviceId: "device-2", envelope: { token: "next-envelope" } };
      const nextTarget = { ...UP, id: "next-watch", host: "current.example" };
      let finish!: (value: string) => void;
      const pending = new Promise<string>((resolve) => { finish = resolve; });
      mocked.mockImplementation(async (grant, name) => {
        if (name === operation) return pending;
        if (name === "monitor.list") return answer(grant === GRANT ? [UP] : [nextTarget]);
        throw new Error(`unexpected ${name}`);
      });
      const view = render(<ServicesWatched grant={GRANT} online />);
      await screen.findByText("127.0.0.1:19000/health");
      if (operation === "monitor.add") {
        fireEvent.change(screen.getByLabelText("Host to watch"), { target: { value: "old.example" } });
        fireEvent.change(screen.getByLabelText("Port to watch"), { target: { value: "443" } });
        fireEvent.click(screen.getByRole("button", { name: "Start watching" }));
      } else if (operation === "monitor.probe") {
        fireEvent.click(screen.getByRole("button", { name: "Probe now" }));
      } else {
        fireEvent.click(screen.getByRole("button", { name: "Stop watching" }));
        fireEvent.click(screen.getByRole("button", { name: "Stop" }));
      }
      expect(mocked.mock.calls.filter(([, name]) => name === operation)).toHaveLength(1);

      view.rerender(<ServicesWatched grant={nextGrant} online />);
      await screen.findByText("current.example:19000/health");
      await act(async () => {
        finish(JSON.stringify(operation === "monitor.probe"
          ? { ts: 1_762_000_000_000, ok: true, latency_ms: 3 }
          : { ok: true }));
      });

      expect(screen.getByText("current.example:19000/health")).toBeInTheDocument();
      expect(screen.queryByText("127.0.0.1:19000/health")).not.toBeInTheDocument();
      expect(mocked.mock.calls.filter(([grant, name]) => grant === GRANT && name === "monitor.list")).toHaveLength(1);
      expect(screen.queryByText(/Watching old.example|Stopped watching/)).not.toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Start watching" })).toBeEnabled();
    }
  );

  it("keeps the last watch list readable offline and disables its mutation controls", async () => {
    mocked.mockResolvedValue(answer([UP]));
    const view = render(<ServicesWatched grant={GRANT} online />);
    await screen.findByText("127.0.0.1:19000/health");
    view.rerender(<ServicesWatched grant={GRANT} online={false} />);
    expect(screen.getByText(/this is the last watch list/)).toBeInTheDocument();
    const probe = screen.getByRole("button", { name: "Probe now" });
    const stop = screen.getByRole("button", { name: "Stop watching" });
    expect(probe).toBeDisabled();
    expect(stop).toBeDisabled();
    fireEvent.click(probe);
    fireEvent.click(stop);
    expect(mocked.mock.calls.filter(([, name]) => name !== "monitor.list")).toHaveLength(0);
  });

  it("offers Keep before Stop and cancellation never removes the watch", async () => {
    mocked.mockResolvedValue(answer([UP]));
    render(<ServicesWatched grant={GRANT} online />);
    await screen.findByText("Up");
    fireEvent.click(screen.getByRole("button", { name: "Stop watching" }));
    const confirmation = screen.getByRole("button", { name: "Keep" }).closest(".service-confirm") as HTMLElement;
    expect(within(confirmation).getAllByRole("button")[0]).toHaveTextContent("Keep");
    fireEvent.click(screen.getByRole("button", { name: "Keep" }));
    expect(screen.queryByRole("button", { name: "Stop" })).not.toBeInTheDocument();
    expect(mocked.mock.calls.filter(([, name]) => name === "monitor.remove")).toHaveLength(0);
  });

  it("drops a target it cannot address, rather than drawing a blank row", async () => {
    mocked.mockResolvedValue(answer([UP, { id: "x", host: "", port: 80, summary: {} }]));
    render(<ServicesWatched grant={GRANT} />);
    await waitFor(() => expect(screen.getByText("Up")).toBeInTheDocument());
    expect(screen.getByText("One target · probed every 15 s on the machine")).toBeInTheDocument();
  });
});
