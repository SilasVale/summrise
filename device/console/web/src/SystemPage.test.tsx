import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SystemPage } from "./SystemPage";
import { callTool, ToolError, type Grant } from "./terminal";
import { clockTimeSeconds, exactMoment, isoDate } from "./format";

// The update card compares the running build with the staged release, so `/installer` is part of the
// page's surface. Each test names the answer it needs; the default is a console with no staging.
const installerApi = vi.hoisted(() => ({ installer: vi.fn() }));
//
// **THE PAGE ALSO MOUNTS THE FORWARD RULES, WHICH READS THE SHARED LIVE-TOOLS HOOK.** That hook asks
// `api.liveTools`, so a mock naming only `installer` is an api this page legitimately needs and does
// not have — measured: 24 of this file's tests died with `api.liveTools is not a function` the moment
// the section was mounted. The `useLiveTools` mock below answers for the whole page, so `api.liveTools`
// is here only to keep the module's shape honest.
const liveApi = vi.hoisted(() => ({ liveTools: vi.fn() }));
vi.mock("./api", () => ({ api: { installer: installerApi.installer, liveTools: liveApi.liveTools } }));

// The page's whole device surface is `callTool`; the real `ToolError` stays so a refusal is the class
// the page actually receives from `./terminal`.
// **THE MACHINE THIS PAGE DRAWS DOES NOT ADVERTISE THE FORWARD TOOLS**, which is the older-build
// case: the section then draws one sentence and makes no call, so this file's `device()` helper is
// never asked for a `forward.*` answer it does not have.
vi.mock("./useLiveTools", () => ({
  useLiveTools: () => ({
    tools: [] as { name: string }[],
    stale: false,
    error: null,
    offline: false,
    loading: false,
    retry: () => {}
  })
}));

vi.mock("./terminal", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./terminal")>();
  return { ...actual, callTool: vi.fn() };
});

const mocked = vi.mocked(callTool);

/**
 * Open a process row's own action menu. The menu is the row's `⋯`, not a button per line: jsdom
 * gives a `<summary>` no button role, so it is found by the label it carries, which is also the
 * accessible name a reader hears.
 */
function openRowMenu(pid: number, name: string): void {
  const summary = screen.getByLabelText(`Actions for PID ${pid} (${name})`);
  const details = summary.closest("details") as HTMLDetailsElement;
  details.open = true;
  fireEvent.click(summary);
}

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

/** The update card's labelled rows, in reading order, read from the rendered DOM. */
function gridRows(container: HTMLElement) {
  const grid = container.querySelector(".system-update dl");
  if (!(grid instanceof HTMLElement)) {
    throw new Error("the update card's grid is not on the page");
  }
  return Array.from(grid.querySelectorAll(":scope > div")).map((row) => ({
    label: row.querySelector("dt")?.textContent?.trim() ?? "",
    value: row.querySelector("dd")?.textContent?.trim() ?? "",
    title: row.querySelector("time")?.getAttribute("title") ?? null
  }));
}

