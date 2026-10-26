import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ConnectCard } from "./ConnectCard";
import { restoreGlobal } from "./test/dom";
import { TOOL_NAMES } from "./useToolNames";

// The card asks the client for the device's registry, and the card does not render the list it gets
// for a session grant: the catalogue is visible only in the grant it mints. So `tools` answers the
// question and `grant` is the boundary the answer is read at; the three token endpoints live beside
// them because the permanent-token mode is the card's other half. Everything else in `./api` stays
// real (importOriginal).
const mocks = vi.hoisted(() => ({
  tools: vi.fn(),
  grant: vi.fn(),
  tokens: vi.fn(),
  createToken: vi.fn(),
  revokeToken: vi.fn()
}));

vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return {
    ...actual,
    api: {
      ...actual.api,
      tools: mocks.tools,
      grant: mocks.grant,
      tokens: mocks.tokens,
      createToken: mocks.createToken,
      revokeToken: mocks.revokeToken
    }
  };
});

const WORKSHOP = {
  id: "dev-workshop-0001",
  name: "workshop",
  online: true,
  version: "0.3.42",
  last_seen_ms: 1
};

// The catalogue the card is served during the catalogue cases: two names, so neither argument can be
// the 38-name fallback by accident.
const SERVED = [{ name: "terminal.create" }, { name: "screen.capture" }];

// What `api.grant` answers with: the grant facts and the console's signature over them, together.
const ENVELOPE = { grant: { jti: "grant-1" }, signature: "signature" };

// A token with the shape the plan fixes: `srm_` and 64 lowercase hex, 68 characters in all.
const TOKEN = "srm_" + "a".repeat(64);
const CREATED_AT = Date.UTC(2026, 9, 26, 12, 0, 0);

// What `POST /tokens` answers with: the metadata plus the one plaintext the console ever returns.
const CREATED = {
  id: "tok-1",
  name: "laptop-dsh",
  device_id: WORKSHOP.id,
  tools: ["files.read", "host.facts"],
  allow_destructive: false,
  created_at_ms: CREATED_AT,
  token: TOKEN
};

// The same token as `GET /tokens` lists it: no `token` key, because the list is metadata only.
const META = {
  id: "tok-1",
  name: "laptop-dsh",
  device_id: WORKSHOP.id,
  tools: ["files.read", "host.facts"],
  allow_destructive: false,
  created_at_ms: CREATED_AT
};

const OTHER = {
  id: "tok-2",
  name: "attic-dsh",
  device_id: "dev-attic-0002",
  tools: ["host.facts"],
  allow_destructive: false,
  created_at_ms: CREATED_AT
};

/// jsdom has no clipboard; a copy test installs one and this restores the absence afterwards.
const originals = { clipboard: Object.getOwnPropertyDescriptor(navigator, "clipboard") };

function renderCard() {
  render(<ConnectCard device={WORKSHOP} onClose={() => {}} />);
}

/** Let a settled request reach the effect and React flush the state it leaves behind. */
async function settle() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

/** Render the card and switch to the permanent-token mode, with the device's token list answered. */
async function openTokens() {
  renderCard();
  await settle();
  fireEvent.click(screen.getByRole("tab", { name: "Permanent token" }));
  await settle();
}

function mint() {
  fireEvent.click(screen.getByRole("button", { name: "Mint a grant and show the config" }));
}

function typeName(name: string) {
  fireEvent.change(screen.getByLabelText("Token name"), { target: { value: name } });
}

function createToken() {
  fireEvent.click(screen.getByRole("button", { name: "Create token" }));
}

