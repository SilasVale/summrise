import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Device, LiveTool } from "./api";
import type { PluginRecord } from "./pluginApi";
import { ToolsTab } from "./ToolsTab";
import { callTool, ToolError, type Grant } from "./terminal";

const live = vi.hoisted(() => ({ value: {} as Record<string, unknown> }));
vi.mock("./useLiveTools", () => ({ useLiveTools: () => live.value }));

const mocks = vi.hoisted(() => ({ list: vi.fn() }));
vi.mock("./pluginApi", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./pluginApi")>();
  return { ...actual, listPlugins: mocks.list };
});

// The run form's whole device surface is `callTool`; the real `ToolError` stays so a refusal is the
// class the form actually receives from `./terminal`.
vi.mock("./terminal", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./terminal")>();
  return { ...actual, callTool: vi.fn() };
});

const mocked = vi.mocked(callTool);

const GRANT: Grant = { deviceId: "dev-workshop", envelope: { token: "envelope" } };

const workshop: Device = {
  id: "dev-workshop",
  name: "workshop",
  online: true,
  version: "0.3.246",
  last_seen_ms: 1
};

/// The form's answer, in the wire shape `runs.exec` really returns.
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

function tool(overrides: Partial<LiveTool> & { name: string }): LiveTool {
  return {
    description: "",
    inputSchema: { type: "object", properties: {} },
    source: "builtin",
    capability: overrides.name.split(".")[0],
    destructive: false,
    ...overrides
  };
}

const plugin: PluginRecord = {
  id: "com.example.hello",
  namespace: "hello",
  name: "Hello",
  version: "0.1.0",
  description: "Two friendly tools",
  state: "running",
  tools: 2,
  path: "C:\\dev\\plugins\\hello",
  last_error: null
};

function renderTools(overrides: Partial<ComponentProps<typeof ToolsTab>> = {}) {
  return render(
    <ToolsTab
      device={workshop}
      deviceOnline
      grant={GRANT}
      tool={null}
      onTool={vi.fn()}
      onMachine={vi.fn()}
      onBrowser={vi.fn()}
      {...overrides}
    />
  );
}

beforeEach(() => {
  live.value = {
    tools: [tool({ name: "files.read", description: "read a file" }), tool({ name: "plugins.list" })],
    stale: false,
    error: null,
    offline: false,
    loading: false,
    retry: vi.fn()
  };
  mocks.list.mockReset();
  mocks.list.mockResolvedValue([plugin]);
  mocked.mockReset();
});

