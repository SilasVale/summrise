import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Device, LiveTool } from "./api";
import { HomePage } from "./HomePage";

const installerApi = vi.hoisted(() => ({ installer: vi.fn() }));
vi.mock("./api", () => ({
  api: { installer: installerApi.installer }
}));

const live = vi.hoisted(() => ({ value: {} as Record<string, unknown> }));
vi.mock("./useLiveTools", () => ({ useLiveTools: () => live.value }));

const workshop: Device = {
  id: "dev-workshop",
  name: "workshop",
  online: true,
  version: "0.3.246",
  last_seen_ms: 1
};

function tool(name: string, capability: string): LiveTool {
  return {
    name,
    description: "",
    inputSchema: { type: "object", properties: {} },
    source: "builtin",
    capability,
    destructive: false
  };
}

const tools = [
  tool("terminal.create", "terminal"),
  tool("terminal.write", "terminal"),
  tool("files.list", "files"),
  tool("files.read", "files"),
  tool("screen.capture", "screen"),
  tool("plugins.list", "plugins")
];

/** The installer facts the console answers with, as a fleet test's `/installer` would. */
const installerFacts = {
  available: true as const,
  filename: "SummriseSetup-0.3.9.exe",
  version: "0.3.9",
  size_bytes: 6_638_339,
  sha256: "26d39c12" + "a".repeat(56)
};

beforeEach(() => {
  live.value = { tools, stale: false, error: null, offline: false, loading: false, retry: vi.fn() };
});

/**
 * Render Home with no machine chosen: the fleet, where the Add-a-machine flow lives. This is the
 * state the gate signs in to and the item is about.
 */
function drawFleet(extra: Record<string, unknown> = {}) {
  const onTokens = vi.fn();
  const onRetryFleet = vi.fn();
  const onAddMachine = vi.fn();
  const onOpen = vi.fn();
  const onOpenMachine = vi.fn();
  const onConnect = vi.fn();
  const onTools = vi.fn();
  const onNotice = vi.fn();
  const onActivity = vi.fn();
  const onDevice = vi.fn();
  const fleetProps = {
    device: null,
    devices: [],
    selectedId: null,
    code: null,
    enrollCommand: "",
    busy: false,
    onOpen,
    onOpenMachine,
    onRename: vi.fn(),
    onRemove: vi.fn(),
    onDiagnostics: vi.fn(),
    onAddMachine,
    onTokens,
    onDismissCode: vi.fn(),
    onNotice,
    onConnect,
    onTools,
    onActivity,
    onDevice,
    onRetryFleet
  };
  const page = (overrides: Record<string, unknown> = {}) => (
    <HomePage {...fleetProps} {...overrides} />
  );
  const view = render(page(extra));
  return {
    ...view,
    onTokens,
    onRetryFleet,
    onAddMachine,
    onOpen,
    onOpenMachine,
    onConnect,
    onTools,
    onNotice,
    onActivity,
    onDevice,
    rerenderFleet: (overrides: Record<string, unknown>) =>
      view.rerender(page({ ...extra, ...overrides }))
  };
}

/** One machine in the new fleet's table shape: online and on the staged installer unless it says. */
function machine(overrides: Partial<Device> = {}): Device {
  return {
    id: `dev-${overrides.name ?? "machine"}`,
    name: "machine",
    online: true,
    version: installerFacts.version,
    last_seen_ms: Date.now() - 5_000,
    ...overrides
  };
}