describe("ConnectCard", () => {
  beforeEach(() => {
    mocks.grant.mockResolvedValue(ENVELOPE);
    mocks.tools.mockResolvedValue(SERVED);
    mocks.tokens.mockResolvedValue([]);
    mocks.createToken.mockResolvedValue(CREATED);
    mocks.revokeToken.mockResolvedValue(undefined);
  });

  afterEach(() => {
    restoreGlobal(navigator, "clipboard", originals.clipboard);
  });

  it("mints the served catalogue once the client answers, in its order", async () => {
    renderCard();
    await settle();

    mint();

    await waitFor(() => expect(mocks.grant).toHaveBeenCalledTimes(1));
    expect(mocks.grant).toHaveBeenCalledWith(
      WORKSHOP.id,
      ["terminal.create", "screen.capture"],
      3600,
      false
    );
  });

  it("mints the 38-name fallback while the catalogue has not answered", async () => {
    mocks.tools.mockImplementation(() => new Promise(() => {}));
    renderCard();

    mint();

    await waitFor(() => expect(mocks.grant).toHaveBeenCalledTimes(1));
    expect(mocks.grant).toHaveBeenCalledWith(WORKSHOP.id, TOOL_NAMES, 3600, false);
  });

  it("keeps the fallback usable when the catalogue request fails", async () => {
    mocks.tools.mockRejectedValue(new Error("offline"));
    renderCard();
    await settle();

    mint();

    await waitFor(() => expect(mocks.grant).toHaveBeenCalledTimes(1));
    expect(mocks.grant).toHaveBeenCalledWith(WORKSHOP.id, TOOL_NAMES, 3600, false);
  });

  it("keeps the fallback when the catalogue answers with no tools", async () => {
    mocks.tools.mockResolvedValue([]);
    renderCard();
    await settle();

    mint();

    await waitFor(() => expect(mocks.grant).toHaveBeenCalledTimes(1));
    expect(mocks.grant).toHaveBeenCalledWith(WORKSHOP.id, TOOL_NAMES, 3600, false);
  });

  it("shows the endpoint and the client config once the grant is minted", async () => {
    renderCard();
    await settle();

    mint();

    expect(await screen.findByText(/URL: .*\/mcp\/dev-workshop-0001/)).toBeInTheDocument();
    expect(screen.getByText("DeepSeek Harness configuration")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Copy endpoint" })).toBeInTheDocument();
  });
});

describe("ConnectCard's permanent-token mode", () => {
  beforeEach(() => {
    mocks.grant.mockResolvedValue(ENVELOPE);
    mocks.tools.mockResolvedValue(SERVED);
    mocks.tokens.mockResolvedValue([]);
    mocks.createToken.mockResolvedValue(CREATED);
    mocks.revokeToken.mockResolvedValue(undefined);
  });

  afterEach(() => {
    restoreGlobal(navigator, "clipboard", originals.clipboard);
  });

  it("offers two modes and starts on the session grant the card has always been", async () => {
    renderCard();
    expect(screen.getByRole("tab", { name: "Session grant" })).toHaveAttribute(
      "aria-selected",
      "true"
    );
    expect(screen.getByRole("button", { name: "Mint a grant and show the config" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Create token" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("tab", { name: "Permanent token" }));

    expect(screen.getByRole("tab", { name: "Permanent token" })).toHaveAttribute(
      "aria-selected",
      "true"
    );
    expect(await screen.findByRole("button", { name: "Create token" })).toBeDisabled();
    await waitFor(() => expect(mocks.tokens).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole("button", { name: "Mint a grant and show the config" })).not.toBeInTheDocument();
  });

  it("creates with the trimmed name, chosen tools and consent, then shows the token once", async () => {
    await openTokens();

    typeName("  laptop-dsh  ");
    fireEvent.click(screen.getByRole("button", { name: "None" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "screen.capture" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Allow destructive tools" }));
    createToken();

    await waitFor(() =>
      expect(mocks.createToken).toHaveBeenCalledWith({
        name: "laptop-dsh",
        device_id: WORKSHOP.id,
        tools: ["screen.capture"],
        allow_destructive: true
      })
    );
    // The token is shown in the panel that only the create response fills, and again in the config
    // a client pastes; the list is asked again so the new token appears there without its value.
    expect((await screen.findAllByText(TOKEN)).length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText(new RegExp(`Authorization: Bearer ${TOKEN}`))).toBeInTheDocument();
    expect(screen.getByText(/shown once/)).toBeInTheDocument();
    expect(screen.getByText(/password/)).toBeInTheDocument();
    await waitFor(() => expect(mocks.tokens).toHaveBeenCalledTimes(2));
  });

  it("copies the token and the config the client pastes", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    await openTokens();
    typeName("laptop-dsh");
    createToken();
    await screen.findByText(TOKEN);

    fireEvent.click(screen.getByRole("button", { name: "Copy token" }));
    expect(writeText).toHaveBeenCalledWith(TOKEN);
    expect(await screen.findByText("Copied")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Copy client config" }));
    expect(writeText).toHaveBeenLastCalledWith(expect.stringContaining(`Authorization: Bearer ${TOKEN}`));
  });

  it("lists only this device's tokens as metadata, never the token value", async () => {
    mocks.tokens.mockResolvedValue([META, OTHER]);
    await openTokens();

    expect(await screen.findByText("laptop-dsh")).toBeInTheDocument();
    expect(screen.getByText(/2 tools/)).toBeInTheDocument();
    expect(screen.getByText(new Date(CREATED_AT).toLocaleDateString())).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Revoke" })).toBeInTheDocument();
    // The other device's token is not this card's business, and no `srm_` value is in the list.
    expect(screen.queryByText("attic-dsh")).not.toBeInTheDocument();
    expect(screen.queryByText(/srm_/)).not.toBeInTheDocument();
  });

  it("says what the empty list means until one is created for this device", async () => {
    await openTokens();

    const list = screen.getByRole("region", { name: "Existing tokens" });
    expect(
      await within(list).findByText(/No permanent tokens for this device/)
    ).toBeInTheDocument();
    expect(within(list).getByText(/fixed MCP configuration/)).toBeInTheDocument();
  });

  it("asks before revoking, sends the delete, then re-lists without the token", async () => {
    mocks.tokens.mockResolvedValueOnce([META]).mockResolvedValueOnce([]);
    await openTokens();
    await screen.findByText("laptop-dsh");

    fireEvent.click(screen.getByRole("button", { name: "Revoke" }));

    const dialog = await screen.findByRole("dialog", { name: /Revoke "laptop-dsh"/ });
    expect(mocks.revokeToken).not.toHaveBeenCalled();
    fireEvent.click(within(dialog).getByRole("button", { name: "Revoke token" }));

    await waitFor(() => expect(mocks.revokeToken).toHaveBeenCalledWith(META.id));
    await waitFor(() => expect(mocks.tokens).toHaveBeenCalledTimes(2));
    expect(await screen.findByText(/No permanent tokens for this device/)).toBeInTheDocument();
    expect(screen.queryByText("laptop-dsh")).not.toBeInTheDocument();
  });

  it("shows the console's sentence when a create is refused", async () => {
    mocks.createToken.mockRejectedValue(new Error("unknown_tool"));
    await openTokens();
    typeName("laptop-dsh");
    createToken();

    expect(await screen.findByText("unknown_tool")).toBeInTheDocument();
    expect(screen.queryByText(/srm_/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Create token" })).not.toBeDisabled();
  });

  it("still mints the session grant after the token mode was shown", async () => {
    await openTokens();
    fireEvent.click(screen.getByRole("tab", { name: "Session grant" }));

    mint();

    await waitFor(() => expect(mocks.grant).toHaveBeenCalledTimes(1));
    expect(mocks.grant).toHaveBeenCalledWith(
      WORKSHOP.id,
      ["terminal.create", "screen.capture"],
      3600,
      false
    );
    expect(mocks.createToken).not.toHaveBeenCalled();
  });
});
