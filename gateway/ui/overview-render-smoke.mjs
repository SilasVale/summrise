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

/** Mount the bundle with `routes` stubbed; returns the document plus the unmocked set. */
async function mount(routes) {
  const dom = new JSDOM(html, {
    url: "https://ai.saisi.online/#/",
    runScripts: "outside-only",
    pretendToBeVisual: true,
  });
  const { window } = dom;
  const unmocked = new Set();
  window.fetch = async (input) => {
    const path = new URL(String(input), "https://ai.saisi.online").pathname;
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
  };
  window.localStorage.setItem("valegate-lang", "zh");
  window.__ims = (s) => s;
  window.eval(
    js
      .replaceAll("import.meta.resolve", "window.__ims")
      .replaceAll("import.meta.url", JSON.stringify("https://ai.saisi.online/")),
  );
  await new Promise((r) => setTimeout(r, 800));
  return { doc: window.document, unmocked };
}

const me = (keys) => ({ username: "admin", role: "admin", token: "tok", keys });

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
  // THE CARD IS FOUND BY ITS TITLE, NOT BY A CLASS THAT NO LONGER EXISTS (round 29 of the standing goal). This
  // smoke looked for `.ov-firstrun` and the view renders `<Card title={t("overview.firstRun")}>` — no such class —
  // so all three first-run checks failed against a page that was rendering the card correctly. Nothing runs this
  // smoke in CI (`ci.yml` runs `render-smoke.mjs` only), which is why the drift survived.
  const card =
    [...doc.querySelectorAll(".card")].find((c) =>
      (c.querySelector(".card-title")?.textContent || "").includes("从这里开始"),
    ) || null;
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
  check("a configured deployment gets NO first-run card", doc.querySelector(".ov-firstrun") === null);
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
  const text = doc.querySelector(".ov-firstrun")?.textContent || "";
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
