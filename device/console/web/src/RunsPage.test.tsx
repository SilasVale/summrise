import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { RunsPage } from "./RunsPage";
import { copyText } from "./copy";
import { callTool, type Grant } from "./terminal";

vi.mock("./copy", () => ({ copyText: vi.fn().mockResolvedValue(true) }));

// The page's whole device surface is `callTool`; the module's other exports are kept as they are.
vi.mock("./terminal", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./terminal")>();
  return { ...actual, callTool: vi.fn() };
});

const mocked = vi.mocked(callTool);

const GRANT: Grant = { deviceId: "device-1", envelope: { token: "envelope" } };

// The page's `RunRecord` is private; this is its wire shape, not a second definition.
type RunRecord = {
  id: string;
  command: string;
  args: string[];
  cwd: string;
  exit_code: number | null;
  timed_out: boolean;
  duration_ms: number;
  started_at_ms: number;
  stdout_bytes: number;
  stderr_bytes: number;
};

const HELLO: RunRecord = {
  id: "run-2",
  command: "echo",
  args: ["hello"],
  cwd: ".",
  exit_code: 0,
  timed_out: false,
  duration_ms: 42,
  started_at_ms: Date.now() - 5 * 60_000,
  stdout_bytes: 6,
  stderr_bytes: 0
};

const CARGO: RunRecord = {
  id: "run-1",
  command: "cargo",
  args: ["test"],
  cwd: ".",
  exit_code: 1,
  timed_out: false,
  duration_ms: 61_000,
  started_at_ms: Date.now() - 3 * 60 * 60_000,
  stdout_bytes: 2048,
  stderr_bytes: 12
};

const EMPTY_NOTE =
  "No runs yet on this machine. Run one from Tools → Run a command; this page asks for the newest " +
  "20 runs, and the machine keeps more history than that.";

/// The calls made to one tool, as `[grant, name, args]` triples, in the order the page made them.
const called = (name: string) => mocked.mock.calls.filter((call) => call[1] === name);

