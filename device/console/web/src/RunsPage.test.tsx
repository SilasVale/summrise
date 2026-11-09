import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { RunsPage } from "./RunsPage";
import { copyText } from "./copy";
import { age } from "./age";
import { clockTime, clockTimeSeconds, exactMoment, isoDate } from "./format";
import { commandLine, shortCommand } from "./runFormat";
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

// THE AUDIT'S SHAPE: the measured record's `powershell.exe -NoProfile -EncodedCommand …` line is
// 4,590 characters. `commandLine` joins the three words and the payload to exactly that length.
const LONG_PAYLOAD = "JABF" + "A".repeat(4_544);
const LONG_RUN: RunRecord = {
  ...HELLO,
  id: "run-long",
  command: "powershell.exe",
  args: ["-NoProfile", "-EncodedCommand", LONG_PAYLOAD],
  cwd: "C:\\Users\\operator",
  exit_code: 1,
  duration_ms: 61_000,
  started_at_ms: Date.now() - 3 * 60 * 60_000,
  stdout_bytes: 2048,
  stderr_bytes: 12
};

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
    // **THE SHELL OWNS THE OFFLINE REASON (audit-44 F5).** The tab says only what is missing.
    expect(screen.getByText("Its run history cannot be read.")).toBeInTheDocument();
    expect(screen.queryByText(/machine is offline/i)).not.toBeInTheDocument();
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

  it("dates a history row's Started with the table's clock, age and exact title", async () => {
    // **THE SAME TIME VOCABULARY AS THE ACTIVITY TABLE.** The history said `31m 11s ago` with no
    // title while the Table, one view away, dated the same class of run `06:59:29 · 7s ago` with
    // the instant in a `<time>`. A run is placed in time on both.
    const startedAt = new Date(2026, 10, 4, 9, 41, 5).getTime();
    const now = new Date(2026, 10, 4, 10, 0, 0).getTime();
    const record: RunRecord = { ...HELLO, started_at_ms: startedAt };
    mocked.mockResolvedValue(JSON.stringify([record]));
    const nowSpy = vi.spyOn(Date, "now").mockReturnValue(now);
    try {
      render(<RunsPage grant={GRANT} />);
      const cell = (await screen.findByText("echo hello")).closest("tr")?.querySelector("td.runs-fact:nth-of-type(4)") as HTMLElement;
      const time = cell.querySelector("time") as HTMLTimeElement;
      expect(time).not.toBeNull();
      expect(time).toHaveTextContent(clockTimeSeconds(startedAt));
      expect(time).toHaveAttribute("title", exactMoment(startedAt));
      expect(time).toHaveAttribute("dateTime", new Date(startedAt).toISOString());
      expect(cell).toHaveTextContent(`· ${age(now - startedAt)} ago`);
    } finally {
      nowSpy.mockRestore();
    }
  });

  it("keeps the Started clock and age in one value cell, not the label's column", async () => {
    // The phone's fact grid is two columns: label, value. The twenty-second list's fix left the
    // clock and the age as separate children of the `<td>`, so the age auto-placed into the label's
    // column under a leading `·` (audit-43 F8 measured label x=48, time x=160, hint x=48). The
    // wrapper is what the grid sees; this is the structure the media query styles.
    const startedAt = new Date(2026, 10, 4, 9, 41, 5).getTime();
    const now = new Date(2026, 10, 4, 10, 0, 0).getTime();
    const record: RunRecord = { ...HELLO, started_at_ms: startedAt };
    mocked.mockResolvedValue(JSON.stringify([record]));
    const nowSpy = vi.spyOn(Date, "now").mockReturnValue(now);
    try {
      render(<RunsPage grant={GRANT} />);
      const cell = (await screen.findByText("echo hello"))
        .closest("tr")
        ?.querySelectorAll("td.runs-fact")[2] as HTMLElement;
      // Two grid items — the label and ONE value — not the three the broken card had.
      expect(cell.children).toHaveLength(2);
      const label = cell.children[0] as HTMLElement;
      const value = cell.children[1] as HTMLElement;
      expect(label).toHaveClass("runs-fact-label");
      expect(value).toHaveClass("runs-started");
      expect(value.querySelector("time")).not.toBeNull();
      expect(value.textContent).toBe(
        `${clockTimeSeconds(startedAt)} · ${age(now - startedAt)} ago`
      );
      expect(cell.textContent?.match(/ · /g)).toHaveLength(1);
    } finally {
      nowSpy.mockRestore();
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

  it("dates a record's Started with the local instant and the exact moment in its title", async () => {
    // Local instants, so the assertions do not depend on the machine's timezone: `format.ts` is the
    // expected clock, the same helpers the page paints with.
    const startedAt = new Date(2026, 10, 4, 9, 41, 5).getTime();
    const now = new Date(2026, 10, 4, 10, 0, 0).getTime();
    const record: RunRecord = { ...HELLO, started_at_ms: startedAt };
    mocked.mockImplementation(async (_grant, name) => {
      if (name === "runs.list") return JSON.stringify([record]);
      if (name === "runs.get") return JSON.stringify(record);
      throw new Error(`the test did not expect ${name}`);
    });
    const nowSpy = vi.spyOn(Date, "now").mockReturnValue(now);

    try {
      const view = render(<RunsPage grant={GRANT} />);
      fireEvent.click(await screen.findByRole("button", { name: "echo hello" }));

      const heading = await screen.findByRole("heading", { name: "Command" });
      const detail = heading.closest("section") as HTMLElement;
      const facts = detail.querySelector("dl.facts") as HTMLElement;
      const started = within(facts).getByText("Started").nextElementSibling as HTMLElement;
      const time = started.querySelector("time") as HTMLTimeElement;

      // THE INSTANT AND ITS EXACT MOMENT. The visible text is the console's local day and clock;
      // `title` is `exactMoment` (local to the second plus the UTC ISO instant) and `dateTime` is
      // the UTC instant itself — the audit's "no title/datetime" is the bug this asserts gone.
      expect(time).not.toBeNull();
      expect(time).toHaveTextContent(`${isoDate(startedAt)} ${clockTime(startedAt)}`);
      expect(time).toHaveAttribute("title", exactMoment(startedAt));
      expect(time).toHaveAttribute("dateTime", new Date(startedAt).toISOString());
      expect(started).toHaveTextContent(
        `${isoDate(startedAt)} ${clockTime(startedAt)} · ${age(now - startedAt)} ago`
      );

      // SIX SECONDS LATER THE INSTANT IS THE SAME. The audit measured `21m 04s ago` become
      // `21m 11s ago` six seconds later; the age may move with the clock, the start may not.
      nowSpy.mockReturnValue(now + 6_000);
      view.rerender(<RunsPage grant={GRANT} />);
      expect(started.querySelector("time")).toHaveTextContent(
        `${isoDate(startedAt)} ${clockTime(startedAt)}`
      );
      expect(started.querySelector("time")).toHaveAttribute("title", exactMoment(startedAt));
      expect(started).toHaveTextContent(`${age(now + 6_000 - startedAt)} ago`);
    } finally {
      nowSpy.mockRestore();
    }
  });

  it("opens a long run on its short command, with the facts above the full line behind a toggle", async () => {
    mocked.mockImplementation(async (_grant, name) => {
      if (name === "runs.list") return JSON.stringify([LONG_RUN]);
      if (name === "runs.get") return JSON.stringify(LONG_RUN);
      throw new Error(`the test did not expect ${name}`);
    });

    render(<RunsPage grant={GRANT} />);
    fireEvent.click(await screen.findByRole("button", { name: /^powershell\.exe/ }));

    const heading = await screen.findByRole("heading", { name: "Command" });
    const detail = heading.closest("section") as HTMLElement;
    expect(detail).not.toBeNull();
    const facts = detail.querySelector("dl.facts") as HTMLElement;
    const command = detail.querySelector("#runs-detail-command-text") as HTMLElement;
    const line = commandLine(LONG_RUN);
    const short = shortCommand(line);

    // THE RECORD IS 1,190 PX OF BASE64 NO LONGER: it paints the short form and states the count.
    expect(line).toHaveLength(4_590);
    expect(short).toHaveLength(110);
    expect(short.endsWith("…")).toBe(true);
    expect(within(detail).getByText(short)).toBeInTheDocument();
    expect(within(detail).queryByText(line)).not.toBeInTheDocument();
    expect(within(detail).getByText("4,481 of 4,590 characters hidden.")).toBeInTheDocument();

    // THE FACTS ARE ABOVE THE COMMAND, and the command is above its own full line: exit/duration/
    // started cannot be pushed off the screen by the 4,590 characters below them.
    expect(facts).not.toBeNull();
    expect(command).not.toBeNull();
    expect(facts.compareDocumentPosition(heading) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(
      facts.compareDocumentPosition(command) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
    expect(within(facts).getByText("exit 1")).toBeInTheDocument();
    expect(within(facts).getByText("1m 01s")).toBeInTheDocument();
    expect(within(facts).getByText("Started")).toBeInTheDocument();

    const toggle = within(detail).getByRole("button", { name: "Show full command" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(toggle);

    // THE FULL LINE IS REACHABLE ON REQUEST, unchanged, and still after the facts.
    const full = await within(detail).findByText(line);
    expect(full.tagName).toBe("P");
    expect(within(detail).getByRole("button", { name: "Hide full command" })).toHaveAttribute(
      "aria-expanded",
      "true"
    );
    expect(facts.compareDocumentPosition(full) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("keeps a short command whole: no toggle and no hidden count", async () => {
    mocked.mockImplementation(async (_grant, name) => {
      if (name === "runs.list") return JSON.stringify([HELLO]);
      if (name === "runs.get") return JSON.stringify(HELLO);
      throw new Error(`the test did not expect ${name}`);
    });

    render(<RunsPage grant={GRANT} />);
    fireEvent.click(await screen.findByRole("button", { name: "echo hello" }));

    const heading = await screen.findByRole("heading", { name: "Command" });
    const detail = heading.closest("section") as HTMLElement;
    expect(detail.querySelector("#runs-detail-command-text")).toHaveTextContent("echo hello");
    expect(
      within(detail).queryByRole("button", { name: "Show full command" })
    ).not.toBeInTheDocument();
    expect(within(detail).queryByText(/characters hidden/)).not.toBeInTheDocument();
  });
});
