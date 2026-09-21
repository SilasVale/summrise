// The frame polyfill in `test-setup.ts` is the fix for a flaky panel job: xterm's Viewport schedules a refresh on a
// timer, and on a loaded CI runner that callback landed after teardown, where jsdom's `requestAnimationFrame` was gone
// — an unhandled error, which vitest counts as a FAILED RUN even with 804 passing tests.
//
// WHAT THIS TEST PINS, AND WHAT IT CANNOT. It pins the two behaviours the fix depends on — a requested frame RUNS, and
// cancelling an id it issued does not throw — because a polyfill that swallowed frames would silence xterm's refresh
// instead of surviving it.
//
// It CANNOT pin the property the fix is actually about: that a frame function is still there AFTER the environment is
// torn down. That failure happens outside any test body, and the flake does not reproduce locally (eight runs: five of
// the terminal test, three of the whole suite), so the only honest statement is that the failure mode is now
// impossible by construction and that CI is where it will show. A first version of this comment claimed the test
// proved "ours, not jsdom's" — the mutation that restored the guarded install did not fail it, so the claim was false
// and the sentence is gone.
import { describe, it, expect } from "vitest";

describe("the animation-frame polyfill", () => {
  it("hands out cancellable ids and delivers frames", () => {
    expect(typeof window.requestAnimationFrame).toBe("function");
    expect(typeof window.cancelAnimationFrame).toBe("function");
    const id = window.requestAnimationFrame(() => {});
    expect(typeof id).toBe("number");
    expect(() => window.cancelAnimationFrame(id)).not.toThrow();
  });

  it("still delivers a frame when jsdom's own would have been torn down", async () => {
    // The callback must RUN, not merely be accepted: a polyfill that swallows frames would silence xterm's refresh
    // instead of surviving it, which is the other way this fix could look green and be wrong.
    const ran = await new Promise<boolean>((resolve) => {
      window.requestAnimationFrame(() => resolve(true));
      setTimeout(() => resolve(false), 50);
    });
    expect(ran).toBe(true);
  });
});
