/**
 * THE DECISION ROUTE'S TYPESCRIPT HALF — and the one distinction it must not collapse.
 *
 * `decide` answers `undefined` when the GATE did not answer (absent, refused, unreachable) and the decision's own
 * value — which may be `null` — when it did. The caller falls back to its TypeScript twin on `undefined` only.
 * **Collapsing the two would let a wrong `null` from Rust be silently corrected by the twin**, which is the one
 * failure this route must not hide: the twin exists to keep the product working when the gate is gone, not to make a
 * wrong answer look right.
 *
 * MUTATION: return `undefined` for a `body.ok === false` answer AND for a null value (i.e. `return body.value`).
 * RESULT:   `null` and "no gate" become the same answer — the third assertion fails, and in production a decision
 *           that answered null would silently take the twin's path instead, so the divergence would never be seen.
 *
 * MUTATION: drop the `try`/`catch` around the fetch.
 * RESULT:   an unreachable gate throws out of the caller instead of falling back — the last assertion fails, and the
 *           admin probe would 500 where it used to answer.
 */
import test from "node:test";
import assert from "node:assert/strict";

const { decide } = await import("../src/decisions.ts");

/** An `env` whose gate answers `body` with `status`. */
function envAnswering(body, status = 200) {
  return {
    WASM_GATE: {
      fetch: async () => new Response(JSON.stringify(body), { status }),
    },
  };
}

test("a decision that ran is returned, and null is a VALUE", async () => {
  const ran = await decide(envAnswering({ ok: true, value: "https://x/v1/models" }), "upstream_models_url", {});
  assert.equal(ran, "https://x/v1/models");

  // **`null` IS A DECISION, NOT AN ABSENCE.** It must NOT become `undefined`.
  const answeredNull = await decide(envAnswering({ ok: true, value: null }), "upstream_models_url", {});
  assert.equal(answeredNull, null);
  assert.notEqual(answeredNull, undefined);
});

test("no gate, a refusal and an unreachable gate are all `undefined`", async () => {
  assert.equal(await decide({}, "upstream_models_url", {}), undefined);
  assert.equal(await decide(envAnswering({ ok: false, error: "unknown decision: x" }, 400), "x", {}), undefined);
  assert.equal(
    await decide(
      {
        WASM_GATE: {
          fetch: async () => {
            throw new Error("binding unreachable");
          },
        },
      },
      "upstream_models_url",
      {},
    ),
    undefined,
  );
});

test("the request is a POST to the decision path, with the name and args as JSON", async () => {
  let seen = null;
  const env = {
    WASM_GATE: {
      fetch: async (request) => {
        seen = { method: request.method, path: new URL(request.url).pathname, body: await request.json() };
        return new Response(JSON.stringify({ ok: true, value: 1 }), { status: 200 });
      },
    },
  };
  await decide(env, "offered_ids", { payload: ["a"] });
  assert.equal(seen.method, "POST");
  assert.equal(seen.path, "/__decision");
  assert.deepEqual(seen.body, { name: "offered_ids", args: { payload: ["a"] } });
});
