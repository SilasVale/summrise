import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SystemPage } from "./SystemPage";
import { callTool, ToolError, type Grant } from "./terminal";

// The page's whole device surface is `callTool`; the real `ToolError` stays so a refusal is the class
// the page actually receives from `./terminal`.
vi.mock("./terminal", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./terminal")>();
  return { ...actual, callTool: vi.fn() };
});

const mocked = vi.mocked(callTool);

const GRANT: Grant = { deviceId: "device-1", envelope: { token: "envelope" } };

// The page's `DeviceProcess` is private; this is its wire shape, not a second definition.
type Process = { pid: number; name: string; memory_kb: number };

const NGINX: Process = { pid: 4242, name: "nginx", memory_kb: 82_000 };
const SSHD: Process = { pid: 77, name: "sshd", memory_kb: 2_048 };

// Answer the named tools; an unasked-for call fails the test instead of returning a plausible value.
function device(answers: (name: string, args: Record<string, unknown>) => string) {
  mocked.mockImplementation(
    async (_grant: Grant, name: string, args: Record<string, unknown>) => answers(name, args)
  );
}

const called = (name: string) => mocked.mock.calls.filter((call) => call[1] === name);

describe("SystemPage", () => {
  beforeEach(() => {
    mocked.mockReset();
  });

  it("lists the device's processes and sends the filter the box asked for", async () => {
    device((name, args) => {
      if (name !== "system.process_list") {
        throw new Error(`the test did not expect a ${name} call`);
      }
      if (args.name === "nginx") return JSON.stringify([NGINX]);
      return JSON.stringify([NGINX, SSHD]);
    });
    render(<SystemPage grant={GRANT} />);

    // The note counts what the device answered, not what the table happens to draw.
    expect(await screen.findByText("2 processes on the device, by pid.")).toBeInTheDocument();
    const table = screen.getByRole("table");
    expect(within(table).getByText("4242")).toBeInTheDocument();
    expect(within(table).getByText("nginx")).toBeInTheDocument();
    expect(within(table).getByText("80.1 MB")).toBeInTheDocument();
    expect(within(table).getByText("sshd")).toBeInTheDocument();
    expect(mocked).toHaveBeenCalledWith(GRANT, "system.process_list", {});

    // The box is only a draft: the filter takes effect when the device is asked for it, trimmed.
    fireEvent.change(screen.getByLabelText("Filter"), { target: { value: "  nginx  " } });
    expect(
      screen.getByText(/The device applies the filter, so the table still shows the previous read/)
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));

    await vi.waitFor(() =>
      expect(mocked).toHaveBeenCalledWith(GRANT, "system.process_list", { name: "nginx" })
    );
    expect(
      await screen.findByText('1 process whose name contains "nginx", case-insensitively, by pid.')
    ).toBeInTheDocument();
    expect(within(screen.getByRole("table")).queryByText("sshd")).not.toBeInTheDocument();

    // An empty box is the whole list; the page does not send a name it does not have.
    fireEvent.change(screen.getByLabelText("Filter"), { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));

    await vi.waitFor(() => expect(called("system.process_list")).toHaveLength(3));
    expect(called("system.process_list")[2][2]).toEqual({});
    expect(await screen.findByText("2 processes on the device, by pid.")).toBeInTheDocument();
    expect(within(screen.getByRole("table")).getByText("sshd")).toBeInTheDocument();
  });

  it("kills the pid the device reported, only after the confirmation, then re-reads", async () => {
    let killed = false;
    device((name) => {
      if (name === "system.process_list") {
        return JSON.stringify(killed ? [SSHD] : [NGINX, SSHD]);
      }
      if (name === "system.process_kill") {
        killed = true;
        return JSON.stringify({ killed: [NGINX.pid], failed: [] });
      }
      throw new Error(`the test did not expect a ${name} call`);
    });
    render(<SystemPage grant={GRANT} />);
    await screen.findByText("2 processes on the device, by pid.");

    fireEvent.click(screen.getByRole("button", { name: "Kill PID 4242 (nginx)" }));
    let dialog = await screen.findByRole("dialog", { name: "Kill PID 4242 (nginx)?" });
    // The confirmation names both halves of the identity it is about to send.
    expect(dialog).toHaveTextContent("PID 4242");
    expect(dialog).toHaveTextContent("nginx");
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(called("system.process_kill")).toHaveLength(0);

    fireEvent.click(screen.getByRole("button", { name: "Kill PID 4242 (nginx)" }));
    dialog = await screen.findByRole("dialog", { name: "Kill PID 4242 (nginx)?" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Kill PID 4242" }));

    // A pid, never the name: a reused pid is a different process.
    await vi.waitFor(() =>
      expect(mocked).toHaveBeenCalledWith(GRANT, "system.process_kill", { pid: 4242 })
    );
    expect(called("system.process_kill")).toHaveLength(1);
    expect(await screen.findByText("The device ended PID 4242 (nginx).")).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    // The list is read again after the attempt, with the filter the table was read with.
    await vi.waitFor(() => expect(called("system.process_list")).toHaveLength(2));
    expect(called("system.process_list")[1][2]).toEqual({});
    expect(await screen.findByText("1 process on the device, by pid.")).toBeInTheDocument();
  });

  it("keeps the confirmation open on a refusal and still re-reads the list", async () => {
    device((name) => {
      if (name === "system.process_list") return JSON.stringify([NGINX]);
      if (name === "system.process_kill") {
        throw new ToolError(
          "the device refused: system.process_kill is not in this grant",
          200,
          null,
          true
        );
      }
      throw new Error(`the test did not expect a ${name} call`);
    });
    render(<SystemPage grant={GRANT} />);
    await screen.findByText("1 process on the device, by pid.");

    fireEvent.click(screen.getByRole("button", { name: "Kill PID 4242 (nginx)" }));
    const dialog = await screen.findByRole("dialog", { name: "Kill PID 4242 (nginx)?" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Kill PID 4242" }));

    // The device's own sentence sits beside the process it refused to end.
    expect(
      await within(dialog).findByText(
        "The device did not end PID 4242 (nginx): the device refused: " +
          "system.process_kill is not in this grant"
      )
    ).toBeInTheDocument();
    // The modal stays open so the operator can read it, cancel, or try again.
    expect(screen.getByRole("dialog", { name: "Kill PID 4242 (nginx)?" })).toBeInTheDocument();

    // A refusal is still an attempt: the list is what says whether the process is gone.
    await vi.waitFor(() => expect(called("system.process_list")).toHaveLength(2));
    expect(called("system.process_kill")).toHaveLength(1);
    expect(screen.getByRole("dialog", { name: "Kill PID 4242 (nginx)?" })).toBeInTheDocument();
  });

  it("tests the host and port, sending the timeout only when it is given", async () => {
    device((name) => {
      if (name === "system.process_list") return JSON.stringify([]);
      if (name === "system.net_test") {
        return JSON.stringify({
          addresses: ["93.184.216.34"],
          connected: "93.184.216.34",
          latency_ms: 12,
          error: null
        });
      }
      throw new Error(`the test did not expect a ${name} call`);
    });
    render(<SystemPage grant={GRANT} />);
    await screen.findByText("The device reports no running processes.");

    fireEvent.change(screen.getByLabelText("Host"), { target: { value: "example.com" } });
    fireEvent.change(screen.getByLabelText("Port"), { target: { value: "443" } });
    // Emptying the timeout means the device's own default; no key rides along.
    fireEvent.change(screen.getByLabelText("Timeout (ms)"), { target: { value: "" } });
    fireEvent.click(screen.getByRole("button", { name: "Test" }));

    await vi.waitFor(() =>
      expect(mocked).toHaveBeenCalledWith(GRANT, "system.net_test", {
        host: "example.com",
        port: 443
      })
    );
    const result = (await screen.findByText("example.com:443")).closest("section")!;
    expect(within(result).getByText("connected")).toBeInTheDocument();
    expect(within(result).getAllByText("93.184.216.34")).toHaveLength(2);
    expect(within(result).getByText("12 ms")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Timeout (ms)"), { target: { value: "1500" } });
    fireEvent.click(screen.getByRole("button", { name: "Test" }));

    await vi.waitFor(() =>
      expect(mocked).toHaveBeenCalledWith(GRANT, "system.net_test", {
        host: "example.com",
        port: 443,
        timeout_ms: 1500
      })
    );
  });

  it("opens the restart confirmation, and Cancel makes no call", async () => {
    device((name) => {
      if (name === "system.process_list") return JSON.stringify([]);
      throw new Error(`the test did not expect a ${name} call`);
    });
    render(<SystemPage grant={GRANT} />);
    await screen.findByText("The device reports no running processes.");

    // The grant decides whether the call is allowed; the page does not ask for a tool list first
    // (this mock fails an unasked-for call) and does not hide the deliberate step on a guess.
    const button = screen.getByRole("button", { name: "Restart app" });
    expect(button).toBeEnabled();
    fireEvent.click(button);

    const dialog = await screen.findByRole("dialog", { name: "Restart the device app?" });
    // The confirmation names the losses and the launcher budget, not just "are you sure".
    expect(dialog).toHaveTextContent(
      "Every open terminal, browser tab and run on the device ends"
    );
    expect(dialog).toHaveTextContent("The launcher allows three consecutive restarts");
    expect(dialog).toHaveTextContent("three quick restarts can leave the device down");
    // Cancel is the first focus, and it is the way out without a call.
    const cancel = within(dialog).getByRole("button", { name: "Cancel" });
    expect(cancel).toHaveFocus();
    fireEvent.click(cancel);

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(called("host.restart")).toHaveLength(0);
  });

  it("restarts through the tool exactly once, then disables the button until a reload", async () => {
    device((name) => {
      if (name === "system.process_list") return JSON.stringify([]);
      if (name === "host.restart") return JSON.stringify({ restarting: true });
      throw new Error(`the test did not expect a ${name} call`);
    });
    render(<SystemPage grant={GRANT} />);
    await screen.findByText("The device reports no running processes.");

    fireEvent.click(screen.getByRole("button", { name: "Restart app" }));
    const dialog = await screen.findByRole("dialog", { name: "Restart the device app?" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Restart app" }));

    // The empty object is the whole argument set: the tool restarts the app, not a named process.
    await vi.waitFor(() => expect(called("host.restart")).toHaveLength(1));
    expect(mocked).toHaveBeenCalledWith(GRANT, "host.restart", {});
    expect(
      await screen.findByText(
        "The device is restarting through its launcher. It will reconnect in a few seconds; " +
          "terminals, browser tabs and runs on the device ended."
      )
    ).toBeInTheDocument();
    // The answer precedes the exit: the sentence says restarting, never that it is back.
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Restart app" })).toBeDisabled();
    expect(called("host.restart")).toHaveLength(1);
  });

  it("shows the device's refusal in its own words and leaves the restart available", async () => {
    const refusal =
      "this device was not started by the launcher; restarting it here would not bring it back";
    device((name) => {
      if (name === "system.process_list") return JSON.stringify([]);
      if (name === "host.restart") throw new ToolError(refusal, 200, null, true);
      throw new Error(`the test did not expect a ${name} call`);
    });
    render(<SystemPage grant={GRANT} />);
    await screen.findByText("The device reports no running processes.");

    fireEvent.click(screen.getByRole("button", { name: "Restart app" }));
    const dialog = await screen.findByRole("dialog", { name: "Restart the device app?" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Restart app" }));

    // The refusal is the device's, shown verbatim: a standalone app has no launcher to bring it back.
    expect(
      await screen.findByText("The device did not restart: " + refusal)
    ).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    // A refused restart did not restart anything, so the operator may try after fixing the grant.
    expect(screen.getByRole("button", { name: "Restart app" })).toBeEnabled();
    expect(called("host.restart")).toHaveLength(1);
  });

  it("shows the tool's error when the call failed before the device answered", async () => {
    device((name) => {
      if (name === "system.process_list") return JSON.stringify([]);
      if (name === "host.restart") {
        throw new ToolError("the console never reached the device: fetch failed", 503);
      }
      throw new Error(`the test did not expect a ${name} call`);
    });
    render(<SystemPage grant={GRANT} />);
    await screen.findByText("The device reports no running processes.");

    fireEvent.click(screen.getByRole("button", { name: "Restart app" }));
    const dialog = await screen.findByRole("dialog", { name: "Restart the device app?" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Restart app" }));

    expect(
      await screen.findByText(
        "The device did not restart: the console never reached the device: fetch failed"
      )
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Restart app" })).toBeEnabled();
  });

  it("shows a net refusal and keeps the result of the last test that finished", async () => {
    let refused = false;
    device((name) => {
      if (name === "system.process_list") return JSON.stringify([]);
      if (name === "system.net_test") {
        if (refused) {
          throw new ToolError(
            "the device refused: the name did not resolve",
            200,
            null,
            true
          );
        }
        return JSON.stringify({
          addresses: ["93.184.216.34"],
          connected: "93.184.216.34",
          latency_ms: 12,
          error: null
        });
      }
      throw new Error(`the test did not expect a ${name} call`);
    });
    render(<SystemPage grant={GRANT} />);
    await screen.findByText("The device reports no running processes.");

    fireEvent.change(screen.getByLabelText("Host"), { target: { value: "example.com" } });
    fireEvent.change(screen.getByLabelText("Port"), { target: { value: "443" } });
    fireEvent.click(screen.getByRole("button", { name: "Test" }));
    const previous = (await screen.findByText("example.com:443")).closest("section")!;
    expect(within(previous).getByText("connected")).toBeInTheDocument();

    // A name the device cannot resolve is its refusal, and the old report is not erased.
    refused = true;
    fireEvent.change(screen.getByLabelText("Host"), { target: { value: "missing.invalid" } });
    fireEvent.click(screen.getByRole("button", { name: "Test" }));

    expect(
      await screen.findByText(
        "Could not test missing.invalid:443: the device refused: the name did not resolve"
      )
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "The result below is from the last test that finished; this attempt did not replace it."
      )
    ).toBeInTheDocument();
    // Still the previous target's report, still labelled as connected.
    const kept = screen.getByText("example.com:443").closest("section")!;
    expect(within(kept).getByText("connected")).toBeInTheDocument();
    expect(within(kept).getByText("12 ms")).toBeInTheDocument();
    expect(called("system.net_test")).toHaveLength(2);
  });
});
