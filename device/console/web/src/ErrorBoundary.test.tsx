import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ErrorBoundary } from "./ErrorBoundary";

function Boom(): never {
  throw new Error("the audit table exploded");
}

describe("ErrorBoundary", () => {
  afterEach(() => vi.restoreAllMocks());

  it("shows a reloadable panel instead of a blank page when a child throws", () => {
    // React reports the caught render error on the console; the test is about the fallback, not that
    // report.
    vi.spyOn(console, "error").mockImplementation(() => {});
    render(
      <ErrorBoundary>
        <Boom />
      </ErrorBoundary>
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("Something broke");
    expect(alert).toHaveTextContent("the audit table exploded");
    expect(screen.getByRole("button", { name: "Reload" })).toBeInTheDocument();
  });

  it("renders its children when nothing throws", () => {
    render(
      <ErrorBoundary>
        <p>the page is fine</p>
      </ErrorBoundary>
    );
    expect(screen.getByText("the page is fine")).toBeInTheDocument();
  });
});
