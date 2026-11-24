import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  addPlugin,
  inspectPlugin,
  listPlugins,
  pluginFailure,
  pluginLogs,
  removePlugin,
  restartPlugin
} from "./pluginApi";

const mocks = vi.hoisted(() => ({ callTool: vi.fn() }));
vi.mock("./terminal", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./terminal")>();
  return { ...actual, callTool: mocks.callTool };
});

const DEVICE = "dev-workshop";
const GRANT = { deviceId: DEVICE, envelope: null };

beforeEach(() => mocks.callTool.mockReset());

describe("pluginApi", () => {
  it("parses plugins.list defensively and keeps an unknown state off the page", async () => {
    mocks.callTool.mockResolvedValue(
      JSON.stringify([
        {
          id: "com.example.hello",
          namespace: "hello",
          name: "Hello",
          version: "0.1.0",
          state: "running",
          tools: 2,
          path: "C:\\plugins\\hello",
          last_error: null
        },
        { namespace: "odd", state: "exploded", tools: "many" }
      ])
    );

    const plugins = await listPlugins(DEVICE);
    expect(mocks.callTool).toHaveBeenCalledWith(GRANT, "plugins.list", {});
    expect(plugins[0]).toMatchObject({ namespace: "hello", state: "running", tools: 2 });
    // A partial record is still a row; an unknown state is not "running" by accident.
    expect(plugins[1]).toMatchObject({ namespace: "odd", state: "stopped", tools: 0 });
  });

  it("refuses an answer that is not a plugin list", async () => {
    mocks.callTool.mockResolvedValue("not json");
    await expect(listPlugins(DEVICE)).rejects.toThrow(/without a list/);
  });

  it("inspects a directory without starting it, and maps the manifest fields", async () => {
    mocks.callTool.mockResolvedValue(
      JSON.stringify({
        id: "com.example.hello",
        namespace: "hello",
        name: "Hello",
        version: "0.1.0",
        description: "Two friendly tools"
      })
    );

    expect(await inspectPlugin(DEVICE, "C:\\plugins\\hello")).toEqual({
      id: "com.example.hello",
      namespace: "hello",
      name: "Hello",
      version: "0.1.0",
      description: "Two friendly tools"
    });
    expect(mocks.callTool).toHaveBeenCalledWith(GRANT, "plugins.inspect", {
      path: "C:\\plugins\\hello"
    });
  });

  it("calls add, remove, restart and logs with the fixed arguments", async () => {
    mocks.callTool
      .mockResolvedValueOnce(JSON.stringify({ plugin: { namespace: "hello", tools: 2 }, tools: 2 }))
      .mockResolvedValueOnce("{}")
      .mockResolvedValueOnce(JSON.stringify({ plugin: { namespace: "hello", state: "running" } }))
      .mockResolvedValueOnce(JSON.stringify({ namespace: "hello", lines: ["a", "b"] }));

    const added = await addPlugin(DEVICE, "C:\\plugins\\hello");
    expect(added.plugin.namespace).toBe("hello");
    await removePlugin(DEVICE, "hello");
    expect((await restartPlugin(DEVICE, "hello")).state).toBe("running");
    expect(await pluginLogs(DEVICE, "hello", 50)).toEqual(["a", "b"]);

    expect(mocks.callTool.mock.calls.slice(1).map((call) => call[1])).toEqual([
      "plugins.remove",
      "plugins.restart",
      "plugins.logs"
    ]);
    expect(mocks.callTool.mock.calls[3][2]).toEqual({ namespace: "hello", lines: 50 });
  });

  it("turns the product's error text into a sentence and an action", () => {
    expect(
      pluginFailure(
        new Error(
          JSON.stringify({
            code: "NO_MANIFEST",
            message: "No plugin.json in that folder.",
            action: "Open the guide"
          })
        )
      )
    ).toEqual({
      code: "NO_MANIFEST",
      message: "No plugin.json in that folder.",
      action: "Open the guide"
    });

    expect(pluginFailure(new Error("the console never reached the device")).message).toBe(
      "the console never reached the device"
    );
  });
});
