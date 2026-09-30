// WorkspacesPanel pins — ADD A MACHINE, then every path on it is a workspace.
//
// WHAT IT PINS: that adding a machine CONNECTS ONCE (a successful `terminal_open` is what saves a connection,
// and it is the only thing that proves the credentials work before a workspace depends on them), that the session
// is closed again, that the machine's root is registered, that browsing lists DIRECTORIES, and the three states
// that are not each other — loading, the agent's refusal verbatim, and the agent not answering at all.
//
// WHAT IT CANNOT PIN: the layout (jsdom has none) and the real connect (no sshd here) — the device is where that
// was measured.
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { callApi } from "../../lib/api";
import { WorkspacesPanel } from "../WorkspacesPanel";

vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  callApi: vi.fn(),
}));
const mockApi = callApi as unknown as ReturnType<typeof vi.fn>;

const SSH = { id: "ssh:zhengsaisi@10.10.61.83:22122", kind: "ssh", label: "10.10.61.83" };

/** The two GETs the page opens with, plus whatever the test adds. */
function reads({ connections = [], mappings = [] }: { connections?: unknown[]; mappings?: unknown[] }) {
  mockApi.mockImplementation((path: string) => {
    if (path === "/api/workspace/connections") return Promise.resolve({ ok: true, connections });
    if (path === "/api/workspace/mappings") return Promise.resolve({ ok: true, mappings });
    // A tool route answers `{ok, result}`; the open's result IS the session id, and without one the page
    // correctly skips the close — which a mock returning a bare `{ok:true}` used to hide.
    if (path === "/api/tools/terminal_open") return Promise.resolve({ ok: true, result: "term-abc-0" });
    // THE HOME, asked through the exec door: the prefix is DERIVED, and this is where it comes from.
    if (path === "/api/workspace/exec")
      return Promise.resolve({
        ok: true,
        stdout_b64: btoa("/home/zhengsaisi\n"),
        stderr_b64: "",
        status: { kind: "exited", code: 0 },
      });
    return Promise.resolve({ ok: true });
  });
}

