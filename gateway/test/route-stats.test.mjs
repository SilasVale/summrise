// Dead-route report (round-179). The instrument, exercised on the shapes that matter.
// NOTE ON THE FILENAME: this was briefly written INTO registry.test.mjs by a script whose
// existence check chose a fallback path but did not guard the write — it destroyed ~15
// passing tests while the suite stayed GREEN. The count (838 instead of 853) is what caught
// it. Recorded here because the file name is the only trace a reader would see.
import test from "node:test";
import assert from "node:assert/strict";
import { createPluginContext, dispatch, routeStats } from "../src/plugins/registry.ts";

const ctx = () => createPluginContext(null, { jsonOk: () => new Response(), jsonError: () => new Response(), readJson: async () => ({}), CORS_HEADERS: {} });
const push = (c, m, p, tag) => c.routes.push({ match: (mm, pp) => mm === m && pp === p, handler: () => tag });

test("routeStats: a route that serves is counted, one that never matches stays at zero", () => {
  const c = ctx();
  push(c, "GET", "/a", "a");
  push(c, "POST", "/b", "b");
  assert.equal(dispatch(c, "GET", "/a"), "a");
  assert.equal(dispatch(c, "GET", "/a"), "a");
  assert.deepEqual(routeStats(c), [{ index: 0, hits: 2 }, { index: 1, hits: 0 }]);
});

test("routeStats: a SHADOWED route is reported at zero — the finding this exists for", () => {
  const c = ctx();
  push(c, "POST", "/same", "first");
  push(c, "POST", "/same", "second"); // registered later, never reached: first-match wins
  assert.equal(dispatch(c, "POST", "/same"), "first");
  const stats = routeStats(c);
  assert.equal(stats[0].hits, 1, "the route that serves is counted");
  assert.equal(stats[1].hits, 0, "the shadowed route never fires and is visible as such");
});

test("routeStats: registration never has to mention hits (the field is lazy on purpose)", () => {
  const c = ctx();
  c.routes.push({ match: () => true, handler: () => "x" });
  assert.equal(routeStats(c)[0].hits, 0, "a bare {match, handler} literal is counted from zero");
});
