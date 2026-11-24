// THE TRANSPORT'S OWN SENTENCES — the hundred-and-thirty-fifth list.
//
// A tool answer arrives as TEXT. Three shapes are not the device's JSON-RPC answer — a web page from
// the edge, a proxy's one-line error, and garbage — and the page prints whatever `callTool` throws in
// the operator's sentence. Measured 2026-11-11 with an HTML 200 on `/mcp` (an Access login page, or
// any proxy's error page): the Terminal read
//
//     Could not read this device's sessions: <!doctype html> <html lang="en"> <head> <meta charset="utf-8"> …
//
// 200 characters of markup where a sentence goes. This file pins the three sentences, and the
// device's own refusal, which must keep passing through VERBATIM.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { callTool, toolFailure, ToolError, type Grant } from "./terminal";

const grant: Grant = { deviceId: "device-1", envelope: null };

type Reply = { status?: number; statusText?: string; text?: string };

function answer(reply: Reply = {}): Response {
  const status = reply.status ?? 200;
  const body = reply.text ?? "";
  return {
    ok: status >= 200 && status < 300,
    status,
    statusText: reply.statusText ?? "OK",
    text: async () => body
  } as Response;
}

let fetchMock: ReturnType<typeof vi.fn>;

beforeEach(() => {
  fetchMock = vi.fn();
  vi.stubGlobal("fetch", fetchMock);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

/** The rejection value, so a test can assert on it without calling twice. */
async function failureOf(promise: Promise<unknown>): Promise<ToolError> {
  try {
    await promise;
  } catch (error) {
    expect(error).toBeInstanceOf(ToolError);
    return error as ToolError;
  }
  throw new Error("expected the call to reject");
}

describe("a non-JSON answer says who answered, not what it contained", () => {
  it("does not put a web page in the operator's sentence", async () => {
    fetchMock.mockResolvedValue(
      answer({
        text: '<!doctype html><html lang="en"><head><meta charset="utf-8">',
        status: 200
      })
    );
    const failure = await failureOf(callTool(grant, "terminal.list", {}));
    expect(failure.message).toBe(
      "the edge answered this call with a web page, not the console (HTTP 200)"
    );
    // THE WHOLE POINT: no markup, no tag, nothing a reader has to scroll past.
    expect(failure.message).not.toContain("<");
  });

  it("keeps a proxy's one short line, because that is the only text naming the objection", async () => {
    fetchMock.mockResolvedValue(
      answer({ text: "upstream connect error or disconnect/reset before headers", status: 502 })
    );
    const failure = await failureOf(callTool(grant, "terminal.list", {}));
    expect(failure.message).toBe("upstream connect error or disconnect/reset before headers");
    expect(failure.status).toBe(502);
  });

  it("bounds anything else to the status", async () => {
    fetchMock.mockResolvedValue(answer({ text: "x".repeat(200), status: 504 }));
    const failure = await failureOf(callTool(grant, "terminal.list", {}));
    expect(failure.message).toBe("the answer was not JSON (HTTP 504)");
  });
});

describe("the device's own sentences still pass through", () => {
  it("keeps a tool refusal verbatim", async () => {
    fetchMock.mockResolvedValue(
      answer({
        text: JSON.stringify({
          result: { content: [{ type: "text", text: "terminal.list: no such session" }], isError: true }
        })
      })
    );
    const failure = await failureOf(callTool(grant, "terminal.list", {}));
    expect(failure.message).toBe("terminal.list: no such session");
    expect(failure.fromDevice).toBe(true);
  });

  it("reads a JSON-RPC error's message rather than its shape", async () => {
    fetchMock.mockResolvedValue(
      answer({
        text: JSON.stringify({ error: { code: -32003, message: "the device is busy" } })
      })
    );
    const failure = await failureOf(callTool(grant, "terminal.list", {}));
    expect(failure.message).toBe("the device is busy");
    expect(failure.code).toBe(-32003);
  });
});

describe("a failed exchange keeps delivery certainty separate from HTTP status", () => {
  it.each([
    [502, { error: "device disconnected" }],
    [504, { error: "device did not answer" }],
    [503, { error: "device busy" }],
    [503, { error: "edge_unavailable", message: "private credential must stay hidden" }]
  ])("does not promise a %s exchange was never sent", async (status, body) => {
    fetchMock.mockResolvedValue(answer({ status, text: JSON.stringify(body) }));
    const failure = await failureOf(callTool(grant, "browser.open", { url: "about:blank" }));
    expect(failure.delivery).toBe("outcome_unknown");
    expect(toolFailure(failure)).toMatch(/may still be working/i);
    expect(toolFailure(failure)).not.toMatch(/never reached|did not send/i);
    expect(toolFailure(failure)).not.toContain("private credential");
  });

  it("keeps a lost browser response unknown and never retries the call", async () => {
    fetchMock.mockRejectedValue(new TypeError("fetch failed"));
    const failure = await failureOf(callTool(grant, "browser.open", { url: "about:blank" }));
    expect(failure.status).toBe(0);
    expect(failure.delivery).toBe("outcome_unknown");
    expect(toolFailure(failure)).toMatch(/check the record before trying again/i);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it.each(["device not connected", "device reconnecting", "device busy"])(
    "uses the Console's explicit pre-send proof for %s without echoing arbitrary messages",
    async (error) => {
      fetchMock.mockResolvedValue(answer({
        status: 503,
        text: JSON.stringify({ error, delivery: "not_sent", message: "private credential must stay hidden" })
      }));
      const failure = await failureOf(callTool(grant, "browser.open", { url: "about:blank" }));
      expect(failure.delivery).toBe("not_sent");
      expect(toolFailure(failure)).toMatch(/did not send this call/i);
      expect(toolFailure(failure)).not.toContain("private credential");
    }
  );

  it("does not trust an unknown edge code carrying a pre-send label", async () => {
    fetchMock.mockResolvedValue(answer({ status: 503, text: JSON.stringify({ error: "edge_unavailable", delivery: "not_sent" }) }));
    const failure = await failureOf(callTool(grant, "browser.open", { url: "about:blank" }));
    expect(failure.delivery).toBe("outcome_unknown");
  });

  it("does not interpret legacy constructor text as pre-send evidence", () => {
    expect(new ToolError("device busy", 503).delivery).toBe("outcome_unknown");
    expect(toolFailure(new ToolError("device busy", 503))).toMatch(/may still be working/i);
  });

  it("keeps an authorization refusal definitive without leaking its additive message", async () => {
    fetchMock.mockResolvedValue(answer({ status: 403, text: JSON.stringify({ error: "grant_expired", message: "private credential must stay hidden" }) }));
    const failure = await failureOf(callTool(grant, "files.write", { path: "a", content: "b" }));
    expect(failure.delivery).toBe("answered");
    expect(failure.message).toBe("grant_expired");
    expect(toolFailure(failure)).toBe("grant_expired");
  });

  it("keeps a failed tool's structured facts on the error rather than turning it into success", async () => {
    const data = { id: "run-failed", exit_code: 7, stdout: "partial output", stderr: "failed output" };
    fetchMock.mockResolvedValue(answer({ text: JSON.stringify({ jsonrpc: "2.0", id: 1, result: {
      isError: true, content: [{ type: "text", text: "runs.exec: run run-failed exited 7" }], structuredContent: data
    } }) }));
    const failure = await failureOf(callTool(grant, "runs.exec", { command: "false" }));
    expect(failure.fromDevice).toBe(true);
    expect(failure.delivery).toBe("answered");
    expect(failure.resultData).toEqual(data);
    expect(toolFailure(failure)).toBe("runs.exec: run run-failed exited 7");
  });
});
