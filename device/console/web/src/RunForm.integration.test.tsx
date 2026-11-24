import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RunForm } from "./RunForm";
import type { Grant } from "./terminal";

// Mock only the wire: the MCP decoder, ToolError and form stay production code.
const GRANT: Grant = { deviceId: "run-machine", envelope: null };
const SUCCESS = {
  id: "run-success", exit_code: 0, timed_out: false, duration_ms: 12,
  stdout: "old successful output", stderr: "", truncated: false, history_recorded: true
};
const FAILED = {
  id: "run-failed", exit_code: 7, timed_out: false, duration_ms: 19,
  stdout: "current partial output", stderr: "current failure details", truncated: false, history_recorded: true
};

function response(result: unknown, status = 200): Response {
  return new Response(JSON.stringify({ jsonrpc: "2.0", id: 1, result }), {
    status, headers: { "content-type": "application/json" }
  });
}
function completed(data: typeof SUCCESS) {
  return response({ content: [{ type: "text", text: JSON.stringify(data) }], isError: false });
}
function failed(data?: unknown, text = "runs.exec: run run-failed exited 7") {
  return response({ content: [{ type: "text", text }], isError: true,
    ...(data === undefined ? {} : { structuredContent: data }) });
}
function submit(command = "run-test") {
  fireEvent.change(screen.getByLabelText("Command"), { target: { value: command } });
  fireEvent.click(screen.getByRole("button", { name: "Run" }));
}
function resultSection() {
  return within(screen.getByText("Result").closest("section")!);
}
let wire: ReturnType<typeof vi.fn>;
beforeEach(() => {
  wire = vi.fn();
  vi.stubGlobal("fetch", wire);
});
afterEach(() => vi.unstubAllGlobals());

describe("a real MCP response reaches the run form", () => {
  it("replaces the prior success with this completed failed run and both output streams", async () => {
    wire.mockResolvedValueOnce(completed(SUCCESS)).mockResolvedValueOnce(failed(FAILED));
    render(<RunForm grant={GRANT} />);
    submit();
    expect(await screen.findByText("run-success")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("button", { name: "Run" })).toBeEnabled());
    submit("fail-test");
    expect(await screen.findByRole("alert")).toHaveTextContent("run-failed exited 7");
    expect(resultSection().getByText("run-failed")).toBeInTheDocument();
    expect(resultSection().getByText("exit 7")).toBeInTheDocument();
    expect(resultSection().getByText("current partial output")).toBeInTheDocument();
    expect(resultSection().getByText("current failure details")).toBeInTheDocument();
    expect(screen.queryByText("old successful output")).not.toBeInTheDocument();
    expect(screen.queryByText(/this attempt did not replace it/)).not.toBeInTheDocument();
    expect(wire).toHaveBeenCalledTimes(2);
  });

  it("shows the completed timeout's empty streams and exit facts", async () => {
    wire.mockResolvedValue(failed({ ...FAILED, id: "run-timeout", exit_code: null, timed_out: true, stdout: "", stderr: "" },
      "runs.exec: run run-timeout timed out after 19 ms"));
    render(<RunForm grant={GRANT} />);
    submit();
    expect(await screen.findByRole("alert")).toHaveTextContent("run-timeout timed out");
    expect(resultSection().getByText("run-timeout")).toBeInTheDocument();
    expect(resultSection().getByText("timed out")).toBeInTheDocument();
    expect(resultSection().getByText("(no stdout)")).toBeInTheDocument();
    expect(resultSection().getByText("(no stderr)")).toBeInTheDocument();
  });

  it.each([
    ["admission refusal", undefined],
    ["partial fields", { id: "pretend-run", exit_code: 7, stdout: "untrusted partial output" }],
    ["wrong field types", { ...FAILED, id: "pretend-run", timed_out: "false", stdout: "untrusted partial output" }],
    ["array payload", [FAILED]]
  ])("keeps the explicitly dated prior result after %s rather than inventing a completed run", async (_, data) => {
    wire.mockResolvedValueOnce(completed(SUCCESS)).mockResolvedValueOnce(failed(data, "runs.exec: request refused"));
    render(<RunForm grant={GRANT} />);
    submit();
    expect(await screen.findByText("run-success")).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole("button", { name: "Run" })).toBeEnabled());
    submit();
    expect(await screen.findByRole("alert")).toHaveTextContent("request refused");
    expect(resultSection().getByText("run-success")).toBeInTheDocument();
    expect(screen.getByText(/this attempt did not replace it/)).toBeInTheDocument();
    expect(screen.queryByText("pretend-run")).not.toBeInTheDocument();
    expect(screen.queryByText("untrusted partial output")).not.toBeInTheDocument();
  });

  it("drops a failed response after the same device's authority was renewed", async () => {
    let finish!: (answer: Response) => void;
    wire.mockReturnValue(new Promise<Response>((resolve) => { finish = resolve; }));
    const view = render(<RunForm grant={GRANT} />);
    submit();
    view.rerender(<RunForm grant={{ ...GRANT, envelope: { renewed: true } }} />);
    await act(async () => { finish(failed(FAILED)); });
    expect(screen.queryByText("Result")).not.toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Run" })).toBeEnabled();
  });
});
