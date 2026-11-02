import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { LiveTool } from "./api";
import { ToolSheet } from "./ToolSheet";
import type { Grant } from "./terminal";

const mocks = vi.hoisted(() => ({ callTool: vi.fn() }));
vi.mock("./terminal", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./terminal")>();
  return { ...actual, callTool: mocks.callTool };
});

const GRANT: Grant = { deviceId: "dev-workshop", envelope: null };

function tool(overrides: Partial<LiveTool> & { name: string }): LiveTool {
  return {
    description: "list a directory",
    inputSchema: {
      type: "object",
      properties: {
        path: { type: "string", description: "Where to list" },
        limit: { type: "integer" },
        recursive: { type: "boolean" }
      },
      required: ["path"]
    },
    source: "builtin",
    capability: "files",
    destructive: false,
    ...overrides
  };
}

function draw(overrides: Partial<Parameters<typeof ToolSheet>[0]> = {}) {
  const onClose = vi.fn();
  const onActivity = vi.fn();
  const view = render(
    <ToolSheet tool={tool({ name: "files.list" })} grant={GRANT} onClose={onClose} onActivity={onActivity} {...overrides} />
  );
  return { ...view, onClose, onActivity };
}

beforeEach(() => {
  mocks.callTool.mockReset();
});

describe("ToolSheet — one tool's detail and Try it", () => {
  it("shows the name, capability, source, annotations and the schema as a field list", () => {
    draw();

    expect(screen.getByRole("dialog", { name: "files.list" })).toBeInTheDocument();
    expect(screen.getByText("files", { selector: ".tool-meta .mono" })).toBeInTheDocument();
    expect(screen.getByText(/Built-in/)).toBeInTheDocument();
    expect(screen.getByText("Read-only")).toBeInTheDocument();
    expect(screen.getAllByText("Where to list")).toHaveLength(2);
    expect(screen.getByText("Required")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Run" })).toBeInTheDocument();
    // A boolean is a three-state choice, never a bare checkbox.
    const recursive = screen.getByLabelText(/^recursive/);
    expect(recursive.tagName).toBe("SELECT");
    expect(Array.from((recursive as HTMLSelectElement).options).map((option) => option.textContent)).toEqual([
      "Not sent",
      "true",
      "false"
    ]);
  });

  it("runs with only the fields that were filled, then shows the formatted answer and the record", async () => {
    mocks.callTool.mockResolvedValue('[{"name":"notes.txt"}]');
    const { onActivity } = draw();

    fireEvent.change(screen.getByLabelText(/^path/), { target: { value: "docs" } });
    fireEvent.change(screen.getByLabelText(/^recursive/), { target: { value: "true" } });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    await screen.findByText(/Completed in \d+ ms/);
    expect(mocks.callTool).toHaveBeenCalledWith(GRANT, "files.list", { path: "docs", recursive: true });
    expect(screen.getByText(/notes\.txt/)).toBeInTheDocument();
    expect(screen.getByText("Recorded in Activity.")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Open Activity" }));
    expect(onActivity).toHaveBeenCalledTimes(1);
  });

  it("refuses a blank required field, names it and focuses it", async () => {
    draw();
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    expect(await screen.findByText('Add a value for "path".')).toBeInTheDocument();
    await waitFor(() => expect(screen.getByLabelText(/^path/)).toHaveFocus());
    expect(mocks.callTool).not.toHaveBeenCalled();
  });

  it("forces Raw JSON for a nested schema and sends what the JSON says", async () => {
    mocks.callTool.mockResolvedValue("ok");
    draw({
      tool: tool({
        name: "files.write",
        inputSchema: {
          type: "object",
          properties: { patch: { type: "object", properties: { line: { type: "string" } } } },
          required: ["patch"]
        }
      })
    });

    expect(screen.getByText("This tool takes a nested object; edit the JSON.")).toBeInTheDocument();
    expect(screen.queryByLabelText(/^patch/)).not.toBeInTheDocument();
    const editor = screen.getByLabelText(/Arguments \(JSON\)/);
    fireEvent.change(editor, { target: { value: '{"patch":{"line":"x"}}' } });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    await screen.findByText(/Completed in \d+ ms/);
    expect(mocks.callTool).toHaveBeenCalledWith(GRANT, "files.write", { patch: { line: "x" } });
  });

  it("refuses to switch back from Raw JSON that does not parse", () => {
    draw();
    fireEvent.click(screen.getByRole("button", { name: "Raw JSON" }));
    const editor = screen.getByLabelText(/Arguments \(JSON\)/);
    fireEvent.change(editor, { target: { value: "{ nope" } });
    fireEvent.click(screen.getByRole("button", { name: "Raw JSON" }));

    expect(screen.getByText("Fix the JSON before switching back.")).toBeInTheDocument();
    expect(screen.getByLabelText(/Arguments \(JSON\)/)).toBeInTheDocument();
  });

  it("asks once for a destructive call, shows the exact JSON, and only Run once sends it", async () => {
    mocks.callTool.mockResolvedValue("done");
    draw({ tool: tool({ name: "files.write", destructive: true }) });

    fireEvent.change(screen.getByLabelText(/^path/), { target: { value: "notes.txt" } });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    expect(
      screen.getByText("This call can change this machine and cannot be undone. Run it?")
    ).toBeInTheDocument();
    expect(screen.getByText(/"path": "notes\.txt"/)).toBeInTheDocument();
    expect(mocks.callTool).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Run once" }));
    await screen.findByText(/Completed in \d+ ms/);
    expect(mocks.callTool).toHaveBeenCalledTimes(1);
  });

  it("shows the device's refusal verbatim", async () => {
    mocks.callTool.mockRejectedValue(new Error("path is outside the files root"));
    draw();

    fireEvent.change(screen.getByLabelText(/^path/), { target: { value: "../../etc" } });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    expect(await screen.findByText("path is outside the files root")).toBeInTheDocument();
  });

  it("says the call may still be running after the two-minute guard", async () => {
    mocks.callTool.mockReturnValue(new Promise(() => undefined));
    draw({ timeoutMs: 20 });

    fireEvent.change(screen.getByLabelText(/^path/), { target: { value: "docs" } });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    expect(
      await screen.findByText("No answer after 2 minutes. The call may still be running; check Activity.")
    ).toBeInTheDocument();
  });

  it("caps a long result at 400 lines until Show all", async () => {
    mocks.callTool.mockResolvedValue(
      Array.from({ length: 401 }, (_, index) => `line ${index}`).join("\n")
    );
    draw();

    fireEvent.change(screen.getByLabelText(/^path/), { target: { value: "docs" } });
    fireEvent.click(screen.getByRole("button", { name: "Run" }));

    fireEvent.click(await screen.findByRole("button", { name: "Show all" }));
    await waitFor(() =>
      expect(screen.getByText(/line 400/)).toBeInTheDocument()
    );
  });
});
