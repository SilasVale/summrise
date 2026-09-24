import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { downloadBlob, type DownloadTarget } from "../download";

/**
 * The panel's ONE anchor touch, tested without rendering anything.
 *
 * This is what the extraction bought: while the six lines lived inside `exportSession`, the only way to reach
 * them was to render the session hook and drive an export through it — which nothing did. The hook's own file
 * has no test for `exportSession`, and the twentieth exploration listed it among the accumulated concerns for
 * exactly that reason.
 */
describe("downloadBlob", () => {
  let created: string[];
  let revoked: string[];
  let clicks: number;
  let anchor: DownloadTarget;

  beforeEach(() => {
    created = [];
    revoked = [];
    clicks = 0;
    anchor = {
      href: "",
      download: "",
      click: () => {
        clicks += 1;
      },
    };
    vi.stubGlobal("URL", {
      ...URL,
      createObjectURL: (b: Blob) => {
        created.push(b.type);
        return "blob:test-url";
      },
      revokeObjectURL: (u: string) => {
        revoked.push(u);
      },
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  const fakeDoc = {
    createElement: () => anchor,
  } as unknown as Pick<Document, "createElement">;

  it("names the download, points it at the object URL, and clicks it once", () => {
    const blob = new Blob(["hello"], { type: "text/plain" });
    const used = downloadBlob("s-1.log", blob, fakeDoc);

    expect(used).toBe(anchor);
    expect(used.download).toBe("s-1.log");
    expect(used.href).toBe("blob:test-url");
    expect(clicks).toBe(1);
    expect(created).toEqual(["text/plain"]);
  });

  it("revokes the object URL, so the Blob does not outlive the click", () => {
    // The export path can build megabytes; an unrevoked URL keeps its Blob alive for the lifetime of the
    // document. This is the assertion that makes the `finally` meaningful rather than decorative.
    downloadBlob("s-1.log", new Blob(["x"]), fakeDoc);

    expect(revoked).toEqual(["blob:test-url"]);
  });

  it("still revokes when the click throws", () => {
    anchor.click = () => {
      throw new Error("the browser refused it");
    };

    expect(() => downloadBlob("s-1.log", new Blob(["x"]), fakeDoc)).toThrow("the browser refused it");
    expect(revoked).toEqual(["blob:test-url"]);
  });
});
