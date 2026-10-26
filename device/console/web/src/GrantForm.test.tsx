import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { GrantForm } from "./GrantForm";

const TOOLS = [{ name: "files.read" }, { name: "files.write" }];

function renderForm() {
  // The form fetches the registry on mount; a stable two-tool answer keeps the checkbox under test
  // from moving when the request settles.
  vi.stubGlobal(
    "fetch",
    vi.fn(() => Promise.resolve({ ok: true, json: async () => TOOLS }))
  );
  render(<GrantForm deviceId="d1" onGranted={() => {}} />);
}

describe("GrantForm", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("states the policy when destructive consent is unticked, and hides it when ticked", async () => {
    renderForm();
    // Wait for /tools to land, so the assertions run against the settled form rather than the
    // fallback list.
    expect(await screen.findByText(/Tools \(2 of 2\)/)).toBeInTheDocument();

    const consent = screen.getByRole("checkbox", { name: "Allow destructive tools" });
    expect(consent).toBeChecked();
    expect(screen.queryByText(/device refuses a destructive call/)).not.toBeInTheDocument();

    fireEvent.click(consent);
    expect(consent).not.toBeChecked();

    const hint = screen.getByText(/device refuses a destructive call/);
    expect(hint).toHaveClass("hint");
    expect(hint).toHaveTextContent("Its refusal asks for the grant to be minted again with this ticked");
    expect(hint).toHaveTextContent("the console's audit keeps the refused exchange");
    expect(hint).toHaveTextContent("the device's error code");
    expect(hint).toHaveTextContent("cannot approve one call at a time");

    fireEvent.click(consent);
    expect(consent).toBeChecked();
    expect(screen.queryByText(/device refuses a destructive call/)).not.toBeInTheDocument();
  });
});
