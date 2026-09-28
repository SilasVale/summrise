// SettingsPage memory-capacity card (round-358): GET prefills the three
// fields, Save PUTs them (retention "" → null = keep forever), and invalid
// input blocks the PUT with a hint.
import { expectOneH1, expectNoSkippedLevel } from "../../test-utils/outline";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor, act } from "@testing-library/react";
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


// ── THE EDIT THAT SURVIVES THE ANSWER ───────────────────────────────────────────────────────────────────────────────────
// The fields are editable from the FIRST frame (they hold the page's defaults), and `/api/settings` is not instant: it
// shells out to `tasklist` for the tunnel state, and a relay adds seconds on top. The hand-rolled read wrote every field
// the reply carried, so an operator who typed while the read was in flight watched the device's value replace their own.
// The read now goes through `useDeviceRead`, and `keepEdits` carries what was typed back OVER the answer — which is also
// why the fields nobody touched still take the device's values (the settle is merged, never withheld).
describe("SettingsPage — the edits the answer must not overwrite", () => {
  /** Answer `GET /api/settings` by hand, so the test owns the order: type first, answer second. */
  function deferSettings() {
    let answer!: (body: unknown) => void;
    const pending = new Promise((res) => {
      answer = res;
    });
    mockCallApi.mockImplementation(async (path: string, opts?: any) => {
      if (path === "/api/settings" && (!opts || !opts.method || opts.method === "GET")) return pending;
      return { ok: true };
    });
    return { answer, pending };
  }
  const valueOf = (label: string) =>
    (screen.getByLabelText(label) as HTMLInputElement).value;

  it("types into a field, lets the read land, and the typed value survives", async () => {
    const { answer, pending } = deferSettings();
    render(<SettingsPage />);
    // THE OPERATOR TYPES BEFORE THE DEVICE ANSWERS. 16 against a device that says 8 (and a default
    // of 8), so nothing but the operator's own text can produce it. The retention field is typed
    // into and then CLEARED — "" means keep forever, and it is the falsy value that a
    // truthiness-guarded write loses. (Typed first because a controlled input whose value does not
    // change fires no `onChange` at all, so clearing an already-empty field is not an edit.)
    fireEvent.change(screen.getByLabelText("Session buffer MiB"), {
      target: { value: "16" },
    });
    fireEvent.change(screen.getByLabelText("Memory retention days"), {
      target: { value: "7" },
    });
    fireEvent.change(screen.getByLabelText("Memory retention days"), {
      target: { value: "" },
    });
    expect(valueOf("Session buffer MiB")).toBe("16");

    await act(async () => {
      answer(SETTINGS);
      await pending;
    });

    expect(
      valueOf("Session buffer MiB"),
      "the typed value survived the answer",
    ).toBe("16");
    expect(
      valueOf("Memory retention days"),
      "and so did the field the operator EMPTIED",
    ).toBe("");
    expect(
      valueOf("Memory max entries"),
      "while a field nobody touched still took the device's value",
    ).toBe("50");
  });

  it("says the read failed, in the sentence it always used, and keeps the defaults", async () => {
    mockCallApi.mockImplementation(async (path: string, opts?: any) => {
      if (path === "/api/settings" && (!opts || !opts.method || opts.method === "GET"))
        throw new Error("HTTP 502");
      return { ok: true };
    });
    render(<SettingsPage />);
    await waitFor(() => expect(screen.getByText("read failed")).toBeTruthy());
    expect(
      valueOf("Session buffer MiB"),
      "a failed read does not blank the form",
    ).toBe("8");
    expect(valueOf("Memory max entries")).toBe("10000");
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
    // **CASE-INSENSITIVE ON PURPOSE.** The reachability sentence became the card's own footnote in round 142, so it
    // starts with a capital — and the first version of this test failed on THAT, not on the sentence: it pinned
    // capitalization, which is not the behaviour it exists to check. **A test that breaks when a sentence is
    // re-punctuated is a test of the punctuation.**
    expect(screen.getByText(/this machine only/i)).toBeTruthy();
    expect(screen.getByText(/the relay, a VPN, or ssh -L/)).toBeTruthy();
  });

  it("does NOT tell a network-bound device that it is local", () => {
    render(<SettingsPage config={{ host: "0.0.0.0", port: 18080 }} />);
    expect(screen.getByText(/Bound to 0\.0\.0\.0:18080/)).toBeTruthy();
    expect(screen.queryByText(/this machine only/i)).toBeNull();
    expect(screen.getByText(/keep the device token secret/)).toBeTruthy();
  });

  it("renders nothing rather than a guess when the agent reports no bind", () => {
    render(<SettingsPage />);
    expect(screen.queryByText(/Bound to/)).toBeNull();
  });
});


