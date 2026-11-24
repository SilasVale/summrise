import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { LiveTool } from "./api";
import { ToolCatalogue } from "./ToolCatalogue";
import type { LiveToolsState } from "./useLiveTools";

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

function state(overrides: Partial<LiveToolsState> = {}): LiveToolsState {
  return {
    tools: TOOLS,
    stale: false,
    error: null,
    offline: false,
    loading: false,
    retrying: false,
    retried: null,
    retry: vi.fn(),
    ...overrides
  };
}

function draw(
  {
    live = state(),
    tool = null,
    offline = false,
    onTool = vi.fn(),
    onMachine = vi.fn(),
    onBrowser = vi.fn()
  }: {
    live?: LiveToolsState;
    tool?: string | null;
    offline?: boolean;
    onTool?: (name: string | null) => void;
    onMachine?: () => void;
    onBrowser?: () => void;
  } = {}
) {
  const view = render(
    <ToolCatalogue
      live={live}
      offline={offline}
      tool={tool}
      onTool={onTool}
      onMachine={onMachine}
      onBrowser={onBrowser}
    />
  );
  return { ...view, onTool, onMachine };
}

describe("ToolCatalogue", () => {
  it("lists every tool by capability, built-ins first, with the description and source", () => {
    draw();

    expect(screen.getByText("4 tools · 3 capabilities")).toBeInTheDocument();
    const groups = screen.getAllByRole("heading", { level: 3 }).map((heading) => heading.textContent);
    expect(groups).toEqual(["terminal", "files", "hello"]);

    expect(screen.getByText("terminal", { selector: ".cap-group-head h3" })).toBeInTheDocument();
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
    draw({ live: state({ tools: [], loading: true }) });
    expect(screen.getByRole("status", { name: "Reading the tool list" })).toBeInTheDocument();
    expect(document.querySelectorAll(".skeleton-row")).toHaveLength(3);
  });

  it("says it is reading only while the list is on the wire", () => {
    draw({ live: state({ tools: [], loading: true }) });
    expect(screen.getByText("Reading the tool list…")).toBeInTheDocument();
  });

  it("stops saying it is reading once the read has finished empty", () => {
    draw({ live: state({ tools: [] }) });
    expect(screen.queryByText("Reading the tool list…")).not.toBeInTheDocument();
    expect(screen.getByText("No tool list from this machine.")).toBeInTheDocument();
  });

  it("names the offline machine instead of reading a list it cannot get", () => {
    draw({
      live: state({
        tools: [],
        error: "this machine is not connected; connect it, then try again",
        offline: true
      }),
      offline: true
    });

    expect(screen.queryByText("Reading the tool list…")).not.toBeInTheDocument();
    // ONE SENTENCE, AND NO SUBTITLE REPEATING IT (audit-43 F9). The shell above already states
    // the machine is offline and dates it; the catalogue states only what it has, once.
    expect(
      screen.getByText("The console has no tool list from this machine yet.")
    ).toBeInTheDocument();
    expect(screen.queryByText("No tool list from this machine; it is offline.")).toBeNull();
  });

  it("draws no Retry an offline machine cannot use", () => {
    const retry = vi.fn();
    draw({
      live: state({
        tools: [],
        error: "this machine is not connected; connect it, then try again",
        offline: true,
        retry
      }),
      offline: true
    });

    expect(screen.getByText("The console has no tool list from this machine yet.")).toBeInTheDocument();
    expect(screen.queryByText(/not connected/)).toBeNull();
    expect(screen.queryByText("No tools")).not.toBeInTheDocument();
    // The shell's offline notice is the one statement, and a press here could not read anything.
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
    expect(retry).not.toHaveBeenCalled();
  });

  it("draws one notice and one Retry for a failed online read", () => {
    const retry = vi.fn();
    draw({
      live: state({
        tools: [],
        error: "the tools read failed",
        retry
      })
    });

    const notice = screen.getByRole("status");
    expect(notice).toHaveTextContent("The console could not read this machine's tools.");
    // The raw envelope sentence is evidence, not the page's sentence.
    expect(notice).toHaveAttribute("title", "the tools read failed");
    expect(screen.queryByText("No tool list from this machine.")).toBeNull();
    const retries = screen.getAllByRole("button", { name: "Retry" });
    expect(retries).toHaveLength(1);
    fireEvent.click(retries[0]);
    expect(retry).toHaveBeenCalledTimes(1);
    expect(screen.queryByText("No tools")).not.toBeInTheDocument();
  });

  it("labels a stale catalogue instead of hiding it", () => {
    draw({ live: state({ stale: true, error: "read failed" }) });
    expect(
      screen.getByText("Showing the last tool list the console read; it could not be refreshed.")
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /files\.list/ })).toBeInTheDocument();
  });

  it("points at Machine when the build advertises no tools", () => {
    const { onMachine } = draw({ live: state({ tools: [] }) });
    fireEvent.click(screen.getByRole("button", { name: "Open Machine" }));
    expect(onMachine).toHaveBeenCalledTimes(1);
  });
});
