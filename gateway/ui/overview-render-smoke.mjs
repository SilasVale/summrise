// Overview first-run hint: does the page tell a fresh operator what to do — and does it stay
// QUIET when the deployment already has credentials of a kind it cannot see in the key matrix?
//
// THE HONESTY RULE THIS PINS. "0/8 keys" is not "no credentials": a CUSTOM PROVIDER's record
// carries its own key, and `keyReady` is that key resolving here. A hint that reads the key
// matrix alone would tell an operator who is already serving traffic to go and set one up. So
// scene 3 is the point of this file, not a variation of it.
//
// Usage: npm run smoke:overview   (after npm run build)
import { JSDOM } from "jsdom";
import { readFileSync } from "node:fs";

const jsPath = process.argv[2];
if (!jsPath) {
  console.error("usage: node overview-render-smoke.mjs <built-js>");
  process.exit(1);
}
const html = readFileSync("../public/index.html", "utf8");
const js = readFileSync(jsPath, "utf8");

const PROVIDER = {
  prefix: "my/",
  label: "My Provider",
  baseURL: "https://api.example.com",
  api: "openai-completions",
  models: [{ id: "llama-3" }],
  advertised: ["my/llama-3"],
  keyEnv: "",
  keyMasked: "sk-…9876",
  keyReady: true,
};

/** Wait until every route this scene stubbed has been ANSWERED, and the render has gone quiet.
 *
 *  Why not a duration, and why not `#root.children.length > 0`: see the block at the call site.
 *  This waits for the thing the checks actually depend on — the page's reads having arrived — which
 *  is independent of what any check asserts, so it cannot turn an assertion into a tautology. The
 *  quiet window then covers the commit that follows the last read (measured: the same 10 ms sample
 *  in which the sixth read is answered is the one where the card is present).
 */
async function waitForReads(window, routes, answered, { ceilingMs = 5000, quietMs = 50 } = {}) {
  const wanted = Object.keys(routes);
  const deadline = Date.now() + ceilingMs;
  let signature = null;
  let quietSince = Date.now();
  while (Date.now() < deadline) {
    const root = window.document.querySelector("#root");
    const now = `${answered.size}:${root ? root.innerHTML.length : -1}`;
    if (now !== signature) {
      signature = now;
      quietSince = Date.now();
    }
    if (wanted.every((p) => answered.has(p)) && Date.now() - quietSince >= quietMs) {
      return { allAnswered: true, missing: [], ceilingMs };
    }
    await new Promise((r) => setTimeout(r, 10));
  }
  return { allAnswered: false, missing: wanted.filter((p) => !answered.has(p)), ceilingMs };
}

