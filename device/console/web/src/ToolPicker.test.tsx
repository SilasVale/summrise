import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ToolPicker } from "./ToolPicker";

const NAMES = ["files.read", "host.facts"];

describe("ToolPicker", () => {
  it("shows the names and how many are chosen", () => {
    render(<ToolPicker names={NAMES} chosen={new Set(NAMES)} onChange={() => {}} />);

    expect(screen.getByText("Tools (2 of 2)")).toBeInTheDocument();
    for (const name of NAMES) {
      expect(screen.getByRole("checkbox", { name })).toBeChecked();
    }
  });

  it("hands the new set up when one tool is toggled", () => {
    const onChange = vi.fn();
    render(<ToolPicker names={NAMES} chosen={new Set(NAMES)} onChange={onChange} />);

    fireEvent.click(screen.getByRole("checkbox", { name: "host.facts" }));

    expect(onChange).toHaveBeenCalledWith(new Set(["files.read"]));
  });

  it("selects everything and nothing", () => {
    const onChange = vi.fn();
    render(<ToolPicker names={NAMES} chosen={new Set()} onChange={onChange} />);

    fireEvent.click(screen.getByRole("button", { name: "All" }));
    expect(onChange).toHaveBeenLastCalledWith(new Set(NAMES));
    fireEvent.click(screen.getByRole("button", { name: "None" }));
    expect(onChange).toHaveBeenLastCalledWith(new Set());
  });
});
