import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { LiveTool } from "./api";
import { ToolSheet } from "./ToolSheet";

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
  const view = render(
    <ToolSheet tool={tool({ name: "files.list" })} onClose={onClose} {...overrides} />
  );
  return { ...view, onClose };
}

describe("ToolSheet — one tool's catalogue entry", () => {
  it("shows the name, capability, source, annotations and the schema as a field list", () => {
    draw();

    expect(screen.getByRole("dialog", { name: "files.list" })).toBeInTheDocument();
    expect(screen.getByText("files", { selector: ".tool-meta .mono" })).toBeInTheDocument();
    expect(screen.getByText(/Built-in/)).toBeInTheDocument();
    expect(screen.getByText("Read-only")).toBeInTheDocument();
    expect(screen.getByText("Where to list")).toBeInTheDocument();
    expect(screen.getByText("Required")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Close" })).toBeInTheDocument();
  });

  it("renders the description and its arguments, but no Try it, Raw JSON or Run control", () => {
    draw();

    expect(screen.getByText("list a directory")).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Input schema" })).toBeInTheDocument();
    expect(screen.getByText("path")).toBeInTheDocument();
    expect(screen.getByText("limit")).toBeInTheDocument();
    expect(screen.getByText("recursive")).toBeInTheDocument();

    expect(screen.queryByText("Try it")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Raw JSON" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Run" })).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  });
});