/** Mount the bundle with `routes` stubbed; returns the document plus the unmocked set. */
async function mount(routes) {
  const dom = new JSDOM(html, {
    url: "https://ai.saisi.online/#/",
    runScripts: "outside-only",
    pretendToBeVisual: true,
  });
  const { window } = dom;
  // THE CONSOLE'S RUST, COMPILED FROM THE SERVED FILE. jsdom has no server to fetch
  // `/ui_logic_bg.wasm` from, and since 2026-09-29 the migrated functions are called DURING RENDER —
  // so without this the smoke renders a page whose marks throw. It is the SAME artifact the browser
  // gets (`../public/ui_logic_bg.wasm`, the file vite copies out of `ui/public/`), compiled here
  // instead of streamed, which is the seam's own "two environments, one artifact" rule.
    // AND JSDOM HAS NO `WebAssembly` AT ALL, which is why the shim beside it is not decoration: without
  // it `initSync`'s `module instanceof WebAssembly.Module` throws inside the seam, the load promise
  // rejects, and the page renders with `logic()` throwing on its first call. The browser has it; the
  // harness must say the same thing about the environment it is standing in for.
  window.WebAssembly = WebAssembly;
  window.__consoleLogicModule = WebAssembly.compile(readFileSync("../public/ui_logic_bg.wasm"));
  const unmocked = new Set();
  const answered = new Set();
  window.fetch = async (input) => {
    const path = new URL(String(input), "https://ai.saisi.online").pathname;
    try {
      if (path in routes) {
        return new Response(JSON.stringify(routes[path]), {
          status: 200,
          headers: { "content-type": "application/json" },
        });
      }
      unmocked.add(path);
      return new Response(JSON.stringify({ error: { message: "not mocked: " + path } }), {
        status: 404,
      });
    } finally {
      // **THE ANSWER, NOT THE ASK — AND THE DIFFERENCE IS THE WHOLE FIX.** A first version of this
      // recorded the path where it was REQUESTED, and a delay injected into the stub exposed it: the
      // page asks for all six routes at once and their answers arrive later, so "all asked" was true
      // ~50 ms in and the checks ran against a page that had read nothing. Measured with every
      // stubbed read delayed 1200 ms: `asked` failed 3/3 while `answered` passes 3/3.
      answered.add(path);
    }
  };
  window.localStorage.setItem("summrisegate-lang", "zh");
  window.__ims = (s) => s;
  window.eval(
    js
      .replaceAll("import.meta.resolve", "window.__ims")
      .replaceAll("import.meta.url", JSON.stringify("https://ai.saisi.online/")),
  );
  // ── WAIT FOR THE READS, NOT FOR A DURATION AND NOT FOR A CHILD (measured 2026-10-09) ────────────
  //
  // THE LINE HERE WAS `await new Promise((r) => setTimeout(r, 800))`, and 800 ms is a stopwatch
  // standing in for a condition: on a loaded box the bundle has not finished its reads, three
  // scene-1 checks fail, and `all-gates` reports `OVERVIEW RENDER FAIL (3)` — a broken console
  // that is actually a slow machine. **A FLAKY GATE IS WORSE THAN A SLOW ONE.**
  //
  // **AND THE OBVIOUS REPLACEMENT IS WORSE, WHICH IS WHY THIS COMMENT IS LONG.** The first fix
  // polled `#root.children.length > 0` (the shape `render-smoke.mjs` uses). That condition is
  // satisfied by the FIRST React commit, which lands BEFORE this page's dashboard reads resolve —
  // and the first-run card is conditioned ON those reads:
  //
  //     const noKeys    = configuredCount === 0 && providers !== null && !providers.some((p) => p.keyReady);
  //     const noDevices = devices !== null && devices.length === 0 && isAdmin;
  //
  // ("a read that failed is not a zero", the view says — an unread state renders NO card.) So the
  // poll broke at the shell and scene 1 failed DETERMINISTICALLY, the same three checks every time,
  // while scenes 2 and 3 passed VACUOUSLY: the card they assert ABSENT had not been given the
  // chance to appear. That is the exact vacuity this file was fixed for in round 29, reintroduced
  // by a fix for the flake.
  //
  // MEASURED, traced at 10 ms over scene 1: 20 ms the shell (`html=62`, 0 cards) · 76 ms the first
  // read answered (`html=6388`, 4 cards) · **92 ms all six stubbed reads answered and the first-run
  // card present**. So the reads are the condition that decides this smoke, and waiting for them
  // costs ~90 ms instead of an 800 ms stopwatch.
  //
  // AND IT IS THE ANSWERS, NOT THE REQUESTS, WHICH THE SAME HANDICAP MEASURED: with every stubbed
  // read delayed 1200 ms, a version that waited for the paths to be REQUESTED failed 3/3 — the page
  // asks for all six at once and their answers arrive later — while this one waits for each answer
  // and passes 3/3. A first version of this fix had exactly that bug, and the delay is what found
  // it; the checks are the same three either way.
  //
  // THE CEILING IS THE FAILURE CASE, NOT A TOLERATED DURATION: hitting it means a read this scene
  // stubbed was never ANSWERED, and the line below NAMES it rather than leaving a reader to guess
  // why the run was slow. The checks then run anyway and say what they see.
  const settle = await waitForReads(window, routes, answered);
  if (!settle.allAnswered) {
    console.log(
      `  ! the page never got an answer for ${settle.missing.join(", ")} within ${settle.ceilingMs} ms — the checks below ran against whatever had rendered`,
    );
  }
  return { doc: window.document, unmocked };
}

const me = (keys) => ({ username: "admin", role: "admin", token: "tok", keys });

