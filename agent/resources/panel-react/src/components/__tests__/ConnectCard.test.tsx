// ConnectCard — the AI-client onboarding surface.
//
// These pin the properties that make the card TRUSTWORTHY, which is not the
// same as pinning its markup:
//
//   * the endpoint is derived from the live location, so it is correct for
//     whoever is looking at the panel (loopback, LAN, or tunnel) — a hardcoded
//     value is wrong for every remote user;
//   * the device token is MASKED until asked for, and NEVER placed in a URL
//     (ADR 0004: the permanent device token never rides in a URL);
//   * the tool counts come from the live /api/spec, not a baked-in list that
//     drifts the moment a tool is added;
//   * the self-test distinguishes a working credential from a broken one —
//     the failure users actually hit is a syntactically perfect config that
//     still 401s.
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { ConnectCard } from "../ConnectCard";
import { callApi, getHost } from "../../lib/api";

const TOKEN =
  "d3adb33fdeadbeefd3adb33fdeadbeefd3adb33fdeadbeefd3adb33fdeadbeef";
const HOST = "panel-test-host.local:18080";

vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  callApi: vi.fn(),
  getHost: vi.fn(() => HOST),
  getToken: vi.fn(() => TOKEN),
}));

const mockCallApi = callApi as unknown as ReturnType<typeof vi.fn>;

// `ok: true` IS PART OF THE DEVICE'S ANSWER, and this fixture used to omit it. The real route
// (`agent/src/web/mod.rs: api_spec`) returns `{"ok": true, "plugins": …}`, and the card's read now
// goes through `useDeviceRead`, which — like every other reader in this panel — refuses a body that
// is not `ok:true` rather than folding it. The fixture was not the device's shape; the migration is
// what said so.
const SPEC = {
  ok: true,
  plugins: [
    {
      name: "terminal",
      tools: [
        { name: "terminal_execute" },
        { name: "terminal_open" },
        { name: "terminal_read" },
      ],
    },
    {
      name: "system",
      tools: [{ name: "system_file_list" }, { name: "system_process_kill" }],
    },
    { name: "memory", tools: [{ name: "memory_save" }] },
    { name: "update", tools: [{ name: "agent_update" }] },
  ],
};

beforeEach(() => {
  mockCallApi.mockReset();
  mockCallApi.mockImplementation((path: string) => {
    if (path === "/api/spec") return Promise.resolve(SPEC);
    if (path === "/api/status")
      return Promise.resolve({
        ok: true,
        version: "1.2.307",
        live_sessions: 2,
      });
    return Promise.resolve({});
  });
});

async function renderCard() {
  render(<ConnectCard />);
  // The spec fetch resolves in an effect; wait for the count to render so the
  // tests below assert on a settled card rather than a loading one.
  await screen.findByText(/tools available on this device/);
}

