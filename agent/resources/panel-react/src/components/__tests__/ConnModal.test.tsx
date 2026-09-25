// ConnModal tests — the round-160 SSH private-key support: key_path rides in
// the connect extras only when entered, and the password field doubles as the
// key passphrase (server-side russh load_secret_key(path, password)).
//
// AND THE SAVED-CONNECTION PICKER, which moved onto `useDeviceRead` with the option that exists for
// it (`read`): the device's connection memory is a TOOL, so this modal's request is the POST that
// `path` cannot state. The cases at the bottom pin what the migration had to keep — the same bytes
// on the wire, only this modal's kind in the list, the pick pre-filling the framing params, and a
// re-target re-reading while the list it already has stays on screen.
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { ConnModal } from "../ConnModal";
import { callApi } from "../../lib/api";

vi.mock("../../lib/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../lib/api")>()),
  callApi: vi.fn(),
  callTool: vi.fn(),
}));
const mockCallApi = callApi as unknown as ReturnType<typeof vi.fn>;

beforeEach(() => {
  mockCallApi.mockReset();
  // THE DEVICE'S OWN ENVELOPE (`api_call_tool`): `{ok:true, result:{connections}}` — the module's
  // refusal guard reads it, and the modal's fold unwraps `result` from it.
  mockCallApi.mockResolvedValue({ ok: true, result: { connections: [] } });
});

const SSH_SAVED = {
  id: "s1",
  kind: "ssh",
  target: "root@box.example.com:2222",
  label: "prod box",
  params: { key_path: "C:\\keys\\id_ed25519" },
};
const SERIAL_SAVED = {
  id: "p1",
  kind: "serial",
  target: "COM3?baud=9600&parity=even",
  label: "rig",
  params: { parity: "even" },
};

function fill(host: string, user: string) {
  fireEvent.change(screen.getByPlaceholderText("host.example.com"), {
    target: { value: host },
  });
  fireEvent.change(screen.getByPlaceholderText("22"), {
    target: { value: "22" },
  });
  fireEvent.change(screen.getByPlaceholderText("user"), {
    target: { value: user },
  });
}

describe("ConnModal (ssh)", () => {
  it("passes key_path in the connect extras when a key path is entered", async () => {
    const onConnect = vi.fn().mockResolvedValue({});
    render(<ConnModal kind="ssh" onClose={() => {}} onConnect={onConnect} />);
    fill("box.example.com", "me");
    fireEvent.change(
      screen.getByPlaceholderText("C:\\Users\\me\\.ssh\\id_ed25519"),
      {
        target: { value: "C:\\keys\\id_ed25519" },
      },
    );
    fireEvent.click(screen.getByText("Connect"));
    await waitFor(() => expect(onConnect).toHaveBeenCalledTimes(1));
    expect(onConnect).toHaveBeenCalledWith("me@box.example.com:22", {
      password: "",
      key_path: "C:\\keys\\id_ed25519",
    });
  });

  it("omits key_path when empty (password-only connect)", async () => {
    const onConnect = vi.fn().mockResolvedValue({});
    render(<ConnModal kind="ssh" onClose={() => {}} onConnect={onConnect} />);
    fill("box.example.com", "root");
    fireEvent.click(screen.getByText("Connect"));
    await waitFor(() => expect(onConnect).toHaveBeenCalledTimes(1));
    expect(onConnect.mock.calls[0]![1]).toEqual({ password: "" });
  });

  it("clears the password field after a successful connect (P2-2)", async () => {
    const onConnect = vi.fn().mockResolvedValue({});
    render(<ConnModal kind="ssh" onClose={() => {}} onConnect={onConnect} />);
    fill("box.example.com", "root");
    const passInput = screen.getByPlaceholderText(
      "leave empty for keychain",
    ) as HTMLInputElement;
    fireEvent.change(passInput, { target: { value: "s3cret" } });
    expect(passInput.value).toBe("s3cret");
    fireEvent.click(screen.getByText("Connect"));
    await waitFor(() => expect(onConnect).toHaveBeenCalledTimes(1));
    expect(passInput.value).toBe("");
  });
});

describe("ConnModal — the saved-connection picker, read through the tool door", () => {
  it("makes the SAME request as before the migration, and keeps only this modal's kind", async () => {
    mockCallApi.mockResolvedValueOnce({
      ok: true,
      result: { connections: [SSH_SAVED, SERIAL_SAVED] },
    });
    render(<ConnModal kind="serial" onClose={() => {}} onConnect={vi.fn()} />);
    await waitFor(() =>
      expect(screen.getByRole("option", { name: "rig (p1)" })).toBeTruthy(),
    );
    expect(
      screen.queryByRole("option", { name: "prod box (s1)" }),
      "an ssh connection is not a serial target",
    ).toBeNull();
    // THE SAME BYTES ON THE WIRE as the hand-written read sent: one POST to the tool that owns the
    // list, with the same body. (The seam is the request, so the assertion is the request.)
    expect(mockCallApi).toHaveBeenCalledWith(
      "/api/tools/terminal_saved_connections",
      { method: "POST", body: "{}" },
    );
    expect(mockCallApi).toHaveBeenCalledTimes(1);
  });

  it("pre-fills the fields from a pick, framing params included (round-102)", async () => {
    mockCallApi.mockResolvedValueOnce({
      ok: true,
      result: { connections: [SERIAL_SAVED] },
    });
    render(<ConnModal kind="serial" onClose={() => {}} onConnect={vi.fn()} />);
    await waitFor(() =>
      expect(screen.getByRole("option", { name: "rig (p1)" })).toBeTruthy(),
    );
    fireEvent.change(screen.getByRole("combobox"), {
      target: { value: "p1" },
    });
    const port = screen.getByPlaceholderText(
      "COM3 or /dev/ttyUSB0",
    ) as HTMLInputElement;
    const baud = screen.getByPlaceholderText("115200") as HTMLInputElement;
    expect(port.value).toBe("COM3");
    expect(baud.value).toBe("9600");
  });

  it("re-reads when the modal is re-targeted, and the list it has stays on screen until the new one lands", async () => {
    mockCallApi
      .mockResolvedValueOnce({ ok: true, result: { connections: [SSH_SAVED] } })
      // The second read never settles: the frame of the switch is what this case is about.
      .mockReturnValueOnce(new Promise(() => {}));
    const { rerender } = render(
      <ConnModal kind="ssh" onClose={() => {}} onConnect={vi.fn()} />,
    );
    await waitFor(() =>
      expect(screen.getByRole("option", { name: "prod box (s1)" })).toBeTruthy(),
    );
    expect(mockCallApi, "one read at mount, not two").toHaveBeenCalledTimes(1);

    rerender(<ConnModal kind="serial" onClose={() => {}} onConnect={vi.fn()} />);
    await waitFor(() => expect(mockCallApi).toHaveBeenCalledTimes(2));
    expect(
      screen.getByRole("option", { name: "prod box (s1)" }),
      "the previous list is still there — a kind switch is a re-read, not a blank modal",
    ).toBeTruthy();
  });
});