// ── THE WAY IN THAT DID NOT EXIST ───────────────────────────────────────────────────────────────────────────────────────
// The loopback sentence listed three ways to reach the machine and the FIRST of them was not on the device that printed it:
// `/api/status` answered `relay: {configured: false, …}` while the card offered "the relay". `useAgentVitals` keeps that as a
// real answer and renders nothing for it — right for a status strip, wrong for the one sentence whose whole subject is HOW TO
// REACH THIS MACHINE. It is also THREE-VALUED, and the third value is the trap: an agent that never mentioned a relay must not
// be told it has none. Saying "no relay configured" off silence is the same lie in the other direction, which is exactly what
// `useAgentVitals` goes out of its way to keep distinguishable (`relay` is cleared to null, not set to `{configured:false}`).
describe("SettingsPage — the way in that does not exist", () => {
  beforeEach(() => {
    mockCallApi.mockResolvedValue({ ok: true });
  });

  it("drops the relay from the list when the device SAID its relay_url was empty", () => {
    render(<SettingsPage config={{ host: "127.0.0.1", port: 18080 }} relayConfigured={false} />);
    expect(screen.queryByText(/the relay/)).toBeNull();
    expect(screen.getByText(/a VPN or ssh -L/)).toBeTruthy();
    // AND IT NAMES THE THING OBSERVED, WHERE IT LIVES AND WHEN IT WAS READ: the config file's path is printed two rows
    // below on this same card, and `configured` is set ONCE at bind time — nothing watches the file afterwards, so an
    // operator who hand-edits config.yaml and sees no change has the reason on screen.
    expect(screen.getByText(/server\.relay_url was empty when the agent started/)).toBeTruthy();
  });

  it("says the same on a network bind, and does not borrow the loopback sentence", () => {
    render(<SettingsPage config={{ host: "0.0.0.0", port: 18080 }} relayConfigured={false} />);
    expect(screen.queryByText(/this machine only/i)).toBeNull();
    expect(screen.getByText(/keep the device token secret\. No relay configured/)).toBeTruthy();
  });

  it("keeps the relay in the list when one IS configured — the list is then true", () => {
    render(<SettingsPage config={{ host: "127.0.0.1", port: 18080 }} relayConfigured />);
    expect(screen.getByText(/the relay, a VPN, or ssh -L/)).toBeTruthy();
    expect(screen.queryByText(/No relay configured/)).toBeNull();
  });

  it("does NOT infer 'there is no relay' from an agent that said nothing about one", () => {
    // `undefined` is the older-agent case (and the no-poll-yet case). The sentence stays as it was, because the reply did
    // not contradict it — only a reply that SAID `configured: false` may drop the relay from the list.
    render(<SettingsPage config={{ host: "127.0.0.1", port: 18080 }} />);
    expect(screen.getByText(/the relay, a VPN, or ssh -L/)).toBeTruthy();
    expect(screen.queryByText(/No relay configured/)).toBeNull();
  });
});