// THE FIRST-RUN CARD, FOUND ONE WAY FOR ALL THREE SCENES — and this line is why scenes 2 and 3 mean
// something again. Scene 1 already looked the card up BY ITS TITLE, after round 29 found that
// `.ov-firstrun` had never been rendered (the view renders <Card title={t("overview.firstRun")}>);
// scenes 2 and 3 kept the dead selector, so `querySelector(".ov-firstrun")` was always null and
// `?.textContent || ""` was always the empty string — two checks that could not fail, one of them
// scene 3, the honesty rule this whole file exists for. A shared lookup means the next drift breaks
// scene 1 loudly instead of hollowing out the others in silence.
//
// The anchor is the RENDERED title, so a change to the zh string stops the card being found — which
// fails the smoke rather than passing it, the direction this file wants.
const firstRunCard = (doc) => [...doc.querySelectorAll(".card")].find((c) => (c.querySelector(".card-title")?.textContent || "").includes("从这里开始")) || null;

const checks = [];
const check = (name, ok) => checks.push([name, ok]);

// ── scene 1: nothing at all — the hint must appear, both lines ───────────────────────
{
  const { doc, unmocked } = await mount({
    "/api/me": me({}),
    "/api/devices": { devices: [] },
    "/api/health": { channels: [] },
    "/api/admin/users": { users: [] },
    "/api/plugins/status": { devices: {} },
    "/api/admin/providers": { providers: [], apis: [], filePrefixes: [] },
  });
  // THE CARD IS FOUND BY ITS TITLE, NOT BY A CLASS THAT NO LONGER EXISTS (round 29 of the standing
  // goal). This smoke looked for `.ov-firstrun` and the view renders
  // `<Card title={t("overview.firstRun")}>` — no such class — so all three first-run checks failed
  // against a page that was rendering the card correctly. Round 29 fixed SCENE 1 and left scenes 2
  // and 3 on the dead selector, where they became checks that could not fail (see `firstRunCard`).
  // AND IT NOW RUNS IN CI: `scripts/test/console-smoke-check.mjs` runs all four console smokes, which
  // is the half of this drift that made the other half survive so long.
  const card = firstRunCard(doc);
  const text = card?.textContent || "";
  const hrefs = [...(card?.querySelectorAll("a") || [])].map((a) => a.getAttribute("href"));
  check("a fresh deployment is TOLD what to do (the card renders)", !!card);
  check("the key line is there and links to the keys page", text.includes("渠道密钥") && hrefs.includes("#/keys"));
  check("the device line is there and links to the devices page", text.includes("设备") && hrefs.includes("#/devices"));
  check(`scene 1 asked for nothing unmocked (${[...unmocked].join(", ")})`, unmocked.size === 0);
}

// ── scene 2: set up — the hint must be GONE (the control case) ───────────────────────
{
  const { doc, unmocked } = await mount({
    "/api/me": me({ DEEPSEEK_API_KEY: { configured: true, masked: "sk-…1" } }),
    "/api/devices": { devices: [{ name: "d1", hostname: "d1.example" }] },
    "/api/health": { channels: [{ prefix: "og/", ok: true }] },
    "/api/admin/users": { users: [] },
    "/api/plugins/status": { devices: {} },
    "/api/admin/providers": { providers: [], apis: [], filePrefixes: [] },
  });
  check("a configured deployment gets NO first-run card", firstRunCard(doc) === null);
  check(`scene 2 asked for nothing unmocked (${[...unmocked].join(", ")})`, unmocked.size === 0);
}

// ── scene 3: 0/8 keys, but a custom provider's key resolves — the honesty rule ───────
{
  const { doc, unmocked } = await mount({
    "/api/me": me({}),
    "/api/devices": { devices: [{ name: "d1", hostname: "d1.example" }] },
    "/api/health": { channels: [] },
    "/api/admin/users": { users: [] },
    "/api/plugins/status": { devices: {} },
    "/api/admin/providers": { providers: [PROVIDER], apis: [], filePrefixes: [] },
  });
  const text = firstRunCard(doc)?.textContent || "";
  check(
    "a provider whose key resolves is NOT told to add a key (0/8 is not 'no credentials')",
    !text.includes("渠道密钥"),
  );
  check(`scene 3 asked for nothing unmocked (${[...unmocked].join(", ")})`, unmocked.size === 0);
}

let fail = 0;
for (const [name, ok] of checks) {
  console.log(ok ? "  ✔" : "  ✘", name);
  if (!ok) fail++;
}
console.log(fail === 0 ? "OVERVIEW RENDER OK" : `OVERVIEW RENDER FAIL (${fail})`);
process.exit(fail === 0 ? 0 : 1);
