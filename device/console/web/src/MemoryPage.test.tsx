import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MemoryPage } from "./MemoryPage";
import { callTool, ToolError } from "./terminal";
import { restoreGlobal } from "./test/dom";

vi.mock("./terminal", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./terminal")>();
  return { ...actual, callTool: vi.fn() };
});

const mocked = vi.mocked(callTool);

const GRANT = { deviceId: "device-1", envelope: {} };

/// The device's answer to `memory.export`: a one-entry prefix of a store too large for the 2 MiB cap.
const EXPORT = {
  text: '{"id":"1"}\n',
  entries: 1,
  truncated: true,
  total_bytes: 9_000_000
};

/// The modules the tests replace whole: jsdom has no clipboard, and `URL` has no object-URL methods.
const originals = {
  clipboard: Object.getOwnPropertyDescriptor(navigator, "clipboard"),
  createObjectURL: Object.getOwnPropertyDescriptor(URL, "createObjectURL"),
  revokeObjectURL: Object.getOwnPropertyDescriptor(URL, "revokeObjectURL")
};

/// Render the page over a device that remembers nothing, then open the export dialog.
async function openExport(answer: typeof EXPORT = EXPORT): Promise<HTMLElement> {
  mocked.mockImplementation(async (_grant, name) => {
    if (name === "memory.list") return "[]";
    if (name === "memory.export") return JSON.stringify(answer);
    throw new Error(`the test did not expect a ${name} call`);
  });
  render(<MemoryPage grant={GRANT} />);
  await screen.findByText("The device remembers nothing yet.");
  fireEvent.click(screen.getByRole("button", { name: "Export" }));
  return screen.findByRole("dialog");
}

describe("MemoryPage export", () => {
  beforeEach(() => {
    mocked.mockReset();
  });

  afterEach(() => {
    restoreGlobal(navigator, "clipboard", originals.clipboard);
    restoreGlobal(URL, "createObjectURL", originals.createObjectURL);
    restoreGlobal(URL, "revokeObjectURL", originals.revokeObjectURL);
  });

  it("shows the whole JSONL and a sentence naming the 2 MiB cap, the cut and the store's bytes", async () => {
    const dialog = await openExport();
    expect(within(dialog).getByRole("textbox")).toHaveValue('{"id":"1"}\n');
    expect(dialog).toHaveTextContent("2 MiB");
    expect(dialog).toHaveTextContent("2,097,152 bytes");
    expect(dialog).toHaveTextContent("was cut");
    expect(dialog).toHaveTextContent("1 entry");
    expect(dialog).toHaveTextContent("9,000,000 bytes");
  });

  it("copies the whole text rather than a display slice, and says Copied", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText }
    });

    const dialog = await openExport();
    fireEvent.click(within(dialog).getByRole("button", { name: "Copy" }));

    expect(writeText).toHaveBeenCalledWith('{"id":"1"}\n');
    expect(await within(dialog).findByText("Copied")).toBeInTheDocument();
  });

  it("downloads the JSONL as a dated file and revokes its object URL", async () => {
    const dialog = await openExport();
    const createObjectURL = vi.fn((_blob: Blob) => "blob:memory-export");
    const revokeObjectURL = vi.fn();
    Object.defineProperty(URL, "createObjectURL", { configurable: true, value: createObjectURL });
    Object.defineProperty(URL, "revokeObjectURL", { configurable: true, value: revokeObjectURL });
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});

    vi.useFakeTimers();
    try {
      fireEvent.click(within(dialog).getByRole("button", { name: "Download" }));

      expect(createObjectURL).toHaveBeenCalledTimes(1);
      expect(await createObjectURL.mock.calls[0][0].text()).toBe('{"id":"1"}\n');
      const anchor = click.mock.instances[0] as HTMLAnchorElement;
      expect(anchor.download).toMatch(/^summrise-memory-\d{4}-\d{2}-\d{2}\.jsonl$/);
      expect(revokeObjectURL).not.toHaveBeenCalled();

      vi.runAllTimers();
      expect(revokeObjectURL).toHaveBeenCalledWith("blob:memory-export");
    } finally {
      vi.useRealTimers();
    }
  });

  it("calls an uncut answer the whole store, not a cut one", async () => {
    const dialog = await openExport({
      text: '{"id":"1"}\n',
      entries: 1,
      truncated: false,
      total_bytes: 11
    });
    expect(dialog).toHaveTextContent("The whole store");
    expect(dialog).toHaveTextContent("this answer fits");
    expect(dialog).not.toHaveTextContent("was cut");
  });

  it("shows the device's refusal, opens no dialog and re-enables the button", async () => {
    mocked.mockImplementation(async (_grant, name) => {
      if (name === "memory.list") return "[]";
      throw new ToolError("no memory store", 200, null, true);
    });
    render(<MemoryPage grant={GRANT} />);
    await screen.findByText("The device remembers nothing yet.");

    fireEvent.click(screen.getByRole("button", { name: "Export" }));

    expect(await screen.findByText(/no memory store/)).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Export" })).toBeEnabled();
  });

  it("says a transport failure never reached the device", async () => {
    mocked.mockImplementation(async (_grant, name) => {
      if (name === "memory.list") return "[]";
      throw new ToolError("Failed to fetch", 0);
    });
    render(<MemoryPage grant={GRANT} />);
    await screen.findByText("The device remembers nothing yet.");

    fireEvent.click(screen.getByRole("button", { name: "Export" }));

    expect(await screen.findByText(/never reached the device/)).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});

