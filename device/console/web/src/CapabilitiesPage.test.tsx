import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Device, LiveTool } from "./api";
import { CapabilitiesPage } from "./CapabilitiesPage";

const live = vi.hoisted(() => ({ value: {} as Record<string, unknown> }));
vi.mock("./useLiveTools", () => ({ useLiveTools: () => live.value }));

const workshop: Device = {
  id: "dev-workshop",
  name: "workshop",
  online: true,
  version: "0.3.246",
  last_seen_ms: 1
};

function tool(overrides: Partial<LiveTool> & { name: string }): LiveTool {
  return {
    description: "",
    inputSchema: { type: "object", properties: {} },
    source: "builtin",
    capability: overrides.name.split(".")[0],
    destructive: false,
    ...overrides
  };
}

const TOOLS = [
  tool({ name: "terminal.create", description: "open a shell", destructive: true }),
  tool({ name: "files.list", description: "list a directory" }),
  tool({ name: "files.read", description: "read a file" }),
  tool({
    name: "hello.greet",
    description: "greet someone",
    source: "plugin:hello",
    capability: "hello"
  })
];

function draw(overrides: Record<string, unknown> = {}) {
  const onTool = vi.fn();
  const onChoose = vi.fn();
  const onMachine = vi.fn();
  const view = render(
    <CapabilitiesPage
      devices={[workshop]}
      device={workshop}
      tool={null}
      onChoose={onChoose}
      onTool={onTool}
      onMachine={onMachine}
      onActivity={vi.fn()}
      {...overrides}
    />
  );
  return { ...view, onTool, onChoose, onMachine };
}

beforeEach(() => {
  live.value = {
    tools: TOOLS,
    stale: false,
    error: null,
    offline: false,
    loading: false,
    retry: vi.fn()
  };
});

describe("CapabilitiesPage", () => {
  it("lists every tool by capability, built-ins first, with the description and source", () => {
    draw();

    expect(screen.getByRole("heading", { name: "Capabilities", level: 1 })).toBeInTheDocument();
    expect(screen.getByText("4 tools · 3 capabilities")).toBeInTheDocument();
    const groups = screen.getAllByRole("heading", { level: 2 }).map((heading) => heading.textContent);
    expect(groups).toEqual(["terminal", "files", "hello"]);

    expect(screen.getByText("terminal", { selector: ".cap-group-head h2" })).toBeInTheDocument();
    expect(screen.getAllByText("Built-in")).toHaveLength(2);
    expect(screen.getByText("Plugin · hello")).toBeInTheDocument();
    const terminal = screen
      .getByRole("heading", { name: "terminal" })
      .closest("section") as HTMLElement;
    expect(terminal).toHaveTextContent("1 tool · 1 can change the machine");
    // The Destructive chip belongs to the sheet, not to every row.
    expect(screen.queryByText("Destructive")).not.toBeInTheDocument();
  });

  it("filters across name, description and capability, and teaches the clear", () => {
    draw();
    const search = screen.getByLabelText("Search tools");

    fireEvent.change(search, { target: { value: "greet" } });
    expect(screen.getByRole("button", { name: /hello\.greet/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /files\.list/ })).not.toBeInTheDocument();

    fireEvent.change(search, { target: { value: "nothing-like-this" } });
    expect(screen.getByText('No tool matches "nothing-like-this".')).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Clear the search" }));
    expect(screen.getByRole("button", { name: /files\.list/ })).toBeInTheDocument();
  });

  it("opens a tool's sheet from its row and from a deep link", () => {
    const { onTool } = draw();
    fireEvent.click(screen.getByRole("button", { name: /terminal\.create/ }));
    expect(onTool).toHaveBeenCalledWith("terminal.create");

    draw({ tool: "files.read" });
    expect(screen.getByRole("dialog", { name: "files.read" })).toBeInTheDocument();
  });

  it("says a deep-linked tool is not on the machine, instead of opening an empty sheet", () => {
    draw({ tool: "gone.tool" });
    expect(screen.getByText("That tool is not on this machine.")).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("renders three skeleton rows while the live list is on the wire", () => {
    live.value = {
      tools: [],
      stale: false,
      error: null,
      offline: false,
      loading: true,
      retry: vi.fn()
    };
    draw();
    expect(screen.getByRole("status", { name: "Reading the tool list" })).toBeInTheDocument();
    expect(document.querySelectorAll(".skeleton-row")).toHaveLength(3);
  });

  it("renders an offline machine as a banner with Retry, never as an empty list", () => {
    const retry = vi.fn();
    live.value = {
      tools: [],
      stale: false,
      error: "this machine is not connected; connect it, then try again",
      offline: true,
      loading: false,
      retry
    };
    draw({ device: { ...workshop, online: false } });

    expect(
      screen.getByText(
        "workshop is not connected. The console keeps trying; open the machine's panel if it stays offline."
      )
    ).toBeInTheDocument();
    expect(screen.queryByText("No tools")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(retry).toHaveBeenCalledTimes(1);
  });

  it("labels a stale catalogue instead of hiding it", () => {
    live.value = {
      tools: TOOLS,
      stale: true,
      error: "read failed",
      offline: false,
      loading: false,
      retry: vi.fn()
    };
    draw();
    expect(
      screen.getByText("Showing the last tool list the console read; it could not be refreshed.")
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /files\.list/ })).toBeInTheDocument();
  });

  it("points at Machine when the build advertises no tools", () => {
    live.value = {
      tools: [],
      stale: false,
      error: null,
      offline: false,
      loading: false,
      retry: vi.fn()
    };
    const { onMachine } = draw();
    fireEvent.click(screen.getByRole("button", { name: "Open Machine" }));
    expect(onMachine).toHaveBeenCalledTimes(1);
  });

  it("asks for a machine when the deep link lands without one", () => {
    const { onChoose } = draw({ device: null });
    expect(screen.getByRole("heading", { name: "Choose a machine" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Use workshop" }));
    expect(onChoose).toHaveBeenCalledWith(workshop.id);
  });
});
