// A stub Playwright page, for reading the PLAN out of a sweep without a browser.
//
// THE ORACLE THIS EXISTS FOR (slice 1 of landing 2b). The plan — which surfaces a sweep visits, in
// which order, at which viewport, with which passes — used to be computed inside the payload, in
// JavaScript. Proving the Rust plan equivalent to it cannot be done by re-reading the JavaScript and
// writing the same thing twice; the JavaScript side has to be EXECUTED. So the pre-change payload is
// run against this page, which answers every measurement with nothing and RECORDS what the payload
// asked the browser to do. That recording is the plan the JavaScript actually executed.
//
// It is deliberately not a browser: no DOM, no rendering, no network. What it captures is exactly the
// plan-shaped part of the driving (navigate, resize, emulate media, set the SPA hash, reload), and the
// probes' answers are irrelevant to it.
//
// ONE SESSION PER RUN. `reset()` starts a fresh recording, because the harness runs many cases in one
// process and a shared trace would splice one case's events into the next.
let current = null;

function fresh() {
  let resolve;
  const finished = new Promise((r) => { resolve = r; });
  current = { trace: [], finished, resolve };
  return current;
}

function record(ev) {
  if (current) current.trace.push(ev);
}

// A page that answers the same nothing every time: array-like, self-returning and CALLABLE, so
// `d.targets`, `rows.filter(...)`, `theme.attr` and `reduced.animating.slice(0, 6)` all behave like an
// empty answer instead of a TypeError that would abort the run before the trace was complete. ONE
// instance, so two reads of the same thing compare equal — which is what a static page does.
const EMPTY = new Proxy(function () { return EMPTY; }, {
  get(t, p) {
    if (p === Symbol.iterator) return Array.prototype[Symbol.iterator].bind([]);
    if (p === "then" || typeof p === "symbol") return undefined;
    if (p === "length") return 0;
    if (p === "toString" || p === "valueOf") return () => "";
    return EMPTY;
  },
  apply() { return EMPTY; },
});

const page = {
  async goto(url) {
    // `cb` is a cache-buster built from `Date.now()`, so it is stripped: it is the one part of the URL
    // that is different on every run, and comparing it would be comparing two clocks.
    record({ t: "goto", url: String(url).replace(/[?&]cb=[^&]*/g, "") });
  },
  async reload() { record({ t: "reload" }); },
  async setViewportSize(vp) { record({ t: "viewport", width: vp.width, height: vp.height }); },
  async emulateMedia(m) { record({ t: "media", ...m }); },
  async evaluate(fn, arg) {
    const src = typeof fn === "string" ? fn : String(fn);
    // THE CONSOLE DRIVES ITS SPA BY HASH, not by URL, so the hash assignment is part of the plan and is
    // classified as such. The argument is a string on most paths and a `[theme, hash]` pair on the ones
    // that also set the theme — the harness normalizes both to the hash.
    if (/location\.hash/.test(src)) record({ t: "hash", hash: arg });
    else record({ t: "evaluate", fn: src.replace(/\s+/g, " ").trim().slice(0, 60) });
    return EMPTY;
  },
  async waitForTimeout(ms) { record({ t: "wait", ms }); },
  async waitForSelector(sel) { record({ t: "waitForSelector", sel }); },
  async waitForFunction() { return true; },
  async route(pattern) { record({ t: "route", pattern: String(pattern) }); },
  async unroute() {},
  async $$() { return []; },
  async $() { return null; },
  async screenshot() { return Buffer.alloc(0); },
  async addInitScript() {},
  on() {},
  url() { return "http://summrise.test/"; },
  mouse: {
    async move() { record({ t: "mouse.move" }); },
    async down() { record({ t: "mouse.down" }); },
    async up() { record({ t: "mouse.up" }); },
    async click() {},
  },
  keyboard: { async press() { record({ t: "key" }); }, async type() {} },
};

const handler = {
  get(target, prop) {
    if (prop in target) return target[prop];
    // Any API this stub does not know is a call that is not part of the plan; recording it as `call`
    // keeps the trace honest about what was ignored rather than pretending it did not happen.
    return async (...args) => {
      record({ t: "call", m: String(prop), a: args.length });
      return undefined;
    };
  },
};

module.exports = {
  /** The recording in progress. */
  get state() { return current || fresh(); },
  reset: fresh,
  async acquireBrowser() {
    if (!current) fresh();
    const session = current;
    return {
      page: new Proxy(page, handler),
      close: async () => { session.resolve(); },
    };
  },
};
