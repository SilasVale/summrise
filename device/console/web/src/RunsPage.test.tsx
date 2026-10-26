import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { RunsPage } from "./RunsPage";
import { callTool, ToolError, type Grant } from "./terminal";

// The page's whole device surface is `callTool`; the real `ToolError` stays so a refusal is the class
// the page actually receives from `./terminal`.
vi.mock("./terminal", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./terminal")>();
  return { ...actual, callTool: vi.fn() };
});

const mocked = vi.mocked(callTool);

const GRANT: Grant = { deviceId: "device-1", envelope: { token: "envelope" } };

// The page's `RunRecord`/`RunResult` are private; these are their wire shapes, not a second definition.
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

type RunResult = {
  id: string;
  exit_code: number | null;
  timed_out: boolean;
  duration_ms: number;
  stdout: string;
  stderr: string;
  truncated: boolean;
  history_recorded: boolean;
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

const RESULT: RunResult = {
  id: "run-3",
  exit_code: 0,
  timed_out: false,
  duration_ms: 12,
  stdout: "hello\n",
  stderr: "",
  truncated: false,
  history_recorded: true
};

const EMPTY_NOTE = "The device's run history is empty; the form above runs the first command.";

/// The calls made to one tool, as `[grant, name, args]` triples, in the order the page made them.
const called = (name: string) => mocked.mock.calls.filter((call) => call[1] === name);

describe("RunsPage", () => {
  beforeEach(() => {
    mocked.mockReset();
  });

  it("lists the history with each row's command, exit and bytes, and Refresh history asks again", async () => {
    mocked.mockResolvedValue(JSON.stringify([HELLO, CARGO]));

    render(<RunsPage grant={GRANT} />);

    expect(
      await screen.findByText("2 recent runs, newest first. Click a row to read its record.")
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

  it("runs an argv line as command plus args, shows the result and reads the history again", async () => {
    let lists = 0;
    mocked.mockImplementation(async (_grant, name) => {
      if (name === "runs.list") {
        lists += 1;
        // The first read is the page's mount; the second is the one after the run.
        return JSON.stringify(lists === 1 ? [] : [HELLO]);
      }
      if (name === "runs.exec") return JSON.stringify(RESULT);
      throw new Error(`the test did not expect a ${name} call`);
    });

    render(<RunsPage grant={GRANT} />);
    await screen.findByText(EMPTY_NOTE);

    fireEvent.change(screen.getByLabelText("Command"), { target: { value: "echo hello" } });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    await waitFor(() => expect(called("runs.exec")).toHaveLength(1));
    // The page's request, not the row the device writes: the timeout is the form's 60000 default.
    expect(called("runs.exec")[0]).toEqual([
      GRANT,
      "runs.exec",
      { command: "echo", args: ["hello"], timeout_ms: 60_000 }
    ]);

    const result = (await screen.findByText("Result")).closest("section") as HTMLElement;
    expect(result).not.toBeNull();
    expect(within(result).getByText("exit 0")).toBeInTheDocument();
    expect(within(result).getByText("recorded")).toBeInTheDocument();
    expect(within(result).getByText("hello")).toBeInTheDocument();

    // The run's row arrives only from the re-read the page does after a successful run.
    expect(await screen.findByText("echo hello")).toBeInTheDocument();
    expect(
      screen.getByText("1 recent run, newest first. Click a row to read its record.")
    ).toBeInTheDocument();
    expect(called("runs.list")).toHaveLength(2);
  });

  it("sends a shell line whole when Through the shell is ticked", async () => {
    mocked.mockImplementation(async (_grant, name) => {
      if (name === "runs.list") return JSON.stringify([]);
      if (name === "runs.exec") return JSON.stringify(RESULT);
      throw new Error(`the test did not expect a ${name} call`);
    });

    render(<RunsPage grant={GRANT} />);
    await screen.findByText(EMPTY_NOTE);

    // Ticking the box changes the page's own wording around the line; the line itself goes whole.
    fireEvent.click(screen.getByLabelText("Through the shell"));
    expect(screen.getByPlaceholderText("echo hello && echo bye")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Command"), {
      target: { value: "echo a && echo b" }
    });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    await waitFor(() => expect(called("runs.exec")).toHaveLength(1));
    expect(called("runs.exec")[0][2]).toEqual({
      command: "echo a && echo b",
      shell: true,
      timeout_ms: 60_000
    });
  });

  it("sends a typed working directory trimmed, and no cwd key when the box is empty", async () => {
    mocked.mockImplementation(async (_grant, name) => {
      if (name === "runs.list") return JSON.stringify([]);
      if (name === "runs.exec") return JSON.stringify(RESULT);
      throw new Error(`the test did not expect a ${name} call`);
    });

    render(<RunsPage grant={GRANT} />);
    await screen.findByText(EMPTY_NOTE);

    fireEvent.change(screen.getByLabelText("Command"), { target: { value: "echo hi" } });
    fireEvent.change(screen.getByLabelText("Working directory (optional)"), {
      target: { value: "  docs  " }
    });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    await waitFor(() => expect(called("runs.exec")).toHaveLength(1));
    expect(called("runs.exec")[0][2]).toEqual({
      command: "echo",
      args: ["hi"],
      cwd: "docs",
      timeout_ms: 60_000
    });

    // The second run waits for the first to settle, then leaves the box empty.
    await waitFor(() => expect(screen.getByRole("button", { name: "Run" })).toBeEnabled());
    fireEvent.change(screen.getByLabelText("Working directory (optional)"), {
      target: { value: "" }
    });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    await waitFor(() => expect(called("runs.exec")).toHaveLength(2));
    const second = called("runs.exec")[1][2];
    expect(second).toEqual({ command: "echo", args: ["hi"], timeout_ms: 60_000 });
    expect(second).not.toHaveProperty("cwd");
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
    fireEvent.click(await screen.findByRole("button", { name: "echo hello" }));

    await waitFor(() => expect(called("runs.get")).toHaveLength(1));
    expect(called("runs.get")[0]).toEqual([GRANT, "runs.get", { id: "run-2" }]);

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

  it("shows a device refusal as its own sentence and keeps the previous result", async () => {
    let runs = 0;
    mocked.mockImplementation(async (_grant, name) => {
      if (name === "runs.list") return JSON.stringify([]);
      if (name === "runs.exec") {
        runs += 1;
        if (runs === 1) return JSON.stringify(RESULT);
        throw new ToolError("the device refused to run it", 200, null, true);
      }
      throw new Error(`the test did not expect a ${name} call`);
    });

    render(<RunsPage grant={GRANT} />);
    await screen.findByText(EMPTY_NOTE);

    fireEvent.change(screen.getByLabelText("Command"), { target: { value: "echo hello" } });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    expect(await screen.findByText("Result")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("button", { name: "Run" })).toBeEnabled());

    fireEvent.change(screen.getByLabelText("Command"), { target: { value: "false" } });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    expect(
      await screen.findByText("Could not run the command: the device refused to run it")
    ).toBeInTheDocument();
    // The failed attempt did not replace the run that did finish; the page says so in its own words.
    expect(screen.getByText("Result")).toBeInTheDocument();
    expect(screen.getByText("hello")).toBeInTheDocument();
    expect(
      screen.getByText(
        "The result below is from the last run that finished; this attempt did not replace it."
      )
    ).toBeInTheDocument();
    // Both the success and the refusal read the history again: mount, after the run, after the refusal.
    await waitFor(() => expect(called("runs.list")).toHaveLength(3));
  });
});