describe("Home — the fleet's installer", () => {
  beforeEach(() => {
    installerApi.installer.mockReset();
  });

  it("offers the installer the console was given, with its own filename, size and hash", async () => {
    installerApi.installer.mockResolvedValue(installerFacts);
    drawFleet();

    const heading = await screen.findByRole("heading", {
      name: "Install the device runtime"
    });
    const link = screen.getByRole("link", { name: "Download Windows installer" });
    expect(link).toHaveAttribute("href", "/installer/download");
    expect(link).toHaveAttribute("download");
    // Every value on the muted line is the /installer answer: the filename, the size at the scale
    // runFormat states it, and the first eight hex characters of the hash it computed.
    // The digest sits in its own element so it can carry the full value in a title; the line is
    // still one sentence, asserted whole rather than by its separate text nodes.
    expect(
      screen.getByText(
        (_, node) =>
          node?.tagName === "P" &&
          node.textContent === "SummriseSetup-0.3.9.exe · 6.3 MB · sha256 26d39c12…"
      )
    ).toBeInTheDocument();
    expect(
      heading.compareDocumentPosition(link) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
  });

  it("keeps the full digest one gesture from the short hash, with a copy control", async () => {
    installerApi.installer.mockResolvedValue(installerFacts);
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true
    });
    const { onNotice } = drawFleet();

    // **THE STRIP'S SHORT HASH IS A PREFIX, NOT THE WHOLE CHECK.** `/installer` carries the 64 hex
    // characters; the page shows the first eight and keeps the full value in the title and on one
    // press, so a 6.6 MB download can be checked without knowing the JSON endpoint.
    const short = await screen.findByText("sha256 26d39c12…");
    expect(short).toHaveAttribute("title", installerFacts.sha256);

    fireEvent.click(screen.getByRole("button", { name: "Copy digest" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(installerFacts.sha256));
    expect(onNotice).toHaveBeenCalledWith("Digest copied.");
  });

  it("sends a machine behind the staged installer to Needs attention", async () => {
    // The measured fleet: 84T3LIN read v0.3.280 while the same page offered the 0.3.284
    // installer, and the row gave the operator no way to notice without comparing two surfaces.
    installerApi.installer.mockResolvedValue(installerFacts);
    drawFleet({ devices: [{ ...workshop, version: "0.3.8" }] });

    expect(
      await screen.findByText("The release is 0.3.9.")
    ).toBeInTheDocument();
    // The staged installer is the RELEASE; the console's own build is another version scheme and
    // the page may not call both "the console" (audit-40 F10).
    expect(screen.queryByText(/the console has/)).toBeNull();
    expect(screen.queryByText("behind 0.3.9")).toBeNull();
  });

  it("draws no fleet cue when the console staged no installer", async () => {
    installerApi.installer.mockResolvedValue({ available: false });
    drawFleet({ devices: [{ ...workshop, version: "0.3.8" }] });

    await screen.findByText(/No installer is published/);
    expect(screen.queryByText(/behind/)).not.toBeInTheDocument();
  });

  it("says plainly when the release carries no installer, and draws no door", async () => {
    installerApi.installer.mockResolvedValue({ available: false });
    drawFleet();

    expect(
      await screen.findByText(
        "No installer is published for this console yet. Install it from the release, then add the machine for its enrollment command."
      )
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("link", { name: "Download Windows installer" })
    ).toBeNull();
    expect(
      screen.queryByRole("heading", { name: "Install the device runtime" })
    ).toBeNull();
  });

  it("hides the block when the read fails rather than showing a door that cannot open", async () => {
    installerApi.installer.mockRejectedValue(new Error("the installer could not be read"));
    drawFleet();

    await waitFor(() => expect(installerApi.installer).toHaveBeenCalledTimes(1));
    expect(
      screen.queryByRole("heading", { name: "Install the device runtime" })
    ).toBeNull();
    expect(screen.queryByText(/No installer is published/)).toBeNull();
  });

  it("reads the installer once, not on every redraw", async () => {
    installerApi.installer.mockResolvedValue(installerFacts);
    const { rerenderFleet } = drawFleet();
    await screen.findByRole("heading", { name: "Install the device runtime" });

    rerenderFleet({ busy: true });
    await act(async () => {});

    expect(installerApi.installer).toHaveBeenCalledTimes(1);
  });

  it("puts the block before the enrollment command in the Add-a-machine panel", async () => {
    installerApi.installer.mockResolvedValue(installerFacts);
    drawFleet({
      code: { code: "code-1", expires_at_ms: Date.now() + 15 * 60_000 },
      enrollCommand:
        "summrise-device.exe --enroll https://console.test code-1 --data C:\\ProgramData\\Summrise"
    });

    const heading = await screen.findByRole("heading", {
      name: "Install the device runtime"
    });
    const command = screen.getByText(
      "summrise-device.exe --enroll https://console.test code-1 --data C:\\ProgramData\\Summrise"
    );
    // The command assumes the binary is installed; the door that installs it comes first.
    expect(
      heading.compareDocumentPosition(command) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
    expect(
      screen.getAllByRole("heading", { name: "Install the device runtime" })
    ).toHaveLength(1);
  });
});

/**
 * THE OPERATOR'S SENTENCE, NOT A SECOND LIST. Home's first fact is whether anything needs a person:
 * one large verdict from the fleet read, then one row per problem with the door that opens that
 * machine. The per-row client-call counters are Activity's noun and must not be on this page.
 */
describe("Home — the verdict and what needs attention", () => {
  beforeEach(() => {
    installerApi.installer.mockResolvedValue(installerFacts);
  });

  it("says all four online and nothing to do when the fleet is healthy", async () => {
    drawFleet({
      devices: [
        machine({ id: "a", name: "alpha" }),
        machine({ id: "b", name: "bravo" }),
        machine({ id: "c", name: "charlie" }),
        machine({ id: "d", name: "delta" })
      ],
      fleetRead: { state: "ready", at: Date.now() }
    });

    expect(
      await screen.findByText("4 of 4 online · nothing to do")
    ).toBeInTheDocument();
  });

  it("counts one offline machine in the verdict and states when it was last seen", async () => {
    drawFleet({
      devices: [
        machine({ id: "a", name: "alpha" }),
        machine({ id: "b", name: "bravo" }),
        machine({ id: "c", name: "charlie" }),
        machine({
          id: "r14",
          name: "DESKTOP-14RJCR8",
          online: false,
          last_frame_ms: Date.now() - (2 * 24 + 5) * 3_600_000
        })
      ],
      fleetRead: { state: "ready", at: Date.now() }
    });

    expect(
      await screen.findByText("3 of 4 online · 1 needs attention")
    ).toBeInTheDocument();
    // The row's heading is the machine and the problem is a line under it (audit-40 F6).
    const attention = screen.getByRole("region", { name: "Needs attention" });
    expect(
      within(attention).getByRole("heading", { name: "DESKTOP-14RJCR8", level: 3 })
    ).toBeInTheDocument();
    // The attention line uses the fleet row's one phrase, not a second spelling (audit-43 F2).
    expect(
      within(attention).getByText("offline · last seen 2d 5h ago")
    ).toBeInTheDocument();
  });

  it("counts a machine behind the staged installer even while every machine is online", async () => {
    drawFleet({
      devices: [
        machine({ id: "a", name: "alpha" }),
        machine({ id: "b", name: "bravo" }),
        machine({ id: "c", name: "charlie" }),
        machine({ id: "old", name: "old", version: "0.3.8" })
      ],
      fleetRead: { state: "ready", at: Date.now() }
    });

    expect(
      await screen.findByText("4 of 4 online · 1 needs attention")
    ).toBeInTheDocument();
    const attention = screen.getByRole("region", { name: "Needs attention" });
    expect(within(attention).getByRole("heading", { name: "old", level: 3 })).toBeInTheDocument();
    // The release is stated once by the block; the row names the machine and the problem.
    expect(within(attention).getByText("The release is 0.3.9.")).toBeInTheDocument();
    expect(within(attention).getByText("Behind the release.")).toBeInTheDocument();
    expect(within(attention).queryByText(/0\.3\.8/)).toBeNull();
  });

  it("draws one row per machine, its problems listed, with one button that opens it", async () => {
    // The real 14R shape: offline and behind at once. The verdict counts machines (one), so the
    // block draws one row with both problems listed and ONE door — four rows and four identical
    // buttons over two machines made the count and the block disagree (audit-40 F6).
    installerApi.installer.mockResolvedValue({ ...installerFacts, version: "0.3.289" });
    const { onOpenMachine } = drawFleet({
      devices: [
        machine({ id: "a", name: "alpha", version: "0.3.289" }),
        machine({ id: "b", name: "bravo", version: "0.3.289" }),
        machine({ id: "c", name: "charlie", version: "0.3.289" }),
        machine({
          id: "r14",
          name: "DESKTOP-14RJCR8",
          online: false,
          version: "0.3.275",
          last_frame_ms: Date.now() - (2 * 24 + 5) * 3_600_000
        })
      ],
      fleetRead: { state: "ready", at: Date.now() }
    });

    expect(
      await screen.findByText("3 of 4 online · 1 needs attention")
    ).toBeInTheDocument();
    const attention = screen.getByRole("region", { name: "Needs attention" });
    expect(attention.querySelectorAll(".attention-row")).toHaveLength(1);
    expect(
      within(attention).getByRole("heading", { name: "DESKTOP-14RJCR8", level: 3 })
    ).toBeInTheDocument();
    const problems = attention.querySelectorAll(".attention-problems li");
    expect(Array.from(problems, (problem) => problem.textContent)).toEqual([
      "offline · last seen 2d 5h ago",
      "Behind the release."
    ]);
    // One release position: the block's sentence. Neither the machine's version nor the release is
    // repeated in the row; the table's Version column is where the build is compared (audit-43 F2).
    expect(within(attention).getAllByText("The release is 0.3.289.")).toHaveLength(1);
    expect(within(attention).queryByText(/0\.3\.275/)).toBeNull();
    // One door for one machine; the name appears once, not in every problem line. The visible verb
    // is `Open`, the same one the fleet row and the picker use (audit-44 F9).
    const doors = within(attention).getAllByRole("button", {
      name: "Open machine for DESKTOP-14RJCR8"
    });
    expect(doors).toHaveLength(1);
    expect(doors[0]).toHaveTextContent(/^Open$/);
    expect(within(attention).getAllByText(/DESKTOP-14RJCR8/)).toHaveLength(1);
    fireEvent.click(doors[0]);
    expect(onOpenMachine).toHaveBeenCalledWith("r14");
  });

  it("folds two machines' four problems into two rows, each with its own door", async () => {
    // The measured 390 page: 14R and CPH each away and behind, four problems and four buttons in
    // four rows. The block keeps every fact and loses the duplicate door: two rows, two doors
    // (audit-40 F6).
    installerApi.installer.mockResolvedValue({ ...installerFacts, version: "0.3.290" });
    const { onOpenMachine } = drawFleet({
      devices: [
        machine({
          id: "r14",
          name: "DESKTOP-14RJCR8",
          online: false,
          version: "0.3.275",
          last_frame_ms: Date.now() - (2 * 24 + 11) * 3_600_000
        }),
        machine({
          id: "cph",
          name: "DESKTOP-CPH4KB2",
          online: false,
          version: "0.3.289",
          last_frame_ms: Date.now() - (9 * 60 + 29) * 60_000
        })
      ],
      fleetRead: { state: "ready", at: Date.now() }
    });

    expect(
      await screen.findByText("0 of 2 online · 2 need attention")
    ).toBeInTheDocument();
    const attention = screen.getByRole("region", { name: "Needs attention" });
    const rows = Array.from(attention.querySelectorAll(".attention-row"));
    expect(rows).toHaveLength(2);
    expect(rows.map((row) => row.querySelector(".attention-machine")?.textContent)).toEqual([
      "DESKTOP-14RJCR8",
      "DESKTOP-CPH4KB2"
    ]);
    expect(rows.map((row) => row.querySelectorAll(".attention-problems li").length)).toEqual([
      2, 2
    ]);
    // One door per machine, named for that machine — the four identical buttons are gone.
    expect(
      within(attention).getAllByRole("button", { name: "Open machine for DESKTOP-14RJCR8" })
    ).toHaveLength(1);
    expect(
      within(attention).getAllByRole("button", { name: "Open machine for DESKTOP-CPH4KB2" })
    ).toHaveLength(1);
    fireEvent.click(
      within(attention).getByRole("button", { name: "Open machine for DESKTOP-CPH4KB2" })
    );
    expect(onOpenMachine).toHaveBeenCalledWith("cph");
  });

  it("says nothing needs attention, with the fleet read's age, when the fleet is healthy", async () => {
    drawFleet({
      devices: [machine({ id: "a", name: "alpha" })],
      fleetRead: { state: "ready", at: Date.now() - 12_000 }
    });

    expect(await screen.findByText("Nothing needs attention.")).toBeInTheDocument();
    expect(screen.getByText(/^Fleet read \d+s ago\.$/)).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "Needs attention" })
    ).toBeNull();
  });

  it("draws no client-call counters on Home", async () => {
    drawFleet({
      devices: [
        machine({
          id: "a",
          name: "alpha",
          today: { client_calls: 283, failed: 38 }
        }),
        machine({ id: "b", name: "bravo", today: { client_calls: 84, failed: 2 } })
      ],
      fleetRead: { state: "ready", at: Date.now() }
    });

    await screen.findByText("2 of 2 online · nothing to do");
    expect(screen.queryByText(/client call/)).toBeNull();
    expect(screen.queryByText(/failed/)).toBeNull();
    expect(screen.queryByText("283 client calls · 38 failed")).toBeNull();
    expect(document.querySelector(".fleet-work")).toBeNull();
  });

  it("draws the compact Machine / Status / Version / Started table, installer below it", async () => {
    const booted = Date.now() - 3 * 3_600_000;
    drawFleet({
      devices: [
        machine({
          id: "a",
          name: "alpha",
          version: "0.3.9",
          update: {
            current: "0.3.9",
            verdict: { outcome: "ok", detail: "started 0.3.9" }
          },
          host: {
            restarts: [{ at_ms: booted, version: "0.3.9", channel: "canary", pid: 42 }]
          }
        }),
        machine({ id: "b", name: "bravo", online: false })
      ],
      fleetRead: { state: "ready", at: Date.now() }
    });

    const table = await screen.findByRole("table", { name: "The fleet" });
    const headers = within(table)
      .getAllByRole("columnheader")
      .map((header) => header.textContent?.trim());
    expect(headers).toEqual(["Machine", "Status", "Version", "Started", "Actions"]);
    expect(within(table).getAllByRole("row")).toHaveLength(3);
    expect(within(table).getByRole("button", { name: "Open alpha" })).toBeInTheDocument();
    expect(within(table).getAllByText("v0.3.9")).toHaveLength(2);
    expect(within(table).getByText(/^3h ago$/)).toBeInTheDocument();
    // The coloured dot is beside the status word, not the name.
    const alphaRow = within(table).getByRole("button", { name: "Open alpha" }).closest("tr");
    expect(alphaRow?.querySelector(".fleet-status .dot.dot-ok")).not.toBeNull();
    expect(alphaRow?.querySelector(".fleet-status .fleet-state")).toHaveTextContent("online");

    const installerHeading = await screen.findByRole("heading", {
      name: "Install the device runtime"
    });
    // The installer is the last surface, not the first card: the fleet comes before it.
    expect(
      table.compareDocumentPosition(installerHeading) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
    expect(installerHeading.closest(".installer")).toHaveClass("installer-strip");
  });
});

