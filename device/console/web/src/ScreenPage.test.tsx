import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ScreenPage } from "./ScreenPage";
import { ToolError, type Grant } from "./terminal";

const mocks = vi.hoisted(() => ({ callTool: vi.fn() }));

vi.mock("./terminal", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./terminal")>();
  return { ...actual, callTool: mocks.callTool };
});

const grant: Grant = { deviceId: "device-1", envelope: { token: "envelope" } };

describe("ScreenPage", () => {
  afterEach(() => {
    // The suite does not enable vitest globals, so Testing Library cannot register its own
    // auto-cleanup: without this the second render finds the first test's page still in the DOM.
    cleanup();
    mocks.callTool.mockReset();
  });

  it("captures the device's screen on demand and offers the picture for download", async () => {
    mocks.callTool.mockResolvedValue(
      JSON.stringify({ format: "png", data: "iVBORw0KGgo=", width: 1280, height: 720 })
    );
    render(<ScreenPage grant={grant} />);

    fireEvent.click(screen.getByRole("button", { name: "Capture screen" }));

    const image = await screen.findByRole("img", { name: "the device's screen" });
    expect(image).toHaveAttribute("src", "data:image/png;base64,iVBORw0KGgo=");
    expect(screen.getByText("1280×720")).toBeInTheDocument();

    const link = screen.getByRole("link", { name: "Download" });
    expect(link).toHaveAttribute("href", "data:image/png;base64,iVBORw0KGgo=");
    expect(link.getAttribute("download")).toMatch(/^summrise-screen-\d{4}-\d{2}-\d{2}-\d{6}\.png$/);
    expect(mocks.callTool).toHaveBeenCalledWith(grant, "screen.capture", {});
  });

  it("shows the device's refusal, leaves no image, and lets the button be used again", async () => {
    mocks.callTool.mockRejectedValue(
      new ToolError("no per-user helper is running on this device", 200, null, true)
    );
    render(<ScreenPage grant={grant} />);

    fireEvent.click(screen.getByRole("button", { name: "Capture screen" }));

    const refusal = await screen.findByText("no per-user helper is running on this device");
    expect(refusal).toHaveClass("error");
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Capture screen" })).toBeEnabled();
  });
});
