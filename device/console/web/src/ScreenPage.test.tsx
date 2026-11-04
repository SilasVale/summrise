import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ScreenPage } from "./ScreenPage";
import { ToolError, type Grant } from "./terminal";

const mocks = vi.hoisted(() => ({ callTool: vi.fn() }));

vi.mock("./terminal", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./terminal")>();
  return { ...actual, callTool: mocks.callTool };
});

vi.mock("./BrowserPane", () => ({ BrowserPane: () => <h3>Browser pane</h3> }));

const grant: Grant = { deviceId: "device-1", envelope: { token: "envelope" } };

/** The view is controlled by the surface, so the tests own the state the console's App owns. */
function ScreenHarness({ initial = "desktop" as "desktop" | "browser" }) {
  const [view, setView] = useState<"desktop" | "browser">(initial);
  return <ScreenPage grant={grant} view={view} onViewChange={setView} />;
}

describe("ScreenPage", () => {
  afterEach(() => {
    mocks.callTool.mockReset();
  });

  it("will not capture a screen from a machine the fleet says is down", () => {
    render(
      <ScreenPage grant={grant} view="desktop" onViewChange={() => undefined} deviceOnline={false} />
    );
    expect(
      screen.getByText("The machine is offline; a capture is not possible until it reconnects.")
    ).toBeInTheDocument();
    expect(screen.queryByText(/No capture yet/)).not.toBeInTheDocument();
    const capture = screen.getByRole("button", { name: "Capture screen" });
    expect(capture).toBeDisabled();
    expect(capture.getAttribute("title")).toMatch(/reconnects/);
  });

  it("says no picture has been taken and names the control that takes one", () => {
    render(<ScreenPage grant={grant} view="desktop" onViewChange={() => undefined} />);
    expect(
      screen.getByText(
        "No capture yet. Capture screen takes one when you ask; nothing is captured on a timer."
      )
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Capture screen" })).toBeInTheDocument();
  });

  it("says the helper has not registered before the operator clicks", () => {
    render(
      <ScreenPage
        grant={grant}
        view="desktop"
        onViewChange={() => undefined}
        helperRunning={false}
      />
    );

    // The fleet already knows; the page should not present the click as if it could succeed.
    expect(screen.getByText(/has not registered, so a capture will be refused/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Capture screen" })).toBeEnabled();
  });

  it("stays quiet when the device did not say or the helper is registered", () => {
    const { rerender } = render(
      <ScreenPage grant={grant} view="desktop" onViewChange={() => undefined} />
    );
    expect(screen.queryByText(/has not registered/)).not.toBeInTheDocument();

    rerender(
      <ScreenPage grant={grant} view="desktop" onViewChange={() => undefined} helperRunning />
    );
    expect(screen.queryByText(/has not registered/)).not.toBeInTheDocument();
  });

  it("captures the device's screen on demand and offers the picture for download", async () => {
    mocks.callTool.mockResolvedValue(
      JSON.stringify({ format: "png", data: "iVBORw0KGgo=", width: 1280, height: 720 })
    );
    render(<ScreenHarness />);

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
    render(<ScreenHarness />);

    fireEvent.click(screen.getByRole("button", { name: "Capture screen" }));

    const refusal = await screen.findByText("no per-user helper is running on this device");
    expect(refusal).toHaveClass("error");
    expect(screen.getByText(/running in the signed-in user's desktop session/)).toBeInTheDocument();
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Capture screen" })).toBeEnabled();
  });

  it("clears the previous frame when a second capture fails", async () => {
    mocks.callTool.mockResolvedValueOnce(
      JSON.stringify({ format: "png", data: "iVBORw0KGgo=", width: 1280, height: 720 })
    );
    mocks.callTool.mockRejectedValueOnce(new ToolError("the session is locked", 200, null, true));
    render(<ScreenHarness />);

    fireEvent.click(screen.getByRole("button", { name: "Capture screen" }));
    expect(await screen.findByRole("img", { name: "the device's screen" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Capture screen" }));
    expect(await screen.findByText("the session is locked")).toBeInTheDocument();
    // An older picture under a refusal would read as the screen the device just failed to take.
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
  });

  it("switches to the browser view without leaving Screen", () => {
    render(<ScreenHarness />);

    fireEvent.click(screen.getByRole("button", { name: "Browser" }));
    expect(screen.getByRole("heading", { name: "Browser pane" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Capture screen" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Machine screen" }));
    expect(screen.getByRole("button", { name: "Capture screen" })).toBeInTheDocument();
  });

  it("opens directly on the browser view when the fragment names it", () => {
    render(<ScreenHarness initial="browser" />);
    expect(screen.getByRole("heading", { name: "Browser pane" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Capture screen" })).not.toBeInTheDocument();
  });

  it("says a transport failure never reached the device", async () => {
    mocks.callTool.mockRejectedValue(new ToolError("Failed to fetch", 0));
    render(<ScreenHarness />);

    fireEvent.click(screen.getByRole("button", { name: "Capture screen" }));

    const failure = await screen.findByText(/never reached the device/);
    expect(failure).toHaveClass("error");
    expect(failure).toHaveTextContent("Failed to fetch");
  });
});