/**
 * THE SELECTION IS THE FLEET'S, NOT THE PAGE'S. Choosing a machine must not turn Home into that
 * machine's page: the verdict, the attention rows and the table are claims about the whole fleet
 * whichever machine is in context, and the machine itself is one section beside them — never a
 * second Activity. The machine's own page is Device.
 */
describe("Home — the selected machine", () => {
  beforeEach(() => {
    installerApi.installer.mockResolvedValue(installerFacts);
  });

  function twoMachines() {
    return [
      machine({ id: "w", name: "workshop", version: workshop.version }),
      machine({
        id: "r14",
        name: "DESKTOP-14RJCR8",
        online: false,
        last_frame_ms: Date.now() - (2 * 24 + 5) * 3_600_000
      })
    ];
  }

  it("keeps the fleet's verdict, attention rows and table while a machine is selected", async () => {
    drawFleet({
      device: workshop,
      selectedId: workshop.id,
      devices: twoMachines(),
      fleetRead: { state: "ready", at: Date.now() }
    });

    expect(await screen.findByText("1 of 2 online · 1 needs attention")).toBeInTheDocument();
    // 59d6b377's grouped block: one row per machine, the name as its heading and the problem as a
    // line under it. The selection must not remove it.
    const attention = screen.getByRole("region", { name: "Needs attention" });
    expect(
      within(attention).getByRole("heading", { name: "DESKTOP-14RJCR8", level: 3 })
    ).toBeInTheDocument();
    expect(attention).toHaveTextContent("offline · last seen 2d 5h ago");
    expect(screen.getByRole("table", { name: "The fleet" })).toBeInTheDocument();
    // One destination, one heading: the selected state does not become a second page.
    expect(screen.getAllByRole("heading", { level: 1 })).toHaveLength(1);
    expect(screen.getByRole("heading", { name: "Home", level: 1 })).toBeInTheDocument();
  });

  it("summarises the selected machine's capabilities in one section of Home", async () => {
    const { onDevice, onConnect, onTools } = drawFleet({
      device: workshop,
      selectedId: workshop.id,
      devices: twoMachines(),
      fleetRead: { state: "ready", at: Date.now() }
    });

    const summary = await screen.findByRole("region", { name: "Selected machine" });
    // **HIGHLIGHTED, NOT REPEATED.** The row and the topbar already state the machine's name,
    // reachability, age and build; the summary repeats none of them (audit-43 F2). Its heading is
    // the generic section label, and the highlighted row says which machine it describes.
    expect(
      within(summary).getByRole("heading", { name: "Selected machine", level: 2 })
    ).toBeInTheDocument();
    // **THE CARD NAMES ITS SUBJECT (audit-45 F1).** audit-43 F2 read the highlighted row and the
    // topbar as enough; the operator's brief is that a section headed "Selected machine" says which
    // machine at a glance, so the name is here once. The AGE stays the row's below it, and the
    // version is stated once on this card rather than a third time on the page.
    expect(within(summary).getByText("workshop")).toHaveClass("machine-summary-name");
    expect(within(summary).getByText("online")).toHaveClass("machine-summary-presence");
    expect(within(summary).queryByText(/last seen|seen \d/)).toBeNull();
    // THE VERSION IS NOT SAID THREE TIMES. The topbar's chip states what runs and the table's
    // Version column repeats it for the fleet; the summary adds no third copy (task 79's rule for
    // the Machine tab's started line).
    expect(within(summary).queryByText(/^v\d/)).toBeNull();

    // What it can do is the hub's counts line, shortened: tools and capabilities, from the live
    // catalogue. The line is still a door to the machine's Tools tab.
    const counts = await within(summary).findByRole(
      "button",
      { name: "6 tools · 4 capabilities" },
      { timeout: 3_000 }
    );
    fireEvent.click(counts);
    expect(onTools).toHaveBeenCalledTimes(1);

    // The machine's own page is Device; the summary hands off instead of becoming it. **ONE
    // VERB, ONE NAME (audit-44 F9):** the button was `Open Device`, now `Open` with the machine as
    // its accessible subject, the same verb as the fleet row and the picker.
    const open = within(summary).getByRole("button", { name: "Open workshop" });
    expect(open).toHaveTextContent(/^Open$/);
    fireEvent.click(open);
    expect(onDevice).toHaveBeenCalledTimes(1);
    fireEvent.click(within(summary).getByRole("button", { name: "Connect a client" }));
    expect(onConnect).toHaveBeenCalledTimes(1);
  });

  it("draws the selected machine once and highlights its row", async () => {
    const { container } = drawFleet({
      device: workshop,
      selectedId: workshop.id,
      devices: [
        machine({ id: workshop.id, name: "workshop", version: workshop.version }),
        machine({ id: "a", name: "attic" })
      ],
      fleetRead: { state: "ready", at: Date.now() }
    });

    const summary = await screen.findByRole("region", { name: "Selected machine" });
    // The name is the row's AND the summary's, because the summary is the section that names its
    // subject (audit-45 F1); "once" now means the table does not repeat the selection a third time.
    expect(within(summary).getByText("workshop")).toHaveClass("machine-summary-name");
    expect(screen.getAllByText("workshop")).toHaveLength(2);
    const table = screen.getByRole("table", { name: "The fleet" });
    expect(within(table).getAllByRole("row")).toHaveLength(3);
    const row = within(table).getByRole("button", { name: "Open workshop" }).closest("tr");
    expect(row).toHaveClass("is-current");
    expect(screen.queryByText("Running")).toBeNull();
    expect(container.querySelector(".machine-summary-head")).toBeNull();
  });

  it("sends the fleet row's door to Overview and the attention door to the machine tab", async () => {
    const { onOpen, onOpenMachine } = drawFleet({
      device: workshop,
      selectedId: workshop.id,
      devices: [
        machine({ id: "w", name: "workshop", version: workshop.version }),
        machine({
          id: "r14",
          name: "DESKTOP-14RJCR8",
          online: false,
          last_frame_ms: Date.now() - 3 * 3_600_000
        })
      ],
      fleetRead: { state: "ready", at: Date.now() }
    });

    const table = screen.getByRole("table", { name: "The fleet" });
    fireEvent.click(within(table).getByRole("button", { name: "Open workshop" }));
    expect(onOpen).toHaveBeenCalledWith("w");
    fireEvent.click(
      screen.getByRole("button", { name: "Open machine for DESKTOP-14RJCR8" })
    );
    expect(onOpenMachine).toHaveBeenCalledWith("r14");
    // The two doors are different verbs: the row opens the machine's Overview, the attention row
    // its Machine tab. Neither is a selection-only press (audit-43 F1).
    expect(onOpen).not.toHaveBeenCalledWith("r14");
    expect(onOpenMachine).not.toHaveBeenCalledWith("w");
  });

  it("draws no second Activity on Home when a machine is selected", async () => {
    drawFleet({
      device: workshop,
      selectedId: workshop.id,
      devices: [machine({ id: "w", name: "workshop", version: workshop.version })],
      fleetRead: { state: "ready", at: Date.now() }
    });

    await screen.findByRole("region", { name: "Selected machine" });
    // The old selected Home was Activity in another place: a Now band over a day timeline. The
    // selection adds a summary, not the record.
    expect(screen.queryByRole("region", { name: "This machine's activity" })).toBeNull();
    expect(screen.queryByRole("heading", { name: "Now" })).toBeNull();
    expect(document.querySelectorAll(".timeline-row")).toHaveLength(0);
    expect(screen.queryByText(/client calls?/)).toBeNull();
    expect(screen.queryByText(/Today no client calls/)).toBeNull();
  });

  it("says the selected machine is offline and draws no fake counts", async () => {
    const offline = { ...workshop, online: false };
    live.value = {
      tools: [],
      stale: false,
      error: "this machine is not connected; connect it, then try again",
      offline: true,
      loading: false,
      retry: vi.fn()
    };
    drawFleet({
      device: offline,
      selectedId: offline.id,
      devices: [offline],
      fleetRead: { state: "ready", at: Date.now() }
    });

    const summary = await screen.findByRole("region", { name: "Selected machine" });
    // **AN OFFLINE MACHINE KEEPS ITS FULL PHRASE (audit-45 F1).** The card's next sentence is about
    // counts it cannot read, so the why belongs here: the fleet row's own `offline · last seen …`
    // phrasing, not a second spelling. An online machine states `online` alone and leaves the age to
    // the row below it.
    expect(within(summary).getByText(/^offline · last seen/)).toHaveClass(
      "machine-summary-presence"
    );
    expect(
      within(summary).getByText("Tool counts are unavailable while the machine is offline.")
    ).toBeInTheDocument();
    expect(within(summary).queryByText(/^0 tools/)).toBeNull();
  });
});

