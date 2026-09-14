// CONSOLE KEYS — the vocabulary is the bytes, so the bytes are what gets pinned.
//
// The failure modes this file exists for are all silent:
//   * a key that sends the WRONG byte (^D instead of ^C) interrupts nothing and looks like a
//     device that ignored you;
//   * a BREAK offered as a write, or a write offered as a break — the wire has one field for
//     each, and `keyParams` is the single place that decides;
//   * a serial-only signal offered on a session that has no serial line: the device refuses it
//     by name, but the button should never have been there.
import { describe, expect, it } from "vitest";
import { CONSOLE_KEYS, bytesToBase64, keyParams, keysForKind } from "./consoleKeys";

describe("bytesToBase64", () => {
  it("encodes exactly the bytes given, one character each", () => {
    // 0x03 = Ctrl+C. base64 of a single 0x03 byte is "Aw==".
    expect(bytesToBase64([0x03])).toBe("Aw==");
    expect(bytesToBase64([0x0d])).toBe("DQ==");
    // CRLF, the shape a Windows console needs.
    expect(bytesToBase64([0x0d, 0x0a])).toBe("DQo=");
    // The escape key is a single ESC byte, not the four characters "\x1b".
    expect(bytesToBase64([0x1b])).toBe("Gw==");
  });
});

describe("keyParams", () => {
  it("carries a control byte as data_base64 and a break as break_ms", () => {
    const ctrlC = CONSOLE_KEYS.find((k) => k.id === "ctrl-c")!;
    expect(keyParams(ctrlC, "s-1")).toEqual({ session_id: "s-1", data_base64: "Aw==" });

    const brk = CONSOLE_KEYS.find((k) => k.id === "break")!;
    const params = keyParams(brk, "s-1");
    expect(params).toEqual({ session_id: "s-1", break_ms: 250 });
    // THE TWO ARE MUTUALLY EXCLUSIVE: a break must never also carry bytes, or the device's
    // "break wins" rule would be doing work the caller should have done.
    expect(params.data_base64).toBeUndefined();
  });

  it("agrees with the module's own key list", () => {
    // Every key is either bytes or a break, never neither and never both.
    for (const k of CONSOLE_KEYS) {
      const hasBytes = Array.isArray(k.bytes) && k.bytes.length > 0;
      const hasBreak = typeof k.breakMs === "number";
      expect(hasBytes !== hasBreak, `${k.id} must be exactly one of bytes/break`).toBe(true);
    }
  });
});

describe("keysForKind", () => {
  it("offers the break only where there is a serial line", () => {
    const serial = keysForKind("serial").map((k) => k.id);
    expect(serial).toContain("break");
    expect(serial).toContain("ctrl-c");

    for (const kind of ["pty", "ssh", "stub", "", null, undefined]) {
      const ids = keysForKind(kind).map((k) => k.id);
      expect(ids, `${kind} must not offer a break`).not.toContain("break");
      // …but every session still gets the interrupts, which is the whole point of the bar.
      expect(ids).toContain("ctrl-c");
      expect(ids).toContain("ctrl-d");
    }
  });

  it("treats an unknown kind as NOT serial", () => {
    // A kind added later (or an older agent's spelling) must not inherit a serial-only signal:
    // offering it would be a button the device refuses.
    expect(keysForKind("serial2").map((k) => k.id)).not.toContain("break");
    expect(keysForKind("SERIAL").map((k) => k.id)).toContain("break"); // case-insensitive match
  });
});