describe("ConnectCard", () => {
  it("builds the endpoint from the LIVE host, not a baked-in one", async () => {
    await renderCard();
    const snippet = document.querySelector(".connect-snippet")!.textContent!;
    expect(snippet, "the live host must appear in the config").toContain(HOST);
    // The loopback default must NOT be assumed: this is the bug that breaks
    // every remote/tunnel user, and it is invisible on the dev machine.
    expect(snippet, "must not hardcode loopback").not.toContain(
      "127.0.0.1:18080",
    );
    expect(snippet).toContain("/mcp");
  });

  // THE TEST ABOVE'S NAME SAYS "LIVE" AND IT ONLY EVER TESTED MOUNT. The endpoint was
  // `useMemo(() => …, [])` — computed once — so a host change left a copy-ready URL pointing at the old
  // device, in the one card whose whole job is telling an operator what to paste. It lives in Settings,
  // which stays mounted across a reconnect, so this is reachable by an ordinary operator. The assertion
  // is the CHANGE, which is the only shape that can tell a live derivation from a frozen one.
  it("re-derives the endpoint when the host changes", async () => {
    const host = getHost as unknown as ReturnType<typeof vi.fn>;
    host.mockReturnValue(HOST);
    const { rerender } = render(<ConnectCard />);
    await screen.findByText(/tools available on this device/);
    expect(document.querySelector(".connect-snippet")!.textContent).toContain(
      HOST,
    );

    host.mockReturnValue("moved.example:18080");
    rerender(<ConnectCard />);
    const after = document.querySelector(".connect-snippet")!.textContent!;
    expect(after, "the new host must appear").toContain("moved.example:18080");
    expect(after, "and the old one must be gone").not.toContain(HOST);
  });

  it("masks the device token until it is explicitly revealed", async () => {
    await renderCard();
    const snippet = () =>
      document.querySelector(".connect-snippet")!.textContent!;
    expect(snippet(), "token must be masked by default").not.toContain(TOKEN);
    expect(snippet(), "mask must be an obvious placeholder").toContain(
      "<your-device-token>",
    );

    fireEvent.click(screen.getByText("Reveal token"));
    await waitFor(() => expect(snippet()).toContain(TOKEN));
    expect(snippet(), "placeholder must be gone once revealed").not.toContain(
      "<your-device-token>",
    );

    fireEvent.click(screen.getByText("Hide token"));
    await waitFor(() => expect(snippet()).not.toContain(TOKEN));
  });

  it("never puts the credential in a URL (ADR 0004) — for EVERY client", async () => {
    await renderCard();
    fireEvent.click(screen.getByText("Reveal token"));
    await waitFor(() =>
      expect(document.querySelector(".connect-snippet")!.textContent).toContain(
        TOKEN,
      ),
    );
    const snippet = () =>
      document.querySelector(".connect-snippet")!.textContent!;

    // Asserted across all three clients, not just the default: "the token is
    // not in a URL" is a property of the card, and a fourth client added later
    // must not be able to opt out of it.
    for (const tab of ["DSH", "Claude Code", "curl (a quick check)"]) {
      fireEvent.click(screen.getByText(tab));
      await waitFor(() => expect(snippet()).toContain(TOKEN));
      // `?token=` is what ADR 0004 exists to prevent, and the panel's own URL
      // handling already treats a URL token as the legacy path.
      expect(
        snippet(),
        `${tab}: token must not ride a query string`,
      ).not.toMatch(/[?&]token=/);
      // A Bearer header is the only sanctioned carrier on the device surface.
      // (The syntax differs per client — JSON key vs --header/-H flag — so the
      // assertion is on the two words, not on the punctuation.)
      expect(
        snippet(),
        `${tab}: token must ride an Authorization header`,
      ).toContain("Authorization");
      expect(snippet(), `${tab}: header must be a Bearer credential`).toContain(
        "Bearer",
      );
    }
  });

  it("gives each client its own config shape", async () => {
    await renderCard();
    const snippet = () =>
      document.querySelector(".connect-snippet")!.textContent!;
    const dsh = snippet();
    fireEvent.click(screen.getByText("Claude Code"));
    await waitFor(() => expect(snippet()).not.toBe(dsh));
    expect(snippet(), "Claude Code uses the mcp add CLI").toContain(
      "claude mcp add",
    );
    fireEvent.click(screen.getByText("curl (a quick check)"));
    await waitFor(() => expect(snippet()).toContain("tools/list"));
  });

  it("counts the tool surface from the live spec, not a baked-in list", async () => {
    await renderCard();
    // 3 terminal + 2 system + 1 memory + 1 other = 7 total, derived from SPEC.
    expect(screen.getByText("7")).toBeTruthy();
    // THE FAMILY BREAKDOWN IS GONE ON PURPOSE (1.2.448). The operator's complaint was that Settings had become cluttered, and
    // this list — five rows of prose counting tools by prefix — was the largest thing on the page that was not a setting. The
    // number above is the part that survived, and it is still derived from the live spec rather than a baked-in list, which
    // is what this test was ever about. `.connect-families` no longer exists; `deadStyles` holds that it stays gone.
  });

  it("reports a WORKING credential as connected, with real detail", async () => {
    await renderCard();
    fireEvent.click(screen.getByText("Test this credential"));
    const ok = await screen.findByText(/Connected —/);
    expect(ok.textContent).toContain("1.2.307");
    expect(ok.textContent).toContain("2 live session");
  });

  it("reports a BROKEN credential as failed rather than staying silent", async () => {
    mockCallApi.mockImplementation((path: string) =>
      path === "/api/spec"
        ? Promise.resolve(SPEC)
        : Promise.reject(new Error("HTTP 401")),
    );
    await renderCard();
    fireEvent.click(screen.getByText("Test this credential"));
    const bad = await screen.findByText(/Failed —/);
    expect(bad.textContent).toContain("401");
    // A failure must not be dressed as success — the whole point of the probe.
    expect(document.querySelector(".connect-probe.ok")).toBeNull();
  });

  it("degrades quietly when the spec cannot be read", async () => {
    mockCallApi.mockImplementation(() => Promise.reject(new Error("offline")));
    render(<ConnectCard />);
    await screen.findByText(/Could not read the tool surface/);
    // The config half must still work — a user can connect a client even if the
    // spec read failed, and losing the snippet here would be a worse failure.
    expect(document.querySelector(".connect-snippet")!.textContent).toContain(
      "/mcp",
    );
  });

  // THE SENTENCE IS THE READ'S, NOT AN EMPTY ARRAY'S. It used to be drawn from `tools.length === 0`
  // — the same empty array the catch wrote — so a REFUSAL and an agent that genuinely runs no tools
  // were one sentence. The read state tells them apart now, and these two tests are the difference.
  it("a device that REFUSES the spec is reported as unreadable, not as zero tools", async () => {
    mockCallApi.mockImplementation((path: string) =>
      path === "/api/spec"
        ? Promise.resolve({ ok: false, error: "spec unavailable" })
        : Promise.resolve({}),
    );
    render(<ConnectCard />);
    await screen.findByText(/Could not read the tool surface/);
    expect(screen.queryByText(/tools available on this device/)).toBeNull();
  });

  it("a spec that WAS read and carries no tools says zero, not 'could not read'", async () => {
    mockCallApi.mockImplementation((path: string) =>
      path === "/api/spec"
        ? Promise.resolve({ ok: true, plugins: [] })
        : Promise.resolve({}),
    );
    render(<ConnectCard />);
    await screen.findByText(/tools available on this device/);
    expect(screen.getByText("0")).toBeTruthy();
    expect(screen.queryByText(/Could not read the tool surface/)).toBeNull();
  });
});