/**
 * The enrollment command names only `summrise-device.exe`, and the console cannot know a machine's
 * install root, so the sentence must carry what the command cannot: the folder chosen on the
 * installer's directory page (its default is C:\Program Files\Summrise). An elevated prompt opened
 * anywhere else fails with "not recognized".
 */
describe("Home — the enrollment sentence", () => {
  beforeEach(() => {
    installerApi.installer.mockResolvedValue(installerFacts);
  });

  it("sends the elevated prompt to the folder where Summrise was installed", () => {
    drawFleet({
      code: { code: "code-1", expires_at_ms: Date.now() + 15 * 60_000 },
      enrollCommand:
        "summrise-device.exe --enroll https://console.test code-1 --data C:\\ProgramData\\Summrise"
    });

    const panel = screen.getByRole("region", { name: "Enrollment command" });
    expect(panel).toHaveTextContent(
      "On the Windows machine, open an elevated Command Prompt in the folder you chose on the installer's directory page (the default is C:\\Program Files\\Summrise), run this command, then restart the SummriseLaunch service."
    );
    // The old sentence sent the person to a prompt whose cwd was not the install folder.
    expect(panel).not.toHaveTextContent("run this in an elevated Command Prompt");
    expect(within(panel).getByText("SummriseLaunch").tagName).toBe("CODE");
    expect(within(panel).getByText("C:\\Program Files\\Summrise").tagName).toBe("CODE");
  });
});