describe("WorkspacesPanel", () => {
  beforeEach(() => {
    mockApi.mockReset();
  });

  it("adds a machine by CONNECTING once, closing that session, and registering its root", async () => {
    reads({ connections: [], mappings: [] });
    render(<WorkspacesPanel />);
    // THE MACHINES COME FIRST: the form is behind a button, so a test that wants it must ask for it.
    fireEvent.click(screen.getByRole("button", { name: /add a host/i }));
    fireEvent.change(screen.getByLabelText("Host"), { target: { value: "10.10.61.83" } });
    fireEvent.change(screen.getByLabelText("User"), { target: { value: "zhengsaisi" } });
    fireEvent.change(screen.getByLabelText("Port"), { target: { value: "22122" } });
    fireEvent.change(screen.getByLabelText("Private key path on THIS device"), {
      target: { value: "C:\\ProgramData\\Summrise\\ws-key" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Connect and add" }));

    await waitFor(() =>
      expect(mockApi).toHaveBeenCalledWith("/api/tools/terminal_open", {
        method: "POST",
        body: JSON.stringify({
          kind: "ssh",
          target: "zhengsaisi@10.10.61.83:22122",
          key_path: "C:\\ProgramData\\Summrise\\ws-key",
          rows: 24,
          cols: 80,
        }),
      }),
    );
    // the session it opened is closed again: adding a machine is not leaving a terminal behind
    await waitFor(() =>
      expect(mockApi).toHaveBeenCalledWith("/api/tools/terminal_close", {
        method: "POST",
        body: JSON.stringify({ session_id: "term-abc-0" }),
      }),
    );
    // and the machine's HOME is what gets registered — derived, not asked for. A path-keyed registry
    // can give "/" to only ONE machine, which is the collision this page must not walk into.
    await waitFor(() =>
      expect(mockApi).toHaveBeenCalledWith("/api/workspace/register", {
        method: "POST",
        body: JSON.stringify({ path: "/home/zhengsaisi", connection_id: "ssh:zhengsaisi@10.10.61.83:22122" }),
      }),
    );
  });

  it("says a machine is added once its root is registered, and offers Add before that", async () => {
    reads({ connections: [SSH], mappings: [] });
    const first = render(<WorkspacesPanel />);
    await waitFor(() => expect(screen.getByRole("button", { name: `Add ${SSH.id}` })).toBeTruthy());
    first.unmount();

    reads({ connections: [SSH], mappings: [{ path: "/home/zhengsaisi", connection_id: SSH.id }] });
    render(<WorkspacesPanel />);
    // ADDED: the machine can be opened, and the offer to add it is gone. The prefix is whatever that
    // machine registered — the page looks for the connection's own mapping, not for a fixed "/".
    await waitFor(() => expect(screen.getByRole("button", { name: /open/i })).toBeTruthy());
    expect(screen.queryByRole("button", { name: `Add ${SSH.id}` })).toBeNull();
  });

  it("shows the agent's refusal VERBATIM", async () => {
    reads({ connections: [SSH], mappings: [] });
    mockApi.mockImplementation((path: string) => {
      if (path === "/api/workspace/connections") return Promise.resolve({ ok: true, connections: [SSH] });
      if (path === "/api/workspace/mappings") return Promise.resolve({ ok: true, mappings: [] });
      return Promise.resolve({ ok: false, error: 'no saved connection "ssh:x@y:22"' });
    });
    render(<WorkspacesPanel />);
    await waitFor(() => expect(screen.getByRole("button", { name: `Add ${SSH.id}` })).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: `Add ${SSH.id}` }));
    await waitFor(() =>
      expect(screen.getByRole("status").textContent).toContain('no saved connection "ssh:x@y:22"'),
    );
  });

  it("treats a 200 with ok:false as a REFUSAL, not as an empty list", async () => {
    // Reachable today: a build without the terminal feature answers `internal` to both reads, and drawing that as
    // "no saved ssh connection yet" is a claim nobody checked.
    mockApi.mockImplementation((path: string) => {
      if (path === "/api/workspace/connections")
        return Promise.resolve({ ok: false, code: "internal", error: "the terminal feature is disabled" });
      return Promise.resolve({ ok: true, mappings: [] });
    });
    render(<WorkspacesPanel />);
    await waitFor(() =>
      expect(screen.getByRole("status").textContent).toContain("the terminal feature is disabled"),
    );
    expect(screen.queryByText(/No saved ssh connection yet/)).toBeNull();
  });

  it("says the agent did not answer instead of drawing an empty list as 'there are none'", async () => {
    mockApi.mockRejectedValue(new Error("unauthorized"));
    render(<WorkspacesPanel />);
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("did not answer"));
    expect(screen.queryByText("None yet — add one above.")).toBeNull();
  });

  it("browses directories only, and 'Use this folder' pins the path it is looking at", async () => {
    mockApi.mockImplementation((path: string) => {
      if (path === "/api/workspace/connections") return Promise.resolve({ ok: true, connections: [SSH] });
      if (path === "/api/workspace/mappings")
        return Promise.resolve({ ok: true, mappings: [{ path: "/", connection_id: SSH.id }] });
      if (path === "/api/workspace/fs")
        return Promise.resolve({
          ok: true,
          entries: [
            { name: "etc", kind: "dir" },
            { name: "vmlinuz", kind: "file" },
          ],
        });
      return Promise.resolve({ ok: true });
    });
    render(<WorkspacesPanel />);
    // OPENING the machine is what lists its files: the browse affordance is the machine's own Open,
    // and the listing lives in that view rather than under the row.
    await waitFor(() => expect(screen.getByRole("button", { name: /open/i })).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: /open/i }));

    await waitFor(() => expect(screen.getByRole("button", { name: "etc/" })).toBeTruthy());
    expect(screen.queryByRole("button", { name: "vmlinuz/" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Use /" }));
    await waitFor(() =>
      expect(mockApi).toHaveBeenCalledWith("/api/workspace/register", {
        method: "POST",
        body: JSON.stringify({ path: "/", connection_id: SSH.id }),
      }),
    );
  });

  // ── THE MACHINE IS THE OBJECT, NOT A PATH PREFIX ────────────────────────────────────────────────
  //
  // The operator's verdict on the first version was "太丑了，根本没法用，还有现在的机制对吗" — and the
  // mechanism part was the load-bearing one: the page was a FORM over a registry of PATH PREFIXES, so the
  // reader had to think about prefixes (an implementation detail) and could not edit anything. These four
  // pin the shape that replaces it: machines first, the form behind a button, one machine opened at a
  // time, its files EDITABLE with the version the read returned, and its prefix derived rather than
  // managed.

  it("shows the MACHINES first: the add form is behind a button, not the opening screen", async () => {
    reads({ connections: [SSH], mappings: [] });
    render(<WorkspacesPanel />);
    await waitFor(() => expect(screen.getByText("10.10.61.83")).toBeTruthy());
    // the form is NOT there until it is asked for
    expect(screen.queryByLabelText("Host")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /add (a )?host/i }));
    expect(screen.getByLabelText("Host")).toBeTruthy();
  });

  it("derives the machine's HOME as its prefix instead of asking the reader for one", async () => {
    reads({ connections: [], mappings: [] });
    mockApi.mockImplementation((path: string, init?: { body?: string }) => {
      if (path === "/api/workspace/connections") return Promise.resolve({ ok: true, connections: [] });
      if (path === "/api/workspace/mappings") return Promise.resolve({ ok: true, mappings: [] });
      if (path === "/api/tools/terminal_open") return Promise.resolve({ ok: true, result: "term-abc-0" });
      if (path === "/api/workspace/exec")
        return Promise.resolve({
          ok: true,
          stdout_b64: btoa("/home/zhengsaisi\n"),
          stderr_b64: "",
          status: { kind: "exited", code: 0 },
        });
      if (path === "/api/workspace/register") return Promise.resolve({ ok: true });
      return Promise.resolve({ ok: true });
    });
    render(<WorkspacesPanel />);
    fireEvent.click(await screen.findByRole("button", { name: /add (a )?host/i }));
    fireEvent.change(screen.getByLabelText("Host"), { target: { value: "10.10.61.83" } });
    fireEvent.change(screen.getByLabelText("User"), { target: { value: "zhengsaisi" } });
    fireEvent.change(screen.getByLabelText("Port"), { target: { value: "22122" } });
    fireEvent.click(screen.getByRole("button", { name: /connect|add/i }));
    await waitFor(() => {
      const registered = mockApi.mock.calls.filter((c) => c[0] === "/api/workspace/register");
      expect(registered.length).toBeGreaterThan(0);
      // THE HOME, NOT "/": a machine's home is where its work is, and "/" can only ever belong to one
      // machine in a path-keyed registry — the collision this page must not walk into.
      expect(String(registered[0][1]?.body)).toContain("/home/zhengsaisi");
    });
  });

  it("opening a machine is a VIEW: its files, a way to run a command, and a way back", async () => {
    reads({ connections: [SSH], mappings: [{ path: "/home/zhengsaisi", connection_id: SSH.id }] });
    mockApi.mockImplementation((path: string) => {
      if (path === "/api/workspace/connections") return Promise.resolve({ ok: true, connections: [SSH] });
      if (path === "/api/workspace/mappings") return Promise.resolve({ ok: true, mappings: [{ path: "/home/zhengsaisi", connection_id: SSH.id }] });
      if (path === "/api/workspace/fs") return Promise.resolve({ ok: true, entries: [{ name: "summrise", kind: "dir" }, { name: "notes.txt", kind: "file" }] });
      return Promise.resolve({ ok: true });
    });
    render(<WorkspacesPanel />);
    fireEvent.click(await screen.findByRole("button", { name: /open/i }));
    // A DIRECTORY IS SPELLED WITH ITS SLASH (the same shape the browse test asserts with "etc/"):
    // the trailing slash is what tells the reader it can be entered, and a file does not get one.
    await waitFor(() => expect(screen.getByText("summrise/")).toBeTruthy());
    expect(screen.getByText("notes.txt")).toBeTruthy();
    // A WAY TO RUN A COMMAND: the field is what makes it a command line (an argv, never a shell
    // string), and Run is what sends it.
    expect(screen.getByLabelText("Command")).toBeTruthy();
    expect(screen.getByRole("button", { name: /^run$/i })).toBeTruthy();
    expect(screen.getByRole("button", { name: /back|machines/i })).toBeTruthy();
  });

  it("EDITING a file sends the version the read returned, and a conflict is shown verbatim", async () => {
    reads({ connections: [SSH], mappings: [{ path: "/home/zhengsaisi", connection_id: SSH.id }] });
    mockApi.mockImplementation((path: string, init?: { body?: string }) => {
      if (path === "/api/workspace/connections") return Promise.resolve({ ok: true, connections: [SSH] });
      if (path === "/api/workspace/mappings") return Promise.resolve({ ok: true, mappings: [{ path: "/home/zhengsaisi", connection_id: SSH.id }] });
      if (path === "/api/workspace/fs") {
        const body = JSON.parse(String(init?.body ?? "{}"));
        if (body.op === "listDir") return Promise.resolve({ ok: true, entries: [{ name: "notes.txt", kind: "file" }] });
        if (body.op === "readText") return Promise.resolve({ ok: true, text: "hello", version: "sha256:abc" });
        if (body.op === "writeText") return Promise.resolve({ ok: false, code: "FS_VERSION_CONFLICT", error: "notes.txt is not the content this write expected" });
        return Promise.resolve({ ok: true });
      }
      return Promise.resolve({ ok: true });
    });
    render(<WorkspacesPanel />);
    fireEvent.click(await screen.findByRole("button", { name: /open/i }));
    fireEvent.click(await screen.findByText("notes.txt"));
    const editor = await screen.findByLabelText(/contents|text/i);
    fireEvent.change(editor, { target: { value: "hello there" } });
    fireEvent.click(screen.getByRole("button", { name: /save/i }));
    await waitFor(() => {
      const writes = mockApi.mock.calls.filter((c) => c[0] === "/api/workspace/fs" && String(c[1]?.body).includes("writeText"));
      expect(writes.length).toBe(1);
      const body = JSON.parse(String(writes[0][1]?.body));
      // THE GUARD IS THE POINT: a write that cannot say what it replaces is the silent loss this door
      // refuses, so the page must carry the version the READ returned.
      expect(body.expect_version).toBe("sha256:abc");
      expect(body.text).toBe("hello there");
    });
    // the refusal is the agent's own sentence, not a paraphrase
    expect(await screen.findByText(/not the content this write expected/)).toBeTruthy();
  });

  // ── THE SURFACE MUST NOT CALL A FAILURE A SUCCESS ───────────────────────────────────────────────
  //
  // The exec door answers `{ok, stdout_b64, stderr_b64, status, stdout_overflow, stderr_overflow}`:
  // `ok` says the CALL worked, `status` says what the COMMAND did, and the overflow flags say whether
  // what came back is all of it. The first version of this page read none of the three — it printed the
  // output of a command that exited 3 as if it had succeeded, and showed a truncated stream as complete.
  // This repository's own rule is the reason these are pinned: a surface that reports a state must name
  // the thing that was observed.

  it("says a command FAILED, with its exit code, instead of printing its output as success", async () => {
    reads({ connections: [SSH], mappings: [{ path: "/home/zhengsaisi", connection_id: SSH.id }] });
    mockApi.mockImplementation((path: string) => {
      if (path === "/api/workspace/connections") return Promise.resolve({ ok: true, connections: [SSH] });
      if (path === "/api/workspace/mappings")
        return Promise.resolve({ ok: true, mappings: [{ path: "/home/zhengsaisi", connection_id: SSH.id }] });
      if (path === "/api/workspace/fs") return Promise.resolve({ ok: true, entries: [] });
      if (path === "/api/workspace/exec")
        return Promise.resolve({
          ok: true,
          stdout_b64: btoa(""),
          stderr_b64: btoa("grep: nope: No such file or directory\n"),
          status: { kind: "exited", code: 2 },
          stdout_overflow: false,
          stderr_overflow: false,
        });
      return Promise.resolve({ ok: true });
    });
    render(<WorkspacesPanel />);
    fireEvent.click(await screen.findByRole("button", { name: /open/i }));
    fireEvent.change(await screen.findByLabelText("Command"), { target: { value: "grep nope /etc/hosts" } });
    fireEvent.click(screen.getByRole("button", { name: /^run$/i }));
    // THE CODE IS THE FACT: "exit 2" is what tells the reader this did not work.
    expect(await screen.findByText(/exit 2/)).toBeTruthy();
    expect(screen.getByText(/No such file or directory/)).toBeTruthy();
  });

  it("says a SIGNALLED command was signalled, and says when the output was CUT", async () => {
    reads({ connections: [SSH], mappings: [{ path: "/home/zhengsaisi", connection_id: SSH.id }] });
    mockApi.mockImplementation((path: string) => {
      if (path === "/api/workspace/connections") return Promise.resolve({ ok: true, connections: [SSH] });
      if (path === "/api/workspace/mappings")
        return Promise.resolve({ ok: true, mappings: [{ path: "/home/zhengsaisi", connection_id: SSH.id }] });
      if (path === "/api/workspace/fs") return Promise.resolve({ ok: true, entries: [] });
      if (path === "/api/workspace/exec")
        return Promise.resolve({
          ok: true,
          stdout_b64: btoa("partial output"),
          stderr_b64: btoa(""),
          status: { kind: "signalled", name: "KILL" },
          stdout_overflow: true,
          stderr_overflow: false,
        });
      return Promise.resolve({ ok: true });
    });
    render(<WorkspacesPanel />);
    fireEvent.click(await screen.findByRole("button", { name: /open/i }));
    fireEvent.change(await screen.findByLabelText("Command"), { target: { value: "sleep 999" } });
    fireEvent.click(screen.getByRole("button", { name: /^run$/i }));
    // A TRUNCATED STREAM IS REPORTED LOSSY — the seam's own rule, and the reason the door sends the flag.
    expect(await screen.findByText(/KILL/)).toBeTruthy();
    expect(screen.getByText(/cut|truncat/i)).toBeTruthy();
  });

  // ── OPENING A TERMINAL ON THE MACHINE ────────────────────────────────────────────────────────────
  //
  // "并在那里开终端 / 跑命令" is what the operator asked for, and the two are different tools: the command
  // line runs ONE argv through the exec door, while a terminal is a SESSION on that machine. The agent
  // already reconnects a saved connection by id (`terminal_connect_saved`), so this page does not need
  // credentials again — it needs the id it already has.

  it("opens a TERMINAL on the machine, by reconnecting the connection the agent saved", async () => {
    reads({ connections: [SSH], mappings: [{ path: "/home/zhengsaisi", connection_id: SSH.id }] });
    mockApi.mockImplementation((path: string) => {
      if (path === "/api/workspace/connections") return Promise.resolve({ ok: true, connections: [SSH] });
      if (path === "/api/workspace/mappings")
        return Promise.resolve({ ok: true, mappings: [{ path: "/home/zhengsaisi", connection_id: SSH.id }] });
      if (path === "/api/workspace/fs") return Promise.resolve({ ok: true, entries: [] });
      if (path === "/api/tools/terminal_connect_saved")
        return Promise.resolve({ ok: true, result: "term-saved-7" });
      return Promise.resolve({ ok: true });
    });
    const onOpenTerminal = vi.fn();
    render(<WorkspacesPanel onOpenTerminal={onOpenTerminal} />);
    fireEvent.click(await screen.findByRole("button", { name: /open/i }));
    fireEvent.click(await screen.findByRole("button", { name: /terminal/i }));
    await waitFor(() =>
      expect(mockApi).toHaveBeenCalledWith("/api/tools/terminal_connect_saved", {
        method: "POST",
        body: JSON.stringify({ id: SSH.id }),
      }),
    );
    // and the page hands the session to the shell, which is what shows it: the terminal UI lives on
    // the Terminal page, and duplicating it here would be a second terminal to keep alive.
    await waitFor(() => expect(onOpenTerminal).toHaveBeenCalledWith("term-saved-7"));
  });

  it("says why a terminal could NOT be opened, in the agent's own words", async () => {
    reads({ connections: [SSH], mappings: [{ path: "/home/zhengsaisi", connection_id: SSH.id }] });
    mockApi.mockImplementation((path: string) => {
      if (path === "/api/workspace/connections") return Promise.resolve({ ok: true, connections: [SSH] });
      if (path === "/api/workspace/mappings")
        return Promise.resolve({ ok: true, mappings: [{ path: "/home/zhengsaisi", connection_id: SSH.id }] });
      if (path === "/api/workspace/fs") return Promise.resolve({ ok: true, entries: [] });
      if (path === "/api/tools/terminal_connect_saved")
        return Promise.resolve({ ok: false, error: "unknown saved connection \"ssh:x@y:22\"; known: (none)" });
      return Promise.resolve({ ok: true });
    });
    const onOpenTerminal = vi.fn();
    render(<WorkspacesPanel onOpenTerminal={onOpenTerminal} />);
    fireEvent.click(await screen.findByRole("button", { name: /open/i }));
    fireEvent.click(await screen.findByRole("button", { name: /terminal/i }));
    expect(await screen.findByText(/unknown saved connection/)).toBeTruthy();
    expect(onOpenTerminal).not.toHaveBeenCalled();
  });
});
