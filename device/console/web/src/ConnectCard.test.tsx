import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ConnectCard } from "./ConnectCard";
import type { TokenMeta } from "./api";
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
      grant: mocks.grant,
      tokens: mocks.tokens,
      createToken: mocks.createToken,
      rotateToken: mocks.rotateToken,
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
// A day after the create, so the last-used sentence cannot be mistaken for the created date.
const LAST_USED_AT = Date.UTC(2026, 9, 27, 12, 0, 0);
// A year after the create, so the expiry sentence cannot be mistaken for either date above.
const EXPIRES_AT = Date.UTC(2027, 9, 26, 12, 0, 0);

// The same token as `GET /tokens` lists it: no `token` key, because the list is metadata only.
const META = {
  id: "tok-1",
  name: "laptop-dsh",
  device_id: WORKSHOP.id,
  tools: ["files.read", "host.facts"],
  allow_destructive: false,
  created_at_ms: CREATED_AT
};

// What `POST /tokens` answers with: the metadata plus the one plaintext the console ever returns.
const CREATED = { ...META, token: TOKEN };

// What `POST /tokens/<id>/rotate` answers with: the create shape, for a record that kept the old scope
// and took a new id, secret and created date.
const ROTATED_TOKEN = "srm_" + "b".repeat(64);
const ROTATED_AT = Date.UTC(2026, 9, 27, 9, 0, 0);
const ROTATED = { ...META, id: "tok-9", created_at_ms: ROTATED_AT, token: ROTATED_TOKEN };

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

/** Pick an Expires option by the label the operator reads, and return the seconds it carries. */
function chooseExpiry(label: string): string {
  const select = screen.getByLabelText("Expires") as HTMLSelectElement;
  const option = within(select).getByRole("option", { name: label }) as HTMLOptionElement;
  fireEvent.change(select, { target: { value: option.value } });
  return option.value;
}

function createToken() {
  fireEvent.click(screen.getByRole("button", { name: "Create token" }));
}

beforeEach(() => {
  mocks.grant.mockResolvedValue(ENVELOPE);
  mocks.tools.mockResolvedValue(SERVED);
  mocks.tokens.mockResolvedValue([]);
  mocks.createToken.mockResolvedValue(CREATED);
  mocks.rotateToken.mockResolvedValue(ROTATED);
  mocks.revokeToken.mockResolvedValue(undefined);
});

afterEach(() => {
  restoreGlobal(navigator, "clipboard", originals.clipboard);
});

