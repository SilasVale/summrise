// SettingsPage memory-capacity card (round-358): GET prefills the three
// fields, Save PUTs them (retention "" → null = keep forever), and invalid
// input blocks the PUT with a hint.
import { expectOneH1, expectNoSkippedLevel } from "../../test-utils/outline";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { SettingsPage } from "../SettingsPage";
import { callApi } from "../../lib/api";

// Partial mock via importOriginal, NOT a hand-written factory. A factory that
// lists only the exports a test happens to use breaks the moment a NEW
// component under SettingsPage reaches for another export — which is exactly
// what happened when ConnectCard began reading getHost/getToken: three
// unrelated memory-card tests failed with "No getHost export is defined on the
// mock". Spreading the real module keeps the mock correct by construction.
vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  callApi: vi.fn(),
}));
const mockCallApi = callApi as unknown as ReturnType<typeof vi.fn>;

const SETTINGS = {
  ok: true,
  buffer_mb: 8,
  console_url: null,
  tunnel_configured: false,
  tunnel_running: false,
  memory_max_entries: 50,
  memory_max_bytes_mb: 16,
  memory_retention_days: 30,
};

beforeEach(() => {
  mockCallApi.mockReset();
  mockCallApi.mockImplementation(async (path: string, opts?: any) => {
    if (!opts || !opts.method || opts.method === "GET") return SETTINGS;
    return { ok: true, buffer_mb: 8 };
  });
});

describe("SettingsPage memory card", () => {
  it("prefills entries/MiB/retention from GET /api/settings", async () => {
    render(<SettingsPage />);
    await waitFor(() => {
      expect(
        (screen.getByLabelText("Memory max entries") as HTMLInputElement).value,
      ).toBe("50");
    });
    expect(
      (screen.getByLabelText("Memory max MiB") as HTMLInputElement).value,
    ).toBe("16");
    expect(
      (screen.getByLabelText("Memory retention days") as HTMLInputElement)
        .value,
    ).toBe("30");
  });

  it("PUTs the edited capacity with retention null when cleared", async () => {
    render(<SettingsPage />);
    await waitFor(() => {
      expect(
        (screen.getByLabelText("Memory max entries") as HTMLInputElement).value,
      ).toBe("50");
    });
    fireEvent.change(screen.getByLabelText("Memory max entries"), {
      target: { value: "100" },
    });
    fireEvent.change(screen.getByLabelText("Memory retention days"), {
      target: { value: "" },
    });
    fireEvent.click(screen.getByLabelText("Save memory capacity"));
    await waitFor(() =>
      expect(screen.getByText("saved — applies immediately")).toBeTruthy(),
    );
    const put = mockCallApi.mock.calls.find((c) => c[1]?.method === "PUT");
    if (!put) throw new Error("expected a PUT /api/settings call");
    expect(JSON.parse(put[1].body)).toEqual({
      memory_max_entries: 100,
      memory_max_bytes_mb: 16,
      memory_retention_days: null,
    });
  });

  it("blocks the PUT on invalid entries with a hint", async () => {
    render(<SettingsPage />);
    await waitFor(() => {
      expect(
        (screen.getByLabelText("Memory max entries") as HTMLInputElement).value,
      ).toBe("50");
    });
    fireEvent.change(screen.getByLabelText("Memory max entries"), {
      target: { value: "0" },
    });
    fireEvent.click(screen.getByLabelText("Save memory capacity"));
    await waitFor(() =>
      expect(screen.getByText("entries must be >= 1")).toBeTruthy(),
    );
    expect(
      mockCallApi.mock.calls.filter((c) => c[1]?.method === "PUT"),
    ).toHaveLength(0);
  });

  it("names itself in the document outline, with no skipped level", () => {
    // The page's OWN name as its only h1 (whatever carries it visually — several pages use a
    // visually hidden one), and no level jumped on the way down. Shared helper so every page
    // inherits the rule: see src/test-utils/outline.ts for why a source scan is not enough.
    const { container } = render(<SettingsPage />);
    expectOneH1(container, "Settings");
    expectNoSkippedLevel(container);
  });
});


// ── THE CONFIGURED BIND (1.2.448) ────────────────────────────────────────────────────────────────────────────────────────
// The operator asked "where is server.host configured?" and no surface could answer: it lived only inside config.yaml on the
// device. The panel now shows it beside `location.host`, which is a DIFFERENT fact, and the sentence after it depends on
// whether the bind is loopback — for a network-bound device "this machine only" would be a lie, which is the case these
// three tests pin.
describe("SettingsPage — the configured bind", () => {
  beforeEach(() => {
    mockCallApi.mockResolvedValue({ ok: true });
  });

  it("shows a loopback bind with the reason and the alternatives", () => {
    render(<SettingsPage config={{ host: "127.0.0.1", port: 18080 }} />);
    expect(screen.getByText(/Bound to 127\.0\.0\.1:18080/)).toBeTruthy();
    expect(screen.getByText(/this machine only/)).toBeTruthy();
    expect(screen.getByText(/the relay, a VPN, or ssh -L/)).toBeTruthy();
  });

  it("does NOT tell a network-bound device that it is local", () => {
    render(<SettingsPage config={{ host: "0.0.0.0", port: 18080 }} />);
    expect(screen.getByText(/Bound to 0\.0\.0\.0:18080/)).toBeTruthy();
    expect(screen.queryByText(/this machine only/)).toBeNull();
    expect(screen.getByText(/keep the device token secret/)).toBeTruthy();
  });

  it("renders nothing rather than a guess when the agent reports no bind", () => {
    render(<SettingsPage />);
    expect(screen.queryByText(/Bound to/)).toBeNull();
  });
});


// ── THE DECLUTTER (1.2.448) ──────────────────────────────────────────────────────────────────────────────────────────────
// The operator's words: "too much unnecessary stuff; the device token and the config file are the two that are needed, trim the
// rest, the panel looks cluttered". Three things follow from that, and each is pinned here: the two named things are ON the
// surface (not behind a click, not in a snippet), the client snippets and the diagnostics are FOLDED, and the config file's
// path is shown when the agent reports it.
describe("SettingsPage — the two things that stay on the surface", () => {
  beforeEach(() => {
    mockCallApi.mockResolvedValue({ ok: true });
  });

  it("shows the device token masked, with a way to reveal it", () => {
    render(<SettingsPage />);
    expect(screen.getByText(/Device token:/)).toBeTruthy();
    expect(screen.getByText("••••••••••••")).toBeTruthy();
    expect(screen.getByText("Reveal")).toBeTruthy();
  });

  it("shows the config file the agent reports", () => {
    render(<SettingsPage config={{ host: "127.0.0.1", port: 18080, path: "C:\\ProgramData\\Vale\\etc\\config.yaml" }} />);
    expect(screen.getByText(/Config file:/)).toBeTruthy();
    expect(screen.getByText(/config\.yaml/)).toBeTruthy();
  });

  it("folds the client snippets and the diagnostics away", () => {
    const { container } = render(<SettingsPage />);
    const folds = [...container.querySelectorAll("details")].map((d) => d.querySelector("summary")?.textContent);
    expect(folds).toContain("Connect an AI client");
    expect(folds).toContain("Diagnostics");
  });
});
