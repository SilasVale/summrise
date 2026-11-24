import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { OfflineNotice } from "./OfflineNotice";

describe("OfflineNotice", () => {
  it("states the machine, when it was last heard, and this surface's consequence", () => {
    render(<OfflineNotice lastSeenMs={Date.now() - 2 * 60_000} detail="A shell cannot start." />);
    const notice = screen.getByRole("status");
    expect(notice).toHaveTextContent("This machine is offline.");
    expect(notice).toHaveTextContent(/last seen 2m ago/i);
    expect(notice).toHaveTextContent("A shell cannot start.");
  });

  it("does not print an age it does not have", () => {
    render(<OfflineNotice lastSeenMs={null} detail="Nothing here can reach it." />);
    const notice = screen.getByRole("status");
    expect(notice).not.toHaveTextContent(/last seen/i);
    expect(notice).toHaveTextContent("Nothing here can reach it.");
  });
});
