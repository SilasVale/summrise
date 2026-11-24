import { beforeEach, describe, expect, it, vi } from "vitest";
import { probeNow } from "./monitorApi";
import { callTool, ToolError, type Grant } from "./terminal";

vi.mock("./terminal", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./terminal")>();
  return { ...actual, callTool: vi.fn() };
});

const mocked = vi.mocked(callTool);
const GRANT: Grant = { deviceId: "device-1", envelope: { token: "envelope" } };
const ID = "127.0.0.1:19000/health";

beforeEach(() => { mocked.mockReset(); });

describe("probeNow — a recorded sample's target verdict", () => {
  it.each([
    ["up", '{"ts":1762000000000,"ok":true,"latency_ms":3}'],
    ["down without a connection", '{"ts":1762000000001,"ok":false,"latency_ms":null}'],
    ["down after an HTTP response", '{"ts":1762000000002,"ok":false,"latency_ms":7}']
  ])("accepts a real %s sample and leaves summary reading to its caller", async (_state, body) => {
    mocked.mockResolvedValue(body);

    await expect(probeNow(GRANT, ID)).resolves.toBeUndefined();

    expect(mocked.mock.calls).toEqual([[GRANT, "monitor.probe", { id: ID }]]);
  });

  it("preserves a tool-level refusal and never issues another call", async () => {
    const refused = new ToolError("monitor.probe: target missing is not watched", 200, null, true);
    mocked.mockRejectedValue(refused);

    await expect(probeNow(GRANT, "missing")).rejects.toBe(refused);
    expect(mocked).toHaveBeenCalledTimes(1);
  });

  it.each([
    ['{"ok":false,"error":"  target missing is not watched  "}', "target missing is not watched"],
    ['{"ok":false,"error":"  "}', "the machine refused the probe"],
    ['{"ok":false}', "the machine refused the probe"],
    ['{"error":"probe unavailable"}', "probe unavailable"],
    ['{"ts":1762000000000,"ok":false,"latency_ms":null,"error":"probe refused"}', "probe refused"]
  ])("keeps an explicit refusal distinct from a down sample: %s", async (body, sentence) => {
    mocked.mockResolvedValue(body);

    await expect(probeNow(GRANT, ID)).rejects.toThrow(sentence);
  });

  it("rejects malformed JSON instead of reporting a completed measurement", async () => {
    mocked.mockResolvedValue("not JSON");

    await expect(probeNow(GRANT, ID)).rejects.toThrow("the machine answered monitor.probe with text");
  });

  it.each(["null", "[]", '"sample"', "false", "23"])("rejects a non-object answer: %s", async (body) => {
    mocked.mockResolvedValue(body);

    await expect(probeNow(GRANT, ID)).rejects.toThrow("the machine answered monitor.probe without an object");
  });

  it.each([
    "{}",
    '{"ok":true}',
    '{"ts":1762000000000,"latency_ms":null}',
    '{"ts":1762000000000,"ok":"false","latency_ms":null}',
    '{"ts":"1762000000000","ok":true,"latency_ms":3}',
    '{"ts":1e400,"ok":true,"latency_ms":3}',
    '{"ts":1762000000000,"ok":true}',
    '{"ts":1762000000000,"ok":true,"latency_ms":"3"}',
    '{"ts":1762000000000,"ok":true,"latency_ms":1e400}'
  ])("rejects an object without a current sample: %s", async (body) => {
    mocked.mockResolvedValue(body);

    await expect(probeNow(GRANT, ID)).rejects.toThrow("the machine answered monitor.probe without a sample");
  });
});
