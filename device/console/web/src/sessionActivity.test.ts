import { describe, expect, it } from "vitest";
import type { LiveLine, StartedCall } from "./liveActivity";
import { actorOf, sessionActivity, sessionOf } from "./sessionActivity";

const device = "device-1";
const session = "session-1";

/** One live line; the device and the active session are the defaults a scoped row needs. */
function line(overrides: Partial<LiveLine> & { ts_ms: number; sentence: string }): LiveLine {
  return { kind: "mcp", status: 200, device_id: device, ...overrides };
}

/** One in-flight call, announced by the proxy before the device answers. */
function started(
  overrides: Partial<StartedCall> & { id: string; tool: string; at_ms: number }
): StartedCall {
  return { device_id: device, caller: "client", ...overrides };
}

/** A write; console by default, or the client whose token is named. */
function write(
  ts_ms: number,
  input: string,
  caller: "console" | "client" = "console",
  token_id?: string
): LiveLine {
  return line({
    ts_ms,
    tool: "terminal.write",
    caller,
    ...(token_id !== undefined ? { token_id } : {}),
    evidence: { session, input },
    sentence: "Typed into a terminal"
  });
}

describe("the session's activity", () => {
  it("keeps only the active device and session", () => {
    const result = sessionActivity(
      [
        line({
          ts_ms: 3,
          device_id: device,
          tool: "terminal.write",
          caller: "console",
          evidence: { session, input: "ls" },
          sentence: "Typed `ls` into a terminal"
        }),
        line({
          ts_ms: 2,
          device_id: device,
          tool: "terminal.write",
          caller: "console",
          evidence: { session: "session-2", input: "pwd" },
          sentence: "Typed `pwd` into a terminal"
        }),
        line({
          ts_ms: 1,
          device_id: "device-2",
          tool: "terminal.write",
          caller: "console",
          evidence: { session, input: "whoami" },
          sentence: "Typed `whoami` into a terminal"
        })
      ],
      [],
      device,
      session,
      new Map()
    );

    expect(result.rows).toHaveLength(1);
    expect(result.rows[0].title).toBe("ls");
    expect(result.withheld).toBe(0);
  });

  it("withholds and counts a sessionless call instead of attributing it", () => {
    const result = sessionActivity(
      [
        line({ ts_ms: 1, tool: "files.read", caller: "client", sentence: "Read a file" }),
        line({
          ts_ms: 2,
          kind: "poll",
          tool: "screen.capture",
          caller: "console",
          sentence: "Polled the screen"
        }),
        line({
          ts_ms: 3,
          device_id: "device-2",
          tool: "files.read",
          caller: "client",
          sentence: "Read a file on another machine"
        }),
        // A session event that names no session is not a call: it is silence, not a withheld call.
        line({ ts_ms: 4, kind: "session", sentence: "device.started on device-1" })
      ],
      [],
      device,
      session,
      new Map()
    );

    expect(result.rows).toEqual([]);
    expect(result.withheld).toBe(2);
  });

  it("includes a running call whose evidence names the session, and withholds one that does not", () => {
    const result = sessionActivity(
      [],
      [
        started({
          id: "call-1",
          tool: "terminal.write",
          at_ms: 30,
          token_id: "token-1",
          evidence: { session, input: "npm test" }
        }),
        started({
          id: "call-2",
          tool: "terminal.read",
          at_ms: 20,
          evidence: { session: "session-2" }
        }),
        started({ id: "call-3", tool: "files.read", at_ms: 10 }),
        started({
          id: "call-4",
          tool: "files.read",
          at_ms: 5,
          device_id: "device-2"
        })
      ],
      device,
      session,
      new Map([["token-1", "gate-laptop"]])
    );

    expect(result.rows).toHaveLength(1);
    expect(result.rows[0]).toMatchObject({
      id: "running:call-1",
      atMs: 30,
      tone: "running",
      title: "npm test",
      actor: { label: "gate-laptop", kind: "client" }
    });
    // The sessionless call on this device is withheld; the other session's and the other machine's
    // are not this session's business and are not counted as holes in it.
    expect(result.withheld).toBe(1);
  });

  it("does not repeat a running call's tool when its evidence names nothing else", () => {
    const result = sessionActivity(
      [],
      [
        started({ id: "call-1", tool: "files.list", at_ms: 20, evidence: { session } }),
        started({
          id: "call-2",
          tool: "runs.exec",
          at_ms: 10,
          evidence: { session, command: "npm", args: ["test", "--", "--run"] }
        })
      ],
      device,
      session,
      new Map()
    );

    // The title is the tool when nothing else is proven; repeating it as a fact would be a second
    // copy of the same word. A command's row still names the tool that is running.
    expect(result.rows.map((row) => row.title)).toEqual(["files.list", "npm test -- --run"]);
    expect(result.rows.map((row) => row.facts)).toEqual([[], ["runs.exec"]]);
  });

  it("coalesces the pieces of one typed line, and never merges two actors", () => {
    const result = sessionActivity(
      [
        write(2_000, "push", "client", "token-2"),
        write(1_800, "git", "client", "token-1"),
        write(1_500, "test"),
        write(1_200, ""),
        write(1_000, "npm")
      ],
      [],
      device,
      session,
      new Map([
        ["token-1", "gate-laptop"],
        ["token-2", "other-client"]
      ])
    );

    // Newest first: the two clients typed within the burst but are not one actor, and the console's
    // own three writes (including the trimmed space keystroke) are one line.
    expect(result.rows.map((row) => row.title)).toEqual(["push", "git", "npm test"]);
    expect(result.rows.map((row) => row.actor.label)).toEqual([
      "other-client",
      "gate-laptop",
      "You"
    ]);
    expect(result.rows[2]).toMatchObject({ atMs: 1_500, jump: "npm test" });
  });

  it("joins a space keystroke into the line instead of running the words together", () => {
    // The console's excerpt trims each write, so the space between two words arrives as `""`. It is
    // a keystroke, not a missing fact: one card must read `npm test`, not `npmtest` or a blank row.
    const result = sessionActivity(
      [write(1_400, "test"), write(1_200, ""), write(1_000, "npm")],
      [],
      device,
      session,
      new Map()
    );

    expect(result.rows).toHaveLength(1);
    expect(result.rows[0]).toMatchObject({ title: "npm test", jump: "npm test" });
  });

  it("does not join writes more than the burst apart", () => {
    const result = sessionActivity(
      [write(3_002, "b"), write(1_000, "a")],
      [],
      device,
      session,
      new Map()
    );

    expect(result.rows.map((row) => row.title)).toEqual(["b", "a"]);
  });

  it("ends the typed burst at a control-only write, so two commands stay two rows", () => {
    // Enter (or a focus report) is an `input`-less terminal.write: it is the boundary between what
    // a person typed before it and after it. Two commands a second apart are still two commands.
    const result = sessionActivity(
      [
        write(3_000, "echo two"),
        line({
          ts_ms: 2_500,
          tool: "terminal.write",
          caller: "console",
          evidence: { session },
          sentence: "Typed into a terminal"
        }),
        write(2_000, "echo one")
      ],
      [],
      device,
      session,
      new Map()
    );

    expect(result.rows.map((row) => row.title)).toEqual(["echo two", "echo one"]);
    expect(result.rows.map((row) => row.jump)).toEqual(["echo two", "echo one"]);
  });

  it("does not draw a card for a control-only write", () => {
    const result = sessionActivity(
      [
        write(2, "\u001b[I"),
        line({
          ts_ms: 1,
          tool: "terminal.write",
          caller: "console",
          evidence: { session },
          sentence: "Typed into a terminal"
        })
      ],
      [],
      device,
      session,
      new Map()
    );

    expect(result.rows).toEqual([]);
    expect(result.withheld).toBe(0);
  });

  it("does not draw a blank card for a line made only of spaces", () => {
    const result = sessionActivity([write(1, "")], [], device, session, new Map());

    expect(result.rows).toEqual([]);
    expect(result.withheld).toBe(0);
  });

  it("names the actor from the record: You, the token, its short id, or Unattributed", () => {
    const names = new Map([["token-1", "gate-laptop"]]);
    const unknown = "c".repeat(64);

    expect(actorOf({ caller: "console" }, names)).toMatchObject({ label: "You", kind: "you" });
    expect(actorOf({ caller: "client", token_id: "token-1" }, names)).toMatchObject({
      label: "gate-laptop",
      kind: "client"
    });
    expect(actorOf({ caller: "client", token_id: unknown }, names)).toMatchObject({
      label: `${"c".repeat(12)}…`,
      kind: "client"
    });
    expect(actorOf({}, names)).toMatchObject({ label: "Unattributed", kind: "unknown" });

    // A token is a credential, not proof of a person or a model: the tooltip says so, and no label
    // claims one.
    for (const actor of [
      actorOf({ caller: "console" }, names),
      actorOf({ caller: "client", token_id: "token-1" }, names),
      actorOf({ caller: "client", token_id: unknown }, names),
      actorOf({}, names)
    ]) {
      expect(`${actor.label} ${actor.title}`).not.toMatch(/\bAI\b/);
    }
    expect(actorOf({ caller: "client", token_id: "token-1" }, names).title).toMatch(
      /not proof of a person or a model/
    );
  });

  it("draws a session event as a Session row, never as a client or an AI", () => {
    const result = sessionActivity(
      [
        line({
          ts_ms: 1,
          kind: "session",
          session,
          sentence: "terminal session-1 opened on device-1"
        })
      ],
      [],
      device,
      session,
      new Map()
    );

    expect(result.rows[0]).toMatchObject({
      tone: "session",
      title: "terminal session-1 opened on device-1",
      actor: { label: "Session", kind: "session" }
    });
    expect(`${result.rows[0].actor.label} ${result.rows[0].actor.title}`).not.toMatch(/\bAI\b/);
    expect(result.rows[0].jump).toBeUndefined();
  });

  it("offers a jump only for text a person typed", () => {
    const result = sessionActivity(
      [
        write(2, "npm test"),
        line({
          ts_ms: 1,
          tool: "files.read",
          caller: "console",
          evidence: { session, path: "notes.txt" },
          sentence: "Read `notes.txt`"
        })
      ],
      [],
      device,
      session,
      new Map()
    );

    expect(result.rows[0].jump).toBe("npm test");
    expect(result.rows[1].title).toBe("notes.txt");
    expect(result.rows[1].jump).toBeUndefined();
  });

  it("titles a row from the input, then the command and its arguments, then the path, then the sentence", () => {
    const result = sessionActivity(
      [
        line({
          ts_ms: 4,
          tool: "runs.exec",
          caller: "console",
          evidence: { session, command: "npm", args: ["test", "--", "--run"] },
          sentence: "Ran `npm`"
        }),
        line({
          ts_ms: 3,
          tool: "files.read",
          caller: "console",
          evidence: { session, path: "notes.txt" },
          sentence: "Read a file"
        }),
        line({
          ts_ms: 2,
          tool: "tools/list",
          caller: "console",
          evidence: { session },
          sentence: "Listed the machine's tools"
        })
      ],
      [],
      device,
      session,
      new Map()
    );

    expect(result.rows.map((row) => row.title)).toEqual([
      "npm test -- --run",
      "notes.txt",
      "Listed the machine's tools"
    ]);
  });

  it("returns rows newest first, from the lines it was given and no others", () => {
    const result = sessionActivity(
      [
        line({
          ts_ms: 1_000,
          tool: "terminal.read",
          caller: "console",
          evidence: { session },
          sentence: "Read a terminal's output"
        }),
        line({
          ts_ms: 3_000,
          tool: "terminal.close",
          caller: "console",
          evidence: { session },
          sentence: "Closed a terminal session"
        }),
        line({
          ts_ms: 2_000,
          tool: "terminal.resize",
          caller: "console",
          evidence: { session },
          sentence: "Resized a terminal"
        })
      ],
      [],
      device,
      session,
      new Map()
    );

    expect(result.rows.map((row) => row.atMs)).toEqual([3_000, 2_000, 1_000]);
  });

  it("says an empty page has no rows rather than inventing a history", () => {
    expect(sessionActivity([], [], device, session, new Map())).toEqual({ rows: [], withheld: 0 });
  });

  it("reads a session from its own field or its evidence, preferring its own", () => {
    expect(sessionOf({ session, evidence: { session: "session-2" } })).toBe(session);
    expect(sessionOf({ evidence: { session } })).toBe(session);
    expect(sessionOf({ session: "", evidence: { session: "" } })).toBeNull();
    expect(sessionOf({})).toBeNull();
  });
});
