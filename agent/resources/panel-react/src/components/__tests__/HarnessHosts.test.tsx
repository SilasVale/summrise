// HarnessHosts — the LEFT pane of the harness page, pinned with a mocked main-process bridge.
//
// WHAT THIS COLUMN IS. The harness page is two panes: hosts on the left, the SELECTED host's own harness
// on the right. The left column is the only part the panel draws for a host — the files, the editor, the
// command line and the terminal all belong to the harness on the far side, which is the arrangement the
// operator chose ("主机的会话" on the right, hosts on the left). So this column does three things and no
// more: it asks the agent where each host's harness is, it says what the agent last observed there, and
// it switches the view.
//
// THE HONESTY OF THE STATE COLUMN, which is the part that is easy to get wrong and expensive to get
// wrong twice. The agent answers with `answering` / `not answering` / `never probed` and the millisecond
// its probe ran. This column prints all three, and the age: a row that says "answering, 40 minutes ago"
// is a different claim from "answering, just now", and printing only the first is how a panel ends up
// reporting a state nobody checked — the defect this repository's rule is named for ("a surface that
// reports a state names the thing that was observed and WHEN").

import { render, screen, fireEvent, waitFor, act } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { HarnessHosts } from "../HarnessHosts";
import { callApi } from "../../lib/api";

vi.mock("../../lib/api", () => ({ callApi: vi.fn() }));

type Row = {
  connection_id: string;
  local_port: number;
  remote_port: number;
  url: string;
  state: string;
  checked_ms: number | null;
  probe: string | null;
};

const HOST: Row = {
  connection_id: "ssh:zhengsaisi@10.10.61.83:22122",
  local_port: 7738,
  remote_port: 7738,
  url: "http://127.0.0.1:7738",
  state: "answering",
  // forty seconds before the moment the test reads the DOM, so the assertion is about the FORM of the
  // claim ("40s ago") and not about when the suite happened to run.
  checked_ms: Date.now() - 40_000,
  probe: "GET http://127.0.0.1:7738/ -> 200",
};
const ZSS: Row = {
  connection_id: "ssh:zss@100.85.163.159",
  local_port: 7801,
  remote_port: 7738,
  url: "http://127.0.0.1:7801",
  state: "never probed",
  checked_ms: null,
  probe: null,
};

let go: ReturnType<typeof vi.fn>;
let rows: Row[];

beforeEach(() => {
  go = vi.fn(() => Promise.resolve({ ok: true, url: "http://127.0.0.1:7738/" }));
  (window as unknown as { summriseDsh?: unknown }).summriseDsh = {
    open: vi.fn(() => Promise.resolve({ ok: true })),
    place: vi.fn(() => Promise.resolve()),
    state: vi.fn(() => Promise.resolve({ ok: true, url: "http://127.0.0.1:18081/" })),
    reload: vi.fn(() => Promise.resolve()),
    recover: vi.fn(() => Promise.resolve()),
    go,
    onGone: vi.fn(() => () => {}),
  };
  rows = [HOST];
  vi.mocked(callApi).mockImplementation(() => Promise.resolve({ ok: true, harnesses: rows }));
});

afterEach(() => {
  delete (window as unknown as { summriseDsh?: unknown }).summriseDsh;
  vi.mocked(callApi).mockReset();
});

describe("HarnessHosts — hosts on the left, each host's own harness on the right", () => {
  it("lists the hosts the agent knows a harness for, and says what the last probe saw", async () => {
    render(<HarnessHosts />);
    await waitFor(() => expect(screen.getByText(/10\.10\.61\.83/)).toBeTruthy());
    // the state a reader acts on, and the SENTENCE behind it — the panel is not inventing a verdict
    expect(screen.getByText(/answering/)).toBeTruthy();
    expect(screen.getByText(/200/)).toBeTruthy();
    // and how old that claim is: "answering" alone would be a claim nobody checked
    expect(screen.getByText(/40s ago/)).toBeTruthy();
  });

  it("a host nobody has probed says SO, and is not dressed as one that is up", async () => {
    rows = [ZSS];
    render(<HarnessHosts />);
    await waitFor(() => expect(screen.getByText(/100\.85\.163\.159/)).toBeTruthy());
    expect(screen.getByText(/never probed/)).toBeTruthy();
    expect(screen.queryByText(/answering/)).toBeNull();
  });

  it("selecting a host points the harness view at THAT host, through the main process", async () => {
    rows = [HOST, ZSS];
    render(<HarnessHosts />);
    await waitFor(() => expect(screen.getByText(/zss@100\.85\.163\.159/)).toBeTruthy());
    await act(async () => {
      fireEvent.click(screen.getByText(/zss@100\.85\.163\.159/));
    });
    // The pane does not navigate: the view is a native WebContentsView, and the main process is what
    // checks the address against the doors the agent configured.
    expect(go).toHaveBeenCalledWith("http://127.0.0.1:7801");
  });

  it("a refused switch is shown, because the view is not where the reader asked for", async () => {
    go = vi.fn(() => Promise.resolve({ ok: true, url: "about:blank" }));
    (window as unknown as { summriseDsh?: unknown }).summriseDsh = {
      ...((window as unknown as { summriseDsh: Record<string, unknown> }).summriseDsh),
      go,
    };
    render(<HarnessHosts selected={null} onSelect={() => {}} />);
    await waitFor(() => expect(screen.getByText(/10\.10\.61\.83/)).toBeTruthy());
    await act(async () => {
      fireEvent.click(screen.getByText(/10\.10\.61\.83/));
    });
    // the reader is told the view did not go where they asked
    await waitFor(() => expect(screen.getByText(/about:blank/)).toBeTruthy());
  });

  it("an agent that cannot be reached says so, rather than showing an empty column as if there were no hosts", async () => {
    vi.mocked(callApi).mockImplementation(() => Promise.resolve({ ok: false, error: "unauthorized", code: "auth" }));
    render(<HarnessHosts />);
    await waitFor(() => expect(screen.getByText(/did not answer/i)).toBeTruthy());
    expect(screen.getByText(/unauthorized/)).toBeTruthy();
  });

  it("a column with no hosts yet says what to do about it", async () => {
    rows = [];
    render(<HarnessHosts />);
    await waitFor(() => expect(screen.getByText(/no host/i)).toBeTruthy());
  });
});