describe("ConnectCard", () => {
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

  it("offers Never as the default and the lifetimes the server accepts", async () => {
    await openTokens();

    const select = screen.getByLabelText("Expires") as HTMLSelectElement;
    expect(select.value).toBe("0");
    expect(within(select).getAllByRole("option").map((option) => option.textContent)).toEqual([
      "Never",
      "30 days",
      "90 days",
      "365 days"
    ]);
  });

  it("creates a token that never expires by default, omitting the lifetime", async () => {
    await openTokens();

    typeName("laptop-dsh");
    expect(chooseExpiry("Never")).toBe("0");
    createToken();

    await waitFor(() =>
      expect(mocks.createToken).toHaveBeenCalledWith({
        name: "laptop-dsh",
        device_id: WORKSHOP.id,
        tools: ["terminal.create", "screen.capture"],
        allow_destructive: false
      })
    );
  });

  it("sends thirty days as the seconds the server measures", async () => {
    await openTokens();

    typeName("laptop-dsh");
    expect(chooseExpiry("30 days")).toBe(String(30 * 86400));
    createToken();

    await waitFor(() =>
      expect(mocks.createToken).toHaveBeenCalledWith({
        name: "laptop-dsh",
        device_id: WORKSHOP.id,
        tools: ["terminal.create", "screen.capture"],
        allow_destructive: false,
        expires_in_s: 30 * 86400
      })
    );
  });

  it("sends a year as the seconds at the server's upper bound", async () => {
    await openTokens();

    typeName("laptop-dsh");
    expect(chooseExpiry("365 days")).toBe(String(365 * 86400));
    createToken();

    await waitFor(() =>
      expect(mocks.createToken).toHaveBeenCalledWith({
        name: "laptop-dsh",
        device_id: WORKSHOP.id,
        tools: ["terminal.create", "screen.capture"],
        allow_destructive: false,
        expires_in_s: 365 * 86400
      })
    );
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
    expect(screen.getByText(/files\.read, host\.facts/)).toBeInTheDocument();
    expect(screen.getByText(new Date(CREATED_AT).toLocaleDateString())).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Revoke" })).toBeInTheDocument();
    // The other device's token is not this card's business, and no `srm_` value is in the list.
    expect(screen.queryByText("attic-dsh")).not.toBeInTheDocument();
    expect(screen.queryByText(/srm_/)).not.toBeInTheDocument();
  });

  it("says when each token was last used, and says never used when it was not", async () => {
    mocks.tokens.mockResolvedValue([
      { ...META, last_used_ms: LAST_USED_AT },
      { ...META, id: "tok-2", name: "spare-dsh" }
    ]);
    await openTokens();

    expect(
      await screen.findByText(`last used ${new Date(LAST_USED_AT).toLocaleDateString()}`)
    ).toBeInTheDocument();
    expect(await screen.findByText("never used")).toBeInTheDocument();
    // The facts the row already carried are still there beside the new one.
    const used = screen.getByText("laptop-dsh").closest("li");
    expect(used).not.toBeNull();
    const row = within(used as HTMLElement);
    expect(row.getByText(new Date(CREATED_AT).toLocaleDateString())).toBeInTheDocument();
    expect(row.getByText(/files\.read, host\.facts/)).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Revoke" })).toHaveLength(2);
  });

  it("shows when a token expires, and never expires for the ones without a date", async () => {
    mocks.tokens.mockResolvedValue([
      { ...META, expires_at_ms: EXPIRES_AT },
      { ...META, id: "tok-2", name: "spare-dsh" },
      { ...META, id: "tok-3", name: "legacy-dsh", expires_at_ms: null }
    ]);
    await openTokens();

    const expiry = `expires ${new Date(EXPIRES_AT).toLocaleDateString()}`;
    expect(await screen.findByText(expiry)).toBeInTheDocument();
    // An absent field and an explicit null are the same permanent default: never expires.
    expect(screen.getAllByText("never expires")).toHaveLength(2);
    // The facts the row already carried are still beside the new one.
    const row = within(screen.getByText("laptop-dsh").closest("li") as HTMLElement);
    expect(row.getByText(expiry)).toBeInTheDocument();
    expect(row.getByText(/files\.read, host\.facts/)).toBeInTheDocument();
    expect(row.getByText(new Date(CREATED_AT).toLocaleDateString())).toBeInTheDocument();
    expect(row.getByText("never used")).toBeInTheDocument();
  });

  it("re-reads the list after a create, so a use the console recorded appears", async () => {
    // One implementation rather than a queue: a failing assertion must not leave a queued answer
    // that the next test inherits.
    let listed: TokenMeta[] = [META];
    mocks.tokens.mockImplementation(async () => listed);
    await openTokens();
    expect(await screen.findByText("never used")).toBeInTheDocument();

    listed = [{ ...META, last_used_ms: LAST_USED_AT }];
    typeName("laptop-dsh");
    createToken();

    expect(
      await screen.findByText(`last used ${new Date(LAST_USED_AT).toLocaleDateString()}`)
    ).toBeInTheDocument();
    expect(screen.queryByText("never used")).not.toBeInTheDocument();
    await waitFor(() => expect(mocks.tokens).toHaveBeenCalledTimes(2));
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

  it("rotates only after a confirmation that names the cost, then shows the new value once and re-lists", async () => {
    let listed: TokenMeta[] = [META];
    mocks.tokens.mockImplementation(async () => listed);
    await openTokens();
    expect(await screen.findByText(new Date(CREATED_AT).toLocaleDateString())).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Rotate" }));

    const dialog = await screen.findByRole("dialog", { name: 'Rotate "laptop-dsh"?' });
    // Nothing has happened yet: the confirm is the only thing that sends the request.
    expect(mocks.rotateToken).not.toHaveBeenCalled();
    expect(within(dialog).getByText(/stops working/)).toBeInTheDocument();
    expect(within(dialog).getByText(/shown once/)).toBeInTheDocument();

    // The console answers with the new record before the list is read again, as a create does.
    listed = [{ ...META, id: ROTATED.id, created_at_ms: ROTATED_AT }];
    fireEvent.click(within(dialog).getByRole("button", { name: "Rotate token" }));

    await waitFor(() => expect(mocks.rotateToken).toHaveBeenCalledWith(META.id));
    // The one-time view: the value and the config a client pastes, shown here and nowhere else.
    expect((await screen.findAllByText(ROTATED_TOKEN)).length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText(new RegExp(`Authorization: Bearer ${ROTATED_TOKEN}`))).toBeInTheDocument();
    expect(screen.getByText(/shown once/)).toBeInTheDocument();
    // The list was re-read: the same name now carries the new record, and the old date is gone.
    expect(await screen.findByText(new Date(ROTATED_AT).toLocaleDateString())).toBeInTheDocument();
    expect(screen.queryByText(new Date(CREATED_AT).toLocaleDateString())).not.toBeInTheDocument();
    await waitFor(() => expect(mocks.tokens).toHaveBeenCalledTimes(2));
    const list = within(screen.getByRole("region", { name: "Existing tokens" }));
    expect(list.queryByText(/srm_/)).not.toBeInTheDocument();
  });

  it("names the expiry a rotation keeps when the token has one", async () => {
    mocks.tokens.mockResolvedValue([{ ...META, expires_at_ms: EXPIRES_AT }]);
    await openTokens();
    await screen.findByText("laptop-dsh");

    fireEvent.click(screen.getByRole("button", { name: "Rotate" }));

    const dialog = await screen.findByRole("dialog", { name: 'Rotate "laptop-dsh"?' });
    expect(within(dialog).getByText(/keeps its expiry/)).toBeInTheDocument();
  });

  it("sends nothing when a rotation is cancelled", async () => {
    mocks.tokens.mockResolvedValue([META]);
    await openTokens();
    await screen.findByText("laptop-dsh");

    fireEvent.click(screen.getByRole("button", { name: "Rotate" }));
    const dialog = await screen.findByRole("dialog", { name: 'Rotate "laptop-dsh"?' });
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));

    expect(screen.queryByRole("dialog", { name: 'Rotate "laptop-dsh"?' })).not.toBeInTheDocument();
    expect(mocks.rotateToken).not.toHaveBeenCalled();
    expect(mocks.tokens).toHaveBeenCalledTimes(1);
    // The record the operator was looking at is untouched, and no value was ever shown.
    expect(screen.getByText("laptop-dsh")).toBeInTheDocument();
    expect(screen.queryByText(/srm_/)).not.toBeInTheDocument();
  });

  it("shows the console's sentence when a rotation is refused, and keeps the old record listed", async () => {
    mocks.rotateToken.mockRejectedValue(new Error("no_such_token"));
    mocks.tokens.mockResolvedValue([META]);
    await openTokens();
    await screen.findByText("laptop-dsh");

    fireEvent.click(screen.getByRole("button", { name: "Rotate" }));
    const dialog = await screen.findByRole("dialog", { name: 'Rotate "laptop-dsh"?' });
    fireEvent.click(within(dialog).getByRole("button", { name: "Rotate token" }));

    expect(await within(dialog).findByText("no_such_token")).toBeInTheDocument();
    expect(screen.queryByText(/srm_/)).not.toBeInTheDocument();
    // Nothing was replaced, so nothing is re-read and the list still holds the token that was there.
    expect(mocks.tokens).toHaveBeenCalledTimes(1);
    expect(screen.getByText("laptop-dsh")).toBeInTheDocument();
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
