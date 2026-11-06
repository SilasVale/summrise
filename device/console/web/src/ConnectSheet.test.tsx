import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HttpError, type Created, type Device, type LiveTool, type TokenMeta } from "./api";
import { ConnectSheet } from "./ConnectSheet";

const mocks = vi.hoisted(() => ({
  tools: vi.fn(),
  liveTools: vi.fn(),
  tokens: vi.fn(),
  createToken: vi.fn(),
  rotateToken: vi.fn(),
  revokeToken: vi.fn()
}));

vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return {
    ...actual,
    api: {
      ...actual.api,
      tools: mocks.tools,
      tokens: mocks.tokens,
      createToken: mocks.createToken,
      rotateToken: mocks.rotateToken,
      revokeToken: mocks.revokeToken
    }
  };
});

vi.mock("./useLiveTools", () => ({
  useLiveTools: () => ({
    tools: mocks.liveTools(),
    stale: false,
    error: null,
    offline: false,
    loading: false,
    retry: vi.fn()
  })
}));

const FALLBACK_TOOL_NAMES = ["files.read", "terminal.write"];
vi.mock("./useToolNames", () => ({
  // A stable array: the real hook returns state, and an effect that depends on it must not re-run
  // every render.
  useToolNames: () => FALLBACK_TOOL_NAMES
}));

const WORKSHOP: Device = {
  id: "dev-workshop",
  name: "workshop",
  online: true,
  version: "0.3.42",
  last_seen_ms: 1
};

function tool(name: string, capability: string, destructive: boolean): LiveTool {
  return {
    name,
    description: "",
    inputSchema: { type: "object", properties: {} },
    source: "builtin",
    capability,
    destructive
  };
}

const TOOLS = [tool("files.read", "files", false), tool("terminal.write", "terminal", true)];

const ATTIC: Device = {
  id: "dev-attic",
  name: "attic",
  online: true,
  version: "0.3.42",
  last_seen_ms: 2
};

function tokenMeta(overrides: Partial<TokenMeta> = {}): TokenMeta {
  return {
    id: "tok-device",
    name: "laptop",
    device_id: WORKSHOP.id,
    tools: ["files.read"],
    allow_destructive: false,
    created_at_ms: Date.UTC(2026, 10, 1),
    ...overrides
  };
}

const CREATED: Created = {
  id: "tok-1",
  name: "laptop",
  device_id: WORKSHOP.id,
  tools: ["files.read", "terminal.write"],
  allow_destructive: false,
  created_at_ms: Date.UTC(2026, 10, 2),
  token: "srm_" + "a".repeat(64)
};

beforeEach(() => {
  mocks.tools.mockResolvedValue([{ name: "files.read" }, { name: "terminal.write" }]);
  mocks.liveTools.mockReturnValue(TOOLS);
  mocks.tokens.mockResolvedValue([]);
  mocks.createToken.mockReset();
  mocks.rotateToken.mockReset();
  mocks.revokeToken.mockReset();
  mocks.createToken.mockResolvedValue(CREATED);
});

function tokenForm(): HTMLElement {
  return document.querySelector(".token-create") as HTMLElement;
}

function draw(overrides: Record<string, unknown> = {}) {
  const onClose = vi.fn();
  const view = render(
    <ConnectSheet
      device={WORKSHOP}
      devices={[WORKSHOP]}
      initialSection="endpoint"
      onClose={onClose}
      {...overrides}
    />
  );
  return { ...view, onClose };
}

