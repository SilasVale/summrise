import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { MonitorsPage } from "./MonitorsPage";
import { callTool, ToolError, type Grant } from "./terminal";

// The page's whole device surface is `callTool`; the real `ToolError` stays so a refusal is the class
// the page actually receives from `./terminal`.
vi.mock("./terminal", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./terminal")>();
  return { ...actual, callTool: vi.fn() };
});

const mocked = vi.mocked(callTool);

const GRANT: Grant = { deviceId: "device-1", envelope: { token: "envelope" } };

// The page's `Target`/`Status` are private; these are their wire shapes, not a second definition.
type Status = {
  up_now: boolean | null;
  since_ms: number | null;
  up_pct: number | null;
  drops: number | null;
  last_status: number | null;
};

type Target = {
  id: string;
  kind: "tcp" | "http";
  host: string;
  port: number;
  path?: string | null;
  expect?: string | null;
  interval_ms?: number;
  summary?: Status | null;
};

const HTTP: Target = {
  id: "example.com:443",
  kind: "http",
  host: "example.com",
  port: 443,
  path: "/health",
  expect: "upstream ok",
  interval_ms: 15_000
};

const UP: Status = { up_now: true, since_ms: null, up_pct: 100, drops: 0, last_status: null };

// A down summary is a fact about the target, not a failure of the page: "down" plus the numbers the
// device measured. The age is exactly five minutes old.
const DOWN: Status = {
  up_now: false,
  since_ms: Date.now() - 5 * 60_000,
  up_pct: 97.5,
  drops: 2,
  last_status: 503
};

// Answer the named tools; an unasked-for call fails the test instead of returning a plausible value.
function device(answers: (name: string, args: Record<string, unknown>) => string) {
  mocked.mockImplementation(
    async (_grant: Grant, name: string, args: Record<string, unknown>) => answers(name, args)
  );
}

const called = (name: string) => mocked.mock.calls.filter((call) => call[1] === name);

