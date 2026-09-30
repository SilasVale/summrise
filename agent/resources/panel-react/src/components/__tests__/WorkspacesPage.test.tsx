// WorkspacesPage pins — the page that turns the registry into something a person can use.
//
// WHAT IT PINS: the states that matter (loading, the agent's own refusal VERBATIM, the agent not answering at
// all), that only SSH connections are offered, and that Add/Remove call the endpoints the registry exposes.
//
// WHAT IT CANNOT PIN: the layout — jsdom has no layout, so this is behaviour only. The rendered page is a
// device's to see.
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { callApi } from "../../lib/api";
import { WorkspacesPage } from "../WorkspacesPage";

vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  callApi: vi.fn(),
}));
const mockCallApi = callApi as unknown as ReturnType<typeof vi.fn>;

/** The two GETs the page opens with, plus whatever the test adds. */
function apiWith({ connections, mappings }: { connections: unknown[]; mappings: unknown[] }) {
  mockCallApi.mockImplementation((path: string) => {
    if (path === "/api/workspace/connections") return Promise.resolve({ ok: true, connections });
    if (path === "/api/workspace/mappings") return Promise.resolve({ ok: true, mappings });
    return Promise.resolve({ ok: true });
  });
}

describe("WorkspacesPage", () => {
  beforeEach(() => {
    mockCallApi.mockReset();
  });

  it("offers only SSH connections, because a pty or a serial line cannot be a workspace", async () => {
    apiWith({
      connections: [
        { id: "pty:powershell", kind: "pty", label: "powershell" },
        { id: "ssh:zhengsaisi@10.10.61.83:22122", kind: "ssh", label: "10.10.61.83" },
      ],
      mappings: [],
    });
    render(<WorkspacesPage />);
    await waitFor(() => expect(screen.getByLabelText("Connection")).toBeTruthy());
    const options = Array.from(screen.getByLabelText("Connection").querySelectorAll("option")).map(
      (o) => o.getAttribute("value"),
    );
    expect(options).toContain("ssh:zhengsaisi@10.10.61.83:22122");
    expect(options).not.toContain("pty:powershell");
  });

  it("shows the agent's refusal VERBATIM — a named refusal is the most useful thing on the page", async () => {
    apiWith({ connections: [{ id: "ssh:a@h:22", kind: "ssh" }], mappings: [] });
    mockCallApi.mockImplementation((path: string) => {
      if (path === "/api/workspace/connections") return Promise.resolve({ ok: true, connections: [{ id: "ssh:a@h:22", kind: "ssh" }] });
      if (path === "/api/workspace/mappings") return Promise.resolve({ ok: true, mappings: [] });
      return Promise.resolve({
        ok: false,
        code: "workspace/unknown-connection",
        error: 'no saved connection "ssh:a@h:22"',
      });
    });
    render(<WorkspacesPage />);
    await waitFor(() => expect(screen.getByLabelText("Path on that host")).toBeTruthy());
    fireEvent.change(screen.getByLabelText("Connection"), { target: { value: "ssh:a@h:22" } });
    fireEvent.change(screen.getByLabelText("Path on that host"), { target: { value: "/srv/x" } });
    fireEvent.click(screen.getByRole("button", { name: "Add workspace" }));
    await waitFor(() =>
      expect(screen.getByRole("status").textContent).toContain('no saved connection "ssh:a@h:22"'),
    );
  });

  it("says the agent did not answer instead of drawing an empty list as 'there are none'", async () => {
    mockCallApi.mockRejectedValue(new Error("unauthorized"));
    render(<WorkspacesPage />);
    await waitFor(() => expect(screen.getByRole("status").textContent).toContain("did not answer"));
    expect(screen.queryByText("None yet.")).toBeNull();
  });

  it("Add registers the path against the chosen connection", async () => {
    apiWith({ connections: [{ id: "ssh:a@h:22", kind: "ssh" }], mappings: [] });
    render(<WorkspacesPage />);
    await waitFor(() => expect(screen.getByLabelText("Path on that host")).toBeTruthy());
    fireEvent.change(screen.getByLabelText("Connection"), { target: { value: "ssh:a@h:22" } });
    fireEvent.change(screen.getByLabelText("Path on that host"), { target: { value: "/srv/x" } });
    fireEvent.click(screen.getByRole("button", { name: "Add workspace" }));
    await waitFor(() =>
      expect(mockCallApi).toHaveBeenCalledWith("/api/workspace/register", {
        method: "POST",
        body: JSON.stringify({ path: "/srv/x", connection_id: "ssh:a@h:22" }),
      }),
    );
  });

  it("Remove unregisters the path it names", async () => {
    apiWith({ connections: [], mappings: [{ path: "/srv/x", connection_id: "ssh:a@h:22" }] });
    render(<WorkspacesPage />);
    await waitFor(() => expect(screen.getByRole("button", { name: "Remove /srv/x" })).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "Remove /srv/x" }));
    await waitFor(() =>
      expect(mockCallApi).toHaveBeenCalledWith("/api/workspace/unregister", {
        method: "POST",
        body: JSON.stringify({ path: "/srv/x" }),
      }),
    );
  });
});