describe("ConnectSheet", () => {
  it("switches between the endpoint, the four clients and the tokens", () => {
    draw();

    expect(screen.getByRole("dialog", { name: "Connect a client" })).toBeInTheDocument();
    expect(
      screen.getByText(`http://localhost:3000/mcp/${WORKSHOP.id}`)
    ).toBeInTheDocument();
    expect(screen.getByText(/A bearer token is the credential/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("tab", { name: "Clients" }));
    const config = screen.getByText(/deepseek-ai\/dsh-mcp-client/);
    expect(config).toHaveTextContent("Bearer <token>");
    fireEvent.click(screen.getByRole("tab", { name: "Claude" }));
    expect(screen.getByText(/claude mcp add --transport http summrise/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("tab", { name: "Tokens" }));
    expect(screen.getByRole("heading", { name: "Create a token" })).toBeInTheDocument();
    expect(screen.getByText(/2 tools · 1 can change the machine/)).toBeInTheDocument();
    // One credential list, not a device list with a fleet list folded into a disclosure.
    expect(screen.queryByText("Fleet tokens")).not.toBeInTheDocument();
    expect(document.querySelector("details.fleet-token-disclosure")).toBeNull();
  });

  it("opens on Tokens when the account menu asked for it, with Tokens as the title", () => {
    draw({ initialSection: "tokens" });
    expect(screen.getByRole("dialog", { name: "Tokens" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Create a token" })).toBeInTheDocument();
  });

  it("creates a token, shows it once with the warning, and forgets it when the sheet closes", async () => {
    const first = draw();
    fireEvent.click(screen.getByRole("tab", { name: "Tokens" }));
    fireEvent.change(within(tokenForm()).getByLabelText("Name"), { target: { value: "laptop" } });
    fireEvent.click(screen.getByRole("button", { name: "Create token" }));

    await waitFor(() =>
      expect(mocks.createToken).toHaveBeenCalledWith({
        name: "laptop",
        device_id: WORKSHOP.id,
        tools: ["files.read", "terminal.write"],
        allow_destructive: false
      })
    );
    expect(await screen.findByText("Copy this token now. It is not shown again.")).toBeInTheDocument();
    expect(screen.getByText(CREATED.token)).toBeInTheDocument();

    first.unmount();
    draw();
    expect(document.body.textContent).not.toContain(CREATED.token);
    expect(screen.queryByText("Copy this token now. It is not shown again.")).not.toBeInTheDocument();
  });

  it("offers Read-only and Choose tools presets, and scopes the token to what is chosen", async () => {
    draw({ initialSection: "tokens" });
    fireEvent.change(within(tokenForm()).getByLabelText("Name"), { target: { value: "read" } });

    fireEvent.click(within(tokenForm()).getByRole("button", { name: "Read-only" }));
    expect(within(tokenForm()).getByText(/1 tool · 0 can change the machine/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Create token" }));
    await waitFor(() =>
      expect(mocks.createToken).toHaveBeenCalledWith(
        expect.objectContaining({ name: "read", tools: ["files.read"] })
      )
    );
  });

  it("opens the full scope grid only behind Choose tools", () => {
    draw({ initialSection: "tokens" });
    expect(within(tokenForm()).queryByRole("checkbox", { name: "terminal.write" })).not.toBeInTheDocument();
    fireEvent.click(within(tokenForm()).getByRole("button", { name: "Choose tools…" }));
    const grid = within(tokenForm()).getByRole("checkbox", { name: "terminal.write" });
    expect(grid).toBeChecked();
    fireEvent.click(grid);
    expect(grid).not.toBeChecked();
    expect(within(tokenForm()).getByText(/1 tool · 0 can change the machine/)).toBeInTheDocument();
  });

  it("shows the console's recovery sentence when the token is refused", async () => {
    mocks.createToken.mockRejectedValue(
      new HttpError(
        400,
        "unknown_tool",
        "the device does not advertise hello.greet; connect it and try again"
      )
    );
    draw({ initialSection: "tokens" });
    fireEvent.change(within(tokenForm()).getByLabelText("Name"), { target: { value: "plugin" } });
    fireEvent.click(screen.getByRole("button", { name: "Create token" }));

    expect(
      await screen.findByText("the device does not advertise hello.greet; connect it and try again")
    ).toBeInTheDocument();
  });

  it("rotates a token after naming the cost, then shows the new value once", async () => {
    mocks.tokens.mockResolvedValue([
      {
        id: "tok-1",
        name: "laptop",
        device_id: WORKSHOP.id,
        tools: ["files.read"],
        allow_destructive: false,
        created_at_ms: Date.UTC(2026, 10, 2)
      }
    ]);
    mocks.rotateToken.mockResolvedValue({ ...CREATED, token: "srm_" + "b".repeat(64) });
    draw({ initialSection: "tokens" });

    fireEvent.click(await screen.findByRole("button", { name: "Rotate" }));
    const dialog = await screen.findByRole("dialog", { name: 'Rotate "laptop"?' });
    expect(within(dialog).getByText(/stops working/)).toBeInTheDocument();
    expect(within(dialog).getByText(/shown once/)).toBeInTheDocument();
    expect(within(dialog).getByText(/its fixed device scope/)).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Rotate token" }));

    await waitFor(() => expect(mocks.rotateToken).toHaveBeenCalledWith("tok-1"));
    expect(await screen.findByText("Copy this token now. It is not shown again.")).toBeInTheDocument();
    expect(screen.getByText("srm_" + "b".repeat(64))).toBeInTheDocument();
  });

  it("revokes a token only after the confirmation names the consequence", async () => {
    mocks.tokens.mockResolvedValue([
      {
        id: "tok-1",
        name: "laptop",
        device_id: WORKSHOP.id,
        tools: ["files.read"],
        allow_destructive: false,
        created_at_ms: Date.UTC(2026, 10, 2)
      }
    ]);
    mocks.revokeToken.mockResolvedValue(undefined);
    draw({ initialSection: "tokens" });

    fireEvent.click(await screen.findByRole("button", { name: "Revoke" }));
    const dialog = await screen.findByRole("dialog", { name: 'Revoke "laptop"?' });
    expect(within(dialog).getByText(/cannot be recovered/)).toBeInTheDocument();
    expect(mocks.revokeToken).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole("button", { name: "Revoke token" }));

    await waitFor(() => expect(mocks.revokeToken).toHaveBeenCalledWith("tok-1"));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: 'Revoke "laptop"?' })).not.toBeInTheDocument());
  });

  it("keeps the fleet endpoint available when no machine is chosen", () => {
    draw({ device: null });
    expect(screen.getByText("http://localhost:3000/mcp/all")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "Tokens" }));
    expect(screen.getByText(/spans the fleet/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create token" })).toBeInTheDocument();
    // With no current machine the only scope the form can mint is the fleet one.
    expect(screen.getByRole("button", { name: "all machines" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "this machine" })).toBeDisabled();
  });

  it("dims the endpoint and disables Copy while the machine is offline", () => {
    draw({ device: { ...WORKSHOP, online: false } });
    const copy = screen.getByRole("button", { name: "Copy" });
    expect(copy).toBeDisabled();
    expect(screen.getByText(`http://localhost:3000/mcp/${WORKSHOP.id}`)).toHaveClass("dim");
  });

  it("drops the client block to a Copied sentence", async () => {
    const clipboard = { writeText: vi.fn().mockResolvedValue(undefined) };
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: clipboard });
    draw();
    fireEvent.click(screen.getByRole("tab", { name: "Clients" }));
    fireEvent.click(screen.getByRole("button", { name: "Copy configuration" }));
    expect(await screen.findByText("Copied. It is not shown again.")).toBeInTheDocument();
    expect(screen.queryByText(/deepseek-ai\/dsh-mcp-client/)).not.toBeInTheDocument();
    delete (navigator as { clipboard?: unknown }).clipboard;
  });
});