describe("MemoryPage create", () => {
  const TEXT = "rotate the cert on the 1st";

  beforeEach(() => {
    mocked.mockReset();
  });

  /// Open the create dialog; the caller has already primed the mock with the initial list answer.
  async function openCreate(): Promise<HTMLElement> {
    render(<MemoryPage grant={GRANT} />);
    await screen.findByText("The device remembers nothing yet.");
    fireEvent.click(screen.getByRole("button", { name: "New entry" }));
    return screen.findByRole("dialog", { name: "Remember something" });
  }

  it("appends the entry with its tags, closes the dialog and re-reads the list", async () => {
    let appended = false;
    const lists: unknown[] = [];
    mocked.mockImplementation(async (_grant, name, args) => {
      if (name === "memory.list") {
        lists.push(args);
        return appended ? JSON.stringify([{ id: "m1", text: TEXT, tags: ["ops"] }]) : "[]";
      }
      if (name === "memory.append") {
        appended = true;
        return JSON.stringify({ appended: true, id: "m1" });
      }
      throw new Error(`the test did not expect a ${name} call`);
    });

    const dialog = await openCreate();
    fireEvent.change(within(dialog).getByLabelText("Text"), { target: { value: TEXT } });
    fireEvent.change(within(dialog).getByLabelText("Tags"), { target: { value: "ops" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Add tag" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "Remember" }));

    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(mocked).toHaveBeenCalledWith(GRANT, "memory.append", { text: TEXT, tags: ["ops"] });
    expect(await screen.findByText(TEXT)).toBeInTheDocument();
    expect(lists).toHaveLength(2);
    expect(await screen.findByText("Remembered m1.")).toBeInTheDocument();
  });

  it("re-reads the filter that is showing", async () => {
    const lists: unknown[] = [];
    mocked.mockImplementation(async (_grant, name, args) => {
      if (name === "memory.list") {
        lists.push(args);
        return "[]";
      }
      if (name === "memory.append") return JSON.stringify({ appended: true, id: "m2" });
      throw new Error(`the test did not expect a ${name} call`);
    });

    render(<MemoryPage grant={GRANT} />);
    await screen.findByText("The device remembers nothing yet.");
    fireEvent.change(screen.getByLabelText("Filter by tag"), { target: { value: "ops" } });
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    await screen.findByText("Nothing is tagged ops.");

    fireEvent.click(screen.getByRole("button", { name: "New entry" }));
    const dialog = await screen.findByRole("dialog", { name: "Remember something" });
    fireEvent.change(within(dialog).getByLabelText("Text"), { target: { value: "a tagged fact" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Remember" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());

    expect(lists).toEqual([{ limit: 100 }, { limit: 100, tag: "ops" }, { limit: 100, tag: "ops" }]);
  });

  it("keeps Remember disabled until the text is not blank", async () => {
    mocked.mockImplementation(async (_grant, name) => {
      if (name === "memory.list") return "[]";
      throw new Error(`the test did not expect a ${name} call`);
    });

    const dialog = await openCreate();
    const remember = within(dialog).getByRole("button", { name: "Remember" });
    expect(remember).toBeDisabled();

    fireEvent.change(within(dialog).getByLabelText("Text"), { target: { value: "   " } });
    expect(remember).toBeDisabled();

    fireEvent.change(within(dialog).getByLabelText("Text"), { target: { value: TEXT } });
    expect(remember).toBeEnabled();
  });

  it("keeps the dialog open and shows the device's refusal", async () => {
    mocked.mockImplementation(async (_grant, name) => {
      if (name === "memory.list") return "[]";
      if (name === "memory.append") {
        throw new ToolError("the entry has 33 tags; the cap is 32", 200, null, true);
      }
      throw new Error(`the test did not expect a ${name} call`);
    });

    const dialog = await openCreate();
    fireEvent.change(within(dialog).getByLabelText("Text"), { target: { value: TEXT } });
    fireEvent.click(within(dialog).getByRole("button", { name: "Remember" }));

    expect(
      await within(dialog).findByText("Could not remember: the entry has 33 tags; the cap is 32")
    ).toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Remember something" })).toBeInTheDocument();
    expect(within(dialog).getByLabelText("Text")).toHaveValue(TEXT);
    expect(within(dialog).getByRole("button", { name: "Remember" })).toBeEnabled();
  });
});

describe("MemoryPage edit", () => {
  beforeEach(() => {
    mocked.mockReset();
  });

  it("keeps Save disabled while the text is blank", async () => {
    mocked.mockImplementation(async (_grant, name) => {
      if (name === "memory.list") {
        return JSON.stringify([{ id: "m1", text: "remembered", tags: [] }]);
      }
      throw new Error(`the test did not expect a ${name} call`);
    });
    render(<MemoryPage grant={GRANT} />);
    await screen.findByText("remembered");
    fireEvent.click(screen.getByRole("button", { name: "Edit" }));

    const text = screen.getByLabelText("Text");
    const save = screen.getByRole("button", { name: "Save" });
    expect(save).toBeEnabled();
    expect(screen.queryByText("A memory entry needs some text.")).not.toBeInTheDocument();

    fireEvent.change(text, { target: { value: "   " } });
    expect(save).toBeDisabled();
    expect(screen.getByText("A memory entry needs some text.")).toBeInTheDocument();

    fireEvent.change(text, { target: { value: "an updated fact" } });
    expect(save).toBeEnabled();
    expect(screen.queryByText("A memory entry needs some text.")).not.toBeInTheDocument();
  });
});
