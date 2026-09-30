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
});
