import { fireEvent, render, screen, waitFor } from "@testing-library/react";
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
  it("lists what the device answers, newest first, with the record's provenance", async () => {
    mocked.mockResolvedValue(answer([RECORD]));
    render(<MemoryPage grant={GRANT} />);
    await waitFor(() => expect(screen.getByText(RECORD.title)).toBeInTheDocument());
    expect(mocked).toHaveBeenCalledWith(GRANT, "memory.list", { limit: 50, include_deleted: true });
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

  it("retires a record with the SOFT delete, and can bring it back", async () => {
    mocked.mockResolvedValue(answer([RECORD]));
    render(<MemoryPage grant={GRANT} />);
    await waitFor(() => expect(screen.getByText(RECORD.title)).toBeInTheDocument());
    mocked.mockResolvedValueOnce(JSON.stringify({ ok: true }));
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() =>
      expect(mocked).toHaveBeenCalledWith(GRANT, "memory.update", { id: RECORD.id, deleted: true })
    );
    // the list is re-read with the retired records included, or a restore would be unreachable
    await waitFor(() =>
      expect(mocked).toHaveBeenCalledWith(GRANT, "memory.list", { limit: 50, include_deleted: true })
    );
  });

  it("offers Restore on a record the device calls deleted, and says the record is still there", async () => {
    mocked.mockResolvedValue(answer([{ ...RECORD, deleted: true }]));
    render(<MemoryPage grant={GRANT} />);
    await waitFor(() => expect(screen.getByRole("button", { name: "Restore" })).toBeInTheDocument());
    mocked.mockResolvedValueOnce(JSON.stringify({ ok: true }));
    fireEvent.click(screen.getByRole("button", { name: "Restore" }));
    await waitFor(() =>
      expect(mocked).toHaveBeenCalledWith(GRANT, "memory.update", { id: RECORD.id, deleted: false })
    );
  });

  it("marks a record the device reports as deleted", async () => {
    mocked.mockResolvedValue(answer([{ ...RECORD, deleted: true }]));
    render(<MemoryPage grant={GRANT} />);
    await waitFor(() => expect(screen.getByText(/· deleted/)).toBeInTheDocument());
  });
});
