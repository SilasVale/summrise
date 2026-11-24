import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FilesPage } from "./FilesPage";
import { callTool, ToolError, type Grant } from "./terminal";
import { restoreGlobal } from "./test/dom";

vi.mock("./terminal", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./terminal")>();
  return { ...actual, callTool: vi.fn() };
});

const mocked = vi.mocked(callTool);

const GRANT: Grant = { deviceId: "device-1", envelope: {} };

/// The device's answer to `files.list {path: ""}`: deliberately name-sorted by the device, so the
/// page's own order (folders first) is a behaviour a test can see.
const ROOT = [
  { name: "notes.txt", kind: "file", len: 12 },
  { name: "docs", kind: "directory", len: 0 }
];

const DOCS = [{ name: "inner.txt", kind: "file", len: 5 }];

/// The modules the download test replaces whole: jsdom's `URL` has no object-URL methods.
const originals = {
  createObjectURL: Object.getOwnPropertyDescriptor(URL, "createObjectURL"),
  revokeObjectURL: Object.getOwnPropertyDescriptor(URL, "revokeObjectURL")
};

/// The path argument of a `files.list` call, as the page keeps it in its listing.
function listedPath(args: Record<string, unknown>): string {
  return args.path as string;
}

describe("FilesPage listing", () => {
  beforeEach(() => {
    mocked.mockReset();
  });

  it("filters the listing by name without asking the device", async () => {
    mocked.mockResolvedValue(JSON.stringify(ROOT));
    render(<FilesPage grant={GRANT} />);
    const filter = await screen.findByLabelText("Filter this folder");
    expect(screen.getByText("docs/")).toBeInTheDocument();
    fireEvent.change(filter, { target: { value: "NOTE" } });

    // Case-insensitive substring over the names already on screen; the device is asked once, for
    // the listing, and never again for a keystroke.
    expect(screen.getByText("notes.txt")).toBeInTheDocument();
    expect(screen.queryByText("docs/")).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("1 of 2 entries match “NOTE”.");
    expect(mocked).toHaveBeenCalledTimes(1);
  });

  it("says when nothing matches instead of an empty list", async () => {
    mocked.mockResolvedValue(JSON.stringify(ROOT));
    render(<FilesPage grant={GRANT} />);
    await screen.findByRole("button", { name: "notes.txt" });

    fireEvent.change(screen.getByLabelText("Filter this folder"), { target: { value: "zzz" } });

    expect(screen.getByRole("status")).toHaveTextContent(
      "No entry matches “zzz” in this folder."
    );
    fireEvent.click(screen.getByRole("button", { name: "Clear filter" }));

    expect(screen.getByLabelText("Filter this folder")).toHaveValue("");
    expect(screen.getByRole("button", { name: "notes.txt" })).toBeInTheDocument();
  });

  it("does not ask a machine the fleet says is down, and says so", async () => {
    render(<FilesPage grant={GRANT} deviceOnline={false} />);
    // THE READ IS NOT SENT. The page used to sit on "Reading the device's files root…" until the
    // console's dial timed out — a stuck page where the truth was an absent machine.
    await waitFor(() => expect(mocked).not.toHaveBeenCalled());
    // **THE SHELL OWNS THE OFFLINE REASON (audit-44 F5).** The Device shell states the machine, its
    // last-seen and that nothing on the page can reach it; this tab's line names only the absence.
    expect(screen.getByText("Its files cannot be read.")).toBeInTheDocument();
    expect(screen.queryByText(/machine is offline/i)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Refresh" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Create" })).toBeDisabled();
    expect(screen.getByLabelText("Upload a file")).toBeDisabled();
  });

  it("drops a pending directory list when the machine goes offline", async () => {
    let resolveList!: (value: string) => void;
    const pending = new Promise<string>((resolve) => { resolveList = resolve; });
    mocked.mockReturnValueOnce(pending);
    const view = render(<FilesPage grant={GRANT} />);
    view.rerender(<FilesPage grant={GRANT} deviceOnline={false} />);
    await act(async () => { resolveList(JSON.stringify(ROOT)); await pending; });
    expect(screen.queryByRole("button", { name: "notes.txt" })).not.toBeInTheDocument();
    expect(screen.getByText("Its files cannot be read.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Refresh" })).toBeDisabled();
  });

  it("keeps an unconfigured shared root refusal separate from workspace files", async () => {
    mocked.mockImplementation(async (_grant, name, args) => {
      if (name === "files.list" && args.root === "shared") throw new ToolError("the shared files root is not configured", 200, null, true);
      if (name === "files.list") return JSON.stringify(ROOT);
      throw new Error(`unexpected ${name}`);
    });
    render(<FilesPage grant={GRANT} />);
    await screen.findByRole("button", { name: "notes.txt" });
    fireEvent.change(screen.getByLabelText("Root"), { target: { value: "shared" } });
    expect(await screen.findByRole("alert")).toHaveTextContent("the shared files root is not configured");
    expect(screen.queryByRole("button", { name: "notes.txt" })).not.toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("New folder"), { target: { value: "drafts" } });
    expect(screen.getByRole("button", { name: "Create" })).toBeDisabled();
    expect(screen.getByLabelText("Upload a file")).toBeDisabled();
  });

  it("keeps the upload control labelled and focusable", async () => {
    mocked.mockResolvedValue(JSON.stringify(ROOT));
    render(<FilesPage grant={GRANT} />);
    await screen.findByText("notes.txt");

    const upload = screen.getByLabelText("Upload a file");
    expect(upload).toHaveAttribute("type", "file");
    expect(upload).not.toHaveAttribute("tabindex", "-1");
  });

  it("selects the shared root and sends it on the listing", async () => {
    const calls: Record<string, unknown>[] = [];
    mocked.mockImplementation(async (_grant, name, args) => {
      if (name !== "files.list") throw new Error(`the test did not expect a ${name} call`);
      calls.push(args);
      if (args.root === "shared") return JSON.stringify([{ name: "shared.txt", kind: "file", len: 1 }]);
      return JSON.stringify(ROOT);
    });

    render(<FilesPage grant={GRANT} />);
    await screen.findByText("notes.txt");
    expect(calls[0]).toEqual({ path: "" });

    fireEvent.change(screen.getByLabelText("Root"), { target: { value: "shared" } });

    expect(await screen.findByText("shared.txt")).toBeInTheDocument();
    expect(calls[calls.length - 1]).toEqual({ path: "", root: "shared" });
    expect(screen.queryByText("notes.txt")).not.toBeInTheDocument();
  });

  it("lists the root, lists a clicked folder, and the breadcrumb says where the page is", async () => {
    const lists: string[] = [];
    mocked.mockImplementation(async (_grant, name, args) => {
      if (name !== "files.list") throw new Error(`the test did not expect a ${name} call`);
      const path = listedPath(args);
      lists.push(path);
      if (path === "") return JSON.stringify(ROOT);
      if (path === "docs") return JSON.stringify(DOCS);
      throw new Error(`the test did not expect a listing of ${path}`);
    });

    render(<FilesPage grant={GRANT} />);

    expect(await screen.findByText("2 entries: 1 folder, 1 file.")).toBeInTheDocument();
    const breadcrumb = screen.getByRole("navigation", { name: "Breadcrumb" });
    expect(within(breadcrumb).getByText("root")).toHaveAttribute("aria-current", "location");
    // The device answered name-sorted; the page shows the file manager's order — folders, files.
    expect(
      within(screen.getByRole("list"))
        .getAllByRole("button")
        .map((button) => button.textContent)
    ).toEqual(["docs/", "notes.txt"]);

    fireEvent.click(screen.getByRole("button", { name: "docs/" }));

    expect(await screen.findByText("1 entry: 1 file.")).toBeInTheDocument();
    expect(within(breadcrumb).getByRole("button", { name: "root" })).toBeInTheDocument();
    expect(within(breadcrumb).getByText("docs")).toHaveAttribute("aria-current", "location");
    expect(screen.getByRole("button", { name: "inner.txt" })).toBeInTheDocument();
    expect(lists).toEqual(["", "docs"]);
  });

  it("shows the device's refusal sentence when the root cannot be listed", async () => {
    const sentence = "the files root is not readable by this grant";
    mocked.mockImplementation(async (_grant, name) => {
      if (name === "files.list") throw new ToolError(sentence, 200, null, true);
      throw new Error(`the test did not expect a ${name} call`);
    });

    render(<FilesPage grant={GRANT} />);

    expect(await screen.findByText("The device's files could not be read.")).toBeInTheDocument();
    expect(
      screen.getByText("Could not list the files root: " + sentence)
    ).toBeInTheDocument();
    expect(screen.queryByRole("list")).not.toBeInTheDocument();
    // A root the page cannot list offers no write that must fail, and says why once. The name is
    // filled in so Create is disabled by the unreadable root, not by an empty field. Refresh is the
    // control that can still succeed, so it stays live.
    fireEvent.change(screen.getByLabelText("New folder"), { target: { value: "drafts" } });
    expect(screen.getByRole("button", { name: "Create" })).toBeDisabled();
    expect(screen.getByLabelText("Upload a file")).toBeDisabled();
    expect(
      screen.getByText(
        "New folder and Upload a file are unavailable until this root can be listed; Refresh tries again."
      )
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Refresh" })).toBeEnabled();
  });

  it("keeps the write controls live when the root is readable", async () => {
    mocked.mockResolvedValue(JSON.stringify(ROOT));

    render(<FilesPage grant={GRANT} />);
    await screen.findByRole("button", { name: "notes.txt" });

    fireEvent.change(screen.getByLabelText("New folder"), { target: { value: "drafts" } });
    expect(screen.getByRole("button", { name: "Create" })).toBeEnabled();
    expect(screen.getByLabelText("Upload a file")).toBeEnabled();
    expect(
      screen.queryByText(
        "New folder and Upload a file are unavailable until this root can be listed; Refresh tries again."
      )
    ).not.toBeInTheDocument();
  });
});

