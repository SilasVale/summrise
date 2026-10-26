import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { GrantForm, TOOL_NAMES } from "./GrantForm";

// The form is the console's only mint, and the HTTP client behind it has its own test. So `grant` is
// a mock and the token is a constant here; everything else in `./api` stays real (importOriginal),
// and `TOOL_NAMES` is the component's own export — the fallback list is one of the subjects.
const mocks = vi.hoisted(() => ({
  grant: vi.fn(),
  getToken: vi.fn()
}));

vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return {
    ...actual,
    api: { ...actual.api, grant: mocks.grant },
    getToken: mocks.getToken
  };
});

/// The tools the console's `/tools` route serves in these tests: two names, so a count of 2 or 1 is
/// impossible to confuse with the 38-name fallback.
const SERVED = [{ name: "terminal.create" }, { name: "screen.capture" }];
/// What `api.grant` answers with: the grant facts and the console's signature over them, together.
const ENVELOPE = { grant: { jti: "grant-1" }, signature: "signature" };

function renderForm() {
  const onGranted = vi.fn();
  render(<GrantForm deviceId="dev-1" onGranted={onGranted} />);
  return { onGranted };
}

/// Answer `/tools` with a catalogue. The mount effect runs on render, so this is called first.
function serveTools(tools: { name: string }[] = SERVED) {
  const fetchMock = vi.fn(async () => ({ ok: true, json: async () => tools }));
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

/// Leave `/tools` unanswered for the whole test: the form has only the fallback to offer.
function leaveToolsUnanswered() {
  vi.stubGlobal("fetch", vi.fn(() => new Promise(() => {})));
}

/// Let a rejected `/tools` request reach the effect's catch and React flush the (unchanged) form.
async function settle() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

/// The tool checkboxes, found through the fieldset's legend rather than a class name: the destructive
/// consent is a checkbox too, and it is not a tool.
function toolCheckboxes() {
  const fieldset = screen.getByRole("group", { name: /^Tools \(\d+ of \d+\)/ });
  return within(fieldset).getAllByRole("checkbox");
}

/// The tool names the form actually offers, read from each checkbox's label.
function toolNames() {
  return toolCheckboxes().map((box) => box.closest("label")?.textContent);
}

describe("GrantForm", () => {
  beforeEach(() => {
    mocks.getToken.mockReturnValue("token");
    mocks.grant.mockResolvedValue(ENVELOPE);
  });

  afterEach(() => vi.unstubAllGlobals());

  it("offers the 38-name fallback, all selected, before a catalogue answer", () => {
    leaveToolsUnanswered();
    renderForm();

    expect(TOOL_NAMES).toHaveLength(38);
    expect(toolNames()).toEqual(TOOL_NAMES);
    toolCheckboxes().forEach((box) => expect(box).toBeChecked());
    expect(screen.getByText("Tools (38 of 38)")).toBeInTheDocument();
  });

  it("takes the served catalogue over the fallback, all selected, with the token on the request", async () => {
    const fetchMock = serveTools();
    renderForm();

    expect(await screen.findByText("Tools (2 of 2)")).toBeInTheDocument();
    expect(toolNames()).toEqual(["terminal.create", "screen.capture"]);
    toolCheckboxes().forEach((box) => expect(box).toBeChecked());
    expect(screen.queryByRole("checkbox", { name: "files.read" })).not.toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledWith("/tools", {
      headers: { authorization: "Bearer token" }
    });
  });

  it("keeps the fallback usable when the catalogue request fails", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(() => Promise.reject(new Error("offline")))
    );
    renderForm();
    await settle();

    expect(toolNames()).toEqual(TOOL_NAMES);
    toolCheckboxes().forEach((box) => expect(box).toBeChecked());
    expect(screen.getByText("Tools (38 of 38)")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("checkbox", { name: "host.facts" }));
    expect(screen.getByText("Tools (37 of 38)")).toBeInTheDocument();
  });

  it("refuses to mint nothing, with its own sentence and no call to the device", async () => {
    serveTools();
    renderForm();
    await screen.findByText("Tools (2 of 2)");

    fireEvent.click(screen.getByRole("button", { name: "None" }));
    expect(screen.getByText("Tools (0 of 2)")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Grant access" }));

    expect(screen.getByText("Pick at least one tool.")).toBeInTheDocument();
    expect(mocks.grant).not.toHaveBeenCalled();
  });

  it("counts what is selected and All puts everything back", async () => {
    serveTools();
    renderForm();
    await screen.findByText("Tools (2 of 2)");

    const unticked = screen.getByRole("checkbox", { name: "screen.capture" });
    fireEvent.click(unticked);
    expect(unticked).not.toBeChecked();
    expect(screen.getByText("Tools (1 of 2)")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "All" }));
    expect(screen.getByText("Tools (2 of 2)")).toBeInTheDocument();
    toolCheckboxes().forEach((box) => expect(box).toBeChecked());
  });

  it("mints exactly the chosen names, lifetime and destructive flag, and reports the envelope", async () => {
    serveTools();
    const { onGranted } = renderForm();
    await screen.findByText("Tools (2 of 2)");

    fireEvent.click(screen.getByRole("checkbox", { name: "screen.capture" }));
    fireEvent.change(screen.getByLabelText("Lifetime (seconds)"), { target: { value: "60" } });
    fireEvent.click(screen.getByRole("checkbox", { name: "Allow destructive tools" }));
    fireEvent.click(screen.getByRole("button", { name: "Grant access" }));

    await waitFor(() => expect(onGranted).toHaveBeenCalledWith(ENVELOPE, ["terminal.create"]));
    expect(mocks.grant).toHaveBeenCalledTimes(1);
    expect(mocks.grant).toHaveBeenCalledWith("dev-1", ["terminal.create"], 60, false);
  });

  it("shows a refused grant's sentence and releases the button from Signing…", async () => {
    serveTools();
    let refuse!: (cause: unknown) => void;
    mocks.grant.mockImplementation(
      () =>
        new Promise<never>((_resolve, reject) => {
          refuse = reject;
        })
    );
    const { onGranted } = renderForm();
    await screen.findByText("Tools (2 of 2)");

    fireEvent.click(screen.getByRole("button", { name: "Grant access" }));
    expect(screen.getByRole("button", { name: "Signing…" })).toBeDisabled();

    await act(async () => {
      refuse(new Error("grant refused: unknown tool"));
      await new Promise((resolve) => setTimeout(resolve, 0));
    });

    expect(screen.getByText("grant refused: unknown tool")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Grant access" })).toBeEnabled();
    expect(onGranted).not.toHaveBeenCalled();
  });

  it("states the policy when destructive consent is unticked, and hides it when ticked", async () => {
    serveTools();
    renderForm();
    // Wait for /tools to land, so the assertions run against the settled form rather than the
    // fallback list.
    expect(await screen.findByText(/Tools \(2 of 2\)/)).toBeInTheDocument();

    const consent = screen.getByRole("checkbox", { name: "Allow destructive tools" });
    expect(consent).toBeChecked();
    expect(screen.queryByText(/device refuses a destructive call/)).not.toBeInTheDocument();

    fireEvent.click(consent);
    expect(consent).not.toBeChecked();

    const hint = screen.getByText(/device refuses a destructive call/);
    expect(hint).toHaveTextContent("Its refusal asks for the grant to be minted again with this ticked");
    expect(hint).toHaveTextContent("the console's audit keeps the refused exchange");
    expect(hint).toHaveTextContent("the device's error code");
    expect(hint).toHaveTextContent("cannot approve one call at a time");

    fireEvent.click(consent);
    expect(consent).toBeChecked();
    expect(screen.queryByText(/device refuses a destructive call/)).not.toBeInTheDocument();
  });
});
