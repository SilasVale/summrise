// ── the five code VALUES are a public contract, and three of them were unpinned ──
//
// `mcp-errors.ts`'s header says the codes "surface to MCP clients in the error
// `data` (round-55) so the model can retry smartly". That makes the five STRINGS a
// contract with every MCP client that matches on them.
//
// Renaming a constant cannot break it silently — TypeScript fails in `mcp.ts`, which
// imports all five and maps to the CONSTANTS rather than to re-typed literals. But
// changing a VALUE compiles cleanly: `SESSION_NOT_FOUND = "SESSION_NOT_FOUND_V2"`
// still type-checks, `mcp.ts` still maps to the constant, and every client matching on
// the old string stops matching.
//
// Measured before writing this: two of the five were pinned, incidentally, because a
// test happened to compare against a literal — `DEVICE_UNREACHABLE` in
// `mcp-browser.test.mjs:305`/`:328` and `SESSION_BUSY` at `:357`. **`SESSION_NOT_FOUND`,
// `TIMEOUT` and `TOOL_ERROR` were pinned nowhere.** Covering 2 of 5 by coincidence is
// not a contract; this pins all five deliberately, and the reason each one is load-
// bearing is stated where a future editor will read it.
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DEVICE_UNREACHABLE,
  TIMEOUT,
  SESSION_NOT_FOUND,
  SESSION_BUSY,
  TOOL_ERROR,
  ToolErr,
} from "../src/mcp-errors.ts";

/** The contract as MCP clients receive it. Changing a value here is a BREAKING change. */
const PUBLIC_CODES = {
  DEVICE_UNREACHABLE: "DEVICE_UNREACHABLE",
  TIMEOUT: "TIMEOUT",
  SESSION_NOT_FOUND: "SESSION_NOT_FOUND",
  SESSION_BUSY: "SESSION_BUSY",
  TOOL_ERROR: "TOOL_ERROR",
};

test("the five MCP tool-failure codes keep the exact strings clients match on", () => {
  assert.equal(
    DEVICE_UNREACHABLE,
    PUBLIC_CODES.DEVICE_UNREACHABLE,
    "DEVICE_UNREACHABLE tells an MCP client the device is DOWN (it sends them on a " +
      "recovery detour). Renaming the value makes every client treat that as an " +
      "unknown code — the same failure round-64 widened the mapping to prevent.",
  );
  assert.equal(
    TIMEOUT,
    PUBLIC_CODES.TIMEOUT,
    "TIMEOUT is the gateway's class for the agent's ssh_timeout; a client that backs " +
      "off on it stops backing off.",
  );
  assert.equal(
    SESSION_NOT_FOUND,
    PUBLIC_CODES.SESSION_NOT_FOUND,
    "SESSION_NOT_FOUND is what makes the gateway retarget to a live PTY instead of " +
      "reporting a dead session (mcp-gateway.test.mjs).",
  );
  assert.equal(
    SESSION_BUSY,
    PUBLIC_CODES.SESSION_BUSY,
    "SESSION_BUSY means 'another execute is running — wait and retry'; distinct from " +
      "the agent's human_in_control by design.",
  );
  assert.equal(
    TOOL_ERROR,
    PUBLIC_CODES.TOOL_ERROR,
    "TOOL_ERROR is the deliberate round-64 fallback meaning 'the device is UP and the " +
      "tool refused' — the class that must NOT send clients on a device-recovery detour.",
  );
});

test("ToolErr carries a code unchanged, so the family is what callers actually see", () => {
  const e = ToolErr(TOOL_ERROR, "boom");
  assert.equal(e.code, PUBLIC_CODES.TOOL_ERROR);
  assert.equal(e.message, "boom");
  assert.ok(e instanceof Error, "ToolErr must stay throwable");
});
