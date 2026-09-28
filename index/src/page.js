// page.js — the landing page's URL boundary.
//
// THE MARKUP IS RUST'S NOW. `index/landing/` renders the whole document at BUILD
// time — the HTML, the stylesheet, the brand mark, the favicon, every sentence and
// the order of the cost sentence and the button — and emits two arms into
// `./landing/`: `setup.html` for a release that published an installer, and
// `npm-only.html` for one that did not. The two behaviours (the theme toggle and
// the particle field) are Rust compiled to wasm, fetched after the page has
// painted. What is left here is the part that cannot be rendered in advance:
//
//   THE THREE URLS, AND THE POLICY ON WHAT MAY GO IN THEM.
//
// WHY THE POLICY COULD NOT MOVE WITH THE REST, and this is a finding rather than an
// oversight: two of the three URLs are built from the request's own origin and the
// third comes from a per-deployment var (CONSOLE_URL), so at build time there is
// nothing to check. The spike carried the whitelist in Rust, ported with the `url`
// crate, and that was right for a renderer that runs per request. Handed a
// placeholder token instead, `safe_page_url(token)` returns the token unchanged —
// it resolves to https against the base — which is a check that checked nothing. So
// the whitelist runs where the values arrive, and THIS IS THE ONLY COPY OF IT.
//
// The division is therefore: the Rust renderer owns every byte a reader sees, and
// this file owns the three substitutions that turn a build-time document into the
// page this deployment serves. `gen` asserts its slots survived rendering; this file
// is what fills them.

// The two arms are GENERATED MODULES, not `.html` files, and that is deliberate:
// the document has two consumers that do not share a module system. The worker
// imports it as text; plain Node — the index tests, `landing-check.mjs`, the landing
// design sweep — cannot import a `.html` at all (`ERR_UNKNOWN_FILE_EXTENSION`), and
// those three are what hold the page's contrast, its one h1, its press states and
// its reflow. A module satisfies both, and the document inside it is still a
// document a person can read.
import setupArm from "./landing/setup.js";
import npmOnlyArm from "./landing/npm-only.js";

/** The slots `index/landing` leaves for the values a request supplies. */
const CONSOLE_SLOT = "{{CONSOLE_URL}}";
const INSTALLER_SLOT = "{{INSTALLER_URL}}";
const SETUP_SLOT = "{{SETUP_URL}}";

export const PAGE = (consoleUrl, installerUrl, setupUrl) => {
  // P2-8: both URLs flow into HTML (href attributes + inline <code> text). They
  // derive from the CONSOLE_URL env var / request origin, so treat them as
  // untrusted: https-only whitelist (http allowed solely for loopback dev) +
  // HTML-escape at the interpolation point. A crafted CONSOLE_URL must never break
  // out of the attribute/element (stored-XSS via env var).
  const safeConsole = escHtml(safePageUrl(consoleUrl, "/"));
  const safeInstaller = escHtml(
    safePageUrl(installerUrl, "/summrise-agent/summrise-agent-latest.tgz"),
  );
  // THE DOOR MUST NOT OFFER WHAT THE RELEASE DOES NOT DESCRIBE (round 125).
  // A tgz-only publish leaves the SummriseAgent-Setup.exe alias serving the PREVIOUS
  // release while /api/version advertises the new one, so a "Download Windows
  // installer" button would hand a fresh install the old build. The caller passes a
  // URL only when the manifest advertises an installer; the fallback that used to
  // live here would have resurrected the link anyway, so there is none. The ARM is
  // the whole difference now — `npm-only.html` has no button and no alias in it, and
  // `gen` refuses to write one that does.
  const arm = setupUrl ? setupArm : npmOnlyArm;
  const filled = {
    [CONSOLE_SLOT]: safeConsole,
    [INSTALLER_SLOT]: safeInstaller,
  };
  if (setupUrl) {
    filled[SETUP_SLOT] = escHtml(
      safePageUrl(setupUrl, "/summrise-agent/SummriseAgent-Setup.exe"),
    );
  }
  // ONE PASS, WITH A FUNCTION. A string replacement would read `$&` and `$1` in a
  // substituted value as replacement patterns; a function cannot. And because the
  // scan is over the ORIGINAL document, a value that itself contained a slot-shaped
  // string could not be re-expanded.
  return arm.replace(/\{\{[A-Z_]+\}\}/g, (slot) => filled[slot] ?? slot);
};

// P2-8 helpers: https-only URL whitelist (http allowed solely for loopback
// dev) + HTML escaping for the landing-page interpolations above.
export function safePageUrl(u, fallback) {
  try {
    const s = String(u);
    const parsed = new URL(s, "https://placeholder.local");
    if (parsed.protocol === "https:") return s;
    const host = parsed.hostname.toLowerCase();
    // round-449: dropped the `host === "::1"` disjunct — a bare ::1 is not
    // a valid URL host (browsers/node require brackets), so the WHATWG
    // parser never yields it; only "[::1]" can occur. Dead branch removed
    // rather than pinned.
    if (
      parsed.protocol === "http:" &&
      (host === "localhost" || host === "127.0.0.1" || host === "[::1]")
    )
      return s;
    return fallback;
  } catch {
    return fallback;
  }
}

export function escHtml(s) {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}