// ── THE GATEWAY LINE SAID `connected`, WHICH NOTHING OBSERVED ────────────────────────────────────────────────────────────
// `connected` was a LITERAL whose only input was `console_url` being a non-empty string — measured on a live device answering
// `console_url=https://api.saisi.online`, where nothing on that path contacted the console at all. The reply's two facts are
// STEPS: `tunnel_configured` is "tunnel.yml exists", `tunnel_running` is a `tasklist` substring match. The line names each as
// the step it is and says outright that reachability was not checked.
describe("SettingsPage — the gateway line says what was observed", () => {
  /** Answer `GET /api/settings` with a console configured, and the tunnel state the test asks for. */
  function answerWith(extra: Record<string, unknown>) {
    mockCallApi.mockImplementation(async (path: string, opts?: any) => {
      if (path === "/api/settings" && (!opts || !opts.method || opts.method === "GET"))
        return { ...SETTINGS, console_url: "https://api.saisi.online", ...extra };
      return { ok: true };
    });
  }
  const statusLine = (container: HTMLElement) =>
    container.querySelector(".settings-status")?.textContent ?? null;
  /** THE CLAIM, SEPARATED FROM WHEN IT WAS MADE — so a test can pin the sentence without pinning a clock
   *  reading. The stamp is a real `Date.now()`; asserting its value would be asserting the test's own runtime. */
  const claim = (container: HTMLElement) =>
    (statusLine(container) ?? "").replace(/ · as of \d\d:\d\d:\d\d$/, "");
  const stamp = (container: HTMLElement) =>
    ((statusLine(container) ?? "").match(/ · as of (\d\d:\d\d:\d\d)$/) ?? [])[1] ?? null;

  it("names the console and the cloudflared process, and admits nothing was reachability-checked", async () => {
    answerWith({ tunnel_configured: true, tunnel_running: true });
    const { container } = render(<SettingsPage />);
    await waitFor(() =>
      expect(claim(container)).toBe(
        "console configured · cloudflared.exe running (reachability not checked)",
      ),
    );
    // THE WORD THAT WAS NEVER EARNED IS GONE FROM THE LINE — `connected` was produced by a non-empty string.
    expect(statusLine(container)).not.toMatch(/connected/);
    // **AND THE LINE CARRIES ITS OWN "WHEN" NOW.** Measured on the live page before this change: it was the
    // ONLY status sentence there with no time, while every sibling had one (`checked 2s ago`, `down 42m`,
    // `79 readings over 39m`, `UP 5m 41s`). The SHAPE is asserted, not the value — the value is a clock.
    expect(stamp(container)).toMatch(/^\d\d:\d\d:\d\d$/);
  });

  it("says the process is NOT running rather than 'tunnel: configured'", async () => {
    answerWith({ tunnel_configured: true, tunnel_running: false });
    const { container } = render(<SettingsPage />);
    await waitFor(() =>
      expect(claim(container)).toBe(
        "console configured · cloudflared.exe not running (reachability not checked)",
      ),
    );
  });

  it("stamps an observation and does NOT stamp a step that is still in flight", async () => {
    // "connecting…" reports something the panel is DOING, not something it OBSERVED — dating it would put a
    // clock on a sentence that makes no claim about the device, and the reader would take the stamp for the
    // age of a reading. The connect request is left pending so the step's own sentence is what is measured.
    mockCallApi.mockImplementation(async (path: string, opts?: any) => {
      if (path === "/api/gateway/connect") return new Promise(() => {});
      if (path === "/api/settings" && (!opts || !opts.method || opts.method === "GET")) return SETTINGS;
      return { ok: true };
    });
    const { container } = render(<SettingsPage />);
    await waitFor(() => expect(screen.getByLabelText("Memory max entries")).toBeTruthy());
    expect(statusLine(container)).toBeNull();

    fireEvent.change(screen.getByLabelText("Gateway URL"), { target: { value: "https://api.saisi.online" } });
    fireEvent.click(screen.getByText("Save & connect"));
    fireEvent.click(screen.getByText("Connect"));
    await waitFor(() => expect(statusLine(container)).toBe("connecting…"));
    expect(stamp(container)).toBeNull();
  });

  it("still shows nothing at all when the reply carries no console", async () => {
    // `null` means "this reply said nothing about a gateway", and an empty `console_url` correctly renders no status —
    // the one place this card already behaved honestly, and it is unchanged.
    mockCallApi.mockImplementation(async (path: string, opts?: any) => {
      if (path === "/api/settings" && (!opts || !opts.method || opts.method === "GET")) return SETTINGS;
      return { ok: true };
    });
    const { container } = render(<SettingsPage />);
    await waitFor(() => expect(screen.getByLabelText("Memory max entries")).toBeTruthy());
    expect(statusLine(container)).toBeNull();
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

  it("shows the device token masked as a FACT, with no second set of controls on the card", () => {
    const { container } = render(<SettingsPage />);
    expect(screen.getByText(/Device token:/)).toBeTruthy();
    expect(screen.getByText("••••••••••••")).toBeTruthy();
    // **THE ACTIONS MOVED INTO THE FOLD, WITH THE SNIPPET THEY PRODUCE.** Measured before this change: the same
    // credential was dressed twice on one screen, ~470px apart — `Reveal` / `Copy` here (31px tall, radius 10,
    // 13px/600) and `Reveal token` / `Copy config` in the fold (26px, radius 8, 12px/400). Asserted on the CARD's
    // own row, and on the two labels being gone rather than "no buttons anywhere" (the Connect card in the fold
    // still has its three, which is where the pair belongs).
    const deviceCard = container.querySelector(".desktop-settings .settings-section")!;
    expect([...deviceCard.querySelectorAll(".settings-row-bar button")]).toEqual([]);
    expect(screen.queryByText("Reveal")).toBeNull();
    expect(screen.queryByText("Copy")).toBeNull();
  });

  it("shows the config file the agent reports", () => {
    render(<SettingsPage config={{ host: "127.0.0.1", port: 18080, path: "C:\\ProgramData\\Summrise\\etc\\config.yaml" }} />);
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


// ── THE MEMORY PATH, WHICH WAS A PLACEHOLDER ON A PAGE THAT PRINTS THIS MACHINE'S REAL PATHS TWICE ───────────────────────
// `<install>/memory/memory.jsonl` sat beside `Config file: D:\Summrise\etc\config.yaml` and `Read from
// C:\ProgramData\Summrise\logs` — two resolved paths and one unresolved one. Measured on the device (1.2.492), those
// two are the INSTALL dir and the DATA dir, and they are different directories: `D:\Summrise\memory\memory.jsonl` does
// not exist (ENOENT) while `C:\ProgramData\Summrise\memory\memory.jsonl` is the store, 1437 bytes of real entries. So
// the placeholder did not merely lack a root; the root it implied held nothing. The device resolves it now.
describe("SettingsPage — the memory path is the device's, not a placeholder", () => {
  const REAL = "C:\\ProgramData\\Summrise\\memory\\memory.jsonl";

  it("prints the file the device resolved", async () => {
    mockCallApi.mockImplementation(async (path: string, opts?: any) => {
      if (path === "/api/settings" && (!opts || !opts.method || opts.method === "GET"))
        return { ...SETTINGS, memory_path: REAL };
      return { ok: true };
    });
    const { container } = render(<SettingsPage />);
    await waitFor(() => expect(container.textContent).toContain(REAL));
    expect(container.textContent).not.toContain("<install>");
  });

  it("names the gap rather than inventing a root when the agent reports none", async () => {
    // The relative tail is the one part the browser CAN know (`default_memory_dir()` is `data_dir()/memory`), so it
    // is printed — with the fact that the root is unreported. A guess here is what the placeholder was.
    mockCallApi.mockImplementation(async (path: string, opts?: any) => {
      if (path === "/api/settings" && (!opts || !opts.method || opts.method === "GET")) return SETTINGS;
      return { ok: true };
    });
    const { container } = render(<SettingsPage />);
    await waitFor(() => expect(container.textContent).toMatch(/memory\/memory\.jsonl/));
    expect(container.textContent).toContain(
      "under the device's data directory (this agent does not report which one)",
    );
    expect(container.textContent).not.toContain("<install>");
  });
});