describe("RunsPage", () => {
  beforeEach(() => {
    mocked.mockReset();
  });

  it("does not ask a machine the fleet says is down, and says why", async () => {
    render(<RunsPage grant={GRANT} deviceOnline={false} />);
    // THE READ IS NOT SENT: it cannot reach the machine, and the page would sit on "Reading the
    // device's run history…" until the console's dial timed out.
    await waitFor(() => expect(mocked).not.toHaveBeenCalled());
    expect(
      screen.getByText("The machine is offline; its run history cannot be read until it reconnects.")
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Refresh history" })).toBeDisabled();
  });

  it("is the history only: no command field, no Run button and no runs.exec", async () => {
    mocked.mockResolvedValue(JSON.stringify([HELLO]));

    render(<RunsPage grant={GRANT} />);

    expect(await screen.findByText("echo hello")).toBeInTheDocument();
    // THE FORM MOVED TO TOOLS. A history page that still carried a second way to run a command
    // would be the place the item deleted; this proves the page has neither the field nor the call.
    expect(screen.queryByLabelText("Command")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Run" })).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/Working directory/)).not.toBeInTheDocument();
    expect(called("runs.exec")).toHaveLength(0);
  });

  it("says there are no runs yet and points at Tools → Run a command", async () => {
    mocked.mockResolvedValue(JSON.stringify([]));

    render(<RunsPage grant={GRANT} />);

    expect(await screen.findByText(EMPTY_NOTE)).toBeInTheDocument();
    expect(EMPTY_NOTE).toContain("Tools → Run a command");
    // The empty state names the page that now makes a run, not a form this page no longer has.
    expect(screen.queryByRole("button", { name: "Run" })).not.toBeInTheDocument();
  });

  it("lists the history with each row's command, exit and bytes, and Refresh history asks again", async () => {
    mocked.mockResolvedValue(JSON.stringify([HELLO, CARGO]));

    render(<RunsPage grant={GRANT} />);

    expect(
      await screen.findByText("2 recent runs, newest first. Choose a command to read its record.")
    ).toBeInTheDocument();
    expect(mocked).toHaveBeenCalledWith(GRANT, "runs.list", { limit: 20 });

    const rows = screen.getAllByRole("row");
    expect(within(rows[1]).getByText("echo hello")).toBeInTheDocument();
    expect(within(rows[1]).getByText("exit 0")).toBeInTheDocument();
    expect(within(rows[1]).getByText("6 B / 0 B")).toBeInTheDocument();
    expect(within(rows[2]).getByText("cargo test")).toBeInTheDocument();
    expect(within(rows[2]).getByText("exit 1")).toBeInTheDocument();
    expect(within(rows[2]).getByText("2.0 KB / 12 B")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Refresh history" }));

    await waitFor(() => expect(called("runs.list")).toHaveLength(2));
    expect(called("runs.list")[1]).toEqual([GRANT, "runs.list", { limit: 20 }]);
  });

  it("labels every history fact, so the phone card carries the head it hides", async () => {
    mocked.mockResolvedValue(JSON.stringify([HELLO, CARGO]));

    render(<RunsPage grant={GRANT} />);

    await screen.findByText("echo hello");
    const rows = screen.getAllByRole("row").slice(1);
    expect(rows).toHaveLength(2);
    for (const [index, row] of rows.entries()) {
      const cells = within(row).getAllByRole("cell");
      expect(cells).toHaveLength(5);
      // THE COMMAND IS THE CARD'S FIRST LINE and the row's one control. The phone card is a media
      // query, so the same DOM is claimed here: five cells, the command first, then four facts.
      const command = within(cells[0]).getByRole("button");
      expect(command).toHaveClass("runs-open");
      expect(cells[1].querySelector(".runs-fact-label")).toHaveTextContent("Exit");
      expect(cells[1].lastElementChild).toHaveTextContent(index === 0 ? "exit 0" : "exit 1");
      expect(cells[2].querySelector(".runs-fact-label")).toHaveTextContent("Duration");
      expect(cells[3].querySelector(".runs-fact-label")).toHaveTextContent("Started");
      expect(cells[4].querySelector(".runs-fact-label")).toHaveTextContent("stdout / stderr");
      expect(cells[4].lastElementChild).toHaveTextContent(
        index === 0 ? "6 B / 0 B" : "2.0 KB / 12 B"
      );
      // ONE TAB STOP PER ROW, THE COMMAND: the labels are text, and the card adds no control.
      const focusable = row.querySelectorAll<HTMLElement>(
        'button, a[href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
      );
      expect(Array.from(focusable)).toEqual([command]);
    }
  });

  it("shortens a long command in the history and keeps the full line in its title", async () => {
    const long = {
      ...HELLO,
      id: "run-long",
      command: "powershell",
      args: ["-NoProfile", "-Command", "x".repeat(200)]
    };
    mocked.mockResolvedValue(JSON.stringify([long]));

    render(<RunsPage grant={GRANT} />);
    const button = await screen.findByRole("button", { name: /powershell/ });
    expect(button.textContent?.length).toBeLessThan(130);
    expect(button.textContent).toContain("…");
    expect(button.getAttribute("title")).toContain("x".repeat(200));
  });

  it("copies a past run's command from its detail", async () => {
    mocked.mockImplementation(async (_grant, name) => {
      if (name === "runs.list") return JSON.stringify([HELLO]);
      if (name === "runs.get") return JSON.stringify(HELLO);
      throw new Error(`the test did not expect ${name}`);
    });

    render(<RunsPage grant={GRANT} />);
    fireEvent.click(await screen.findByRole("button", { name: "echo hello" }));
    await waitFor(() => expect(called("runs.get")).toHaveLength(1));
    await screen.findByRole("heading", { name: "Command" });

    fireEvent.click(screen.getByRole("button", { name: "Copy command" }));

    await waitFor(() => expect(copyText).toHaveBeenCalledWith("echo hello"));
    expect(screen.getByRole("button", { name: "Copied" })).toBeInTheDocument();
  });

  it("opens the run id carried from Activity", async () => {
    mocked.mockImplementation(async (_grant, name) => {
      if (name === "runs.list") return JSON.stringify([]);
      if (name === "runs.get") return JSON.stringify({ ...HELLO, id: "run-9" });
      throw new Error(`the test did not expect ${name}`);
    });
    const consumed = vi.fn();

    render(<RunsPage grant={GRANT} focusRunId="run-9" onFocusConsumed={consumed} />);

    await waitFor(() =>
      expect(called("runs.get")[0]).toEqual([GRANT, "runs.get", { id: "run-9" }])
    );
    expect(await screen.findByText("echo hello")).toBeInTheDocument();
    expect(consumed).toHaveBeenCalled();
  });

  it("opens a run that arrives by hash while the page is already open", async () => {
    mocked.mockImplementation(async (_grant, name, args) => {
      if (name === "runs.list") return JSON.stringify([]);
      if (name === "runs.get") return JSON.stringify({ ...HELLO, id: (args as { id: string }).id });
      throw new Error(`the test did not expect ${name}`);
    });
    const consumed = vi.fn();

    const view = render(<RunsPage grant={GRANT} onFocusConsumed={consumed} />);
    await waitFor(() => expect(called("runs.list")).toHaveLength(1));

    // The hash change happens with the page already mounted: nothing re-reads the history, and the
    // focus used to be consumed only inside that read, so this run was never loaded.
    view.rerender(<RunsPage grant={GRANT} focusRunId="run-late" onFocusConsumed={consumed} />);

    await waitFor(() =>
      expect(called("runs.get")).toEqual([[GRANT, "runs.get", { id: "run-late" }]])
    );
    expect(await screen.findByText("echo hello")).toBeInTheDocument();
    expect(consumed).toHaveBeenCalled();
  });

  it("brings the record it opens into view, so a phone's read starts at the record", async () => {
    mocked.mockImplementation(async (_grant, name) => {
      if (name === "runs.list") return JSON.stringify([HELLO]);
      if (name === "runs.get") return JSON.stringify(HELLO);
      throw new Error(`the test did not expect a ${name} call`);
    });
    const scrollIntoView = vi.fn();
    const original = HTMLElement.prototype.scrollIntoView;
    HTMLElement.prototype.scrollIntoView = scrollIntoView;

    try {
      render(<RunsPage grant={GRANT} />);
      fireEvent.click(await screen.findByRole("button", { name: "echo hello" }));

      // The record is drawn above the history; once the phone's history is the page, the record it
      // asked for must be the thing on screen. jsdom has no layout, so the call is the assertion.
      await waitFor(() => expect(scrollIntoView).toHaveBeenCalled());
      const section = document.querySelector(".runs-detail");
      expect(section).not.toBeNull();
      expect(scrollIntoView.mock.contexts[0]).toBe(section);
      expect(scrollIntoView).toHaveBeenCalledWith({ block: "nearest", behavior: "smooth" });
    } finally {
      if (original) HTMLElement.prototype.scrollIntoView = original;
      else delete (HTMLElement.prototype as { scrollIntoView?: unknown }).scrollIntoView;
    }
  });

  it("opens a history row with runs.get and Close returns to the table", async () => {
    const record: RunRecord = {
      ...HELLO,
      cwd: "docs",
      duration_ms: 1500,
      started_at_ms: Date.now() - 10_000
    };
    mocked.mockImplementation(async (_grant, name) => {
      if (name === "runs.list") return JSON.stringify([HELLO]);
      if (name === "runs.get") return JSON.stringify(record);
      throw new Error(`the test did not expect a ${name} call`);
    });

    render(<RunsPage grant={GRANT} />);
    const opener = await screen.findByRole("button", { name: "echo hello" });
    // The command is the row's control, and it says whether its record is open.
    expect(opener.tagName).toBe("BUTTON");
    expect(opener).toHaveAttribute("type", "button");
    expect(opener).toHaveAttribute("aria-expanded", "false");

    fireEvent.click(opener);

    await waitFor(() => expect(called("runs.get")).toHaveLength(1));
    expect(called("runs.get")[0]).toEqual([GRANT, "runs.get", { id: "run-2" }]);
    expect(opener).toHaveAttribute("aria-expanded", "true");

    const heading = await screen.findByRole("heading", { name: "Command" });
    const detail = heading.closest("section");
    expect(detail).not.toBeNull();
    expect(within(detail as HTMLElement).getByText("run-2")).toBeInTheDocument();
    expect(within(detail as HTMLElement).getByText("echo hello")).toBeInTheDocument();
    expect(within(detail as HTMLElement).getByText("exit 0")).toBeInTheDocument();
    expect(within(detail as HTMLElement).getByText("1.5 s")).toBeInTheDocument();
    expect(within(detail as HTMLElement).getByText("docs")).toBeInTheDocument();
    expect(
      within(detail as HTMLElement).getByText(/^This is the record the history kept\./)
    ).toBeInTheDocument();

    fireEvent.click(within(detail as HTMLElement).getByRole("button", { name: "Close" }));

    expect(screen.queryByRole("heading", { name: "Command" })).not.toBeInTheDocument();
    expect(screen.queryByText("run-2")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "echo hello" })).toBeInTheDocument();
    expect(called("runs.get")).toHaveLength(1);
  });
});
