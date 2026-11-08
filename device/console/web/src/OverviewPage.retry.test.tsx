import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HttpError, type Device, type LiveTool } from "./api";
import { clockTime } from "./format";

/**
 * The integration `HomePage.test.tsx` cannot write: that file replaces `useLiveTools` with a value,
 * so its Retry fixtures describe a state rather than prove the button reaches the hook. This file
 * renders the machine's Overview page with the real hook (only the HTTP calls are mocked) and
 * presses the button.
 *
 * It exists because commit 2bd4fc3 passed both suites — the hook's own tests and the button's
 * fixtures — while the deployed page's press looked like the poll: nothing tied the click to the
 * hook. A unit test on each side of a seam is not a test of the seam.
 */

const mocks = vi.hoisted(() => ({
  liveTools: vi.fn(),
  audit: vi.fn(),
  installer: vi.fn()
}));

vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return {
    ...actual,
    api: {
      ...actual.api,
      liveTools: mocks.liveTools,
      audit: mocks.audit,
      installer: mocks.installer
    }
  };
});

const plugins = vi.hoisted(() => ({ list: vi.fn() }));
vi.mock("./pluginApi", () => ({ listPlugins: plugins.list }));

import { OverviewPage } from "./OverviewPage";
import { resetLiveToolsCache } from "./useLiveTools";

/** An answer the test decides when to give. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((give, refuse) => {
    resolve = give;
    reject = refuse;
  });
  return { promise, resolve, reject };
}

/** The live 14R: offline, with the attempt the hub's notice states. */
const OFFLINE: Device = {
  id: "dev-14r",
  name: "DESKTOP-14RJCR8",
  online: false,
  version: "0.3.275",
  last_seen_ms: 1,
  last_attempt: {
    attempts: 1,
    reason: "the console accepted the channel and then dropped it"
  }
};

function hub() {
  return render(
    <OverviewPage
      device={OFFLINE}
      onTools={vi.fn()}
      onActivity={vi.fn()}
    />
  );
}

/**
 * Let the mount read land and drop its in-flight entry, so the press starts a request of its own
 * rather than joining the page's silent one (that join is the cache's dedupe, not the press's read).
 */
async function settleMount(read: ReturnType<typeof deferred<{ tools: LiveTool[] }>>) {
  await act(async () => {
    read.resolve({ tools: [] });
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

beforeEach(() => {
  resetLiveToolsCache();
  mocks.liveTools.mockReset();
  mocks.audit.mockReset();
  mocks.audit.mockResolvedValue([]);
  mocks.installer.mockReset();
  plugins.list.mockReset();
  plugins.list.mockResolvedValue([]);
});

describe("Overview — the Retry button drives useLiveTools", () => {
  it("shows the press's own read in flight, then the same offline result with its moment", async () => {
    const mount = deferred<{ tools: LiveTool[] }>();
    mocks.liveTools.mockReturnValueOnce(mount.promise);
    hub();
    // The mount read is the page's silent read: it leaves no working state and no outcome.
    expect(screen.getByRole("button", { name: "Retry" })).toBeEnabled();
    expect(screen.queryByText(/last checked/)).toBeNull();
    await settleMount(mount);

    const press = deferred<{ tools: LiveTool[] }>();
    mocks.liveTools.mockReturnValueOnce(press.promise);
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(mocks.liveTools).toHaveBeenCalledTimes(2));

    // The click handler sets `retrying` in the same render, so the button is disabled before the
    // read effect runs: a second press cannot start a second request.
    const pending = screen.getByRole("button", { name: "Retrying…" });
    expect(pending).toBeDisabled();
    expect(screen.queryByText(/last checked/)).toBeNull();

    const at = new Date(2026, 10, 7, 6, 52, 0).getTime();
    const clock = vi.spyOn(Date, "now").mockReturnValue(at);
    try {
      await act(async () => {
        press.reject(
          new HttpError(
            503,
            "device_offline",
            "this machine is not connected; connect it, then try again"
          )
        );
        await new Promise((resolve) => setTimeout(resolve, 0));
      });
    } finally {
      clock.mockRestore();
    }

    const notice = screen.getByRole("status");
    const note = `Still offline · last checked ${clockTime(at)}.`;
    expect(within(notice).getByText(note)).toBeInTheDocument();
    expect(within(notice).getByRole("button", { name: "Retry" })).toBeEnabled();
    // The note is inside the notice the live check reads, which is the whole point: the deployed
    // press must change that element, not only a hook field.
    expect(notice).toHaveTextContent(note);
    // One press is one `/fleet/<id>/tools` read; nothing on this page polls the catalogue.
    await act(async () => {});
    expect(mocks.liveTools).toHaveBeenCalledTimes(2);
    expect(mocks.liveTools).toHaveBeenNthCalledWith(2, OFFLINE.id);
  });

  it("clears the failed press's note and states the next press's own result", async () => {
    const mount = deferred<{ tools: LiveTool[] }>();
    mocks.liveTools.mockReturnValueOnce(mount.promise);
    hub();
    await settleMount(mount);

    const first = deferred<{ tools: LiveTool[] }>();
    mocks.liveTools.mockReturnValueOnce(first.promise);
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(mocks.liveTools).toHaveBeenCalledTimes(2));
    await act(async () => {
      first.reject(new HttpError(503, "device_offline", "this machine is not connected"));
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    const notice = screen.getByRole("status");
    expect(notice).toHaveTextContent(/Still offline · last checked \d{2}:\d{2}\./);

    const second = deferred<{ tools: LiveTool[] }>();
    mocks.liveTools.mockReturnValueOnce(second.promise);
    fireEvent.click(within(notice).getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(mocks.liveTools).toHaveBeenCalledTimes(3));
    expect(screen.getByRole("button", { name: "Retrying…" })).toBeDisabled();
    // A new press is a new attempt: the previous attempt's outcome is not its answer.
    expect(screen.queryByText(/last checked|reached the machine/)).toBeNull();

    const at = new Date(2026, 10, 7, 6, 55, 0).getTime();
    const clock = vi.spyOn(Date, "now").mockReturnValue(at);
    try {
      await act(async () => {
        second.resolve({ tools: [] });
        await new Promise((resolve) => setTimeout(resolve, 0));
      });
    } finally {
      clock.mockRestore();
    }

    const settled = screen.getByRole("status");
    expect(settled).toHaveTextContent(
      `The console reached the machine · checked ${clockTime(at)}.`
    );
    expect(within(settled).getByRole("button", { name: "Retry" })).toBeEnabled();
    expect(mocks.liveTools).toHaveBeenCalledTimes(3);
  });
});
