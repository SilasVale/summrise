import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { FilesPage } from "./FilesPage";
import { callTool, ToolError, type Grant } from "./terminal";

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

/// Put a replaced global back exactly as it was, or remove the property when jsdom never had it.
function restore(target: object, key: string, descriptor?: PropertyDescriptor) {
  if (descriptor) Object.defineProperty(target, key, descriptor);
  else Reflect.deleteProperty(target, key);
}

/// The path argument of a `files.list` call, as the page keeps it in its listing.
function listedPath(args: Record<string, unknown>): string {
  return args.path as string;
}

describe("FilesPage listing", () => {
  beforeEach(() => {
    mocked.mockReset();
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
    expect(screen.getByRole("button", { name: "Refresh" })).toBeEnabled();
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

    expect(screen.queryByLabelText("Text")).not.toBeInTheDocument();
    expect(screen.queryByText("/notes.txt")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "notes.txt" })).toBeInTheDocument();
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
    await screen.findByText("This folder is empty; create one above or upload a file into it.");

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

describe("FilesPage binary files", () => {
  beforeEach(() => {
    mocked.mockReset();
  });

  afterEach(() => {
    restore(URL, "createObjectURL", originals.createObjectURL);
    restore(URL, "revokeObjectURL", originals.revokeObjectURL);
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