describe("FilesPage opening a file", () => {
  beforeEach(() => {
    mocked.mockReset();
  });

  it("reads a text file with files.read, shows its text, and Close returns to the list", async () => {
    mocked.mockImplementation(async (_grant, name) => {
      if (name === "files.list") return JSON.stringify(ROOT);
      if (name === "files.read") return "hello world\n";
      throw new Error(`the test did not expect a ${name} call`);
    });

    render(<FilesPage grant={GRANT} />);
    await screen.findByRole("button", { name: "notes.txt" });

    fireEvent.click(screen.getByRole("button", { name: "notes.txt" }));

    expect(await screen.findByLabelText("Text")).toHaveValue("hello world\n");
    expect(screen.getByText("/notes.txt")).toBeInTheDocument();
    expect(mocked).toHaveBeenCalledWith(GRANT, "files.read", { path: "notes.txt" });
    // The device answered, so the page is not claiming a failure.
    expect(screen.queryByText(/Could not open/)).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Close" }));

    // Nothing has been typed, so there is nothing to ask about.
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Text")).not.toBeInTheDocument();
    expect(screen.queryByText("/notes.txt")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "notes.txt" })).toBeInTheDocument();
  });

  it("drops the previous grant's file read after switching machines", async () => {
    const nextGrant: Grant = { deviceId: "device-2", envelope: {} };
    let resolveFile!: (value: string) => void;
    const pending = new Promise<string>((resolve) => { resolveFile = resolve; });
    mocked.mockImplementation(async (grant, name) => {
      if (name === "files.list") return JSON.stringify(grant === GRANT ? ROOT : DOCS);
      if (name === "files.read") return pending;
      throw new Error(`unexpected ${name}`);
    });
    const view = render(<FilesPage grant={GRANT} />);
    fireEvent.click(await screen.findByRole("button", { name: "notes.txt" }));
    view.rerender(<FilesPage grant={nextGrant} />);
    await screen.findByRole("button", { name: "inner.txt" });
    await act(async () => { resolveFile("previous machine text"); await pending; });
    expect(screen.queryByLabelText("Text")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "inner.txt" })).toBeInTheDocument();
    expect(screen.queryByText("previous machine text")).not.toBeInTheDocument();
  });

  it("writes the edited text with files.write and says it saved", async () => {
    const lists: string[] = [];
    mocked.mockImplementation(async (_grant, name, args) => {
      if (name === "files.list") {
        lists.push(listedPath(args));
        return JSON.stringify(ROOT);
      }
      if (name === "files.read") return "hello world\n";
      if (name === "files.write") return JSON.stringify({ path: "notes.txt", bytes: 12 });
      throw new Error(`the test did not expect a ${name} call`);
    });

    render(<FilesPage grant={GRANT} />);
    await screen.findByRole("button", { name: "notes.txt" });
    fireEvent.click(screen.getByRole("button", { name: "notes.txt" }));
    const editor = await screen.findByLabelText("Text");
    // A freshly opened file has nothing to save.
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();

    fireEvent.change(editor, { target: { value: "hello again\n" } });
    expect(screen.getByText("Unsaved changes.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByText("Saved /notes.txt (12 B).")).toBeInTheDocument();
    expect(mocked).toHaveBeenCalledWith(GRANT, "files.write", {
      path: "notes.txt",
      content: "hello again\n"
    });
    expect(screen.getByText("No unsaved changes.")).toBeInTheDocument();
    // The listing showed the old size, so the page re-lists the folder the file is in.
    expect(lists).toEqual(["", ""]);
  });

  it("keeps the draft after a refused write and can save the same draft again", async () => {
    let writes = 0;
    mocked.mockImplementation(async (_grant, name) => {
      if (name === "files.list") return JSON.stringify(ROOT);
      if (name === "files.read") return "saved text";
      if (name === "files.write") {
        if (++writes === 1) throw new ToolError("write refused", 200, null, true);
        return JSON.stringify({ path: "notes.txt", bytes: 5 });
      }
      throw new Error(`unexpected ${name}`);
    });
    render(<FilesPage grant={GRANT} />);
    fireEvent.click(await screen.findByRole("button", { name: "notes.txt" }));
    fireEvent.change(await screen.findByLabelText("Text"), { target: { value: "draft" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText("Could not save /notes.txt: write refused");
    expect(screen.getByLabelText("Text")).toHaveValue("draft");
    expect(screen.getByText("Unsaved changes.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText("Saved /notes.txt (5 B).");
    expect(screen.getByLabelText("Text")).toHaveValue("draft");
    expect(screen.getByText("No unsaved changes.")).toBeInTheDocument();
  });

  it("reads a file at the 1 MiB boundary but refuses an upload one byte over it", async () => {
    mocked.mockImplementation(async (_grant, name) => {
      if (name === "files.list") return JSON.stringify([{ name: "boundary.txt", kind: "file", len: 1_048_576 }]);
      if (name === "files.read") return "boundary text";
      throw new Error(`unexpected ${name}`);
    });
    render(<FilesPage grant={GRANT} />);
    fireEvent.click(await screen.findByRole("button", { name: "boundary.txt" }));
    expect(await screen.findByLabelText("Text")).toHaveValue("boundary text");
    expect(mocked).toHaveBeenCalledWith(GRANT, "files.read", { path: "boundary.txt" });
    fireEvent.change(screen.getByLabelText("Upload a file"), {
      target: { files: [new File([new Uint8Array(1_048_577)], "large.bin")] }
    });
    expect(screen.getByText(/an upload is capped at 1 MiB/)).toBeInTheDocument();
    expect(mocked).not.toHaveBeenCalledWith(GRANT, "files.write", expect.anything());
    expect(screen.getByLabelText("Text")).toHaveValue("boundary text");
  });

  it("creates a folder under the folder on screen and re-lists it", async () => {
    let docs = [] as { name: string; kind: string; len: number }[];
    const lists: string[] = [];
    mocked.mockImplementation(async (_grant, name, args) => {
      if (name === "files.list") {
        const path = listedPath(args);
        lists.push(path);
        return JSON.stringify(path === "" ? ROOT : docs);
      }
      if (name === "files.mkdir") {
        docs = [{ name: "reports", kind: "directory", len: 0 }];
        return JSON.stringify({ path: "docs/reports" });
      }
      throw new Error(`the test did not expect a ${name} call`);
    });

    render(<FilesPage grant={GRANT} />);
    await screen.findByRole("button", { name: "docs/" });
    fireEvent.click(screen.getByRole("button", { name: "docs/" }));
    await screen.findByText("This folder is empty. Use New folder or Upload a file to add something.");

    fireEvent.change(screen.getByLabelText("New folder"), {
      target: { value: "reports" }
    });
    fireEvent.click(screen.getByRole("button", { name: "Create" }));

    expect(await screen.findByText("Created /docs/reports.")).toBeInTheDocument();
    expect(mocked).toHaveBeenCalledWith(GRANT, "files.mkdir", { path: "docs/reports" });
    expect(await screen.findByRole("button", { name: "reports/" })).toBeInTheDocument();
    expect(lists).toEqual(["", "docs", "docs"]);
  });
});

describe("FilesPage closing a dirty editor", () => {
  beforeEach(() => {
    mocked.mockReset();
  });

  /// Open `notes.txt`, let the device answer, and type a draft that is not what the page read.
  async function openAndEdit(rows = ROOT) {
    mocked.mockImplementation(async (_grant, name, args) => {
      if (name === "files.list") return JSON.stringify(rows);
      if (name === "files.read") return args.path === "other.txt" ? "second file\n" : "hello world\n";
      if (name === "files.write") return JSON.stringify({ path: "notes.txt", bytes: 12 });
      throw new Error(`the test did not expect a ${name} call`);
    });

    render(<FilesPage grant={GRANT} />);
    await screen.findByRole("button", { name: "notes.txt" });
    fireEvent.click(screen.getByRole("button", { name: "notes.txt" }));
    const editor = await screen.findByLabelText("Text");
    fireEvent.change(editor, { target: { value: "hello again\n" } });
    expect(screen.getByText("Unsaved changes.")).toBeInTheDocument();
    return editor;
  }

  it("keeps the dirty draft on a root change until discard is confirmed", async () => {
    await openAndEdit();
    const root = screen.getByLabelText("Root");
    fireEvent.change(root, { target: { value: "shared" } });

    expect(screen.getByLabelText("Text")).toHaveValue("hello again\n");
    const dialog = screen.getByRole("dialog", { name: "Discard unsaved changes?" });
    expect(within(dialog).getByRole("button", { name: "Cancel" })).toHaveFocus();
    expect(root).toHaveValue("workspace");
    expect(mocked).not.toHaveBeenCalledWith(GRANT, "files.list", { path: "", root: "shared" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.getByLabelText("Text")).toHaveValue("hello again\n");

    fireEvent.change(root, { target: { value: "shared" } });
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Discard changes" }));
    await waitFor(() => expect(mocked).toHaveBeenCalledWith(GRANT, "files.list", { path: "", root: "shared" }));
    expect(root).toHaveValue("shared");
    expect(screen.queryByLabelText("Text")).not.toBeInTheDocument();
    expect(mocked).not.toHaveBeenCalledWith(GRANT, "files.write", expect.anything());
  });

  it("keeps the dirty draft until a replacement file is confirmed", async () => {
    await openAndEdit([...ROOT, { name: "other.txt", kind: "file", len: 12 }]);
    fireEvent.click(screen.getByRole("button", { name: "other.txt" }));

    expect(screen.getByLabelText("Text")).toHaveValue("hello again\n");
    expect(mocked).not.toHaveBeenCalledWith(GRANT, "files.read", { path: "other.txt" });
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Cancel" }));
    expect(screen.getByLabelText("Text")).toHaveValue("hello again\n");
    fireEvent.click(screen.getByRole("button", { name: "other.txt" }));
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Discard changes" }));

    await waitFor(() => expect(screen.getByLabelText("Text")).toHaveValue("second file\n"));
    expect(mocked).toHaveBeenCalledWith(GRANT, "files.read", { path: "other.txt" });
    expect(mocked).not.toHaveBeenCalledWith(GRANT, "files.write", expect.anything());
  });

  it("asks before an over-cap file replaces a dirty draft without sending a read", async () => {
    await openAndEdit([...ROOT, { name: "large.bin", kind: "file", len: 1_048_577 }]);
    fireEvent.click(screen.getByRole("button", { name: "large.bin" }));
    expect(screen.getByLabelText("Text")).toHaveValue("hello again\n");
    fireEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Discard changes" }));
    expect(screen.getByText("Too large to read.")).toBeInTheDocument();
    expect(mocked).not.toHaveBeenCalledWith(GRANT, "files.read", { path: "large.bin" });
    expect(screen.queryByRole("button", { name: "Open as base64" })).not.toBeInTheDocument();
  });

  it("asks before discarding and leaves the editor up until the discard is confirmed", async () => {
    await openAndEdit();

    fireEvent.click(screen.getByRole("button", { name: "Close" }));

    // The question is named, and the body names the file and what is lost.
    const dialog = screen.getByRole("dialog", { name: "Discard unsaved changes?" });
    expect(dialog).toHaveTextContent("unsaved changes to /notes.txt.");
    expect(dialog).toHaveTextContent("the file on the device keeps the bytes last saved");
    // The viewer and the draft stay while the question is open, and the path is on both the
    // viewer's header and the question.
    expect(screen.getByLabelText("Text")).toHaveValue("hello again\n");
    expect(screen.getAllByText("/notes.txt")).toHaveLength(2);

    fireEvent.click(within(dialog).getByRole("button", { name: "Discard changes" }));

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Text")).not.toBeInTheDocument();
    expect(screen.queryByText("/notes.txt")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "notes.txt" })).toBeInTheDocument();
    // Discarding is not saving: nothing was written to the device.
    expect(mocked).not.toHaveBeenCalledWith(GRANT, "files.write", expect.anything());
  });

  it("cancelling keeps the editor and the draft, and Save still works", async () => {
    await openAndEdit();
    fireEvent.click(screen.getByRole("button", { name: "Close" }));

    fireEvent.click(
      within(screen.getByRole("dialog", { name: "Discard unsaved changes?" })).getByRole("button", {
        name: "Cancel"
      })
    );

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Text")).toHaveValue("hello again\n");
    expect(screen.getByText("Unsaved changes.")).toBeInTheDocument();
    expect(screen.getByText("/notes.txt")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByText("Saved /notes.txt (12 B).")).toBeInTheDocument();
    expect(mocked).toHaveBeenCalledWith(GRANT, "files.write", {
      path: "notes.txt",
      content: "hello again\n"
    });
    expect(screen.getByText("No unsaved changes.")).toBeInTheDocument();
  });
});

describe("FilesPage review boundaries", () => {
  beforeEach(() => {
    mocked.mockReset();
  });

  it("preserves the draft and directory through temporary offline and reconnect", async () => {
    mocked.mockImplementation(async (_grant, name, args) => {
      if (name === "files.list") return JSON.stringify(args.path === "docs" ? DOCS : ROOT);
      if (name === "files.read") return "saved";
      if (name === "files.write") return JSON.stringify({ path: "docs/inner.txt", bytes: 5 });
      throw new Error(`unexpected ${name}`);
    });
    const view = render(<FilesPage grant={GRANT} />);
    fireEvent.click(await screen.findByRole("button", { name: "docs/" }));
    fireEvent.click(await screen.findByRole("button", { name: "inner.txt" }));
    fireEvent.change(await screen.findByLabelText("Text"), { target: { value: "draft" } });
    const callsBeforeOffline = mocked.mock.calls.length;
    view.rerender(<FilesPage grant={GRANT} deviceOnline={false} />);
    expect(screen.getByLabelText("Text")).toHaveValue("draft");
    expect(screen.getByLabelText("Text")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    expect(mocked).toHaveBeenCalledTimes(callsBeforeOffline);
    expect(within(screen.getByRole("navigation", { name: "Breadcrumb" })).getByText("docs")).toHaveAttribute("aria-current", "location");
    view.rerender(<FilesPage grant={GRANT} deviceOnline />);
    await waitFor(() => expect(mocked.mock.calls.filter((call) => call[1] === "files.list" && call[2].path === "docs")).toHaveLength(2));
    expect(screen.getByLabelText("Text")).toHaveValue("draft");
    expect(screen.getByLabelText("Text")).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await screen.findByText("Saved /docs/inner.txt (5 B).");
    expect(mocked).toHaveBeenCalledWith(GRANT, "files.write", { path: "docs/inner.txt", content: "draft" });
  });

  it("suspends editing the old clean file while its replacement read is pending", async () => {
    let resolveFile!: (value: string) => void;
    const pending = new Promise<string>((resolve) => { resolveFile = resolve; });
    mocked.mockImplementation(async (_grant, name, args) => {
      if (name === "files.list") return JSON.stringify([...ROOT, { name: "other.txt", kind: "file", len: 5 }]);
      if (name === "files.read") return args.path === "other.txt" ? pending : "first file";
      throw new Error(`unexpected ${name}`);
    });
    render(<FilesPage grant={GRANT} />);
    fireEvent.click(await screen.findByRole("button", { name: "notes.txt" }));
    const editor = await screen.findByLabelText("Text");
    expect(editor).toBeEnabled();
    fireEvent.click(screen.getByRole("button", { name: "other.txt" }));
    // A real user cannot type into a disabled textarea; do not fire a synthetic change through it.
    expect(editor).toBeDisabled();
    expect(editor).toHaveValue("first file");
    await act(async () => { resolveFile("second file"); await pending; });
    expect(screen.getByLabelText("Text")).toHaveValue("second file");
    expect(screen.getByLabelText("Text")).toBeEnabled();
  });

  const mutationCases = (["save", "folder", "upload"] as const).flatMap((operation) =>
    (["root", "offline"] as const).flatMap((transition) =>
      (["success", "failure"] as const).map((outcome) => ({ operation, transition, outcome }))
    )
  );
  it.each(mutationCases)("isolates a pending $operation $outcome after a $transition transition from a new operation", async ({ operation, transition, outcome }) => {
    let resolveOld!: (value: string) => void;
    let rejectOld!: (reason: Error) => void;
    let resolveNew!: (value: string) => void;
    const oldRequest = new Promise<string>((resolve, reject) => { resolveOld = resolve; rejectOld = reject; });
    const newRequest = new Promise<string>((resolve) => { resolveNew = resolve; });
    let mutations = 0;
    mocked.mockImplementation(async (_grant, name, args) => {
      if (name === "files.list") return JSON.stringify(args.root === "shared" ? [{ name: "shared.txt", kind: "file", len: 5 }] : ROOT);
      if (name === "files.read") return "saved";
      if (name === "files.write" || name === "files.mkdir") return ++mutations === 1 ? oldRequest : newRequest;
      throw new Error(`unexpected ${name}`);
    });
    const view = render(<FilesPage grant={GRANT} />);
    await screen.findByRole("button", { name: "notes.txt" });
    const startOperation = async (next: boolean) => {
      const path = next && transition === "root" ? "shared.txt" : "notes.txt";
      if (operation === "save") {
        if (!screen.queryByLabelText("Text")) {
          fireEvent.click(screen.getByRole("button", { name: path }));
          await screen.findByLabelText("Text");
        }
        fireEvent.change(screen.getByLabelText("Text"), { target: { value: next ? "new" : "old" } });
        fireEvent.click(screen.getByRole("button", { name: "Save" }));
      } else if (operation === "folder") {
        fireEvent.change(screen.getByLabelText("New folder"), { target: { value: next ? "new-folder" : "old-folder" } });
        fireEvent.click(screen.getByRole("button", { name: "Create" }));
      } else {
        fireEvent.change(screen.getByLabelText("Upload a file"), { target: { files: [new File([next ? "new" : "old"], "upload.txt")] } });
      }
      await waitFor(() => expect(mutations).toBe(next ? 2 : 1));
    };
    await startOperation(false);
    if (transition === "root") {
      fireEvent.change(screen.getByLabelText("Root"), { target: { value: "shared" } });
      const dialog = screen.queryByRole("dialog");
      if (dialog) fireEvent.click(within(dialog).getByRole("button", { name: "Discard changes" }));
      await screen.findByRole("button", { name: "shared.txt" });
    } else {
      view.rerender(<FilesPage grant={GRANT} deviceOnline={false} />);
      view.rerender(<FilesPage grant={GRANT} deviceOnline />);
      await waitFor(() => expect(mocked.mock.calls.filter((call) => call[1] === "files.list")).toHaveLength(2));
      await screen.findByRole("button", { name: "notes.txt" });
    }
    await startOperation(true);
    const listsBeforeOldAnswer = mocked.mock.calls.filter((call) => call[1] === "files.list").length;
    await act(async () => {
      if (outcome === "success") resolveOld(JSON.stringify({ path: "old", bytes: 3 }));
      else rejectOld(new ToolError("old operation refused", 200, null, true));
      await oldRequest.catch(() => {});
    });
    expect(mocked.mock.calls.filter((call) => call[1] === "files.list")).toHaveLength(listsBeforeOldAnswer);
    expect(screen.queryByText(/^(Saved|Created|Uploaded) /)).not.toBeInTheDocument();
    expect(screen.queryByText(/old operation refused/)).not.toBeInTheDocument();
    if (operation === "save") expect(screen.getByRole("button", { name: "Saving…" })).toBeDisabled();
    else if (operation === "folder") expect(screen.getByRole("button", { name: "Creating…" })).toBeDisabled();
    else expect(screen.getByLabelText("Upload a file")).toBeDisabled();
    const rootArgs = transition === "root" ? { root: "shared" } : {};
    const expectedArgs = operation === "save"
      ? { path: transition === "root" ? "shared.txt" : "notes.txt", content: "new", ...rootArgs }
      : operation === "folder"
        ? { path: "new-folder", ...rootArgs }
        : { path: "upload.txt", content: "bmV3", encoding: "base64", ...rootArgs };
    expect(mocked).toHaveBeenLastCalledWith(GRANT, operation === "folder" ? "files.mkdir" : "files.write", expectedArgs);
    await act(async () => { resolveNew(JSON.stringify({ path: "new", bytes: 3 })); await newRequest; });
    await waitFor(() => expect(mocked.mock.calls.filter((call) => call[1] === "files.list")).toHaveLength(listsBeforeOldAnswer + 1));
    expect(screen.getByRole("button", { name: transition === "root" ? "shared.txt" : "notes.txt" })).toBeInTheDocument();
    if (operation === "folder") expect(screen.getByLabelText("New folder")).toHaveValue("");
    if (operation === "upload") expect(screen.getByLabelText("Upload a file")).toBeEnabled();
  });

  it.each(["root", "offline", "unmount"] as const)("does not dispatch an upload whose file read finishes after %s", async (transition) => {
    mocked.mockImplementation(async (_grant, name, args) => {
      if (name === "files.list") return JSON.stringify(args.root === "shared" ? [{ name: "shared.txt", kind: "file", len: 5 }] : ROOT);
      if (name === "files.write") return JSON.stringify({ path: "upload.txt", bytes: 3 });
      throw new Error(`unexpected ${name}`);
    });
    let reader!: FileReader;
    const browserRead = vi.spyOn(FileReader.prototype, "readAsDataURL").mockImplementationOnce(function (this: FileReader) { reader = this; });
    try {
      const view = render(<FilesPage grant={GRANT} />);
      await screen.findByRole("button", { name: "notes.txt" });
      fireEvent.change(screen.getByLabelText("Upload a file"), { target: { files: [new File(["old"], "upload.txt")] } });
      if (transition === "root") {
        fireEvent.change(screen.getByLabelText("Root"), { target: { value: "shared" } });
        await screen.findByRole("button", { name: "shared.txt" });
      } else if (transition === "offline") view.rerender(<FilesPage grant={GRANT} deviceOnline={false} />);
      else view.unmount();
      await act(async () => {
        Object.defineProperty(reader, "result", { configurable: true, value: "data:application/octet-stream;base64,b2xk" });
        reader.onload?.call(reader, new ProgressEvent("load") as ProgressEvent<FileReader>);
      });
      expect(mocked.mock.calls.filter((call) => call[1] === "files.write")).toHaveLength(0);
      if (transition === "root") expect(screen.getByRole("button", { name: "shared.txt" })).toBeInTheDocument();
    } finally {
      browserRead.mockRestore();
    }
  });
});

describe("FilesPage binary files", () => {
  beforeEach(() => {
    mocked.mockReset();
  });

  afterEach(() => {
    restoreGlobal(URL, "createObjectURL", originals.createObjectURL);
    restoreGlobal(URL, "revokeObjectURL", originals.revokeObjectURL);
  });

  it("turns a UTF-8 refusal into a base64 download of the file's exact bytes", async () => {
    const refusal = 'files.read: the file is not UTF-8 text; ask for encoding "base64"';
    mocked.mockImplementation(async (_grant, name, args) => {
      if (name === "files.list") {
        return JSON.stringify([{ name: "blob.bin", kind: "file", len: 4 }]);
      }
      if (name === "files.read") {
        if (args.encoding === "base64") return "AAECAw==";
        throw new ToolError(refusal, 200, null, true);
      }
      throw new Error(`the test did not expect a ${name} call`);
    });

    render(<FilesPage grant={GRANT} />);
    await screen.findByRole("button", { name: "blob.bin" });
    fireEvent.click(screen.getByRole("button", { name: "blob.bin" }));

    // The refusal is the device's sentence and a fact about the file, not an empty editor.
    expect(await screen.findByText(refusal)).toBeInTheDocument();
    expect(screen.queryByLabelText("Text")).not.toBeInTheDocument();
    expect(screen.getByText(/The device refused a UTF-8 read\./)).toBeInTheDocument();
    // The first read asked for text, with no encoding; only the retry names base64.
    expect(mocked).toHaveBeenCalledWith(GRANT, "files.read", { path: "blob.bin" });

    fireEvent.click(screen.getByRole("button", { name: "Open as base64" }));

    expect(await screen.findByText(/The device returned 4 B as base64\./)).toBeInTheDocument();
    expect(mocked).toHaveBeenCalledWith(GRANT, "files.read", {
      path: "blob.bin",
      encoding: "base64"
    });

    const createObjectURL = vi.fn((_blob: Blob) => "blob:files-download");
    const revokeObjectURL = vi.fn();
    Object.defineProperty(URL, "createObjectURL", {
      configurable: true,
      value: createObjectURL
    });
    Object.defineProperty(URL, "revokeObjectURL", {
      configurable: true,
      value: revokeObjectURL
    });
    const click = vi
      .spyOn(HTMLAnchorElement.prototype, "click")
      .mockImplementation(() => {});

    vi.useFakeTimers();
    try {
      fireEvent.click(screen.getByRole("button", { name: "Download" }));

      expect(createObjectURL).toHaveBeenCalledTimes(1);
      const blob = createObjectURL.mock.calls[0][0];
      expect(blob.size).toBe(4);
      expect(new Uint8Array(await blob.arrayBuffer())).toEqual(
        new Uint8Array([0x00, 0x01, 0x02, 0x03])
      );
      const anchor = click.mock.instances[0] as HTMLAnchorElement;
      expect(anchor.download).toBe("blob.bin");
      expect(revokeObjectURL).not.toHaveBeenCalled();

      vi.runAllTimers();
      expect(revokeObjectURL).toHaveBeenCalledWith("blob:files-download");
    } finally {
      vi.useRealTimers();
    }
  });
});
