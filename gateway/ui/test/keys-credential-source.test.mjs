// keys-credential-source.test.mjs — THE KEYS PAGE SHOWS WHO PAYS, AND DOES NOT DECIDE IT.
//
// WHY (2026-09-21, the incident that produced it). An operator rotated their OpenCode Go key in this console and
// their clients stopped connecting. The page could say exactly one thing about every channel — "configured" or
// "not configured" — which is a fact about the USER'S KEY BOX, not about what a request will be spent on. Two
// things were invisible because of it:
//
//   * a channel served by the deployment's Worker secret read as "not configured" on a page whose whole job is to
//     explain why requests do or do not work;
//   * and after a rotation, "configured" was true while the value in force was the wrong one, with nothing on the
//     page saying that the user's key is the one being spent.
//
// The gateway now answers it: `userKeysStatus` returns `source: "user" | "deployment" | "none"`, derived from
// `BYOK_CHANNELS.envKey` — the same authority the request path uses. THIS PAGE MUST RENDER THAT FIELD. If it ever
// computes the source itself (reading `env`, or the channel table, or inferring "deployment" from a missing key),
// the console and the gateway can disagree about who pays for a request, which is the defect class this repo has a
// ratchet for everywhere else (`device-facts.test.mjs`, `sessionFacts.test.ts`).
//
// The masked-value box and the copy/reveal control stay gated on `configured` (the USER's key): a deployment secret
// is not this user's to copy, and offering "reveal" would hand a Worker secret to any session that can open the page.
import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const SRC = path.join(HERE, "..", "src");
const KEYS = readFileSync(path.join(SRC, "views", "Keys.tsx"), "utf8");
const I18N = readFileSync(path.join(SRC, "i18n.ts"), "utf8");

test("the keys page renders the gateway's `source`, all three states", () => {
  assert.match(KEYS, /source\?: "user" \| "deployment" \| "none"/, "the wire field must be typed");
  assert.match(KEYS, /const source = info\?\.source/, "the page reads the field");
  for (const value of ["user", "deployment", "none"]) {
    assert.match(KEYS, new RegExp(`"${value}"`), `the ${value} state must be handled`);
  }
  // The badge is chosen BY the source, not by `configured` — the old two-state vocabulary was
  // the thing that could not describe a deployment-served channel.
  assert.match(
    KEYS,
    /tone=\{source === "user" \? "success" : source === "deployment" \? "info" : "muted"\}/,
    "the badge must be a function of the source",
  );
});

test("the keys page does NOT compute the source itself", () => {
  // One source of truth per fact: the gateway owns which credential is in force. A view that reads the channel
  // table or the Worker env is starting a second derivation of the same answer.
  for (const forbidden of ["BYOK_CHANNELS", "envKey", "byok.ts"]) {
    assert.ok(
      !KEYS.includes(forbidden),
      `Keys.tsx must not derive the credential source (found ${forbidden}) — the API answers it`,
    );
  }
  assert.ok(
    !/\benv\s*\[/.test(KEYS),
    "Keys.tsx must not index an env bag: it cannot see the Worker's secrets, and pretending to would be a guess",
  );
});

test("a deployment-served channel offers no copy and no reveal", () => {
  // The masked box + CopyButton are inside `{configured && (...)}` — the USER's key. The deployment branch renders
  // a sentence instead, which is the only honest thing to show when the credential is not the reader's.
  const configuredGate = KEYS.indexOf("{configured && (");
  const copyButton = KEYS.indexOf("<CopyButton");
  assert.ok(configuredGate > 0, "the masked row must stay gated on the user's own key");
  assert.ok(
    copyButton > configuredGate,
    "the copy control must live INSIDE that gate, or a deployment secret becomes copyable",
  );
  assert.match(
    KEYS,
    /source === "deployment" &&[\s\S]{0,200}t\("key\.deploymentServed"/,
    "the deployment state must say so in words",
  );
  assert.ok(
    !/revealKey[\s\S]{0,120}deployment/.test(KEYS),
    "reveal is for the user's own key only",
  );
});

test("the summary counts the two groups separately", () => {
  // "0 of 9 configured" was a lie about a worker that serves seven channels. The badge counts what the USER pays
  // for, and additionally reports how many the deployment serves.
  assert.match(KEYS, /keys\[n\]\?\.source === "user"/, "the first count is the user's own keys");
  assert.match(KEYS, /keys\[n\]\?\.source === "deployment"/, "and the deployment's are reported too");
  assert.match(KEYS, /deployment: String\(/, "the badge receives both numbers");
});

test("both languages carry the three labels and the explanation", () => {
  for (const k of ["key.sourceUser", "key.sourceDeployment", "key.sourceNone", "key.deploymentServed"]) {
    const hits = I18N.split(`"${k}":`).length - 1;
    assert.equal(hits, 2, `${k} must exist in BOTH dictionaries (found ${hits})`);
  }
  assert.match(I18N, /"keys\.summary": "[^"]*\{deployment\}/, "the summary string takes the deployment count");
});
