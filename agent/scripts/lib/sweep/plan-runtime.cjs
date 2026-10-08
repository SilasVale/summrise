// plan-runtime.cjs — THE PAYLOAD SIDE OF THE PLAN.
//
// The plan itself is Rust (`agent/sweep-plan/`), computed at EMIT time and embedded in the bundle. This
// module is the only JavaScript that touches it, and it PLACES what it was given: it looks a surface up
// by the kind of block that renders it, turns a surface's fields into the browser calls the payload has
// always made, and answers `wants(name)` from the map the plan carries. It decides nothing — there is
// no default, no fallback and no second copy of any list here.
//
// THE POINT OF THE SEAM. Before this, each payload held its own matrices (`[['panel','/panel/',{...}],
// ['desktop','/desktop/',{...}]]` written out at twenty-nine call sites), its own pass gates, and its
// own caps. Three implementations of one decision is the shape this repository's design sweep exists to
// find in the product; here it is in the instrument.

/**
 * @param {object} plan  the JSON `summrise-sweep-plan` printed, embedded by the emitter
 * @param {() => string|number} stamp  the per-run cache-buster the payload already computes
 */
function planRuntime(plan, stamp) {
  if (!plan || !Array.isArray(plan.surfaces) || !plan.wants) {
    throw new Error("plan-runtime: the pieces module carries no plan — the payload would measure nothing");
  }
  const byKind = new Map();
  for (const s of plan.surfaces) {
    if (!byKind.has(s.kind)) byKind.set(s.kind, []);
    byKind.get(s.kind).push(s);
  }

  /** The URL a surface is served at, cache-buster included. */
  const url = (s) => {
    const q = (s.query || []).map(([k, v]) => k + "=" + v).join("&");
    return plan.origin + s.path + (q ? "?" + q + "&" : "?") + "cb=" + stamp();
  };

  const steps = async (list, page) => {
    for (const step of list || []) {
      if (step.emulateMedia) await page.emulateMedia(step.emulateMedia);
    }
  };

  return {
    plan,
    /** The surfaces of one block, IN THE ORDER THE PLAN NAMED THEM (which is visit order). */
    kind: (k) => byKind.get(k) || [],
    /** Is this pass wanted at all? A LOOKUP, not a re-derivation of the flag. */
    wants: (name) => plan.wants[name] === true,
    url,
    /**
     * OPEN A SURFACE: everything the plan says happens before the measurement starts. Three fields are
     * load-bearing here and all three are READ rather than assumed — `set_viewport` (the console and
     * the landing resize once per width, and the landing's motion pass does not resize at all),
     * `media`, and whether the surface navigates (the panel's motion pass reloads rather than
     * navigating, because the emulated preference needs a fresh style resolution).
     */
    open: async (s, page) => {
      await steps(s.pre, page);
      // THE THREE BOOLEANS ARE ALWAYS IN THE PLAN (the Rust serializes them unconditionally), so these
      // are READS rather than defaults: a payload that decided "absent means true" would still be
      // deciding something.
      if (s.set_viewport) await page.setViewportSize(s.viewport);
      if (s.media) await page.emulateMedia(s.media);
      if (s.navigate) await page.goto(url(s), { waitUntil: "load" });
      return s;
    },
    /** The reload a panel surface performs after the fixture flag is set. */
    reload: async (s, page) => {
      if (s.reload) await page.reload({ waitUntil: "load" });
    },
    /** Steps the plan puts after a surface. */
    after: async (s, page) => steps(s.post, page),
    /** Steps the plan puts after the LAST surface. */
    finish: async (page) => steps(plan.post, page),
  };
}

module.exports = { planRuntime };
