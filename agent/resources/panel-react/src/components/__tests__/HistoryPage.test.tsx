// HistoryPage — the composition check the per-page tests could not make.
//
// WHY THIS FILE EXISTS (round 34). ArchivePage and ActivityPage each passed their own outline test
// while HistoryPage, which EMBEDS them, rendered TWO <h1>s: the page's own hidden one plus the
// archive section's. A per-component check cannot see a page assembled from components, which is
// exactly the shape the live measurement caught.
import { describe, it, expect } from "vitest";
import { render, fireEvent } from "@testing-library/react";
import { HistoryPage } from "../HistoryPage";
import { expectOneH1, expectNoSkippedLevel } from "../../test-utils/outline";

describe("HistoryPage", () => {
  it("owns the page's only h1, and its two halves are sections under it", () => {
    const { container } = render(<HistoryPage sessions={[]} />);
    expectOneH1(container, "History");
    expectNoSkippedLevel(container);
    // Both halves carry an h2, and only one of them is mounted at a time.
    expect([...container.querySelectorAll("h2")].map((h) => h.textContent?.trim())).toContain("Sessions");
  });

  it("switches scope with a tablist, and the other half names itself too", () => {
    const { container } = render(<HistoryPage sessions={[]} />);
    const tabs = [...container.querySelectorAll('[role="tab"]')];
    expect(tabs.map((t) => t.textContent?.trim())).toEqual(["By session", "By run"]);
    fireEvent.click(tabs[1]);
    expectOneH1(container, "History");
    expectNoSkippedLevel(container);
    expect([...container.querySelectorAll("h2")].map((h) => h.textContent?.trim())).toContain("Runs");
  });
});
