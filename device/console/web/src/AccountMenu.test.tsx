import { fireEvent, render, screen, within } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { AccountMenu } from "./AccountMenu";
import { Sheet } from "./Sheet";

function draw() {
  const actions = { onSignOut: vi.fn(), onConnect: vi.fn(), onTokens: vi.fn(), onNotice: vi.fn() };
  const view = render(
    <>
      <AccountMenu session={{ authenticated: true, via: "token" }} consoleVersion="0.3.42" {...actions} />
      <button type="button">Outside</button>
    </>
  );
  const summary = screen.getByLabelText("Account menu");
  const details = summary.closest("details") as HTMLDetailsElement;
  fireEvent.click(summary);
  expect(details.open).toBe(true);
  return { ...view, ...actions, summary, details };
}

describe("AccountMenu dismissal", () => {
  it("closes on an outside press while an inside press leaves it open", () => {
    const { details } = draw();
    fireEvent.mouseDown(screen.getByText("signed in with the operator token"));
    expect(details.open).toBe(true);
    fireEvent.mouseDown(screen.getByRole("button", { name: "Outside" }));
    expect(details.open).toBe(false);
  });

  it("closes with Escape and returns focus to its summary", () => {
    const { details, summary } = draw();
    const tokens = within(details).getByRole("button", { name: "Tokens" });
    tokens.focus();
    fireEvent.keyDown(tokens, { key: "Escape" });
    expect(details.open).toBe(false);
    expect(summary).toHaveFocus();
  });

  it.each(["Connect a client", "Tokens", "Sign out"])("closes after choosing %s", (name) => {
    const { details } = draw();
    fireEvent.click(within(details).getByRole("button", { name }));
    expect(details.open).toBe(false);
  });

  it("restores the account trigger after the selected sheet closes", () => {
    function Page() {
      const [connected, setConnected] = useState(false);
      return (
        <>
          <AccountMenu
            session={{ authenticated: true, via: "token" }}
            consoleVersion={null}
            onConnect={() => setConnected(true)}
            onTokens={() => undefined}
            onSignOut={() => undefined}
            onNotice={() => undefined}
          />
          {connected && <Sheet title="Connect" onClose={() => setConnected(false)}>Endpoint</Sheet>}
        </>
      );
    }
    render(<Page />);
    const summary = screen.getByLabelText("Account menu");
    fireEvent.click(summary);
    const connect = screen.getByRole("button", { name: "Connect a client" });
    connect.focus();
    fireEvent.click(connect);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(summary.closest("details")).not.toHaveAttribute("open");
    expect(summary).toHaveFocus();
  });
});
