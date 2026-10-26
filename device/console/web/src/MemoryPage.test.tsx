import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MemoryPage } from "./MemoryPage";
import { callTool, ToolError } from "./terminal";

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

/// Put a replaced global back exactly as it was, or remove the property when jsdom never had it.
function restore(target: object, key: string, descriptor?: PropertyDescriptor) {
  if (descriptor) Object.defineProperty(target, key, descriptor);
  else Reflect.deleteProperty(target, key);
}

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
    restore(navigator, "clipboard", originals.clipboard);
    restore(URL, "createObjectURL", originals.createObjectURL);
    restore(URL, "revokeObjectURL", originals.revokeObjectURL);
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
