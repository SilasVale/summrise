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
  // A test that fails mid-fallback must not leave its textarea in the document for the next one.
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

  it("answers false, not a rejection, when the fallback command itself throws", async () => {
    // Some browsers throw instead of answering false; the caller has no catch, so the answer must
    // stay a boolean and the temporary textarea must not survive the failed attempt.
    stubClipboard(async () => {
      throw new Error("NotAllowedError: the clipboard needs a permission");
    });
    stubExecCommand(() => {
      throw new Error("the browser blocked the command");
    });

    await expect(copyText("x")).resolves.toBe(false);

    expect(document.querySelector("textarea")).toBeNull();
  });
});
