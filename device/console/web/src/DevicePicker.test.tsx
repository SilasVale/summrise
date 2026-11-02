import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Device } from "./api";
import { DevicePicker } from "./DevicePicker";

const devices: Device[] = [
  { id: "a", name: "Alpha", online: true, version: "1.2.3", last_seen_ms: 1 },
  { id: "b", name: "Beta", online: false, version: null, last_seen_ms: null }
];

describe("DevicePicker", () => {
  it("names each Use button with its machine and speaks the online state", () => {
    render(<DevicePicker devices={devices} onChoose={vi.fn()} />);

    expect(screen.getByRole("button", { name: "Use Alpha" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Use Beta" })).toBeInTheDocument();
    expect(screen.getByText("online")).toHaveClass("sr-only");
    expect(screen.getByText("offline")).toHaveClass("sr-only");
  });
});
