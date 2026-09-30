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
    // and the ROOT is what gets registered — that is what makes every path on the machine resolvable
    await waitFor(() =>
      expect(mockApi).toHaveBeenCalledWith("/api/workspace/register", {
        method: "POST",
        body: JSON.stringify({ path: "/", connection_id: "ssh:zhengsaisi@10.10.61.83:22122" }),
      }),
    );
  });

  it("says a machine is added once its root is registered, and offers Add before that", async () => {
    reads({ connections: [SSH], mappings: [] });
    const first = render(<WorkspacesPanel />);
    await waitFor(() => expect(screen.getByRole("button", { name: `Add ${SSH.id}` })).toBeTruthy());
    first.unmount();

    reads({ connections: [SSH], mappings: [{ path: "/", connection_id: SSH.id }] });
    render(<WorkspacesPanel />);
    await waitFor(() => expect(screen.getByText(/every path on it works/)).toBeTruthy());
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
    await waitFor(() => expect(screen.getByRole("button", { name: `Browse ${SSH.id}` })).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: `Browse ${SSH.id}` }));

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
      if (path === "/api/workspace/exec") return Promise.resolve({ ok: true, stdout_b64: btoa("/home/zhengsaisi\n"), code: 0 });
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
    await waitFor(() => expect(screen.getByText("summrise")).toBeTruthy());
    expect(screen.getByText("notes.txt")).toBeTruthy();
    expect(screen.getByRole("button", { name: /command/i })).toBeTruthy();
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
});