describe("MonitorsPage", () => {
  beforeEach(() => {
    mocked.mockReset();
  });

  it("lists an http target with its status, and a down target reads as a fact", async () => {
    device((name) => {
      if (name === "monitor.list") return JSON.stringify([{ ...HTTP, summary: DOWN }]);
      if (name === "monitor.status") return JSON.stringify(DOWN);
      throw new Error(`the test did not expect a ${name} call`);
    });
    render(<MonitorsPage grant={GRANT} />);

    const card = await screen.findByRole("article");
    expect(within(card).getByText("example.com:443/health")).toBeInTheDocument();
    expect(within(card).getByText("http")).toBeInTheDocument();
    expect(within(card).getByText(/^down for /)).toBeInTheDocument();
    expect(within(card).getByText("97.5%")).toBeInTheDocument();
    expect(within(card).getByText("2")).toBeInTheDocument();
    expect(within(card).getByText("HTTP 503")).toBeInTheDocument();
    expect(within(card).getByText("every 15000 ms")).toBeInTheDocument();
    expect(
      screen.getByText("1 target watched. The dot is each target's newest probe.")
    ).toBeInTheDocument();

    expect(mocked).toHaveBeenCalledWith(GRANT, "monitor.list", {});
    // The list carries the device's summary at that instant; the page then asks for the live one.
    await vi.waitFor(() =>
      expect(mocked).toHaveBeenCalledWith(GRANT, "monitor.status", { id: "example.com:443" })
    );
  });

  it("offers notifications above the target list, with no grant of its own", async () => {
    device((name) => {
      if (name === "monitor.list") return JSON.stringify([{ ...HTTP, summary: UP }]);
      if (name === "monitor.status") return JSON.stringify(UP);
      throw new Error(`the test did not expect a ${name} call`);
    });
    const { container } = render(<MonitorsPage grant={GRANT} />);

    // The card is the page's, not the grant's: it renders before the list has answered, and jsdom
    // having no Notification is a sentence on it, never a crash.
    const region = screen.getByRole("region", { name: "Monitor notifications" });
    expect(
      within(region).getByText(
        "This browser does not support notifications. The down chip still shows the count."
      )
    ).toBeInTheDocument();

    await screen.findByRole("article");
    const list = container.querySelector(".cards");
    expect(list).not.toBeNull();
    expect(region.compareDocumentPosition(list!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    // The card's state is the operator's, not the device's: it never calls a tool.
    expect(mocked.mock.calls.every((call) => call[1] === "monitor.list" || call[1] === "monitor.status")).toBe(true);
  });

  it("adds a tcp target with exactly the tcp arguments, then re-lists", async () => {
    let targets: Target[] = [];
    device((name, args) => {
      if (name === "monitor.list") return JSON.stringify(targets);
      if (name === "monitor.add") {
        // `interval_ms` is the form's visible default and the device requires it (>= 5000); `path`
        // and `expect` are http-only and must not ride along on a tcp add.
        targets = [
          {
            id: "10.0.0.9:22",
            kind: "tcp",
            host: String(args.host),
            port: Number(args.port),
            interval_ms: Number(args.interval_ms)
          }
        ];
        return JSON.stringify({});
      }
      if (name === "monitor.status") return JSON.stringify(UP);
      throw new Error(`the test did not expect a ${name} call`);
    });
    render(<MonitorsPage grant={GRANT} />);
    await screen.findByText("No targets are watched yet; the form above adds the first one.");

    fireEvent.change(screen.getByLabelText("Host"), { target: { value: "10.0.0.9" } });
    fireEvent.change(screen.getByLabelText("Port"), { target: { value: "22" } });
    fireEvent.click(screen.getByRole("button", { name: "Add target" }));

    await vi.waitFor(() =>
      expect(mocked).toHaveBeenCalledWith(GRANT, "monitor.add", {
        kind: "tcp",
        host: "10.0.0.9",
        port: 22,
        interval_ms: 60_000
      })
    );
    // The add is followed by a list, so the device's registry is what the page shows.
    expect(await screen.findByText("10.0.0.9:22")).toBeInTheDocument();
    await vi.waitFor(() => expect(called("monitor.list")).toHaveLength(2));
    // The draft the add consumed is cleared; the untouched interval keeps its default.
    expect(screen.getByLabelText("Host")).toHaveValue("");
    // A cleared number input has no numeric value; jest-dom reports that as null, not "".
    expect(screen.getByLabelText("Port")).toHaveValue(null);
    expect(screen.getByLabelText("Interval (ms)")).toHaveValue(60_000);
  });

  it("adds an http target with its path and expect", async () => {
    let targets: Target[] = [];
    device((name, args) => {
      if (name === "monitor.list") return JSON.stringify(targets);
      if (name === "monitor.add") {
        targets = [{ ...HTTP, path: String(args.path), expect: String(args.expect) }];
        return JSON.stringify({});
      }
      if (name === "monitor.status") return JSON.stringify(UP);
      throw new Error(`the test did not expect a ${name} call`);
    });
    render(<MonitorsPage grant={GRANT} />);
    await screen.findByText("No targets are watched yet; the form above adds the first one.");

    fireEvent.change(screen.getByLabelText("Kind"), { target: { value: "http" } });
    fireEvent.change(screen.getByLabelText("Host"), { target: { value: "example.com" } });
    fireEvent.change(screen.getByLabelText("Port"), { target: { value: "443" } });
    fireEvent.change(screen.getByLabelText("Path"), { target: { value: "/health" } });
    fireEvent.change(screen.getByLabelText("Expect"), { target: { value: "upstream ok" } });
    fireEvent.click(screen.getByRole("button", { name: "Add target" }));

    await vi.waitFor(() =>
      expect(mocked).toHaveBeenCalledWith(GRANT, "monitor.add", {
        kind: "http",
        host: "example.com",
        port: 443,
        interval_ms: 60_000,
        path: "/health",
        expect: "upstream ok"
      })
    );
    await vi.waitFor(() => expect(called("monitor.list")).toHaveLength(2));
    expect(await screen.findByText("example.com:443/health")).toBeInTheDocument();
  });

  it("probes the target and then reads its summary back", async () => {
    const after: Status = { up_now: true, since_ms: null, up_pct: 99.9, drops: 3, last_status: 200 };
    let probed = false;
    device((name) => {
      if (name === "monitor.list") return JSON.stringify([{ ...HTTP, summary: null }]);
      if (name === "monitor.status") return JSON.stringify(probed ? after : UP);
      if (name === "monitor.probe") {
        probed = true;
        return JSON.stringify({ ok: true, latency_ms: 3, ts: 1 });
      }
      throw new Error(`the test did not expect a ${name} call`);
    });
    render(<MonitorsPage grant={GRANT} />);
    const card = await screen.findByRole("article");
    // The list carried no summary; wait for the page's first live read, then forget those calls so
    // the assertion below is only about the probe.
    await within(card).findByText("up");
    mocked.mockClear();

    fireEvent.click(within(card).getByRole("button", { name: "Probe now" }));

    // `monitor.probe` answers with a sample; the card shows the summary, so the page reads it back.
    expect(await within(card).findByText("99.9%")).toBeInTheDocument();
    expect(within(card).getByText("3")).toBeInTheDocument();
    expect(within(card).getByText("HTTP 200")).toBeInTheDocument();
    expect(mocked.mock.calls.map((call) => [call[1], call[2]])).toEqual([
      ["monitor.probe", { id: "example.com:443" }],
      ["monitor.status", { id: "example.com:443" }]
    ]);
  });

  it("removes a target only through the page's confirmation", async () => {
    let targets: Target[] = [{ ...HTTP, summary: UP }];
    device((name) => {
      if (name === "monitor.list") return JSON.stringify(targets);
      if (name === "monitor.status") return JSON.stringify(UP);
      if (name === "monitor.remove") {
        targets = [];
        return JSON.stringify({ removed: true });
      }
      throw new Error(`the test did not expect a ${name} call`);
    });
    render(<MonitorsPage grant={GRANT} />);
    const card = await screen.findByRole("article");

    fireEvent.click(within(card).getByRole("button", { name: "Remove" }));
    let dialog = await screen.findByRole("dialog", { name: "Remove example.com:443/health?" });
    expect(dialog).toHaveTextContent("The device stops probing it and discards its series.");
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(called("monitor.remove")).toHaveLength(0);

    fireEvent.click(within(card).getByRole("button", { name: "Remove" }));
    dialog = await screen.findByRole("dialog", { name: "Remove example.com:443/health?" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Remove" }));

    await vi.waitFor(() =>
      expect(mocked).toHaveBeenCalledWith(GRANT, "monitor.remove", { id: "example.com:443" })
    );
    expect(called("monitor.remove")).toHaveLength(1);
    await vi.waitFor(() => expect(screen.queryByRole("article")).not.toBeInTheDocument());
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    // A successful remove re-lists, and the device's empty answer is what the page shows.
    await vi.waitFor(() => expect(called("monitor.list")).toHaveLength(2));
    expect(
      await screen.findByText("No targets are watched yet; the form above adds the first one.")
    ).toBeInTheDocument();
  });

  it("shows the device's refusal and keeps the draft that was refused", async () => {
    device((name) => {
      if (name === "monitor.list") return JSON.stringify([]);
      if (name === "monitor.add") {
        throw new ToolError("the device refused: monitor.add is not in this grant", 200, null, true);
      }
      throw new Error(`the test did not expect a ${name} call`);
    });
    render(<MonitorsPage grant={GRANT} />);
    await screen.findByText("No targets are watched yet; the form above adds the first one.");

    fireEvent.change(screen.getByLabelText("Host"), { target: { value: "10.0.0.9" } });
    fireEvent.change(screen.getByLabelText("Port"), { target: { value: "22" } });
    fireEvent.click(screen.getByRole("button", { name: "Add target" }));

    // The device's own sentence, exactly as it sent it, behind the page's own prefix.
    expect(
      await screen.findByText(
        "Could not add 10.0.0.9:22: the device refused: monitor.add is not in this grant"
      )
    ).toBeInTheDocument();
    // A refused add is a refused grant, not a bad form: nothing typed is thrown away.
    expect(screen.getByLabelText("Kind")).toHaveValue("tcp");
    expect(screen.getByLabelText("Host")).toHaveValue("10.0.0.9");
    expect(screen.getByLabelText("Port")).toHaveValue(22);
    expect(screen.getByLabelText("Interval (ms)")).toHaveValue(60_000);
    expect(screen.getByRole("button", { name: "Add target" })).toBeEnabled();
    // No re-list: the device never accepted the target.
    expect(called("monitor.list")).toHaveLength(1);
  });
});