/**
 * The credential's door. Tokens used to live only under Account → Tokens, so a person adding a
 * machine never met the thing a client needs; the fleet header and the Add-a-machine panel each
 * open the same Connect sheet on its Tokens section. There is no second token surface to test.
 */
describe("Home — the fleet's Tokens door", () => {
  beforeEach(() => {
    installerApi.installer.mockResolvedValue(installerFacts);
  });

  it("opens the Connect sheet's Tokens section from the fleet header", async () => {
    const { onTokens } = drawFleet({ devices: [workshop] });

    const door = screen.getByRole("button", { name: "Tokens" });
    // The accent belongs to Add a machine; the credential is the secondary control beside it.
    expect(door).not.toHaveClass("primary");
    fireEvent.click(door);
    expect(onTokens).toHaveBeenCalledTimes(1);
  });

  it("keeps the door in the fleet's zero state", () => {
    drawFleet();

    expect(screen.getByRole("heading", { name: "Home", level: 1 })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Tokens" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add a machine" })).toBeInTheDocument();
  });

  it("puts a Tokens control in the Add-a-machine panel", () => {
    const { onTokens } = drawFleet({
      code: { code: "code-1", expires_at_ms: Date.now() + 15 * 60_000 },
      enrollCommand:
        "summrise-device.exe --enroll https://console.test code-1 --data C:\\ProgramData\\Summrise"
    });

    const panel = screen.getByRole("region", { name: "Enrollment command" });
    expect(
      within(panel).getByRole("heading", { name: "Enrollment command" })
    ).toBeInTheDocument();
    fireEvent.click(within(panel).getByRole("button", { name: "Tokens" }));
    expect(onTokens).toHaveBeenCalledTimes(1);
  });
});

