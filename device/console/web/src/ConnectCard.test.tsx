import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ConnectCard } from "./ConnectCard";
import { TOOL_NAMES } from "./GrantForm";

// The card asks the client for the device's registry, and the card does not render the list it gets:
// the catalogue is visible only in the grant it mints. So `tools` answers the question and `grant` is
// the boundary the answer is read at; everything else in `./api` stays real (importOriginal).
const mocks = vi.hoisted(() => ({
  tools: vi.fn(),
  grant: vi.fn()
}));

vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return {
    ...actual,
    api: { ...actual.api, tools: mocks.tools, grant: mocks.grant }
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

function renderCard() {
  render(<ConnectCard device={WORKSHOP} onClose={() => {}} />);
}

/** Let a settled `/tools` request reach the effect and React flush the state it leaves behind. */
async function settle() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

function mint() {
  fireEvent.click(screen.getByRole("button", { name: "Mint a grant and show the config" }));
}

describe("ConnectCard", () => {
  beforeEach(() => {
    mocks.grant.mockResolvedValue(ENVELOPE);
    mocks.tools.mockResolvedValue(SERVED);
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
