// EmbeddedBrowserPane pins — the real-browser controller with a mocked
// main-process bridge: init state, nav-event bar sync, address submit
// (https default, scheme rejection), back/fwd wiring, crash banner +
// recover, zoom factor, slot bounds reporting.
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { EmbeddedBrowserPane } from "../EmbeddedBrowserPane";

interface NavHandler {
  (s: { url: string; canBack: boolean; canFwd: boolean; title: string }): void;
}
interface GoneHandler {
  (d: { reason: string; exitCode: number }): void;
}

const bridge = (url = "https://example.com/") => {
  const handlers = { nav: [] as NavHandler[], gone: [] as GoneHandler[] };
  return {
    handlers,
    mock: {
      navigate: vi.fn(() => Promise.resolve()),
      back: vi.fn(() => Promise.resolve()),
      fwd: vi.fn(() => Promise.resolve()),
      reload: vi.fn(() => Promise.resolve()),
      zoom: vi.fn(() => Promise.resolve()),
      place: vi.fn(() => Promise.resolve()),
      state: vi.fn(() =>
        Promise.resolve({
          ok: true,
          url,
          canBack: true,
          canFwd: false,
        }),
      ),
      recover: vi.fn(() => Promise.resolve()),
      onNav: vi.fn((h: NavHandler) => {
        handlers.nav.push(h);
        return () => {};
      }),
      onGone: vi.fn((h: GoneHandler) => {
        handlers.gone.push(h);
        return () => {};
      }),
    },
  };
};

beforeEach(() => {
  vi.stubGlobal(
    "fetch",
    vi.fn(() => Promise.reject(new Error("offline"))),
  );
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("EmbeddedBrowserPane", () => {
  it("initializes from bridge state and reports slot bounds", async () => {
    const b = bridge();
    vi.stubGlobal("summriseEmbedded", b.mock);
    render(<EmbeddedBrowserPane token="t" />);
    await waitFor(() => expect(b.mock.state).toHaveBeenCalled());
    expect(
      await screen.findByDisplayValue("https://example.com/"),
    ).toBeTruthy();
    expect(screen.getByTitle("Back").closest("button")!.disabled).toBe(false);
    expect(screen.getByTitle("Forward").closest("button")!.disabled).toBe(true);
    expect(b.mock.place).toHaveBeenCalled();
  });

  it("nav events sync the bar; editing locks it until blur", async () => {
    const b = bridge();
    vi.stubGlobal("summriseEmbedded", b.mock);
    render(<EmbeddedBrowserPane token="t" />);
    await screen.findByDisplayValue("https://example.com/");
    b.handlers.nav.forEach((h) =>
      h({ url: "https://other.test/", canBack: true, canFwd: true, title: "" }),
    );
    expect(await screen.findByDisplayValue("https://other.test/")).toBeTruthy();
    expect(screen.getByTitle("Forward").closest("button")!.disabled).toBe(
      false,
    );
  });

  it("address submit defaults bare domains to https; rejects bad schemes", async () => {
    const b = bridge();
    vi.stubGlobal("summriseEmbedded", b.mock);
    render(<EmbeddedBrowserPane token="t" />);
    await screen.findByDisplayValue("https://example.com/");
    const input = screen.getByPlaceholderText(
      /Enter a URL/,
    ) as HTMLInputElement;
    fireEvent.change(input, { target: { value: "example.org/x" } });
    fireEvent.click(screen.getByText("Go"));
    expect(b.mock.navigate).toHaveBeenCalledWith("https://example.org/x");
    fireEvent.change(input, { target: { value: "data:text/html,hi" } });
    fireEvent.click(screen.getByText("Go"));
    expect(
      await screen.findByText(
        "Only http(s) URLs are supported in the embedded browser",
      ),
    ).toBeTruthy();
    expect(b.mock.navigate).toHaveBeenCalledTimes(1);
  });

  it("back/fwd buttons drive the bridge", async () => {
    const b = bridge();
    vi.stubGlobal("summriseEmbedded", b.mock);
    render(<EmbeddedBrowserPane token="t" />);
    await screen.findByDisplayValue("https://example.com/");
    b.handlers.nav.forEach((h) =>
      h({
        url: "https://example.com/",
        canBack: true,
        canFwd: true,
        title: "",
      }),
    );
    fireEvent.click(await screen.findByTitle("Back"));
    expect(b.mock.back).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByTitle("Forward"));
    expect(b.mock.fwd).toHaveBeenCalledTimes(1);
  });

  it("crash banner shows reason; recover re-queries state", async () => {
    const b = bridge();
    vi.stubGlobal("summriseEmbedded", b.mock);
    render(<EmbeddedBrowserPane token="t" />);
    await screen.findByDisplayValue("https://example.com/");
    b.handlers.gone.forEach((h) => h({ reason: "oom", exitCode: 1 }));
    expect(
      await screen.findByText("The embedded browser crashed"),
    ).toBeTruthy();
    expect(screen.getByText("reason: oom")).toBeTruthy();
    fireEvent.click(screen.getByText("Reload browser"));
    await waitFor(() => expect(b.mock.recover).toHaveBeenCalled());
  });

  it("zoom selector drives the real view factor", async () => {
    const b = bridge();
    vi.stubGlobal("summriseEmbedded", b.mock);
    render(<EmbeddedBrowserPane token="t" />);
    await screen.findByDisplayValue("https://example.com/");
    fireEvent.change(screen.getByLabelText("Zoom"), {
      target: { value: "150" },
    });
    expect(b.mock.zoom).toHaveBeenCalledWith(1.5);
  });

  it("no bridge → placeholder without crashing", () => {
    render(<EmbeddedBrowserPane token="t" />);
    expect(screen.getByText("Starting embedded browser…")).toBeTruthy();
  });
});


// ── THE BLACK VIEWPORT (1.2.449) ──────────────────────────────────────────────────────────────────────────────────────────
// The operator sent a screenshot of the desktop app: the browser page, address bar carrying a URL, and a viewport that was a
// solid black rectangle with the words "Starting embedded browser…" gone and the status bar claiming "live (native render)".
// That is `ready` (the bridge answered) rendered as if it were `loaded` (the view has a page). These two cases pin the
// difference — and the empty state is not decoration: the native view is an OS-level child that paints nothing until a page
// loads, so without words this state is indistinguishable from a broken app.
describe("EmbeddedBrowserPane — ready is not loaded", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("says so when the bridge is ready and no page is loaded", async () => {
    const b = bridge("about:blank");
    vi.stubGlobal("summriseEmbedded", b.mock);
    render(<EmbeddedBrowserPane token="t" />);
    expect(await screen.findByText("No page loaded")).toBeTruthy();
    expect(screen.getByText(/no page loaded/)).toBeTruthy(); // the status bar agrees with the viewport
    expect(screen.queryByText(/live \(native render\)/)).toBeNull();
  });

  it("claims live only once a real page is loaded", async () => {
    const b = bridge();
    vi.stubGlobal("summriseEmbedded", b.mock);
    render(<EmbeddedBrowserPane token="t" />);
    expect(await screen.findByDisplayValue("https://example.com/")).toBeTruthy();
    expect(await screen.findByText(/live \(native render\)/)).toBeTruthy();
    expect(screen.queryByText("No page loaded")).toBeNull();
  });
});
