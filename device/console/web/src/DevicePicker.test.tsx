import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Device } from "./api";
import { DevicePicker } from "./DevicePicker";

const devices: Device[] = [
  { id: "a", name: "Alpha", online: true, version: "1.2.3", last_seen_ms: 1 },
  { id: "b", name: "Beta", online: false, version: null, last_seen_ms: null }
];

describe("DevicePicker", () => {
  it("names each Use button with its machine and states the state in words", () => {
    render(<DevicePicker devices={devices} onChoose={vi.fn()} />);

    expect(screen.getByRole("button", { name: "Use Alpha" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Use Beta" })).toBeInTheDocument();
    // **THE STATE IS A WORD, NOT ONLY A COLOUR.** The words used to be `.sr-only` under an
    // `aria-hidden` dot; a sighted operator who cannot read green from grey had no state at all.
    const online = screen.getByText("online");
    const offline = screen.getByText("offline");
    expect(online).toHaveClass("picker-state");
    expect(offline).toHaveClass("picker-state");
    expect(online).not.toHaveClass("sr-only");
    // The age stays one hover away, as Home's row spells it; a machine the console never heard
    // states the state alone rather than inventing a moment.
    expect(online).toHaveAttribute("title", expect.stringMatching(/^online · seen .+ ago$/));
    expect(offline).toHaveAttribute("title", "offline");
  });

  it("keeps the version's v, so it is the same fact as Home's column", () => {
    render(<DevicePicker devices={devices} onChoose={vi.fn()} />);
    expect(screen.getByText("v1.2.3")).toBeInTheDocument();
    expect(screen.queryByText("1.2.3")).not.toBeInTheDocument();
  });
});
