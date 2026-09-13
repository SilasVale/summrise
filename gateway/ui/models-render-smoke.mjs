// Models-page render smoke: mock the admin APIs, mount #/models, and drive the FACET
// EDITOR on a model that belongs to a CUSTOM PROVIDER — the half round 105 left out.
//
// What this exists to prove, none of which a source grep can:
//   * a provider-owned model HAS an edit control (round 105 rendered none, deliberately);
//   * opening it SEEDS the draft from the provider record — an editor opened empty would
//     save that emptiness back over the record;
//   * Saving re-posts /api/admin/providers (the store that OWNS the list), and the entry it
//     rebuilds carries the edited facet while the untouched entries keep theirs;
//   * the DEFAULT row renders NO edit control (its ids cannot be addressed by the model
//     route — every save there answered 400 "unknown channel prefix");
//   * a provider's model renders NO delete control (that button calls the MODEL route and
//     404s for an id it does not own).
// Usage: npm run smoke:models   (after npm run build)
import { JSDOM } from "jsdom";
import { readFileSync } from "node:fs";

const jsPath = process.argv[2];
if (!jsPath) {
  console.error("usage: node models-render-smoke.mjs <built-js>");
  process.exit(1);
}
const html = readFileSync("../public/index.html", "utf8");
const js = readFileSync(jsPath, "utf8");

const PROVIDER_MODEL = {
  id: "llama-3",
  name: "Llama 3",
  contextWindow: 100000,
  reasoningEffort: "high",
  input: ["image"],
};

const routes = {
  "/api/me": { username: "admin", role: "admin", token: "tok-abc", keys: {} },
  "/api/health": { channels: [{ prefix: "og/", ok: true }] },
  "/api/me/route": { effective: "og/deepseek/deepseek-v4.1-flash" },
  "/api/admin/public": {
    models: [
      "og/deepseek/deepseek-v4.1-flash",
      "og/from-file",
      "my/llama-3",
      "deepseek/deepseek-v4.1-flash",
    ],
    routes: [
      { prefix: "og/", backend: "og", models: ["deepseek/deepseek-v4.1-flash", "from-file"] },
      { prefix: "my/", backend: "my", models: ["llama-3"] },
      { prefix: "none", backend: "", models: ["deepseek/deepseek-v4.1-flash"] },
    ],
  },
  // A model the CONFIG FILE declares AND the console also has a record for: the file wins, so
  // every control the console offers on it is a no-op that answers 200. The panel was told
  // about file-declared PREFIXES only, which is why this one kept its controls until round 118.
  "/api/admin/models": {
    custom: ["og/from-file"],
    disabled: [],
    facets: { "og/from-file": { name: "From file" } },
    fileModels: ["og/from-file"],
    fileOverrides: [],
  },
  "/api/admin/providers": {
    providers: [
      {
        prefix: "my/",
        label: "My Provider",
        baseURL: "https://api.example.com",
        api: "openai-completions",
        // TWO models: the second is never touched, so a re-post that drops a field on the
        // entries it carries must show up here (that is how "Adopt" erased effort).
        models: [PROVIDER_MODEL, { id: "llama-4", reasoningEffort: "low", input: ["image"] }],
        advertised: ["my/llama-3"],
        keyEnv: "",
        keyMasked: "sk-…9876",
        keyReady: true,
      },
    ],
    apis: ["openai-completions"],
    filePrefixes: [],
  },
};