/**
 * THE FLEET IS A READ BEFORE IT IS A LIST. A pending `/fleet` said `0 machines · 0 online` and
 * `No machines yet`, and a failed one said the same under a dismissable banner. These fixtures are
 * the whole state machine — reading, failed, and the successful empty answer that is the only zero
 * state. The setup doors do not wait for the read: adding a machine mints a code and the installer
 * is a file the console serves, so the read's state never takes them away (task 72).
 */
describe("Home — the fleet's read", () => {
  beforeEach(() => {
    // The installer answer lands while the fleet is pending too, so its absence below is the page's
    // rule rather than a fixture that had not resolved.
    installerApi.installer.mockResolvedValue(installerFacts);
  });

  it("says it is reading the machine list instead of claiming the fleet is empty", async () => {
    drawFleet({ fleetRead: { state: "reading" } });

    expect(screen.getByText("Reading the machine list…")).toBeInTheDocument();
    expect(
      screen.getByRole("status", { name: "Reading the machine list" })
    ).toBeInTheDocument();
    expect(screen.queryByText("No machines yet")).toBeNull();
    expect(screen.queryByText("0 machines · 0 online")).toBeNull();
    // The reading sentence is the page's claim about the fleet; the setup doors are the account's
    // and do not wait for that claim. A person on a slow link may be adding the machine now.
    expect(screen.getByRole("button", { name: "Add a machine" })).toBeInTheDocument();
    await waitFor(() => expect(installerApi.installer).toHaveBeenCalledTimes(1));
    expect(
      screen.getByRole("heading", { name: "Install the device runtime" })
    ).toBeInTheDocument();
    // The credential door is not the setup action; it stays too.
    expect(screen.getByRole("button", { name: "Tokens" })).toBeInTheDocument();
  });

  it("names a failed read with its reason and offers a retry", async () => {
    const { onRetryFleet } = drawFleet({
      fleetRead: { state: "failed", reason: "Failed to fetch" }
    });

    // ONE FAILURE, ONE STATEMENT, ONE RETRY, ON THIS PAGE (audit-44 F3). The shell's banner no
    // longer repeats the read: this notice names the failure, keeps the cause in its title, and is
    // the one control that reads it again.
    const notice = screen
      .getByText("The console could not read the machine list.")
      .closest(".notice");
    expect(notice).not.toBeNull();
    expect(notice).not.toHaveTextContent("Failed to fetch");
    expect(within(notice as HTMLElement).getByTitle("Failed to fetch")).toBeInTheDocument();
    expect(screen.getAllByText("The console could not read the machine list.")).toHaveLength(1);
    expect(screen.getAllByRole("button", { name: "Retry" })).toHaveLength(1);
    expect(document.body.textContent).not.toContain("Failed to fetch");
    expect(screen.queryByText("No machines yet")).toBeNull();
    // The failed read states its own failure; it does not take away the next step.
    expect(screen.getByRole("button", { name: "Add a machine" })).toBeInTheDocument();
    await waitFor(() => expect(installerApi.installer).toHaveBeenCalledTimes(1));
    expect(
      screen.getByRole("heading", { name: "Install the device runtime" })
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetryFleet).toHaveBeenCalledTimes(1);
  });

  it("keeps the last list it read below a failed read, and does not call it empty", () => {
    drawFleet({
      devices: [workshop],
      fleetRead: { state: "failed", reason: "503 device_unavailable" }
    });

    expect(
      screen.getByText(
        "The console could not read the machine list; the machines below are the last list it read."
      )
    ).toBeInTheDocument();
    // The stale clause is part of the one statement; the console's status is the title's.
    expect(screen.getByTitle("503 device_unavailable")).toBeInTheDocument();
    expect(document.body.textContent).not.toContain("503 device_unavailable");
    expect(screen.getByText("workshop")).toBeInTheDocument();
    expect(screen.queryByText("No machines yet")).toBeNull();
    // A stale list is still not a clean page: the door to add another machine is there.
    expect(screen.getByRole("button", { name: "Add a machine" })).toBeInTheDocument();
  });

  it("keeps Add a machine and the installer doors while the machine list is pending", async () => {
    const { onAddMachine } = drawFleet({ fleetRead: { state: "reading" } });

    // The read's own sentence stays: the doors are drawn beside it, not instead of it.
    expect(screen.getByText("Reading the machine list…")).toBeInTheDocument();
    // Neither door reads `/fleet`: adding a machine mints a code with a POST, and the installer is
    // a file this console serves. A slow list is when a person may need them most.
    const add = screen.getByRole("button", { name: "Add a machine" });
    expect(add).toBeEnabled();
    fireEvent.click(add);
    expect(onAddMachine).toHaveBeenCalledTimes(1);
    expect(
      await screen.findByRole("heading", { name: "Install the device runtime" })
    ).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Download Windows installer" })).toHaveAttribute(
      "href",
      "/installer/download"
    );
    // A door is not a fleet: the page still does not claim the list is empty.
    expect(screen.queryByText("No machines yet")).toBeNull();
    expect(screen.queryByText("0 machines · 0 online")).toBeNull();
  });

  it("keeps Add a machine and the installer doors while the machine list failed", async () => {
    const { onAddMachine, onRetryFleet } = drawFleet({
      fleetRead: { state: "failed", reason: "Failed to fetch" }
    });

    // The missing list and its one Retry stay; the doors are the next step beside them.
    expect(
      screen.getByText("The console could not read the machine list.")
    ).toBeInTheDocument();
    expect(document.body.textContent).not.toContain("Failed to fetch");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(onRetryFleet).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Add a machine" }));
    expect(onAddMachine).toHaveBeenCalledTimes(1);
    expect(
      await screen.findByRole("heading", { name: "Install the device runtime" })
    ).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Download Windows installer" })).toHaveAttribute(
      "href",
      "/installer/download"
    );
    expect(screen.queryByText("No machines yet")).toBeNull();
  });

  it("offers enrollment only when a successful read returned no machines", async () => {
    drawFleet({ fleetRead: { state: "ready" } });

    // THE ZERO STATE IS ONE SENTENCE. `No machines yet` and `0 machines · 0 online` said the same
    // thing twice; the fleet's size is the sentence, not a second line.
    expect(screen.getByText("No machines yet")).toBeInTheDocument();
    expect(screen.queryByText("0 machines · 0 online")).toBeNull();
    expect(screen.getByRole("button", { name: "Add a machine" })).toBeInTheDocument();
    // THE DOORS ARE THE NEXT STEP, LABELLED, IN THE ORDER THEY ARE USED. The old empty paragraph
    // told the reader to run the command before the installer had been offered, and the command's
    // panel had no visible name.
    expect(screen.getByRole("heading", { name: "Add your first machine" })).toBeInTheDocument();
    expect(screen.queryByText(/Run the command/)).toBeNull();
    const install = await screen.findByRole("heading", { name: "Install the device runtime" });
    const firstRun = screen.getByRole("heading", { name: "Add your first machine" });
    expect(
      firstRun.compareDocumentPosition(install) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
  });

  it("puts the installer before the labelled enrollment command", async () => {
    drawFleet({
      fleetRead: { state: "ready" },
      code: { code: "code-1", expires_at_ms: Date.now() + 15 * 60_000 },
      enrollCommand:
        "summrise-device.exe --enroll https://console.test code-1 --data C:\\ProgramData\\Summrise"
    });

    const install = await screen.findByRole("heading", { name: "Install the device runtime" });
    const enroll = screen.getByRole("heading", { name: "Enrollment command" });
    expect(
      install.compareDocumentPosition(enroll) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
  });
});