describe("SystemPage", () => {
  beforeEach(() => {
    mocked.mockReset();
    installerApi.installer.mockReset();
    installerApi.installer.mockResolvedValue({ available: false });
  });

  it("does not ask a machine the fleet says is down, and says why", async () => {
    render(<SystemPage grant={GRANT} deviceOnline={false} />);
    await waitFor(() => expect(mocked).not.toHaveBeenCalled());
    expect(
      screen.getByText("The machine is offline; its processes cannot be read until it reconnects.")
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Refresh" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Restart app" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Test" })).toBeDisabled();
  });

  it("dates the console's missing machine facts instead of claiming the device never sent any", async () => {
    // The measured offline Overview pointed at Machine "for the machine's last facts"; the card
    // said the console had none "yet" — a claim about the device that was true only of a console
    // process restarted at ~20:40Z. The absence is dated by when this console started, so a
    // reader knows the facts may exist and this process has not seen them.
    device((name) => {
      if (name !== "system.process_list") {
        throw new Error(`the test did not expect a ${name} call`);
      }
      return JSON.stringify([]);
    });
    const consoleStartedAt = new Date(2026, 10, 7, 20, 40, 0).getTime();
    render(
      <SystemPage
        grant={GRANT}
        consoleStartedAt={consoleStartedAt}
        device={{ id: "device-1", name: "workshop", online: true, version: "0.3.1", last_frame_ms: 1 }}
      />
    );

    expect(
      await screen.findByText(
        /The console has no machine facts from this device since this console started at 2026-11-07 20:40/
      )
    ).toBeInTheDocument();
    expect(screen.getByText(/repeats every few seconds/)).toBeInTheDocument();
    expect(screen.queryByText(/from this device yet/)).toBeNull();
  });

  it("names the console-process boundary even when its start was not reported", async () => {
    device((name) => {
      if (name !== "system.process_list") {
        throw new Error(`the test did not expect a ${name} call`);
      }
      return JSON.stringify([]);
    });
    render(
      <SystemPage
        grant={GRANT}
        device={{ id: "device-1", name: "workshop", online: true, version: "0.3.1", last_frame_ms: 1 }}
      />
    );

    expect(
      await screen.findByText(/The console has no machine facts from this device since this console started\./)
    ).toBeInTheDocument();
    expect(screen.queryByText(/from this device yet/)).toBeNull();
  });

  it("dates an offline machine's missing facts by the machine's own last seen, not the console's restart", async () => {
    // **THE TWO DATES MUST NOT CONTRADICT EACH OTHER (audit-44 F6).** The measured 14R page said
    // `no machine facts … since this console started at 09:52` beside the shell notice's
    // `Last seen 2d 18h ago`; the 09:52 was this console process's start, so the absence read as
    // the machine's last fact. Both facts now stand, each labelled, and the machine's own last-seen
    // is the date the card answers with.
    device((name) => {
      if (name !== "system.process_list") {
        throw new Error(`the test did not expect a ${name} call`);
      }
      return JSON.stringify([]);
    });
    const consoleStartedAt = new Date(2026, 10, 7, 20, 40, 0).getTime();
    const seenAt = Date.now() - (2 * 24 + 18) * 3_600_000;
    render(
      <SystemPage
        grant={GRANT}
        deviceOnline={false}
        consoleStartedAt={consoleStartedAt}
        device={{
          id: "device-14r",
          name: "DESKTOP-14RJCR8",
          online: false,
          version: "0.3.1",
          last_frame_ms: null,
          last_seen_ms: seenAt
        }}
      />
    );

    expect(
      await screen.findByText(
        /The console restarted at 2026-11-07 20:40 and has not heard from this machine since; it was last seen 2d 18h ago\./
      )
    ).toBeInTheDocument();
    // The console's own boundary is named, so the machine's date is not mistaken for it.
    expect(screen.queryByText(/since this console started/)).toBeNull();
    expect(screen.getByText(/They travel with the fleet read/)).toBeInTheDocument();
  });

  it("dates an offline machine's facts with the age of its last frame", async () => {
    // audit-38 F7: the 9VH had been offline five hours, and the Machine tab still read `#3 connected`
    // and `Helper not registered this boot` as present-tense facts. The frame's own moment is what
    // dates them; "now" would repeat the lie the banner already corrects.
    device((name) => {
      if (name !== "system.process_list") {
        throw new Error(`the test did not expect a ${name} call`);
      }
      return JSON.stringify([]);
    });
    const frameAt = Date.now() - 5 * 3_600_000;
    render(
      <SystemPage
        grant={GRANT}
        deviceOnline={false}
        device={{
          id: "device-1",
          name: "workshop",
          online: false,
          version: "0.3.1",
          last_frame_ms: frameAt,
          host: {
            memory: { total_bytes: 8 * 1024 ** 3, available_bytes: 4 * 1024 ** 3 },
            sessions: [{ id: 3, state: "connected" }],
            helper: { registered: false }
          }
        }}
      />
    );

    const age = screen.getByText(/^Last frame 5h ago/);
    expect(age).toHaveAttribute("title", exactMoment(frameAt));
    // The facts themselves are still the frame's, not re-read: the age says which moment they are.
    expect(screen.getByText("4.0 GiB free of 8.0 GiB")).toBeInTheDocument();
    expect(screen.getByText("#3 connected")).toBeInTheDocument();
    // The host frame carries no repair task, but this is an offline block: the dated-absence shape
    // draws no row, so the running version is not handed to a frame it may not have written.
    expect(screen.queryByText("Repair task")).toBeNull();
  });

  it("dates an offline machine the console never saw a frame from by its last seen moment", async () => {
    // audit-38 F7's live follow-up: the measured 14R's fleet row has `last_frame_ms: null` and
    // `last_seen_ms` set, because the console restarted mid-outage. Passing only the frame left the
    // block undated; the registry's last-seen is the moment left, and the line names that evidence.
    device((name) => {
      if (name !== "system.process_list") {
        throw new Error(`the test did not expect a ${name} call`);
      }
      return JSON.stringify([]);
    });
    const seenAt = Date.now() - 5 * 3_600_000;
    render(
      <SystemPage
        grant={GRANT}
        deviceOnline={false}
        device={{
          id: "device-14r",
          name: "DESKTOP-14RJCR8",
          online: false,
          version: "0.3.1",
          last_frame_ms: null,
          last_seen_ms: seenAt,
          host: {
            memory: { total_bytes: 8 * 1024 ** 3, available_bytes: 4 * 1024 ** 3 },
            sessions: [{ id: 3, state: "connected" }],
            helper: { registered: false }
          }
        }}
      />
    );

    const age = screen.getByText(/^Last seen 5h ago/);
    expect(age).toHaveAttribute("title", exactMoment(seenAt));
    // It is a last-seen date, not a frame date: the sentence says so instead of handing the
    // values a moment the console cannot prove they came from.
    expect(age).toHaveTextContent("the console has no timestamp for that frame");
    expect(screen.queryByText(/^Last frame/)).toBeNull();
    // The values are unchanged: only their date is stated, from the evidence that exists.
    expect(screen.getByText("4.0 GiB free of 8.0 GiB")).toBeInTheDocument();
    expect(screen.getByText("#3 connected")).toBeInTheDocument();
  });

  it("leaves the running version to the header chip, once per surface", async () => {
    // The measured Machine tab carried `build 0.3.277 · online` under a header that already said
    // `Running · v0.3.277`: a third copy on a page that needs one. The status line is labelled now,
    // and the update card is the only place the update story is written.
    device((name) => {
      if (name !== "system.process_list") {
        throw new Error(`the test did not expect a ${name} call`);
      }
      return JSON.stringify([]);
    });
    render(
      <SystemPage
        grant={GRANT}
        device={{ id: "device-1", name: "workshop", online: true, version: "0.3.1", last_frame_ms: 1 }}
      />
    );

    expect(await screen.findByText("online")).toBeInTheDocument();
    // The word now has a subject: `Status · online`, not a bare value under the facts card.
    expect(screen.getByText("Status").parentElement).toHaveTextContent("Status · online");
    // **THE RULE IS ABOUT A BUILD *LABEL*, NOT THE WORD.** This read `/build/` and was written when
    // no section on this tab could say "build"; mounting the forward rules brought the product's
    // established `Older build` state (the same phrase `PluginList.tsx` draws) onto this page, and a
    // page-wide word match then fails on a sentence that is not the third copy the rule forbids. The
    // measured thing was `build 0.3.277 · online`, so that is what the assertion names.
    expect(screen.queryByText(/build 0\.3/)).toBeNull();
    expect(screen.queryByText(/0\.3\.1/)).toBeNull();
  });

  it("says an online machine's build does not report the repair task, and names it", async () => {
    // audit-42 F7: the measured 84T on 0.3.291 is online and its `/fleet` host object has no
    // `repair_task` key, so the Machine tab showed no row at all — which read as "no task". The
    // page hands the block the fleet's own reachability and running version, so the row can say
    // which build's frame this silence belongs to.
    device((name) => {
      if (name !== "system.process_list") {
        throw new Error(`the test did not expect a ${name} call`);
      }
      return JSON.stringify([]);
    });
    render(
      <SystemPage
        grant={GRANT}
        deviceOnline
        device={{
          id: "device-84t",
          name: "DESKTOP-84T3LIN",
          online: true,
          version: "0.3.291",
          last_frame_ms: Date.now(),
          host: {
            memory: { total_bytes: 16 * 1024 ** 3, available_bytes: 5 * 1024 ** 3 },
            sessions: [{ id: 5, state: "connected" }],
            helper: { registered: false }
          }
        }}
      />
    );

    expect(await screen.findByText("Repair task")).toBeInTheDocument();
    expect(screen.getByText("not reported by this build (v0.3.291)")).toBeInTheDocument();
    expect(screen.queryByText("not registered")).toBeNull();
  });

  it("compares the machine against the fleet Home measures it by, not against the installer", async () => {
    // **ONE MACHINE HAS ONE CURRENCY (the thirty-fourth list).** Measured on `0.3.273-ux130`: Home
    // listed DESKTOP-84T3LIN as `On v0.3.291 · behind 0.3.292.` while that machine's own Machine tab
    // read `0.3.291 · current (the release)` — in one visit. Home measured against the fleet's newest
    // build and the tab against the staged installer (0.3.291), and those two numbers are on opposite
    // sides of 84T. The card now takes the fleet and computes the same target Home does.
    const installed = {
      id: "device-1",
      name: "workshop",
      online: true,
      version: "0.3.291",
      last_frame_ms: 1,
      update: {
        current: "0.3.291",
        last_check: null,
        last_update: { to: "0.3.291", outcome: "applied", at_ms: Date.now() - 60_000 }
      },
      host: {
        update: {
          current: { version: "0.3.291", channel: "stable" },
          previous: null,
          pending: null,
          last_swap: null,
          repairs: []
        },
        restarts: [
          { at_ms: Date.now() - 90_000, version: "0.3.291", channel: "stable", pid: 4200 }
        ],
        helper: { registered: false }
      }
    };
    installerApi.installer.mockResolvedValue({
      available: true,
      filename: "SummriseSetup-0.3.291.exe",
      version: "0.3.291",
      size_bytes: 1,
      sha256: "a".repeat(64)
    });
    const { container } = render(
      <SystemPage
        grant={GRANT}
        device={installed}
        fleet={[{ version: "0.3.292" }, { version: "0.3.275" }, { version: "0.3.291" }]}
      />
    );

    await screen.findByText("online");
    const rows = gridRows(container);
    // The applied version leads when it is the one running (the card's own order), so the row that
    // carries the comparison is whichever of the two states this build.
    const versionRow = rows.find((row) => row.label === "Applied" || row.label === "Running");
    // The machine is behind the FLEET's newest build — the same fact Home states about it.
    expect(versionRow?.value).toBe("0.3.291 · behind the fleet's 0.3.292");
    // The version row carries the fleet's comparison — the same fact Home states about this machine.
    // (The installer row is drawn only when the fleet has outrun a NEWER file; here the installer and
    // 84T are on the same build, so the fleet is behind the file, not the other way round.)
  });

  it("states the installer's own standing when the fleet has outrun the file", async () => {
    // The measured fleet ran 0.3.292 while the staged installer was 0.3.291. The installer being
    // older is a property of the DOWNLOAD, not of any machine, so it gets its own row: a reader
    // comparing a machine's build with "the release" is comparing against the wrong number
    // (the thirty-fourth list).
    const installed = {
      id: "device-1",
      name: "workshop",
      online: true,
      version: "0.3.292",
      last_frame_ms: 1,
      update: {
        current: "0.3.292",
        last_check: null,
        last_update: { to: "0.3.292", outcome: "applied", at_ms: Date.now() - 60_000 }
      },
      host: {
        update: {
          current: { version: "0.3.292", channel: "stable" },
          previous: null,
          pending: null,
          last_swap: null,
          repairs: []
        },
        restarts: [
          { at_ms: Date.now() - 90_000, version: "0.3.292", channel: "stable", pid: 4200 }
        ],
        helper: { registered: false }
      }
    };
    installerApi.installer.mockResolvedValue({
      available: true,
      filename: "SummriseSetup-0.3.291.exe",
      version: "0.3.291",
      size_bytes: 1,
      sha256: "a".repeat(64)
    });
    const { container } = render(
      <SystemPage
        grant={GRANT}
        device={installed}
        fleet={[{ version: "0.3.292" }, { version: "0.3.275" }]}
      />
    );

    await screen.findByText("online");
    const installerRow = gridRows(container).find((row) => row.label === "Installer");
    expect(installerRow?.value).toBe(
      "The staged installer is 0.3.291, behind the fleet's 0.3.292."
    );
  });

  it("states the device's own update facts, including the last check", async () => {
    // The measured Machine tab left the operator with `Running · v0.3.284` and a sentence whose
    // last clause was the answer. The card states the facts as rows: the running version and the
    // last target are different versions here, so each one has its own row.
    device(() => JSON.stringify([]));
    const checkAt = new Date(2026, 10, 6, 17, 26, 2).getTime();
    const resultAt = new Date(2026, 10, 4, 19, 11, 5).getTime();
    const { container } = render(
      <SystemPage
        grant={GRANT}
        device={{
          id: "device-1",
          name: "workshop",
          online: true,
          version: "0.3.284",
          last_frame_ms: 1,
          update: {
            current: "0.3.284",
            last_check: { at_ms: checkAt, trigger: "channel" },
            last_update: { to: "0.3.273", outcome: "applied", at_ms: resultAt }
          },
          host: {
            update: {
              current: { version: "0.3.284", channel: "canary" },
              previous: null,
              pending: null,
              last_swap: null,
              repairs: []
            }
          }
        }}
      />
    );

    await screen.findByText("online");
    const rows = gridRows(container);
    expect(rows.map((row) => row.label)).toEqual([
      "Running",
      "Last target",
      "Last outcome",
      "Last result",
      "Last check",
      "Channel"
    ]);
    expect(rows[0].value).toBe("0.3.284 · release unknown");
    expect(rows[1].value).toBe("0.3.273");
    expect(rows[2].value).toBe("applied");
    expect(rows[3].value).toBe(`${isoDate(resultAt)} ${clockTimeSeconds(resultAt)}`);
    expect(rows[4].value).toBe(`${isoDate(checkAt)} ${clockTimeSeconds(checkAt)}`);
    expect(rows[4].title).toBe(exactMoment(checkAt));
    expect(rows[5].value).toBe("canary");
  });

  it("draws the applied version first when it is the one running, and states it once", async () => {
    // THE IN-CHANNEL SHAPE: the update applied and this process runs it. The answer is the card's
    // first row, and the version is written on exactly one row — the measured paragraph wrote it
    // three times on the first screen and twice in one line.
    device(() => JSON.stringify([]));
    const checkAt = new Date(2026, 10, 6, 17, 26, 2).getTime();
    const appliedAt = new Date(2026, 10, 6, 16, 0, 0).getTime();
    const { container } = render(
      <SystemPage
        grant={GRANT}
        device={{
          id: "device-1",
          name: "workshop",
          online: true,
          version: "0.3.290",
          last_frame_ms: 1,
          update: {
            current: "0.3.290",
            last_check: { at_ms: checkAt, trigger: "schedule" },
            last_update: { to: "0.3.290", outcome: "applied", at_ms: appliedAt }
          },
          host: {
            restarts: [{ at_ms: appliedAt, version: "0.3.290", channel: "canary" }]
          }
        }}
      />
    );

    await screen.findByText("online");
    const rows = gridRows(container);
    expect(rows.map((row) => row.label)).toEqual([
      "Applied",
      "Last result",
      "Last check",
      "Channel"
    ]);
    expect(rows[0]).toMatchObject({ label: "Applied", value: "0.3.290 · release unknown" });
    // ONE VERSION, ONE ROW: no `Running` or `Last target` row repeats what the applied row states.
    expect(rows.filter((row) => row.value.includes("0.3.290"))).toHaveLength(1);
  });

  it("puts the running version and the last target on their own rows when the machine is behind", async () => {
    // THE BEHIND SHAPE: the updater applied 0.3.290 but the process still runs 0.3.280. Each version
    // is a row, so the gap is read instead of buried in a clause; the chip above states the same
    // running version, and the card does not pretend the target is current.
    device(() => JSON.stringify([]));
    const checkAt = new Date(2026, 10, 7, 2, 25, 43).getTime();
    const appliedAt = new Date(2026, 10, 8, 0, 22, 52).getTime();
    const { container } = render(
      <SystemPage
        grant={GRANT}
        device={{
          id: "device-1",
          name: "workshop",
          online: true,
          version: "0.3.280",
          last_frame_ms: 1,
          update: {
            current: "0.3.280",
            last_check: { at_ms: checkAt, trigger: "channel" },
            last_update: { to: "0.3.290", outcome: "applied", at_ms: appliedAt }
          },
          host: {
            restarts: [{ at_ms: appliedAt, version: "0.3.280", channel: "canary" }]
          }
        }}
      />
    );

    await screen.findByText("online");
    const rows = gridRows(container);
    expect(rows.map((row) => row.label)).toEqual([
      "Running",
      "Last target",
      "Last outcome",
      "Last result",
      "Last check",
      "Channel"
    ]);
    expect(rows[0].value).toBe("0.3.280 · release unknown");
    expect(rows[1].value).toBe("0.3.290");
    expect(rows[2].value).toBe("applied");
    // The applied version is not the running one, so it is not the first row.
    expect(rows[0].label).not.toBe("Applied");
  });

  it("keeps the machine's start line in the Device app section", async () => {
    // **THE START IS THE DEVICE APP'S FACT, NOT THE SHELL'S FIRST LINE (audit-44 F1).** Every tab
    // used to open with `started … ago`; the Machine tab is where the start is the subject, and it
    // now sits in the section that also carries the update card, rather than above the page.
    device(() => JSON.stringify([]));
    const startedAt = Date.now() - 25 * 3_600_000;
    const { container } = render(
      <SystemPage
        grant={GRANT}
        device={{
          id: "device-1",
          name: "workshop",
          online: true,
          version: "0.3.42",
          last_frame_ms: 1,
          update: {
            current: "0.3.42",
            verdict: { outcome: "ok", detail: "started 0.3.42" },
            last_update: { to: "0.3.41", outcome: "applied", at_ms: Date.now() - 26 * 3_600_000 }
          },
          host: { restarts: [{ at_ms: startedAt, version: "0.3.42" }] }
        }}
      />
    );

    await screen.findByText("online");
    const section = container.querySelector('section[aria-labelledby="system-device-app"]');
    if (!(section instanceof HTMLElement)) throw new Error("the Device app section is not on the page");
    // **THE TOPBAR STATES THE BUILD; THE START LINE STATES WHEN.** The update card carries the
    // version and the release comparison; this line keeps only the moment a chip cannot state.
    const line = within(section).getByText("started 1d 1h ago");
    expect(line).toHaveAttribute("title", exactMoment(startedAt));
    expect(screen.queryByText(/updated/)).not.toBeInTheDocument();
  });

  it("puts the recovery line beside the start, from the device's own boot journal", async () => {
    // **THE MEASURED CPH SHAPE (audit-44 F12).** The console never wrote the pair for that 15 h
    // silence, so the device's own journal is the record: the host's uptime names the boot it
    // returned on, and the boot before it closes the gap. The line says whose record it is.
    device(() => JSON.stringify([]));
    const backBoot = new Date(2026, 10, 8, 8, 51, 43).getTime();
    const previousBoot = backBoot - (14 * 3_600_000 + 57 * 60_000 + 59_000);
    const { container } = render(
      <SystemPage
        grant={GRANT}
        consoleStartedAt={backBoot + 3_600_000}
        device={{
          id: "device-1",
          name: "workshop",
          online: true,
          version: "0.3.292",
          last_frame_ms: backBoot + 1_000,
          update: {
            current: "0.3.292",
            verdict: { outcome: "ok", detail: "started 0.3.292" }
          },
          host: {
            // The frame's host booted four seconds before the app; the newer entry is the update's
            // restart 44 minutes later, not the boot.
            uptime_s: 5,
            restarts: [
              { at_ms: backBoot + 44 * 60_000, version: "0.3.292" },
              { at_ms: backBoot, version: "0.3.289" },
              { at_ms: previousBoot, version: "0.3.289" }
            ]
          }
        }}
      />
    );

    await screen.findByText("online");
    const section = container.querySelector('section[aria-labelledby="system-device-app"]');
    if (!(section instanceof HTMLElement)) throw new Error("the Device app section is not on the page");
    const line = within(section).getByText(
      /^back after 14 h 57 m \(the device's journal recorded its boot at /
    );
    expect(line).toHaveAttribute("data-recovery-source", "journal");
    expect(line).toHaveAttribute("title", exactMoment(backBoot));
  });

  it("labels an applied outcome older than this start as history", async () => {
    // The measured 9VH Machine tab: the process started 0.3.286 at 06:16, the only update fact was
    // an apply to 0.3.273 two days earlier. The old apply is still shown — it is the device's own
    // record — but it is labelled as before this start rather than as the machine's last outcome.
    device(() => JSON.stringify([]));
    const startedAt = new Date(2026, 10, 7, 6, 16, 8).getTime();
    const appliedAt = new Date(2026, 10, 5, 3, 11, 0).getTime();
    const checkAt = new Date(2026, 10, 7, 6, 30, 32).getTime();
    const { container } = render(
      <SystemPage
        grant={GRANT}
        device={{
          id: "device-1",
          name: "workshop",
          online: true,
          version: "0.3.286",
          last_frame_ms: 1,
          update: {
            current: "0.3.286",
            verdict: { outcome: "ok", detail: "started 0.3.286" },
            last_update: { to: "0.3.273", outcome: "applied", at_ms: appliedAt },
            last_check: { at_ms: checkAt, trigger: "channel" }
          },
          host: { restarts: [{ at_ms: startedAt, version: "0.3.286" }] }
        }}
      />
    );

    await screen.findByText("online");
    const rows = gridRows(container);
    expect(rows[0]).toMatchObject({ label: "Running", value: "0.3.286 · release unknown" });
    expect(rows[1]).toMatchObject({ label: "Last target", value: "0.3.273" });
    // The old apply is not printed as the last outcome this process produced.
    expect(rows[2]).toMatchObject({
      label: "Last outcome",
      value: "applied · before this start"
    });
    expect(rows[3].value).toBe(`${isoDate(appliedAt)} ${clockTimeSeconds(appliedAt)}`);
  });

  it("keeps an applied outcome current when it followed this start", async () => {
    device(() => JSON.stringify([]));
    const startedAt = new Date(2026, 10, 5, 2, 0, 0).getTime();
    const appliedAt = new Date(2026, 10, 5, 3, 11, 0).getTime();
    const { container } = render(
      <SystemPage
        grant={GRANT}
        device={{
          id: "device-1",
          name: "workshop",
          online: true,
          version: "0.3.286",
          last_frame_ms: 1,
          update: {
            current: "0.3.286",
            verdict: { outcome: "ok", detail: "started 0.3.286" },
            last_update: { to: "0.3.273", outcome: "applied", at_ms: appliedAt }
          },
          host: { restarts: [{ at_ms: startedAt, version: "0.3.286" }] }
        }}
      />
    );

    await screen.findByText("online");
    const rows = gridRows(container);
    expect(rows.find((row) => row.label === "Last outcome")?.value).toBe("applied");
    expect(rows.some((row) => row.value.includes("before this start"))).toBe(false);
  });

  it("says which update facts the device did not report instead of guessing them", async () => {
    // THE NO-RECORD SHAPE: the device sent its running version and nothing else about updates. The
    // card states that absence in the panel's rows; the console-link line and the check line keep
    // saying what is absent rather than inventing a join, a retry or a console check.
    device(() => JSON.stringify([]));
    const { container } = render(
      <SystemPage
        grant={GRANT}
        device={{
          id: "device-1",
          name: "workshop",
          online: true,
          version: "0.3.284",
          last_frame_ms: 1,
          update: { current: "0.3.284" }
        }}
      />
    );

    await screen.findByText("online");
    const rows = gridRows(container);
    expect(rows.map((row) => row.label)).toEqual([
      "Running",
      "Last target",
      "Last outcome",
      "Last result",
      "Last check",
      "Channel"
    ]);
    expect(rows[0].value).toBe("0.3.284 · release unknown");
    for (const row of rows.slice(1, 5)) expect(row.value).toBe("not recorded");
    expect(rows[5].value).toBe("not reported");
    expect(container.querySelector(".system-channel")).toHaveTextContent(
      "no record of this machine's channel attempts"
    );
    expect(container.querySelector(".system-check")).toHaveTextContent(
      'a check runs on the machine\'s own panel ("Check for updates now")'
    );
  });

  it("names the restart's channel, the device's verdict and its console retries", async () => {
    // The measured 84T: `host.update.current` was null, so the line said "channel not reported"
    // while the restart journal said canary and the verdict said `started 0.3.280`. The device's own
    // console attempt (14 failures) reached the console as `last_attempt`; the Machine tab is where
    // the sentence that explains it belongs. The verdict is the card's result when no swap answer
    // was folded in.
    device(() => JSON.stringify([]));
    const startedAt = Date.now() - 7 * 3_600_000;
    const { container } = render(
      <SystemPage
        grant={GRANT}
        device={{
          id: "device-1",
          name: "workshop",
          online: true,
          version: "0.3.280",
          last_frame_ms: 1,
          last_attempt: {
            attempts: 14,
            reason: "the console accepted the channel and then dropped it"
          },
          update: {
            current: "0.3.280",
            verdict: { outcome: "ok", detail: "started 0.3.280" }
          },
          host: {
            update: { current: null, previous: null, pending: null, last_swap: null },
            restarts: [{ at_ms: startedAt, version: "0.3.280", channel: "canary" }]
          }
        }}
      />
    );

    await screen.findByText("online");
    const rows = gridRows(container);
    expect(rows[0]).toMatchObject({ label: "Running", value: "0.3.280 · release unknown" });
    expect(rows.find((row) => row.label === "Channel")?.value).toBe("canary");
    expect(rows.find((row) => row.label === "Verdict")?.value).toMatch(
      /^ok · started 7h ago$/
    );
    expect(rows.some((row) => row.value === "not reported")).toBe(false);

    const channel = container.querySelector(".system-channel");
    expect(channel).toHaveTextContent(
      "The device's last console attempt: retried after 14 failed attempts — " +
        "the console accepted the channel and then dropped it."
    );
  });

  it("falls back to the audit's joins and leaves when no retry note reached the console", async () => {
    // A console that restarted forgets `last_attempt`; the audit file does not. The fallback is the
    // joined/left tally over the same newest window the fleet's absences are read from, stated with
    // the lines it actually read and the span those lines cover, so a count can be read as a rate
    // and cannot read as all time. The measured window was 1,000 lines over 17 h (task 76).
    device(() => JSON.stringify([]));
    const { container } = render(
      <SystemPage
        grant={GRANT}
        device={{
          id: "device-1",
          name: "workshop",
          online: true,
          version: "0.3.280",
          last_frame_ms: 1,
          channel_activity: { joined: 24, left: 2, window: 1000, span_ms: 17 * 3_600_000 },
          update: {
            current: "0.3.280",
            verdict: { outcome: "ok", detail: "started 0.3.280" }
          },
          host: {
            restarts: [{ at_ms: Date.now() - 7 * 3_600_000, version: "0.3.280", channel: "canary" }]
          }
        }}
      />
    );

    await screen.findByText("online");
    const channel = container.querySelector(".system-channel");
    expect(channel).toHaveTextContent(
      "The console's audit recorded 24 joins and 2 leaves for this machine in its newest 1,000 " +
        "lines, spanning 17 h."
    );
    expect(channel).toHaveTextContent(
      "A join is the console accepting this machine's channel; a left is the channel closing"
    );
  });

  it("states the channel window's lines without a duration when its ends carry no moment", async () => {
    // The ruling's second half: a window whose first or last line has no `ts_ms` has no span to
    // state, so the sentence keeps the lines it read and invents no duration.
    device(() => JSON.stringify([]));
    const { container } = render(
      <SystemPage
        grant={GRANT}
        device={{
          id: "device-1",
          name: "workshop",
          online: true,
          version: "0.3.280",
          last_frame_ms: 1,
          channel_activity: { joined: 2, left: 1, window: 1000 },
          update: {
            current: "0.3.280",
            verdict: { outcome: "ok", detail: "started 0.3.280" }
          },
          host: {
            restarts: [{ at_ms: Date.now() - 7 * 3_600_000, version: "0.3.280", channel: "canary" }]
          }
        }}
      />
    );

    await screen.findByText("online");
    const channel = container.querySelector(".system-channel");
    expect(channel).toHaveTextContent(
      "The console's audit recorded 2 joins and 1 leave for this machine in its newest 1,000 lines."
    );
    expect(channel).not.toHaveTextContent("spanning");
  });

  it.each([
    // **THE SENTENCE NAMES ITS OWN SUBJECT (the thirty-third and thirty-fourth lists).** With no
    // fleet passed, the card's own answer is the installer, so these three read "the release".
    ["0.3.292", "0.3.291", "0.3.292 · ahead of the release 0.3.291"],
    ["0.3.291", "0.3.291", "0.3.291 · current (the release)"],
    ["0.3.289", "0.3.291", "0.3.289 · behind the release 0.3.291"]
  ])(
    "answers the running %s against the staged release %s",
    async (running, release, expected) => {
      // **THE PAGE'S SECOND QUESTION IS "IS IT CURRENT?".** The measured 84T on the release and the
      // 9VH one release ahead looked identical except for the digits; the card's version row now
      // carries the comparison with the installer Home already offers.
      installerApi.installer.mockResolvedValue({
        available: true,
        filename: `SummriseSetup-${release}.exe`,
        version: release,
        size_bytes: 1,
        sha256: "a".repeat(64)
      });
      device(() => JSON.stringify([]));
      const { container } = render(
        <SystemPage
          grant={GRANT}
          device={{
            id: "device-1",
            name: "workshop",
            online: true,
            version: running,
            last_frame_ms: 1,
            update: {
              current: running,
              verdict: { outcome: "ok", detail: `started ${running}` },
              last_update: { to: running, outcome: "applied", at_ms: Date.now() - 60_000 },
              last_check: { at_ms: Date.now() - 20 * 60_000 }
            },
            host: { restarts: [{ at_ms: Date.now() - 60_000, version: running }] }
          }}
        />
      );

      await screen.findByText("online");
      await waitFor(() => expect(gridRows(container)[0]?.value).toBe(expected));
    }
  );

  it("says the console has no update facts when the device never sent a story", async () => {
    device(() => JSON.stringify([]));
    const { container } = render(
      <SystemPage
        grant={GRANT}
        device={{
          id: "device-1",
          name: "workshop",
          online: true,
          version: "0.3.284",
          last_frame_ms: 1
        }}
      />
    );

    await screen.findByText("online");
    expect(container.querySelector(".system-update")).toHaveTextContent(
      "The console has no update facts from this device: it has not sent its update story."
    );
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
    expect(await screen.findByText("2 processes on this machine, by pid.")).toBeInTheDocument();
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
    expect(await screen.findByText("2 processes on this machine, by pid.")).toBeInTheDocument();
    expect(within(screen.getByRole("table")).getByText("sshd")).toBeInTheDocument();
  });

  it("does not offer Kill for an operating-system process", async () => {
    const system = { pid: 4, name: "System", memory_kb: 0 };
    device((name) => {
      if (name === "system.process_list") return JSON.stringify([system, NGINX]);
      throw new Error(`the test did not expect a ${name} call`);
    });
    render(<SystemPage grant={GRANT} />);
    await screen.findByText("2 processes on this machine, by pid.");

    const row = screen.getByText("System").closest("tr") as HTMLElement;
    expect(within(row).getByText("Protected")).toBeInTheDocument();
    expect(within(row).queryByRole("button", { name: /Kill/ })).not.toBeInTheDocument();
    // The ordinary process next to it still has its action — behind its own row menu, because a
    // table of 235 rows must not draw 222 buttons (the twenty-eighth list, item 3).
    expect(screen.getByLabelText("Actions for PID 4242 (nginx)")).toBeInTheDocument();
  });

  it("draws one menu per live process, not a Kill button on every row", async () => {
    // **MEASURED ON THE OPERATOR'S MACHINE PAGE (the twenty-eighth list, item 3):** 235 process
    // rows, **222 `Kill` buttons** at 33 x 28 px — the page's most repeated control was the
    // destructive one, and it was below the 44 px touch target the phone needs. A row is a fact;
    // its one action lives behind its own `⋯`, the same menu Home's fleet rows use.
    const protectedOne: Process = { pid: 4, name: "System", memory_kb: 0 };
    const many: Process[] = Array.from({ length: 40 }, (_, index) => ({
      pid: 1000 + index,
      name: `worker-${index}.exe`,
      memory_kb: 4096
    }));
    device((name) =>
      name === "system.process_list" ? JSON.stringify([protectedOne, ...many]) : JSON.stringify({})
    );
    render(<SystemPage grant={GRANT} />);
    await screen.findByText("41 processes on this machine, by pid.");

    // jsdom keeps a closed `<details>`'s children in the DOM, so the claim is not "the button
    // does not exist" but "the button is not one a reader can reach": every `Kill PID n` sits
    // inside a menu that is closed, and the only control the row offers is its `⋯`.
    const killButtons = screen.queryAllByRole("button", { name: /^Kill PID / });
    expect(killButtons).toHaveLength(40);
    for (const button of killButtons) {
      const menu = button.closest("details") as HTMLDetailsElement;
      expect(menu.open).toBe(false);
    }
    expect(screen.getAllByLabelText(/^Actions for PID /)).toHaveLength(40);
    // The protected row still says `Protected` and offers nothing at all.
    expect(screen.getByText("Protected")).toBeInTheDocument();
    expect(screen.queryByLabelText("Actions for PID 4 (System)")).toBeNull();
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
    await screen.findByText("2 processes on this machine, by pid.");

    openRowMenu(4242, "nginx");
    fireEvent.click(screen.getByRole("button", { name: "Kill PID 4242" }));
    let dialog = await screen.findByRole("dialog", { name: "Kill PID 4242 (nginx)?" });
    // The confirmation names both halves of the identity it is about to send.
    expect(dialog).toHaveTextContent("PID 4242");
    expect(dialog).toHaveTextContent("nginx");
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(called("system.process_kill")).toHaveLength(0);

    openRowMenu(4242, "nginx");
    fireEvent.click(screen.getByRole("button", { name: "Kill PID 4242" }));
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
    expect(await screen.findByText("1 process on this machine, by pid.")).toBeInTheDocument();
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
    await screen.findByText("1 process on this machine, by pid.");

    openRowMenu(4242, "nginx");
    fireEvent.click(screen.getByRole("button", { name: "Kill PID 4242" }));
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
    await screen.findByText("The machine reports no running processes.");

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
    await screen.findByText("The machine reports no running processes.");

    // The grant decides whether the call is allowed; the page does not ask for a tool list first
    // (this mock fails an unasked-for call) and does not hide the deliberate step on a guess.
    const button = screen.getByRole("button", { name: "Restart app" });
    expect(button).toBeEnabled();
    fireEvent.click(button);

    const dialog = await screen.findByRole("dialog", { name: "Restart the device app?" });
    // The confirmation names the losses and the launcher budget, not just "are you sure".
    expect(dialog).toHaveTextContent(
      "Every open terminal, browser tab and run on this machine ends"
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
    await screen.findByText("The machine reports no running processes.");

    fireEvent.click(screen.getByRole("button", { name: "Restart app" }));
    const dialog = await screen.findByRole("dialog", { name: "Restart the device app?" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Restart app" }));

    // The empty object is the whole argument set: the tool restarts the app, not a named process.
    await vi.waitFor(() => expect(called("host.restart")).toHaveLength(1));
    expect(mocked).toHaveBeenCalledWith(GRANT, "host.restart", {});
    expect(
      await screen.findByText(
        "The device is restarting through its launcher. It will reconnect in a few seconds; " +
          "terminals, browser tabs and runs on this machine ended."
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
    await screen.findByText("The machine reports no running processes.");

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
    await screen.findByText("The machine reports no running processes.");

    fireEvent.click(screen.getByRole("button", { name: "Restart app" }));
    const dialog = await screen.findByRole("dialog", { name: "Restart the device app?" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Restart app" }));

    expect(
      await screen.findByText(
        "The connection ended before the device confirmed the restart. It may be restarting; " +
          "reload the page once it reconnects instead of asking again."
      )
    ).toBeInTheDocument();
    // A transport failure is not a refusal: the app may already be exiting, and a second click would
    // spend another of the launcher's three restarts.
    expect(screen.getByRole("button", { name: "Restart app" })).toBeDisabled();
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
    await screen.findByText("The machine reports no running processes.");

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