describe("ToolsTab", () => {
  it("answers with the run form first, the catalogue second and the plugins third, one h2 each", async () => {
    renderTools();

    expect(
      screen.getAllByRole("heading", { level: 2 }).map((heading) => heading.textContent)
    ).toEqual(["Run a command on this machine", "Tools on this machine", "Plugins"]);
    expect(screen.getByRole("button", { name: /files\.read/ })).toBeInTheDocument();
    expect(await screen.findByText("Hello")).toBeInTheDocument();

    // The exec form is a section of this page, not a second page: its fields live under the h2, and
    // the section comes before the catalogue a person would otherwise scroll past.
    const run = screen
      .getByRole("heading", { name: "Run a command on this machine", level: 2 })
      .closest("section") as HTMLElement;
    const catalogue = screen
      .getByRole("heading", { name: "Tools on this machine", level: 2 })
      .closest("section") as HTMLElement;
    expect(within(run).getByLabelText("Command")).toBeInTheDocument();
    expect(within(run).getByRole("button", { name: "Run" })).toBeInTheDocument();
    expect(
      run.compareDocumentPosition(catalogue) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
  });

  it("tabs through the form before it reaches the catalogue", async () => {
    renderTools();
    await screen.findByRole("button", { name: /files\.read/ });

    const order = tabOrder(document.body);
    const run = screen
      .getByRole("heading", { name: "Run a command on this machine", level: 2 })
      .closest("section") as HTMLElement;
    const catalogue = screen
      .getByRole("heading", { name: "Tools on this machine", level: 2 })
      .closest("section") as HTMLElement;

    // No positive tabindex anywhere: the keyboard order the page promises is the DOM order.
    expect(order.every((element) => element.tabIndex <= 0)).toBe(true);
    const lastFormControl = Math.max(
      ...order.map((element, index) => (run.contains(element) ? index : -1))
    );
    const firstCatalogueControl = order.findIndex((element) => catalogue.contains(element));
    expect(lastFormControl).toBeGreaterThanOrEqual(0);
    expect(firstCatalogueControl).toBeGreaterThan(lastFormControl);
    // The catalogue's own search is still the first control a tab reaches after the form.
    expect(order[firstCatalogueControl]).toBe(within(catalogue).getByLabelText("Search tools"));
  });

  it("opens a tool's sheet inside the catalogue section", () => {
    renderTools({ tool: "files.read" });
    expect(screen.getByRole("dialog", { name: "files.read" })).toBeInTheDocument();
  });
});

/**
 * The focusable controls in DOM order — which is the tab order when nothing has a positive
 * tabindex. jsdom does not implement Tab, so the order is read from the document the way a browser
 * would build it, and the no-positive-tabindex assertion above is what makes it the keyboard order.
 */
function tabOrder(root: HTMLElement): HTMLElement[] {
  return Array.from(
    root.querySelectorAll<HTMLElement>(
      "a[href], button, input, select, textarea, summary, [tabindex]"
    )
  ).filter(
    (element) =>
      !element.hasAttribute("disabled") &&
      !element.hasAttribute("hidden") &&
      element.getAttribute("aria-hidden") !== "true"
  );
}

describe("the run form under Tools", () => {
  it("calls runs.exec with the form's line and opens the finished run's record", async () => {
    mocked.mockResolvedValue(JSON.stringify(RESULT));

    renderTools();
    fireEvent.change(screen.getByLabelText("Command"), { target: { value: "echo hello" } });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    await waitFor(() => expect(mocked).toHaveBeenCalledTimes(1));
    // The request the form made on the Runs page, moved without a wire change.
    expect(mocked.mock.calls[0]).toEqual([
      GRANT,
      "runs.exec",
      { command: "echo", args: ["hello"], timeout_ms: 60_000 }
    ]);

    // A completed run still opens its record: the id, the exit and the retained output.
    const result = (await screen.findByText("Result")).closest("section") as HTMLElement;
    expect(result).not.toBeNull();
    expect(within(result).getByText("run-3")).toBeInTheDocument();
    expect(within(result).getByText("exit 0")).toBeInTheDocument();
    expect(within(result).getByText("recorded")).toBeInTheDocument();
    expect(within(result).getByText("hello")).toBeInTheDocument();
  });

  it("sends a shell line whole when Through the shell is ticked", async () => {
    mocked.mockResolvedValue(JSON.stringify(RESULT));

    renderTools();
    fireEvent.click(screen.getByLabelText("Through the shell"));
    expect(screen.getByPlaceholderText("echo hello && echo bye")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Command"), {
      target: { value: "echo a && echo b" }
    });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    await waitFor(() => expect(mocked).toHaveBeenCalledTimes(1));
    expect(mocked.mock.calls[0][2]).toEqual({
      command: "echo a && echo b",
      shell: true,
      timeout_ms: 60_000
    });
  });

  it("puts a program the machine has in the field, not a shell builtin", () => {
    // The measured bug: the placeholder was `echo hello`, and both answering machines are Windows
    // 11, where `echo` is a command-prompt builtin rather than a program. Typed as it read, the form
    // sent command `echo`, the device answered "runs.exec: program not found" in 4 ms, and the
    // product had offered a command it could not run. The example now comes from the device's own
    // OS fact: on Windows the program is cmd.exe, which is in System32 on every Windows machine.
    renderTools({ device: { ...workshop, host: { os: { name: "Windows", version: "11" } } } });

    expect(screen.getByLabelText("Command")).toHaveAttribute(
      "placeholder",
      "cmd.exe /c echo hello"
    );
  });

  it("names a program that exists on a machine that is not Windows", () => {
    // The same rule elsewhere: `uname` is a program (/usr/bin/uname on this Linux host), so the
    // example survives the field's whitespace split without quoting. A quoted `sh -c 'echo hello'`
    // could not: the form does no quoting, so argv mode would pass the quotes through literally.
    renderTools({ device: { ...workshop, host: { os: { name: "Linux" } } } });

    expect(screen.getByLabelText("Command")).toHaveAttribute("placeholder", "uname -a");
  });

  it("says the field's shape and names no command when the fleet has no OS fact", () => {
    // `workshop` carries no host facts: an offline machine, or a console restarted since the last
    // frame, has no platform to choose an example from. Then the field gets no example, only shape.
    renderTools();

    const command = screen.getByLabelText("Command");
    expect(command).toHaveAttribute("placeholder", "program and arguments");
    expect(command.getAttribute("placeholder")).not.toMatch(/echo|cmd|uname/);
  });

  it("sends a typed working directory trimmed, and no cwd key when the box is empty", async () => {
    mocked.mockResolvedValue(JSON.stringify(RESULT));

    renderTools();
    fireEvent.change(screen.getByLabelText("Command"), { target: { value: "echo hi" } });
    fireEvent.change(screen.getByLabelText("Working directory (optional)"), {
      target: { value: "  docs  " }
    });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    await waitFor(() => expect(mocked).toHaveBeenCalledTimes(1));
    expect(mocked.mock.calls[0][2]).toEqual({
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

    await waitFor(() => expect(mocked).toHaveBeenCalledTimes(2));
    const second = mocked.mock.calls[1][2] as Record<string, unknown>;
    expect(second).toEqual({ command: "echo", args: ["hi"], timeout_ms: 60_000 });
    expect(second).not.toHaveProperty("cwd");
  });

  it("sends a typed timeout as a number", async () => {
    mocked.mockResolvedValue(JSON.stringify(RESULT));

    renderTools();
    fireEvent.change(screen.getByLabelText("Command"), { target: { value: "echo hello" } });
    fireEvent.change(screen.getByLabelText("Timeout (ms)"), { target: { value: "120000" } });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    await waitFor(() => expect(mocked).toHaveBeenCalledTimes(1));
    expect(mocked.mock.calls[0][2]).toEqual({
      command: "echo",
      args: ["hello"],
      timeout_ms: 120_000
    });
  });

  it("shows a device refusal as its own sentence and keeps the previous result", async () => {
    let runs = 0;
    mocked.mockImplementation(async () => {
      runs += 1;
      if (runs === 1) return JSON.stringify(RESULT);
      throw new ToolError("the device refused to run it", 200, null, true);
    });

    renderTools();
    fireEvent.change(screen.getByLabelText("Command"), { target: { value: "echo hello" } });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    expect(await screen.findByText("Result")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("button", { name: "Run" })).toBeEnabled());

    fireEvent.change(screen.getByLabelText("Command"), { target: { value: "false" } });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    expect(
      await screen.findByText("Could not run the command: the device refused to run it")
    ).toBeInTheDocument();
    // The failed attempt did not replace the run that did finish; the form says so in its own words.
    const result = screen.getByText("Result").closest("section") as HTMLElement;
    expect(within(result).getByText("hello")).toBeInTheDocument();
    expect(
      screen.getByText(
        "The result below is from the last run that finished; this attempt did not replace it."
      )
    ).toBeInTheDocument();
  });

  it("names the same reason the audit keeps when the run cannot start", async () => {
    // The measured failure: the program does not exist, the device answers `isError: true` with
    // "runs.exec: program not found", and the console now records that same sentence as the audit
    // line's detail. The form must show it, not a success card.
    const reason = "runs.exec: program not found";
    mocked.mockRejectedValue(new ToolError(reason, 200, null, true));

    renderTools();
    fireEvent.change(screen.getByLabelText("Command"), { target: { value: "echo audit5-live" } });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    expect(
      await screen.findByText("Could not run the command: " + reason)
    ).toBeInTheDocument();
    expect(screen.queryByText("Result")).not.toBeInTheDocument();
  });

  it("warns that a disconnected run may have executed", async () => {
    mocked.mockRejectedValue(new ToolError("device disconnected", 502));

    renderTools();
    fireEvent.change(screen.getByLabelText("Command"), { target: { value: "echo once" } });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    expect(
      await screen.findByText(
        "The device disconnected before the console heard the result: the command may have run. " +
          "Check the history before running it again."
      )
    ).toBeInTheDocument();
  });

  it("does not send a command to a machine the fleet says is down", async () => {
    renderTools({ deviceOnline: false });

    expect(screen.getByRole("button", { name: "Run" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Command"), { target: { value: "echo hello" } });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));
    await waitFor(() => expect(mocked).not.toHaveBeenCalled());
  });
});