describe("Home — opening the enrollment panel", () => {
  beforeEach(() => {
    installerApi.installer.mockResolvedValue(installerFacts);
  });

  it("brings the panel into view and focuses its heading when the code arrives", async () => {
    // **A PRESS WHOSE PANEL IS BELOW THE FOLD LOOKS DEAD (audit-44 F7).** At 390 with a populated
    // fleet the Add-a-machine panel opened at y=1045 with scrollTop still 0, so the screen did not
    // move and the operator could not tell the press had worked. The panel is scrolled in and its
    // heading focused, which is also where a keyboard reader starts.
    const scrollIntoView = vi.fn();
    const original = Element.prototype.scrollIntoView;
    Element.prototype.scrollIntoView = scrollIntoView;
    try {
      const fleet = drawFleet({
        devices: [
          machine({ id: "a", name: "alpha" }),
          machine({ id: "b", name: "bravo" }),
          machine({ id: "c", name: "charlie" }),
          machine({ id: "d", name: "delta" })
        ],
        fleetRead: { state: "ready", at: Date.now() },
        code: null,
        enrollCommand: ""
      });

      fleet.rerenderFleet({
        code: { code: "code-1", expires_at_ms: Date.now() + 15 * 60_000 },
        enrollCommand:
          "summrise-device.exe --enroll https://console.test code-1 --data C:\\ProgramData\\Summrise"
      });

      const heading = await screen.findByRole("heading", { name: "Enrollment command" });
      expect(heading).toHaveAttribute("tabindex", "-1");
      expect(heading).toHaveFocus();
      expect(scrollIntoView).toHaveBeenCalledWith({ block: "start" });
      expect(scrollIntoView.mock.instances[0]).toBe(heading);
    } finally {
      Element.prototype.scrollIntoView = original;
    }
  });
});