const posts = [];
const dom = new JSDOM(html, {
  url: "https://ai.saisi.online/#/models",
  runScripts: "outside-only",
  pretendToBeVisual: true,
});
const { window } = dom;
const unmocked = new Set();
window.fetch = async (input, init = {}) => {
  const path = new URL(String(input), "https://ai.saisi.online").pathname;
  if ((init.method || "GET").toUpperCase() === "POST") posts.push({ path, body: init.body });
  if (path in routes) {
    return new Response(JSON.stringify(routes[path]), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  }
  unmocked.add(path);
  return new Response(JSON.stringify({ error: { message: "not mocked: " + path } }), { status: 404 });
};
window.localStorage.setItem("valegate-lang", "zh");
window.__ims = (s) => s;
window.eval(
  js
    .replaceAll("import.meta.resolve", "window.__ims")
    .replaceAll("import.meta.url", JSON.stringify("https://ai.saisi.online/")),
);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
await sleep(800);

const doc = window.document;
const rows = [...doc.querySelectorAll(".prov-list > li")];
const rowFor = (name) => rows.find((r) => (r.querySelector(".prov-name")?.textContent || "") === name);
const editBtn = (row) => row?.querySelector(".prov-model-actions button[aria-expanded]") || null;
const delBtn = (row) => row?.querySelector(".prov-model-actions button.btn-danger") || null;

/** Expand a provider row: its model list (and every per-model control) renders only then —
 *  which is also why a "no control there" assertion made against a COLLAPSED row is a false
 *  pass, the trap this harness fell into on its first run. */
const openRow = async (row) => {
  const head = [...(row?.querySelectorAll(".prov-head button") || [])].find((b) =>
    (b.textContent || "").includes("编辑"),
  );
  head?.dispatchEvent(new window.MouseEvent("click", { bubbles: true, cancelable: true }));
  await sleep(60);
};

const checks = [];
const providerRow = rowFor("my");
await openRow(providerRow);
// A harness that cannot tell "the app rendered nothing" from "I did not mock that" is not a
// harness (devices-render-smoke.mjs learned this twice by hand). If no rows are there, say
// what IS on the page.
if (!providerRow) {
  console.error(
    "NO ROWS RENDERED. body text: " +
      (doc.body.textContent || "").replace(/\s+/g, " ").slice(0, 300),
  );
}
checks.push(["the custom provider row renders", !!providerRow]);
checks.push(["its model row is there", !!providerRow?.querySelector(".prov-model-id")]);
checks.push([
  "the declared facets are shown in the row",
  (providerRow?.querySelector(".prov-model-facets")?.textContent || "").includes("effort high"),
]);

// ── drive the editor ────────────────────────────────────────────────────────────────
// ONE ROW IS OPEN AT A TIME (`open` is a single state), so every assertion about a row's
// per-model controls must run while THAT row is the open one — the first version of this
// harness asserted the delete rule after opening another row and passed against an empty DOM.
checks.push([
  "a provider's model has NO delete control (the model route owns no such id)",
  delBtn(providerRow) === null,
]);
const edit = editBtn(providerRow);
checks.push(["a provider-owned model HAS an edit control", !!edit]);
if (edit) {
  edit.dispatchEvent(new window.MouseEvent("click", { bubbles: true, cancelable: true }));
  await sleep(50);
}
const form = providerRow?.querySelector(".prov-facets") || null;
checks.push(["clicking it opens the facet editor", !!form]);
const val = (label) => {
  const l = [...(form?.querySelectorAll("label") || [])].find((n) =>
    (n.querySelector("span")?.textContent || "").includes(label),
  );
  return l?.querySelector("input, select")?.value ?? null;
};
checks.push([
  "the editor is SEEDED from the provider record, not empty",
  val("显示名") === "Llama 3" && val("上下文窗口") === "100000" && val("推理档位") === "high",
]);

if (form) {
  const nameInput = [...form.querySelectorAll("label")].find((n) =>
    (n.querySelector("span")?.textContent || "").includes("显示名"),
  )?.querySelector("input");
  // React tracks the previous value on the node; set through the native setter.
  const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set;
  setter.call(nameInput, "Llama 3 (edited)");
  nameInput.dispatchEvent(new window.Event("input", { bubbles: true }));
  await sleep(20);
  const save = [...form.querySelectorAll("button")].find((b) =>
    (b.textContent || "").includes("保存"),
  );
  save?.dispatchEvent(new window.MouseEvent("click", { bubbles: true, cancelable: true }));
  await sleep(120);
}

const providerPost = posts.find((p) => p.path === "/api/admin/providers");
checks.push(["Save re-posts the PROVIDER record, not the model route", !!providerPost]);
checks.push([
  "it does NOT write to /api/admin/models (the store that would shadow it)",
  !posts.some((p) => p.path === "/api/admin/models"),
]);
if (providerPost) {
  const body = JSON.parse(providerPost.body);
  const entry = body.models?.[0] || {};
  checks.push(["the edited facet travels", entry.name === "Llama 3 (edited)"]);
  checks.push(["the untouched facets survive (input = vision)", !!entry.input?.includes("image")]);
  const other = body.models?.[1] || {};
  checks.push([
    "an UNTOUCHED entry keeps everything (id, effort, input) — the ADOPT defect",
    other.id === "llama-4" && other.reasoningEffort === "low" && !!other.input?.includes("image"),
  ]);
  checks.push(["the key is not invented (keyless re-post)", !("apiKey" in body) && !("apiKeyEnv" in body)]);
}

// ── the controls that must NOT be there ─────────────────────────────────────────────
const defaultRow = rowFor("none");
await openRow(defaultRow);
checks.push(["the DEFAULT row renders", !!defaultRow]);
checks.push([
  "the DEFAULT row's model has NO edit control (every save there was a 400)",
  !!defaultRow && editBtn(defaultRow) === null,
]);


// ── the FILE-declared model, asserted while ITS row is the open one ──────────────────
const ogRow = rowFor("og");
await openRow(ogRow);
const modelRow = (row, id) =>
  [...(row?.querySelectorAll(".prov-model") || [])].find(
    (n) => (n.querySelector(".prov-model-id")?.textContent || "") === id,
  );
const fileModel = modelRow(ogRow, "og/from-file");
const plainModel = modelRow(ogRow, "og/deepseek/deepseek-v4.1-flash");
checks.push(["the og row renders both models", !!fileModel && !!plainModel]);
checks.push([
  "a FILE-declared model shows the config-file tag",
  (fileModel?.querySelector(".prov-tag")?.textContent || "").includes("配置文件"),
]);
checks.push([
  "a FILE-declared model offers NO edit control (the file would revert it)",
  (fileModel?.querySelector(".prov-model-actions button[aria-expanded]") ?? null) === null,
]);
checks.push([
  "a FILE-declared model offers NO delete control",
  (fileModel?.querySelector(".prov-model-actions button.btn-danger") ?? null) === null,
]);
// THE CONTROL CASE: without it, all three assertions above would also pass on a row that
// rendered nothing at all — the false-pass this harness has already fallen into twice.
checks.push([
  "CONTROL: a model the file does NOT declare still offers delete",
  (plainModel?.querySelector(".prov-model-actions button.btn-danger") ?? null) !== null,
]);

let fail = 0;
for (const [name, ok] of checks) {
  console.log(ok ? "  ✔" : "  ✘", name);
  if (!ok) fail++;
}
if (unmocked.size > 0) {
  console.error("UNMOCKED REQUESTS: " + [...unmocked].join(", "));
  fail++;
}
console.log(fail === 0 ? "MODELS RENDER OK" : `MODELS RENDER FAIL (${fail})`);
process.exit(fail === 0 ? 0 : 1);
