import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { BrowserPane } from "./BrowserPane";
import { callTool, ToolError, type Grant } from "./terminal";

vi.mock("./terminal", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./terminal")>();
  return { ...actual, callTool: vi.fn() };
});

type Target = { id: string; title?: string; url?: string };

const A: Target = { id: "A", title: "A", url: "https://a.test" };
const B: Target = { id: "B", title: "B", url: "https://b.test" };

const callToolMock = vi.mocked(callTool);
const grant: Grant = { deviceId: "dev-1", envelope: {} };

// `browser.list` answers a sequence: the first entry is what the pane finds on mount, a later one is
// what a re-list finds after a tab closed. Tests that never re-list only need the first.
let listAnswers: Target[][] = [];
let listCalls = 0;

function answerFor(name: string, args: Record<string, unknown>): string {
  if (name === "browser.list") {
    const index = Math.min(listCalls, listAnswers.length - 1);
    listCalls += 1;
    return JSON.stringify(listAnswers[index] ?? [A]);
  }
  if (name === "browser.screenshot") return JSON.stringify({ data: "AAAA", bytes: 3 });
  if (name === "browser.navigate") return JSON.stringify({ target: args.target, url: args.url });
  return JSON.stringify({ stopped: true });
}

async function openPane() {
  const view = render(<BrowserPane grant={grant} />);
  await screen.findByText("1 target(s).");
  return view;
}

describe("BrowserPane", () => {
  beforeEach(() => {
    callToolMock.mockReset();
    listAnswers = [[A]];
    listCalls = 0;
    callToolMock.mockImplementation(async (_grant, name, args) => answerFor(name, args));
  });

  it("navigates the selected target to the address and captures the page", async () => {
    await openPane();
    fireEvent.change(screen.getByLabelText("Address"), { target: { value: "https://b.test" } });
    fireEvent.click(screen.getByRole("button", { name: "Navigate" }));

    await waitFor(() =>
      expect(callToolMock).toHaveBeenCalledWith(grant, "browser.navigate", {
        target: "A",
        url: "https://b.test"
      })
    );
    // The picture must be the page the operator just asked for, not the one from before the call.
    await waitFor(() =>
      expect(callToolMock).toHaveBeenCalledWith(grant, "browser.screenshot", { target: "A" })
    );
    expect(screen.getByText("Captured 3 bytes.")).toBeInTheDocument();
  });

  it("keeps Navigate disabled until there is both a target and an address", async () => {
    await openPane();
    const navigate = screen.getByRole("button", { name: "Navigate" });
    expect(navigate).toBeDisabled();

    fireEvent.change(screen.getByLabelText("Address"), { target: { value: "https://b.test" } });
    expect(navigate).toBeEnabled();
  });

  it("disables Navigate and Close tab when no target is listed", async () => {
    listAnswers = [[]];
    render(<BrowserPane grant={grant} />);
    // The initial status is the same sentence, so wait for the answer itself before asserting on it.
    await waitFor(() => expect(callToolMock).toHaveBeenCalledWith(grant, "browser.list", {}));
    await screen.findByText("No browser is open on this device.");

    fireEvent.change(screen.getByLabelText("Address"), { target: { value: "https://b.test" } });
    expect(screen.getByRole("button", { name: "Navigate" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Close tab" })).toBeDisabled();
  });

  it("closes the selected tab, re-lists, and follows the survivor", async () => {
    listAnswers = [[A], [B]];
    await openPane();
    fireEvent.click(screen.getByRole("button", { name: "Close tab" }));

    await waitFor(() =>
      expect(callToolMock).toHaveBeenCalledWith(grant, "browser.close", { target: "A" })
    );
    await waitFor(() => {
      const names = callToolMock.mock.calls.map(([, name]) => name);
      expect(names.filter((name) => name === "browser.list")).toHaveLength(2);
    });
    await waitFor(() =>
      expect(callToolMock).toHaveBeenCalledWith(grant, "browser.screenshot", { target: "B" })
    );
    expect(screen.getByLabelText("Target")).toHaveValue("B");
  });

  it("stops the browser and clears the targets, the picture and the selection", async () => {
    const { container } = await openPane();
    fireEvent.click(screen.getByRole("button", { name: "Refresh view" }));
    await waitFor(() => expect(container.querySelector(".browser-shot")).not.toBeNull());

    fireEvent.click(screen.getByRole("button", { name: "Stop browser" }));
    await waitFor(() =>
      expect(callToolMock).toHaveBeenCalledWith(grant, "browser.shutdown", {})
    );

    const stopped = await screen.findByText(/No browser is open on this device/);
    expect(stopped).toHaveTextContent(/browser is stopped/i);
    expect(container.querySelector(".browser-shot")).toBeNull();
    expect(screen.queryByLabelText("Target")).toBeNull();
  });

  it("clears the previous picture when a screenshot fails", async () => {
    const { container } = await openPane();
    fireEvent.click(screen.getByRole("button", { name: "Refresh view" }));
    await waitFor(() => expect(container.querySelector(".browser-shot")).not.toBeNull());

    callToolMock.mockImplementation(async (_grant, name, args) => {
      if (name === "browser.screenshot") throw new ToolError("the tab is gone", 200, null, true);
      return answerFor(name, args);
    });
    fireEvent.click(screen.getByRole("button", { name: "Refresh view" }));

    expect(await screen.findByText("Screenshot failed: the tab is gone")).toBeInTheDocument();
    expect(container.querySelector(".browser-shot")).toBeNull();
  });

  it("says a transport failure never reached the device", async () => {
    callToolMock.mockImplementation(async (_grant, name, args) => {
      if (name === "browser.navigate") throw new ToolError("Failed to fetch", 0);
      return answerFor(name, args);
    });
    await openPane();
    fireEvent.change(screen.getByLabelText("Address"), { target: { value: "https://b.test" } });
    fireEvent.click(screen.getByRole("button", { name: "Navigate" }));

    const failure = await screen.findByText(/Navigate failed: the console never reached the device/);
    expect(failure).toHaveTextContent("Failed to fetch");
  });

  it("shows the tool's own refusal under the action that failed", async () => {
    callToolMock.mockImplementation(async (_grant, name, args) => {
      if (name === "browser.navigate") throw new ToolError("the tab is gone", 200, null, true);
      return answerFor(name, args);
    });
    await openPane();
    fireEvent.change(screen.getByLabelText("Address"), { target: { value: "https://b.test" } });
    fireEvent.click(screen.getByRole("button", { name: "Navigate" }));

    const failure = await screen.findByText("Navigate failed: the tab is gone");
    expect(failure).toHaveClass("error");
  });
});