describe("ConnectSheet's one credential list", () => {
  it("shows a device token and a fleet token together, each with its reach, newest first", async () => {
    const deviceToken = tokenMeta();
    const fleetToken = tokenMeta({
      id: "tok-fleet",
      name: "dsh-fleet",
      device_id: "",
      device_ids: [WORKSHOP.id, ATTIC.id, "dev-a-third-machine"],
      tools: ["files.read", "terminal.write"],
      allow_destructive: true,
      created_at_ms: Date.UTC(2026, 10, 2)
    });
    // One registry carries both scopes; the list must not draw the same record twice and must
    // label each row by what the record can actually reach.
    mocks.tokens.mockResolvedValue([fleetToken, deviceToken]);

    draw({ initialSection: "tokens" });

    const deviceRow = await screen.findByRole("row", { name: /laptop/ });
    const fleetRow = screen.getByRole("row", { name: /dsh-fleet/ });
    expect(deviceRow).toHaveTextContent("this machine");
    expect(fleetRow).toHaveTextContent("workshop, dev-attic");
    expect(fleetRow).not.toHaveTextContent("all machines");
    const scopeChip = within(fleetRow).getByText(/workshop, dev-attic/);
    expect(scopeChip.getAttribute("title")).toContain("Covers 3 machines: workshop, dev-attic");
    expect(scopeChip.getAttribute("title")).toContain("enrolled after");
    expect(screen.getAllByRole("row", { name: /laptop|dsh-fleet/ })).toHaveLength(2);

    const dataRows = within(screen.getByRole("table")).getAllByRole("row").slice(1);
    expect(dataRows[0]).toHaveTextContent("dsh-fleet");
    expect(dataRows[1]).toHaveTextContent("laptop");
  });

  it("marks the token list as the container the narrow rule stacks", async () => {
    // The sheet is 600 px wide even at 1280, so the token table is measured against its scroller,
    // not the viewport. At <=639 px of that width the row becomes a card: name, scope and tools on
    // top, the actions on their own line under the name. The container is the anchor the CSS
    // queries, and the actions keep the class the rule targets.
    mocks.tokens.mockResolvedValue([tokenMeta()]);
    draw({ initialSection: "tokens" });

    const row = (await screen.findByRole("row", { name: /laptop/ })) as HTMLTableRowElement;
    const scroller = row.closest(".token-table-scroll");
    expect(scroller).not.toBeNull();
    expect(scroller).toHaveClass("table-scroll");
    const nameCell = row.cells[0];
    expect(nameCell.querySelector(".token-name")).not.toBeNull();
    const actions = row.querySelector("td.token-actions");
    expect(actions).not.toBeNull();
    expect(actions).toBe(row.cells[row.cells.length - 1]);
    expect(within(actions as HTMLElement).getByRole("button", { name: "Rotate" })).toBeInTheDocument();
    expect(within(actions as HTMLElement).getByRole("button", { name: "Revoke" })).toBeInTheDocument();
  });

  it("shows a token minted for another machine under that machine's name", async () => {
    mocks.tokens.mockResolvedValue([
      tokenMeta({ id: "tok-attic", name: "attic-ops", device_id: ATTIC.id })
    ]);

    draw({ initialSection: "tokens", devices: [WORKSHOP, ATTIC] });

    const row = await screen.findByRole("row", { name: /attic-ops/ });
    expect(row).toHaveTextContent("attic");
    expect(row).not.toHaveTextContent("this machine");
    // The registry is not empty, so the "none has been minted" sentence would be a lie.
    expect(screen.queryByText(/none has been minted yet/)).not.toBeInTheDocument();
  });

  it("lists every registry token when no machine is selected", async () => {
    mocks.tokens.mockResolvedValue([
      tokenMeta({ id: "tok-workshop", name: "workshop-token" }),
      tokenMeta({
        id: "tok-attic",
        name: "attic-token",
        device_id: ATTIC.id,
        created_at_ms: Date.UTC(2026, 10, 3)
      })
    ]);

    draw({ device: null, devices: [WORKSHOP, ATTIC], initialSection: "tokens" });

    expect(await screen.findByRole("row", { name: /workshop-token/ })).toBeInTheDocument();
    expect(screen.getByRole("row", { name: /attic-token/ })).toBeInTheDocument();
    expect(within(screen.getByRole("table")).getAllByRole("row")).toHaveLength(3);
    expect(screen.queryByText(/none has been minted yet/)).not.toBeInTheDocument();
  });

  it("mints against all machines with the fleet payload and shows the new row with that scope", async () => {
    mocks.createToken.mockResolvedValue({
      ...CREATED,
      id: "tok-fleet",
      name: "fleet",
      device_id: "",
      device_ids: [WORKSHOP.id, ATTIC.id],
      token: "srm_" + "c".repeat(64)
    });
    draw({ initialSection: "tokens", devices: [WORKSHOP, ATTIC] });

    fireEvent.click(within(tokenForm()).getByRole("button", { name: "all machines" }));
    fireEvent.change(within(tokenForm()).getByLabelText("Name"), { target: { value: "fleet" } });
    fireEvent.click(screen.getByRole("button", { name: "Create token" }));

    await waitFor(() =>
      expect(mocks.createToken).toHaveBeenCalledWith({
        name: "fleet",
        device_ids: [WORKSHOP.id, ATTIC.id],
        tools: ["files.read", "terminal.write"],
        allow_destructive: false
      })
    );
    const row = await screen.findByRole("row", { name: /fleet/ });
    expect(row).toHaveTextContent("workshop, attic");
    // The one-time value is still the reveal; the row is metadata and never the secret.
    expect(screen.getByText("srm_" + "c".repeat(64))).toBeInTheDocument();
    expect(row).not.toHaveTextContent("srm_");
  });

  it("revokes a fleet row through the token endpoint after naming the consequence", async () => {
    mocks.tokens.mockResolvedValue([
      tokenMeta({
        id: "tok-fleet",
        name: "dsh-fleet",
        device_id: "",
        device_ids: [WORKSHOP.id, ATTIC.id]
      })
    ]);
    mocks.revokeToken.mockResolvedValue(undefined);
    draw({ initialSection: "tokens" });

    const row = await screen.findByRole("row", { name: /dsh-fleet/ });
    fireEvent.click(within(row).getByRole("button", { name: "Revoke" }));
    const dialog = await screen.findByRole("dialog", { name: 'Revoke "dsh-fleet"?' });
    expect(within(dialog).getByText(/cannot be recovered/)).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Revoke token" }));

    await waitFor(() => expect(mocks.revokeToken).toHaveBeenCalledWith("tok-fleet"));
    expect(mocks.revokeToken).toHaveBeenCalledTimes(1);
  });

  it("says what a token is for only when the registry truly has none, and keeps the mint form visible", async () => {
    draw({ initialSection: "tokens" });

    expect(await screen.findByText(/none has been minted yet/)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Create a token" })).toBeInTheDocument();
    expect(screen.queryByRole("table")).not.toBeInTheDocument();
  });

  it("opens on the token a record named, scrolled to its row", async () => {
    mocks.tokens.mockResolvedValue([
      tokenMeta({ id: "tok-first", name: "first" }),
      tokenMeta({ id: "tok-wanted", name: "wanted", created_at_ms: Date.UTC(2026, 10, 2) })
    ]);
    const scrollIntoView = vi.fn();
    const original = HTMLElement.prototype.scrollIntoView;
    HTMLElement.prototype.scrollIntoView = scrollIntoView;

    try {
      draw({ initialSection: "tokens", focusTokenId: "tok-wanted" });

      const row = await screen.findByRole("row", { name: /wanted/ });
      expect(row).toHaveClass("is-managed");
      // `findByRole` waits for the row's render, not for the effect that scrolls to it: the effect
      // runs in a later tick, so asserting the call here raced it (a flake under parallel load).
      await waitFor(() => expect(scrollIntoView).toHaveBeenCalled());
    } finally {
      HTMLElement.prototype.scrollIntoView = original;
    }
  });

  it("defaults the mint scope to this machine and can be switched before minting", async () => {
    mocks.createToken.mockResolvedValue({ ...CREATED, name: "here" });
    draw({ initialSection: "tokens", devices: [WORKSHOP, ATTIC] });

    expect(within(tokenForm()).getByRole("button", { name: "this machine" })).toHaveAttribute(
      "aria-pressed",
      "true"
    );
    fireEvent.click(within(tokenForm()).getByRole("button", { name: "all machines" }));
    expect(within(tokenForm()).getByRole("button", { name: "all machines" })).toHaveAttribute(
      "aria-pressed",
      "true"
    );
    fireEvent.change(within(tokenForm()).getByLabelText("Name"), { target: { value: "here" } });
    fireEvent.click(screen.getByRole("button", { name: "Create token" }));

    await waitFor(() =>
      expect(mocks.createToken).toHaveBeenCalledWith(
        expect.objectContaining({ device_ids: [WORKSHOP.id, ATTIC.id] })
      )
    );
  });

  it("states the fixed device scope and says a different scope needs a new token", () => {
    draw({ initialSection: "tokens" });

    expect(
      screen.getByText(
        "A token's device scope is fixed when it is minted. Rotate keeps it; to cover different machines, mint a new token."
      )
    ).toBeInTheDocument();
  });

  it("names the machine the mint form will cover", () => {
    draw({ initialSection: "tokens" });

    expect(
      within(tokenForm()).getByText("This token will cover workshop.")
    ).toBeInTheDocument();
  });

  it("names every machine a fleet mint will cover, as the form opens on the fleet", () => {
    draw({ initialSection: "tokens", device: null, devices: [WORKSHOP, ATTIC] });

    expect(
      within(tokenForm()).getByText(
        "This token will cover the 2 machines enrolled now: workshop (online), attic (online)."
      )
    ).toBeInTheDocument();
  });

  it("marks an offline machine in the coverage sentence with its silence, and still covers it", async () => {
    // The measured bug: a machine 23 hours silent was listed exactly like the live ones. The age is
    // the fleet row's own evidence and formatter, and the clock is pinned so the sentence is exact.
    const now = Date.UTC(2026, 10, 6, 12, 0, 0);
    const clock = vi.spyOn(Date, "now").mockReturnValue(now);
    const asleep: Device = {
      id: "dev-attic",
      name: "attic",
      online: false,
      version: "0.3.42",
      // The measured DESKTOP-14RJCR8 shape: no frame, the silence ages from the newest open close.
      last_seen_ms: now - 2 * 3_600_000,
      last_left_ms: now - (1 * 60 + 49) * 60_000
    };
    const unseen: Device = {
      id: "dev-basement",
      name: "basement",
      online: false,
      version: "0.3.42",
      last_seen_ms: null
    };

    draw({ initialSection: "tokens", device: null, devices: [WORKSHOP, asleep, unseen] });

    expect(
      within(tokenForm()).getByText(
        "This token will cover the 3 machines enrolled now: workshop (online), " +
          "attic (offline, last seen 1h 49m ago), basement (offline)."
      )
    ).toBeInTheDocument();

    // THE STATE IS A STATEMENT, NOT A FILTER: the two offline machines are still in the minted
    // scope. The pinned clock is released before the async mint so `waitFor` reads real time.
    clock.mockRestore();
    fireEvent.change(within(tokenForm()).getByLabelText("Name"), { target: { value: "all" } });
    fireEvent.click(within(tokenForm()).getByRole("button", { name: "Create token" }));
    await waitFor(() =>
      expect(mocks.createToken).toHaveBeenCalledWith(
        expect.objectContaining({ device_ids: [WORKSHOP.id, asleep.id, unseen.id] })
      )
    );
  });
});

describe("ConnectSheet's token dates", () => {
  // Local instants, so the assertions do not depend on the machine's timezone.
  const CREATED_AT = new Date(2026, 10, 2, 17, 3, 9).getTime();
  const USED_AT = new Date(2026, 10, 4, 9, 41, 5).getTime();
  const EXPIRES_AT = new Date(2026, 11, 4, 9, 41, 5).getTime();

  it("dates created, used and expires in ISO, with the exact moment in the title", async () => {
    // The measured token sheet read `used 11/6/2026` / `created 11/4/2026` in en-US: the browser's
    // locale chose the order. The audit's absolute dates are ISO, and so are these.
    mocks.tokens.mockResolvedValue([
      tokenMeta({
        name: "dated",
        created_at_ms: CREATED_AT,
        last_used_ms: USED_AT,
        expires_at_ms: EXPIRES_AT
      })
    ]);

    draw({ initialSection: "tokens" });
    const row = await screen.findByRole("row", { name: /dated/ });
    const created = row.querySelector(".token-created time") as HTMLTimeElement;
    const used = row.querySelector(".token-name .hint time") as HTMLTimeElement;
    const expires = row.querySelector(".token-expires time") as HTMLTimeElement;

    expect(created).toHaveTextContent(/^2026-11-02$/);
    expect(used).toHaveTextContent(/^2026-11-04$/);
    expect(expires).toHaveTextContent(/^2026-12-04$/);
    // The exact time is the cell's title, with the audit's UTC instant beside it.
    expect(created).toHaveAttribute(
      "title",
      `2026-11-02 17:03:09 (${new Date(CREATED_AT).toISOString()})`
    );
    expect(used.getAttribute("title")).toContain("09:41:05");
    expect(expires.getAttribute("title")).toContain("09:41:05");
    expect(created).toHaveAttribute("dateTime", new Date(CREATED_AT).toISOString());
    // Not one locale's slash form anywhere on the row.
    expect(row.textContent).not.toMatch(/\d{1,2}\/\d{1,2}\/\d{4}/);
  });

  it("keeps never for a token with no expiry and never used for one no call has used", async () => {
    mocks.tokens.mockResolvedValue([
      tokenMeta({ name: "unused", created_at_ms: CREATED_AT })
    ]);

    draw({ initialSection: "tokens" });
    const row = await screen.findByRole("row", { name: /unused/ });
    const expires = row.querySelector(".token-expires") as HTMLElement;
    const hint = row.querySelector(".token-name .hint") as HTMLElement;

    expect(expires.querySelector("time")).toBeNull();
    expect(expires).toHaveTextContent("never");
    expect(hint).toHaveTextContent("never used");
    expect(hint.querySelector("time")).toBeNull();
  });
});
