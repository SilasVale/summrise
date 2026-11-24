import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { MemoryPage } from "./MemoryPage";
import { callTool, type Grant } from "./terminal";

vi.mock("./terminal", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./terminal")>();
  return { ...actual, callTool: vi.fn() };
});

const mocked = vi.mocked(callTool);
const GRANT: Grant = { deviceId: "device-1", envelope: { token: "envelope" } };

/// The record the DEVICE sends (`MemoryRecord` in `agent/src/plugins/memory/store.rs`): seconds for the stamps,
/// `source` for the writer, and an optional `run_id` that is ABSENT when the client declared no run.
const RECORD = {
  id: "m-1",
  title: "The build host is DESKTOP-9VH92BJ",
  content: "Regenerate tokens with scripts/generate-tokens.sh before the console build.",
  tags: ["build", "tokens"],
  namespace: "shared",
  source: "unknown",
  created_at: 1_761_830_000,
  updated_at: 1_761_830_400,
  deleted: false
};

const answer = (results: unknown[]) => JSON.stringify({ ok: true, results });

beforeEach(() => {
  mocked.mockReset();
});

describe("MemoryPage — the machine's memory, read", () => {
  it("does not read an offline machine, including search submitted with Enter", async () => {
    render(<MemoryPage grant={GRANT} deviceOnline={false} />);
    fireEvent.change(screen.getByLabelText("Search the machine's memory"), { target: { value: "tokens" } });
    fireEvent.submit(screen.getByRole("search"));
    expect(mocked).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Search" })).toBeDisabled();
    expect(screen.getByText("Its memory cannot be read.")).toBeInTheDocument();
    expect(screen.queryByText(/last list/)).not.toBeInTheDocument();
  });

  it.each(["success", "failure"])("keeps the last list when an offline machine's pending search answers with %s", async (outcome) => {
    let resolveRead!: (value: string) => void;
    let rejectRead!: (reason: Error) => void;
    const pending = new Promise<string>((resolve, reject) => { resolveRead = resolve; rejectRead = reject; });
    mocked.mockResolvedValueOnce(answer([RECORD])).mockReturnValueOnce(pending);
    const view = render(<MemoryPage grant={GRANT} />);
    await screen.findByText(RECORD.title);
    fireEvent.change(screen.getByLabelText("Search the machine's memory"), { target: { value: "late" } });
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    view.rerender(<MemoryPage grant={GRANT} deviceOnline={false} />);
    await act(async () => {
      if (outcome === "success") resolveRead(answer([{ ...RECORD, id: "late", title: "Late result" }]));
      else rejectRead(new Error("connection closed"));
      await pending.catch(() => {});
    });
    expect(screen.getByText(RECORD.title)).toBeInTheDocument();
    expect(screen.queryByText("Late result")).not.toBeInTheDocument();
    expect(screen.getByText("1 entry · newest first")).toBeInTheDocument();
    expect(screen.getByText(/last list the console could read/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Search" })).toBeDisabled();
  });

  it("clears the previous machine's rows and query while the new grant is being read", async () => {
    const nextGrant: Grant = { deviceId: "device-2", envelope: { token: "second" } };
    let resolveOld!: (value: string) => void;
    let resolveNext!: (value: string) => void;
    const oldRead = new Promise<string>((resolve) => { resolveOld = resolve; });
    const nextRead = new Promise<string>((resolve) => { resolveNext = resolve; });
    mocked.mockResolvedValueOnce(answer([RECORD])).mockReturnValueOnce(oldRead).mockReturnValueOnce(nextRead);
    const view = render(<MemoryPage grant={GRANT} />);
    await screen.findByText(RECORD.title);
    fireEvent.change(screen.getByLabelText("Search the machine's memory"), { target: { value: "old query" } });
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    view.rerender(<MemoryPage grant={nextGrant} />);
    expect(screen.queryByText(RECORD.title)).not.toBeInTheDocument();
    expect(screen.getByLabelText("Search the machine's memory")).toHaveValue("");
    expect(mocked).toHaveBeenLastCalledWith(nextGrant, "memory.list", { limit: 50 });
    await act(async () => { resolveNext(answer([{ ...RECORD, id: "next", title: "Second machine" }])); await nextRead; });
    await act(async () => { resolveOld(answer([{ ...RECORD, id: "old", title: "Old machine late" }])); await oldRead; });
    expect(screen.getByText("Second machine")).toBeInTheDocument();
    expect(screen.queryByText("Old machine late")).not.toBeInTheDocument();
  });

  it("lists what the device answers, newest first, with the record's provenance", async () => {
    mocked.mockResolvedValue(answer([RECORD]));
    render(<MemoryPage grant={GRANT} />);
    await waitFor(() => expect(screen.getByText(RECORD.title)).toBeInTheDocument());
    expect(mocked).toHaveBeenCalledWith(GRANT, "memory.list", { limit: 50 });
    expect(screen.getByText(/shared · unknown ·/)).toBeInTheDocument();
    expect(screen.getByText("1 entry · newest first")).toBeInTheDocument();
    expect(screen.getByText("build")).toBeInTheDocument();
    expect(screen.getByText("tokens")).toBeInTheDocument();
  });

  it("asks the SEARCH tool when the box has a question, and says which question it answered", async () => {
    mocked.mockResolvedValueOnce(answer([RECORD]));
    render(<MemoryPage grant={GRANT} />);
    await waitFor(() => expect(screen.getByText(RECORD.title)).toBeInTheDocument());
    mocked.mockResolvedValueOnce(answer([]));
    fireEvent.change(screen.getByLabelText(/Search the machine's memory/), {
      target: { value: "tokens" }
    });
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    await waitFor(() =>
      expect(mocked).toHaveBeenLastCalledWith(GRANT, "memory.search", { query: "tokens", limit: 50 })
    );
    await waitFor(() => expect(screen.getByText("Nothing matches “tokens”.")).toBeInTheDocument());
  });

  it("reads the {id,tags,text} records the SHIPPED device answers with", async () => {
    // Measured on the live fleet 2026-11-12: 0.3.292 answers a bare array of `{id, tags, text}` — no title, no timestamps, no
    // namespace — and the page said "the machine answered without a list".
    mocked.mockResolvedValue(
      JSON.stringify([
        { id: "ed4873", tags: ["agent", "port"], text: "summrise-agent canonical port is 7737 since 2026-11-04." }
      ])
    );
    render(<MemoryPage grant={GRANT} />);
    await waitFor(() =>
      expect(screen.getByText("summrise-agent canonical port is 7737 since 2026-11-04.")).toBeInTheDocument()
    );
    expect(screen.getByText("1 entry · newest first")).toBeInTheDocument();
    // no invented namespace, source or date
    expect(screen.queryByText(/shared · unknown/)).toBeNull();
  });

  it("draws an empty store as a SENTENCE, not as a blank panel", async () => {
    mocked.mockResolvedValue(answer([]));
    render(<MemoryPage grant={GRANT} />);
    await waitFor(() =>
      expect(screen.getByText("This machine's memory is empty.")).toBeInTheDocument()
    );
  });

  it("says a refused read in the device's own words, and offers the retry", async () => {
    mocked.mockResolvedValueOnce(JSON.stringify({ ok: false, error: "the memory store is unusable" }));
    render(<MemoryPage grant={GRANT} />);
    await waitFor(() => expect(screen.getByText("the memory store is unusable")).toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  it("drops a record it cannot name, rather than drawing an empty row", async () => {
    mocked.mockResolvedValue(
      answer([RECORD, { id: "", title: "no id" }, { id: "m-2", title: "" }])
    );
    render(<MemoryPage grant={GRANT} />);
    await waitFor(() => expect(screen.getByText(RECORD.title)).toBeInTheDocument());
    expect(screen.getByText("1 entry · newest first")).toBeInTheDocument();
  });

  it("opens only the selected memory body and returns focus when the inspector closes", async () => {
    mocked.mockResolvedValue(answer([
      RECORD,
      { ...RECORD, id: "m-2", title: "Connection notes", content: "Use the configured endpoint for this machine.", tags: ["connection"] }
    ]));
    render(<MemoryPage grant={GRANT} />);
    const opener = await screen.findByRole("button", { name: RECORD.title });
    expect(screen.queryByText(RECORD.content)).not.toBeInTheDocument();
    expect(screen.queryByText("Use the configured endpoint for this machine.")).not.toBeInTheDocument();
    fireEvent.click(opener);
    const detail = screen.getByRole("region", { name: "Memory entry" });
    expect(within(detail).getByText(RECORD.content)).toBeInTheDocument();
    expect(within(detail).getByText("m-1")).toBeInTheDocument();
    expect(within(detail).queryByText("Use the configured endpoint for this machine.")).not.toBeInTheDocument();
    expect(detail).toHaveFocus();
    expect(opener).toHaveAttribute("aria-expanded", "true");
    fireEvent.click(within(detail).getByRole("button", { name: "Close entry" }));
    expect(screen.queryByRole("region", { name: "Memory entry" })).not.toBeInTheDocument();
    expect(opener).toHaveFocus();
    expect(mocked).toHaveBeenCalledTimes(1);
  });

  it("moves between memory results with arrow keys and Escape returns to the selected result", async () => {
    mocked.mockResolvedValue(answer([
      RECORD,
      { ...RECORD, id: "m-2", title: "Connection notes", content: "Second entry body.", tags: [] }
    ]));
    render(<MemoryPage grant={GRANT} />);
    const first = await screen.findByRole("button", { name: RECORD.title });
    const second = screen.getByRole("button", { name: "Connection notes" });
    first.focus();
    fireEvent.keyDown(first, { key: "ArrowDown" });
    expect(second).toHaveFocus();
    fireEvent.keyDown(second, { key: "Home" });
    expect(first).toHaveFocus();
    fireEvent.keyDown(first, { key: "End" });
    expect(second).toHaveFocus();
    fireEvent.click(second);
    const detail = screen.getByRole("region", { name: "Memory entry" });
    expect(within(detail).getByText("Second entry body.")).toBeInTheDocument();
    fireEvent.keyDown(detail, { key: "Escape" });
    expect(screen.queryByRole("region", { name: "Memory entry" })).not.toBeInTheDocument();
    expect(second).toHaveFocus();
    expect(mocked).toHaveBeenCalledTimes(1);
  });

  it("returns to the existing inspector when its selected result is chosen again", async () => {
    mocked.mockResolvedValue(answer([RECORD]));
    render(<MemoryPage grant={GRANT} />);
    const opener = await screen.findByRole("button", { name: RECORD.title });
    fireEvent.click(opener);
    const detail = screen.getByRole("region", { name: "Memory entry" });
    expect(detail).toHaveFocus();
    opener.focus();
    fireEvent.click(opener);
    expect(detail).toHaveFocus();
    expect(mocked).toHaveBeenCalledTimes(1);
  });

  it("clears an inspected entry when the completed search no longer contains it", async () => {
    mocked.mockResolvedValueOnce(answer([RECORD])).mockResolvedValueOnce(answer([
      { ...RECORD, id: "m-2", title: "Connection notes", content: "Second entry body.", tags: [] }
    ]));
    render(<MemoryPage grant={GRANT} />);
    fireEvent.click(await screen.findByRole("button", { name: RECORD.title }));
    expect(within(screen.getByRole("region", { name: "Memory entry" })).getByText(RECORD.content)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Search the machine's memory"), { target: { value: "connection" } });
    fireEvent.submit(screen.getByRole("search"));
    await screen.findByRole("button", { name: "Connection notes" });
    expect(screen.queryByRole("region", { name: "Memory entry" })).not.toBeInTheDocument();
    expect(screen.queryByText(RECORD.content)).not.toBeInTheDocument();
    expect(screen.getByText("1 entry for “connection” · newest first")).toBeInTheDocument();
  });

  it("marks a record the device reports as deleted", async () => {
    mocked.mockResolvedValue(answer([{ ...RECORD, deleted: true }]));
    render(<MemoryPage grant={GRANT} />);
    await waitFor(() => expect(screen.getByText(/· deleted/)).toBeInTheDocument());
  });
});
