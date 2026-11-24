import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
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
  await screen.findByText("1 page.");
  return view;
}

const shotCalls = () =>
  callToolMock.mock.calls.filter(([, name]) => name === "browser.screenshot");

/** Advance fake time far enough for a tick to start and settle; live polls every 1.5 s. */
async function tick(ms = 1_500) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

async function startLive() {
  fireEvent.click(screen.getByRole("button", { name: "Start live view" }));
  // Let React install the interval before time moves; otherwise the first tick can be counted from
  // the moment the test advanced past it.
  await tick(0);
}

describe("BrowserPane", () => {
  beforeEach(() => {
    callToolMock.mockReset();
    listAnswers = [[A]];
    listCalls = 0;
    callToolMock.mockImplementation(async (_grant, name, args) => answerFor(name, args));
  });

  describe("operation context", () => {
    type Operation = "open" | "navigate" | "close" | "stop" | "click" | "type" | "capture";
    type Change = "device" | "grant" | "offline" | "ABA";
    let serial = 0;
    function deferred() {
      let resolve!: (value: string) => void;
      let reject!: (reason: unknown) => void;
      const promise = new Promise<string>((yes, no) => { resolve = yes; reject = no; });
      return { promise, resolve, reject };
    }
    async function prepare(operation: Operation) {
      const id = `operation-context-${++serial}`;
      const firstGrant: Grant = { deviceId: id + "-A", envelope: {} };
      const nextGrant: Grant = { deviceId: id + "-B", envelope: {} };
      const rotatedGrant: Grant = { ...firstGrant, envelope: { rotated: true } };
      const pending = deferred();
      const names: Record<Operation, string> = {
        open: "browser.open", navigate: "browser.navigate", close: "browser.close",
        stop: "browser.shutdown", click: "browser.eval", type: "browser.eval", capture: "browser.screenshot"
      };
      callToolMock.mockImplementation(async (authority, name) => {
        if (authority === firstGrant && name === names[operation]) return pending.promise;
        if (name === "browser.list") return JSON.stringify(authority === firstGrant ? [A] : [B]);
        if (name === "browser.screenshot") return JSON.stringify({ data: authority === firstGrant ? "QUFB" : "QkJC", bytes: 3 });
        return JSON.stringify({ value: "CURRENT_RESULT", stopped: true });
      });
      const props = { grant: firstGrant, deviceId: id + "-A" };
      const view = render(<BrowserPane {...props} />);
      await waitFor(() => expect(screen.getByLabelText("Page")).toHaveValue("A"));
      if (operation === "click" || operation === "type") {
        fireEvent.click(screen.getByRole("button", { name: "Refresh view" }));
        await screen.findByRole("img");
      }
      if (operation === "open" || operation === "navigate") {
        fireEvent.change(screen.getByLabelText("Address"), { target: { value: "https://action.test" } });
      }
      if (operation === "type") {
        fireEvent.change(screen.getByLabelText("Text to type into the focused field"), { target: { value: "old draft" } });
      }
      if (operation === "click") fireEvent.click(screen.getByRole("img"));
      else {
        const buttons: Record<Exclude<Operation, "click">, string> = {
          open: "Open new tab", navigate: "Navigate", close: "Close tab", stop: "Stop browser", type: "Type", capture: "Refresh view"
        };
        fireEvent.click(screen.getByRole("button", { name: buttons[operation] }));
      }
      expect(callToolMock.mock.calls.some(([authority, name]) => authority === firstGrant && name === names[operation])).toBe(true);
      async function change(which: Change) {
        if (which === "offline") {
          view.rerender(<BrowserPane {...props} deviceOnline={false} />);
          expect(screen.getByText("The machine is offline; its browser cannot be listed or opened until it reconnects.")).toBeInTheDocument();
          return firstGrant;
        }
        const authority = which === "grant" ? rotatedGrant : nextGrant;
        view.rerender(<BrowserPane grant={authority} deviceId={which === "grant" ? props.deviceId : id + "-B"} />);
        if (which === "grant") fireEvent.click(screen.getByRole("button", { name: "Refresh pages" }));
        await waitFor(() => expect(screen.getByLabelText("Page")).toHaveValue("B"));
        if (which === "ABA") {
          view.rerender(<BrowserPane {...props} />);
          await waitFor(() => expect(screen.getByLabelText("Page")).toHaveValue("A"));
          return firstGrant;
        }
        return authority;
      }
      async function settle(outcome: "success" | "refusal") {
        await act(async () => {
          if (outcome === "refusal") pending.reject(new ToolError("OLD_CONTEXT_REFUSAL", 200, null, true));
          else pending.resolve(JSON.stringify({ target: "A-opened", value: "OLD_CONTEXT_RESULT", data: "T0xE", bytes: 3, stopped: true }));
          await pending.promise.catch(() => undefined);
        });
      }
      return { ...view, pending, firstGrant, nextGrant, props, change, settle };
    }

    const cases = (["open", "navigate", "close"] as const).flatMap((operation) =>
      (["device", "grant", "offline", "ABA"] as const).flatMap((change) =>
        (["success", "refusal"] as const).map((outcome) => ({ operation, change, outcome }))));
    const related = (["stop", "click", "type", "capture"] as const).flatMap((operation) =>
      (["device", "ABA"] as const).flatMap((change) =>
        (["success", "refusal"] as const).map((outcome) => ({ operation, change, outcome }))));
    it.each([...cases, ...related])("ignores old $operation $outcome after $change without follow-ups or stale state", async ({ operation, change, outcome }) => {
      const fixture = await prepare(operation);
      const currentGrant = await fixture.change(change);
      const status = fixture.container.querySelector(".browser-status")?.textContent;
      const sent = callToolMock.mock.calls.length;
      await fixture.settle(outcome);
      expect(callToolMock.mock.calls).toHaveLength(sent);
      expect(fixture.container.querySelector(".browser-status")?.textContent).toBe(status);
      expect(screen.queryByText(/OLD_CONTEXT/)).not.toBeInTheDocument();
      if (change === "offline") expect(screen.queryByLabelText("Page")).not.toBeInTheDocument();
      else {
        expect([...screen.getByLabelText("Page").querySelectorAll("option")].map((option) => option.value)).toEqual([change === "ABA" ? "A" : "B"]);
        if (change !== "ABA") {
          fireEvent.click(screen.getByRole("button", { name: "Refresh view" }));
          const picture = await screen.findByRole("img");
          expect(picture).toHaveAttribute("src", "data:image/png;base64,QkJC");
          expect(callToolMock).toHaveBeenCalledWith(currentGrant, "browser.screenshot", { target: "B" });
        }
      }
    });

    it.each(["open", "navigate", "close"] as const)("does not dispatch an old %s follow-up after unmount", async (operation) => {
      const fixture = await prepare(operation);
      fixture.unmount();
      const sent = callToolMock.mock.calls.length;
      await fixture.settle("success");
      expect(callToolMock.mock.calls).toHaveLength(sent);
    });

    it("does not let an old completion retire a pending current-device list", async () => {
      const old = deferred();
      const listing = deferred();
      const oldGrant: Grant = { deviceId: "pending-list-old", envelope: {} };
      const newGrant: Grant = { deviceId: "pending-list-new", envelope: {} };
      callToolMock.mockImplementation(async (authority, name) => {
        if (name === "browser.navigate") return old.promise;
        if (name === "browser.list") return authority === oldGrant ? JSON.stringify([A]) : listing.promise;
        return JSON.stringify({ data: "QkJC", bytes: 3 });
      });
      const view = render(<BrowserPane grant={oldGrant} deviceId="pending-list-old" />);
      await screen.findByRole("option", { name: "A" });
      fireEvent.change(screen.getByLabelText("Address"), { target: { value: "https://old.test" } });
      fireEvent.click(screen.getByRole("button", { name: "Navigate" }));
      view.rerender(<BrowserPane grant={newGrant} deviceId="pending-list-new" />);
      await waitFor(() => expect(callToolMock).toHaveBeenCalledWith(newGrant, "browser.list", {}));
      const sent = callToolMock.mock.calls.length;
      await act(async () => { old.resolve("{}"); await old.promise; });
      expect(callToolMock.mock.calls).toHaveLength(sent);
      await act(async () => { listing.resolve(JSON.stringify([B])); await listing.promise; });
      expect(screen.getByLabelText("Page")).toHaveValue("B");
      expect(screen.getByRole("option", { name: "B" })).toBeInTheDocument();
      fireEvent.click(screen.getByRole("button", { name: "Refresh view" }));
      expect(await screen.findByRole("img")).toHaveAttribute("src", "data:image/png;base64,QkJC");
    });

    it("does not let an old follow-up poison the launch-failure memory for a later visit", async () => {
      const old = deferred();
      const firstGrant: Grant = { deviceId: "sticky-operation-A", envelope: {} };
      const nextGrant: Grant = { deviceId: "sticky-operation-B", envelope: {} };
      let retired = false;
      callToolMock.mockImplementation(async (authority, name) => {
        if (name === "browser.navigate") return old.promise;
        if (name === "browser.list") {
          if (authority === firstGrant && retired) throw new ToolError("browser.list: the browser did not start: chrome exited with exit code: 1002 before it answered", 200, null, true);
          return JSON.stringify(authority === firstGrant ? [A] : [B]);
        }
        return "{}";
      });
      const view = render(<BrowserPane grant={firstGrant} deviceId="sticky-operation-A" />);
      await screen.findByRole("option", { name: "A" });
      fireEvent.change(screen.getByLabelText("Address"), { target: { value: "https://old.test" } });
      fireEvent.click(screen.getByRole("button", { name: "Navigate" }));
      view.rerender(<BrowserPane grant={nextGrant} deviceId="sticky-operation-B" />);
      await screen.findByRole("option", { name: "B" });
      retired = true;
      await act(async () => { old.resolve("{}"); await old.promise; });
      retired = false;
      view.unmount();
      render(<BrowserPane grant={firstGrant} deviceId="sticky-operation-A" />);
      expect(await screen.findByRole("option", { name: "A" })).toBeInTheDocument();
      expect(screen.queryByText("The device's browser did not come up.")).not.toBeInTheDocument();
    });

    it("lets a current open select, list and capture its own new page, then type escaped text", async () => {
      const newPage = { id: "NEW", title: "New page", url: "https://new.test" };
      let lists = 0;
      callToolMock.mockImplementation(async (_authority, name) => {
        if (name === "browser.list") return JSON.stringify(++lists === 1 ? [B] : [B, newPage]);
        if (name === "browser.open") return JSON.stringify({ target: "NEW" });
        if (name === "browser.screenshot") return JSON.stringify({ data: "TkVX", bytes: 3 });
        return JSON.stringify({ value: "typed" });
      });
      render(<BrowserPane grant={grant} />);
      await screen.findByRole("option", { name: "B" });
      fireEvent.change(screen.getByLabelText("Address"), { target: { value: "  https://new.test  " } });
      fireEvent.click(screen.getByRole("button", { name: "Open new tab" }));
      expect(await screen.findByRole("img")).toHaveAttribute("src", "data:image/png;base64,TkVX");
      expect(screen.getByLabelText("Page")).toHaveValue("NEW");
      expect(callToolMock).toHaveBeenCalledWith(grant, "browser.open", { url: "https://new.test" });
      expect(callToolMock).toHaveBeenCalledWith(grant, "browser.screenshot", { target: "NEW" });
      fireEvent.change(screen.getByLabelText("Text to type into the focused field"), { target: { value: 'a"b\\c\\nline' } });
      expect(screen.getByLabelText("Text to type into the focused field")).toHaveValue('a"b\\c\\nline');
      fireEvent.click(screen.getByRole("button", { name: "Type" }));
      await waitFor(() => expect(callToolMock).toHaveBeenCalledWith(grant, "browser.eval", {
        target: "NEW", expression: expect.stringContaining('"a\\"b\\\\c\\\\nline"')
      }));
    });
  });

  it("does not invite a browser on a machine the fleet says is down", async () => {
    render(<BrowserPane grant={grant} deviceOnline={false} />);
    await waitFor(() => expect(callToolMock).not.toHaveBeenCalled());
    expect(
      screen.getByText(
        "The machine is offline; its browser cannot be listed or opened until it reconnects."
      )
    ).toBeInTheDocument();
    // The empty state must not say "choose Open new tab" when that control is disabled.
    expect(screen.queryByText(/choose Open new tab/)).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Address"), { target: { value: "https://b.test" } });
    expect(screen.getByRole("button", { name: "Open new tab" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Refresh pages" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Stop browser" })).toBeDisabled();
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

  it("hides the browser's own internal targets", async () => {
    listAnswers = [[
      A,
      { id: "newtab", title: "New Tab", url: "chrome://newtab/" },
      { id: "untrusted", title: "New Tab", url: "chrome-untrusted://new-tab-page/" },
      { id: "toolbar", title: "Omnibox Popup", url: "chrome://webui-toolbar.top-chrome" }
    ]];
    await openPane();

    const target = screen.getByLabelText("Page");
    expect(target).toHaveValue("A");
    expect([...target.querySelectorAll("option")].map((option) => option.textContent)).toEqual(["A"]);
    expect(screen.getByText("1 page.")).toBeInTheDocument();
    expect(screen.getByText("https://a.test")).toBeInTheDocument();
  });

  it("focuses the element a click on the picture lands on", async () => {
    const { container } = await openPane();
    fireEvent.click(screen.getByRole("button", { name: "Refresh view" }));
    await waitFor(() => expect(container.querySelector(".browser-shot")).not.toBeNull());
    const image = container.querySelector(".browser-shot") as HTMLImageElement;

    fireEvent.click(image);

    await waitFor(() =>
      expect(callToolMock.mock.calls.some(([, name]) => name === "browser.eval")).toBe(true)
    );
    const call = callToolMock.mock.calls.find(([, name]) => name === "browser.eval");
    expect((call?.[2] as { expression: string }).expression).toContain(".focus()");
  });

  it.each(["selection", "re-list"] as const)(
    "discards a first screenshot whose target changed by %s before any image existed",
    async (change) => {
      listAnswers = [[A, B], [B]];
      let release!: (value: string) => void;
      const first = new Promise<string>((resolve) => { release = resolve; });
      callToolMock.mockImplementation(async (_grant, name, args) => {
        if (name === "browser.screenshot") {
          return args.target === "A" ? first : JSON.stringify({ data: "QkJC", bytes: 3 });
        }
        return answerFor(name, args);
      });
      render(<BrowserPane grant={grant} />);
      await screen.findByText("2 pages.");
      fireEvent.click(screen.getByRole("button", { name: "Refresh view" }));
      expect(callToolMock).toHaveBeenCalledWith(grant, "browser.screenshot", { target: "A" });
      expect(screen.queryByRole("img")).not.toBeInTheDocument();

      if (change === "selection") {
        fireEvent.change(screen.getByLabelText("Page"), { target: { value: "B" } });
      } else {
        fireEvent.click(screen.getByRole("button", { name: "Refresh pages" }));
        await waitFor(() => expect(screen.getByLabelText("Page")).toHaveValue("B"));
      }
      await act(async () => {
        release(JSON.stringify({ data: "QUFB", bytes: 3 }));
        await first;
      });

      // The first request is still for A, even though B is now selected. Its picture must never
      // become a clickable view of B, and changing selection without an image starts no capture.
      expect(screen.getByLabelText("Page")).toHaveValue("B");
      expect(screen.queryByRole("img")).not.toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Type" })).toBeDisabled();
      expect(shotCalls()).toHaveLength(1);
      expect(callToolMock.mock.calls.filter(([, name]) => name === "browser.eval")).toHaveLength(0);

      fireEvent.click(screen.getByRole("button", { name: "Refresh view" }));
      const image = await screen.findByRole("img", { name: "the device's browser page" });
      expect(image).toHaveAttribute("src", "data:image/png;base64,QkJC");
      Object.defineProperties(image, {
        naturalWidth: { configurable: true, value: 1280 },
        naturalHeight: { configurable: true, value: 720 }
      });
      vi.spyOn(image, "getBoundingClientRect").mockReturnValue({
        x: 10, y: 20, left: 10, top: 20, right: 650, bottom: 380,
        width: 640, height: 360, toJSON: () => ({})
      });
      fireEvent.click(image, { clientX: 330, clientY: 200, detail: 1 });
      await waitFor(() => expect(callToolMock).toHaveBeenCalledWith(grant, "browser.eval", {
        target: "B", expression: expect.stringContaining("document.elementFromPoint(640, 360)")
      }));
    }
  );

  it("removes the previous target's image while its replacement capture is pending", async () => {
    listAnswers = [[A, B]];
    let release!: (value: string) => void;
    const replacement = new Promise<string>((resolve) => { release = resolve; });
    callToolMock.mockImplementation(async (_grant, name, args) => {
      if (name === "browser.screenshot" && args.target === "B") return replacement;
      return answerFor(name, args);
    });
    render(<BrowserPane grant={grant} />);
    await screen.findByText("2 pages.");
    fireEvent.click(screen.getByRole("button", { name: "Refresh view" }));
    await screen.findByRole("img", { name: "the device's browser page" });

    fireEvent.change(screen.getByLabelText("Page"), { target: { value: "B" } });
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Type" })).toBeDisabled();
    expect(callToolMock).toHaveBeenCalledWith(grant, "browser.screenshot", { target: "B" });
    await act(async () => {
      release(JSON.stringify({ data: "QkJC", bytes: 3 }));
      await replacement;
    });
    expect(screen.getByRole("img")).toHaveAttribute("src", "data:image/png;base64,QkJC");
    fireEvent.change(screen.getByLabelText("Text to type into the focused field"), {
      target: { value: "B field" }
    });
    fireEvent.click(screen.getByRole("button", { name: "Type" }));
    await waitFor(() => expect(callToolMock).toHaveBeenCalledWith(grant, "browser.eval", {
      target: "B", expression: expect.stringContaining('el.value = (el.value || "") + "B field";')
    }));
  });

  it.each(["grant", "device", "offline"] as const)(
    "discards a pending first screenshot when its %s context changes",
    async (change) => {
      let release!: (value: string) => void;
      const first = new Promise<string>((resolve) => { release = resolve; });
      callToolMock.mockImplementation(async (_grant, name, args) => {
        if (name === "browser.screenshot") return first;
        return answerFor(name, args);
      });
      const view = render(<BrowserPane grant={grant} deviceId="context-device-1" />);
      await screen.findByText("1 page.");
      fireEvent.click(screen.getByRole("button", { name: "Refresh view" }));
      view.rerender(<BrowserPane
        grant={change === "grant" ? { ...grant, envelope: { rotated: true } } : grant}
        deviceId={change === "device" ? "context-device-2" : "context-device-1"}
        deviceOnline={change !== "offline"}
      />);
      await act(async () => {
        release(JSON.stringify({ data: "QUFB", bytes: 3 }));
        await first;
      });
      expect(screen.queryByRole("img")).not.toBeInTheDocument();
      expect(screen.queryByText("Captured 3 bytes.")).not.toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Type" })).toBeDisabled();
      expect(shotCalls()).toHaveLength(1);
      expect(callToolMock.mock.calls.filter(([, name]) => name === "browser.eval")).toHaveLength(0);
    }
  );

  it("keeps Navigate and Open new tab disabled until there is an address", async () => {
    await openPane();
    const navigate = screen.getByRole("button", { name: "Navigate" });
    const open = screen.getByRole("button", { name: "Open new tab" });
    expect(navigate).toBeDisabled();
    expect(open).toBeDisabled();

    fireEvent.change(screen.getByLabelText("Address"), { target: { value: "https://b.test" } });
    expect(navigate).toBeEnabled();
    expect(open).toBeEnabled();
  });

  it("disables Navigate and Close tab when no target is listed", async () => {
    listAnswers = [[]];
    render(<BrowserPane grant={grant} />);
    // The initial status is the same sentence, so wait for the answer itself before asserting on it.
    await waitFor(() => expect(callToolMock).toHaveBeenCalledWith(grant, "browser.list", {}));
    const empty = await screen.findByText(/No browser is open on this device/);
    expect(empty).toHaveTextContent(/Open new tab/);

    fireEvent.change(screen.getByLabelText("Address"), { target: { value: "https://b.test" } });
    expect(screen.getByRole("button", { name: "Navigate" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Close tab" })).toBeDisabled();
  });

  it("turns a browser start failure into a sentence, an action and the device's answer", async () => {
    const raw =
      "browser.list: the browser did not start: chrome exited with exit code: 1002 before it " +
      "answered http://127.0.0.1:62544/json/version; no stderr output";
    callToolMock.mockImplementation(async (_grant, name, args) => {
      if (name === "browser.list") throw new ToolError(raw, 200, null, true);
      return answerFor(name, args);
    });
    render(<BrowserPane grant={grant} />);

    const primary = await screen.findByText("The device's browser did not come up.");
    expect(primary).toHaveClass("error");
    // The ephemeral port and the exit code are evidence; they are not the line the operator reads.
    expect(primary.textContent).not.toContain("127.0.0.1");
    expect(primary.textContent).not.toContain("exit code");

    expect(
      screen.getByText("Stop the browser, then choose Refresh pages to start it again.")
    ).toBeInTheDocument();
    // The recovery line names the two controls the failed state leaves enabled.
    expect(screen.getByRole("button", { name: "Refresh pages" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Stop browser" })).toBeEnabled();

    const evidence = screen.getByText("The device's answer").closest("details");
    expect(evidence).not.toBeNull();
    expect(evidence?.hasAttribute("open")).toBe(false);
    expect(evidence?.textContent).toContain(raw);
  });

  it("names the page endpoint when the browser never answered it", async () => {
    const raw =
      "browser.list: the browser did not start: chrome did not answer " +
      "http://127.0.0.1:62544/json/version within 15000 ms; chrome is still running; no stderr output";
    callToolMock.mockImplementation(async (_grant, name, args) => {
      if (name === "browser.list") throw new ToolError(raw, 200, null, true);
      return answerFor(name, args);
    });
    render(<BrowserPane grant={grant} />);

    expect(
      await screen.findByText("The device's browser page endpoint did not answer.")
    ).toBeInTheDocument();
    expect(
      screen.getByText("Stop the browser, then choose Refresh pages to start it again.")
    ).toBeInTheDocument();
    expect(screen.getByText("The device's answer").closest("details")?.textContent).toContain(raw);
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
    expect(screen.getByLabelText("Page")).toHaveValue("B");
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
    expect(stopped).toHaveTextContent(/Open new tab/);
    expect(container.querySelector(".browser-shot")).toBeNull();
    expect(screen.queryByLabelText("Page")).toBeNull();
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

  it("keeps a lost transport response's outcome unknown", async () => {
    callToolMock.mockImplementation(async (_grant, name, args) => {
      if (name === "browser.navigate") throw new ToolError("Failed to fetch", 0);
      return answerFor(name, args);
    });
    await openPane();
    fireEvent.change(screen.getByLabelText("Address"), { target: { value: "https://b.test" } });
    fireEvent.click(screen.getByRole("button", { name: "Navigate" }));

    const failure = await screen.findByText(/Navigate failed: the call's outcome is unknown; the device may still be working/);
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

  it("captures one live frame per 1.5 s and counts them", async () => {
    const { container } = await openPane();
    vi.useFakeTimers();
    try {
      await startLive();
      expect(screen.getByRole("button", { name: "Stop live view" })).toBeInTheDocument();
      expect(shotCalls()).toHaveLength(0);

      await tick();
      expect(shotCalls()).toHaveLength(1);
      await tick(3_000);
      expect(shotCalls()).toHaveLength(3);
      expect(screen.getByText("Live · 1.5 s · frame 3")).toBeInTheDocument();
      expect(container.querySelector(".browser-shot")).not.toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps one capture in flight when a shot is slower than the interval", async () => {
    let release!: (value: string) => void;
    const slow = new Promise<string>((resolve) => {
      release = resolve;
    });
    callToolMock.mockImplementation(async (_grant, name, args) => {
      if (name === "browser.screenshot") return slow;
      return answerFor(name, args);
    });
    await openPane();
    vi.useFakeTimers();
    try {
      await startLive();
      await tick();
      expect(shotCalls()).toHaveLength(1);

      // Three more ticks while the first answer is outstanding: all must be skipped, not queued.
      await tick(4_500);
      expect(shotCalls()).toHaveLength(1);

      await act(async () => {
        release(JSON.stringify({ data: "AAAA", bytes: 3 }));
        await slow;
      });
      await tick();
      expect(shotCalls()).toHaveLength(2);
      expect(screen.getByText("Live · 1.5 s · frame 2")).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("stops capturing when the live toggle is switched off", async () => {
    await openPane();
    vi.useFakeTimers();
    try {
      await startLive();
      await tick();
      expect(shotCalls()).toHaveLength(1);

      fireEvent.click(screen.getByRole("button", { name: "Stop live view" }));
      expect(screen.getByRole("button", { name: "Start live view" })).toBeInTheDocument();
      await tick(6_000);
      expect(shotCalls()).toHaveLength(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("clears the live timer on unmount", async () => {
    const { unmount } = await openPane();
    vi.useFakeTimers();
    try {
      await startLive();
      await tick();
      expect(shotCalls()).toHaveLength(1);

      unmount();
      await tick(6_000);
      expect(shotCalls()).toHaveLength(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("skips ticks while the document is hidden and resumes when it is visible", async () => {
    await openPane();
    vi.useFakeTimers();
    try {
      await startLive();
      await tick();
      expect(shotCalls()).toHaveLength(1);

      Object.defineProperty(document, "hidden", { configurable: true, get: () => true });
      try {
        await act(async () => {
          document.dispatchEvent(new Event("visibilitychange"));
          await vi.advanceTimersByTimeAsync(6_000);
        });
        expect(shotCalls()).toHaveLength(1);

        Object.defineProperty(document, "hidden", { configurable: true, get: () => false });
        await act(async () => {
          document.dispatchEvent(new Event("visibilitychange"));
          await vi.advanceTimersByTimeAsync(1_500);
        });
        expect(shotCalls()).toHaveLength(2);
      } finally {
        Reflect.deleteProperty(document, "hidden");
      }
    } finally {
      vi.useRealTimers();
    }
  });

  it("stops when the grant changes under a running live view", async () => {
    const view = await openPane();
    vi.useFakeTimers();
    try {
      await startLive();
      await tick();
      expect(shotCalls()).toHaveLength(1);

      view.rerender(<BrowserPane grant={{ ...grant, envelope: { rotated: true } }} />);
      expect(screen.getByRole("button", { name: "Start live view" })).toBeInTheDocument();
      expect(screen.getByText("Live stopped: the grant changed.")).toBeInTheDocument();
      await tick(6_000);
      expect(shotCalls()).toHaveLength(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps watching when the selected target changes", async () => {
    listAnswers = [[A, B]];
    const view = render(<BrowserPane grant={grant} />);
    await screen.findByText("2 pages.");
    vi.useFakeTimers();
    try {
      await startLive();
      await tick();
      const calls = shotCalls();
      expect(calls[calls.length - 1]?.[2]).toEqual({ target: "A" });

      fireEvent.change(screen.getByLabelText("Page"), { target: { value: "B" } });
      await tick(0);
      expect(screen.getByRole("button", { name: "Stop live view" })).toBeInTheDocument();

      await tick();
      expect(
        shotCalls().some(([, , args]) => (args as { target?: string }).target === "B")
      ).toBe(true);
      fireEvent.click(screen.getByRole("button", { name: "Stop live view" }));
      void view;
    } finally {
      vi.useRealTimers();
    }
  });

  it("stops when re-listing finds no target", async () => {
    listAnswers = [[A], []];
    await openPane();
    vi.useFakeTimers();
    try {
      await startLive();
      await tick();
      expect(shotCalls()).toHaveLength(1);

      fireEvent.click(screen.getByRole("button", { name: "Refresh pages" }));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(screen.getByText(/No browser is open on this device/)).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Start live view" })).toBeInTheDocument();
      await tick(6_000);
      expect(shotCalls()).toHaveLength(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("stops live on a refusal and shows the screenshot failure", async () => {
    callToolMock.mockImplementation(async (_grant, name, args) => {
      if (name === "browser.screenshot") throw new ToolError("the grant expired", 403, -32001);
      return answerFor(name, args);
    });
    await openPane();
    vi.useFakeTimers();
    try {
      await startLive();
      await tick();

      expect(screen.getByText("Screenshot failed: the grant expired")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Start live view" })).toBeInTheDocument();
      await tick(6_000);
      expect(shotCalls()).toHaveLength(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("stops live on any other screenshot failure too", async () => {
    callToolMock.mockImplementation(async (_grant, name, args) => {
      if (name === "browser.screenshot") throw new ToolError("the tab is gone", 200, null, true);
      return answerFor(name, args);
    });
    await openPane();
    vi.useFakeTimers();
    try {
      await startLive();
      await tick();

      expect(screen.getByText("Screenshot failed: the tab is gone")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Start live view" })).toBeInTheDocument();
      await tick(6_000);
      expect(shotCalls()).toHaveLength(1);
    } finally {
      vi.useRealTimers();
    }
  });


  it("prefills the address from a one-shot deep link and opens nothing", async () => {
    const handled = vi.fn();
    render(
      <BrowserPane
        grant={grant}
        openBrowser={{ url: "https://example.test/a", nonce: 1 }}
        onOpenBrowserHandled={handled}
      />
    );
    await screen.findByText("1 page.");

    // **THE LINK IS AN ADDRESS, NOT A PRESS.** The tray names a URL; the operator still chooses
    // Open new tab (or Navigate). `browser.open` from a fragment would be a page that acts without
    // the operator asking.
    expect(screen.getByLabelText("Address")).toHaveValue("https://example.test/a");
    expect(callToolMock.mock.calls.some(([, name]) => name === "browser.open")).toBe(false);
    expect(handled).toHaveBeenCalledTimes(1);
  });

  it("does not ask a machine twice for a browser it already failed to start", async () => {
    // **THE CONSOLE'S OWN FAILED CALLS WERE POLLUTING THE RECORD (the hundred-and-ninety-eighth list).** `browser.list` STARTS the
    // browser, so on a machine where chrome cannot come up every visit fired one doomed call — three `browser.list` failures in a
    // single audit window, all the console's own. The second visit says what the first learned and fires NOTHING.
    const launchFailure = new ToolError(
      "browser.list: the browser did not start: chrome exited with exit code: 1002 before it answered http://127.0.0.1:1/json/version; no stderr output",
      200,
      null,
      true
    );
    callToolMock.mockRejectedValue(launchFailure);
    const first = render(<BrowserPane grant={grant} deviceId="machine-remembered" />);
    await screen.findByText(/did not come up/i);
    expect(callToolMock).toHaveBeenCalledTimes(1);
    first.unmount();

    const second = render(<BrowserPane grant={grant} deviceId="machine-remembered" />);
    await screen.findByText(/did not come up/i);
    expect(callToolMock).toHaveBeenCalledTimes(1); // the visit cost nothing
    second.unmount();
  });
});
