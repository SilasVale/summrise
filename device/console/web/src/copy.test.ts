// `copyText`'s comment is its contract: answer true only when the text reached the clipboard, and
// still copy on a plain-HTTP console where `navigator.clipboard` does not exist. jsdom implements
// neither half of that story, so each test installs the half it exercises and `restoreGlobal` puts
// the original descriptor (or its absence) back afterwards.
import { afterEach, describe, expect, it, vi } from "vitest";
import { copyText } from "./copy";
import { restoreGlobal } from "./test/dom";

const originals = {
  clipboard: Object.getOwnPropertyDescriptor(navigator, "clipboard"),
  execCommand: Object.getOwnPropertyDescriptor(document, "execCommand")
};

afterEach(() => {
  restoreGlobal(navigator, "clipboard", originals.clipboard);
  restoreGlobal(document, "execCommand", originals.execCommand);
  // The failing-command test leaves the fallback's textarea behind; do not hand it to the next test.
  document.querySelectorAll("textarea").forEach((area) => area.remove());
});

/** Install a `navigator.clipboard` whose `writeText` behaves how the test needs. */
function stubClipboard(writeText: (text: string) => Promise<void>): void {
  Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
}

/** Install `document.execCommand` (absent in jsdom) and return the mock, so a test can inspect it. */
function stubExecCommand(implementation: () => boolean): ReturnType<typeof vi.fn> {
  const execCommand = vi.fn(implementation);
  Object.defineProperty(document, "execCommand", { configurable: true, value: execCommand });
  return execCommand;
}

describe("copyText", () => {
  it("answers true and skips the fallback when the clipboard takes the text", async () => {
    const writeText = vi.fn(async () => {});
    stubClipboard(writeText);
    const execCommand = stubExecCommand(() => true);

    await expect(copyText("the diagnostics")).resolves.toBe(true);

    expect(writeText).toHaveBeenCalledWith("the diagnostics");
    expect(execCommand).not.toHaveBeenCalled();
    expect(document.querySelector("textarea")).toBeNull();
  });

  it("falls back to a temporary textarea when the clipboard refuses", async () => {
    stubClipboard(async () => {
      throw new Error("NotAllowedError: the clipboard needs a permission");
    });
    const areas: HTMLTextAreaElement[] = [];
    const execCommand = stubExecCommand(() => {
      const area = document.querySelector("textarea");
      if (area) areas.push(area);
      return true;
    });

    await expect(copyText("line one\nline two")).resolves.toBe(true);

    expect(execCommand).toHaveBeenCalledWith("copy");
    expect(areas).toHaveLength(1);
    expect(areas[0].value).toBe("line one\nline two");
    expect(document.querySelector("textarea")).toBeNull();
  });

  it("answers false when the fallback command refuses", async () => {
    stubClipboard(async () => {
      throw new Error("NotAllowedError: the clipboard needs a permission");
    });
    stubExecCommand(() => false);

    await expect(copyText("x")).resolves.toBe(false);

    expect(document.querySelector("textarea")).toBeNull();
  });

  it("falls back when the browser has no clipboard at all", async () => {
    // A console reached over plain HTTP: reading `navigator.clipboard.writeText` throws, not rejects.
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: undefined });
    const execCommand = stubExecCommand(() => true);

    await expect(copyText("plain http")).resolves.toBe(true);

    expect(execCommand).toHaveBeenCalledTimes(1);
  });

  it("rejects when the fallback command itself throws, rather than claiming it copied", async () => {
    // `copy.ts` does not wrap `execCommand` in a try/finally, so a throwing command rejects out of
    // `copyText` (it does not answer false) and leaves its textarea in the document. The rejection is
    // what callers must survive; the leaked textarea is reported as a defect, not pinned here.
    stubClipboard(async () => {
      throw new Error("NotAllowedError: the clipboard needs a permission");
    });
    stubExecCommand(() => {
      throw new Error("the browser blocked the command");
    });

    await expect(copyText("x")).rejects.toThrow("the browser blocked the command");
  });
});
