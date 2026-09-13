// studio-links.js — DSH chat path rewriter for code-server (vscode.saisi.online).
//
// Turns real workspace file paths appearing in DSH chat (tool-call headers,
// prose, code blocks) into one-click links that open the file's folder in
// code-server. Since the studio retirement (ADR 0006) there is NO studio API:
// resolution is purely LOCAL — absolute paths link as-is, relative paths are
// resolved against the configurable workspace base (default /home/zhengsaisi,
// the code-server workspace). code-server URLs open FOLDERS (not file+line —
// VS Code web has no line-level URL), so the line number rides in the tooltip.
//
// Failure posture: nothing here touches the network any more — a link that
// points at a non-existent path simply opens the folder. The processed flag
// is set unconditionally after a scan (no API outage can stall it).
// The options toggle turns the whole thing off if anything looks off.

(() => {
  // Distinct from null: reserved semantics no longer needed (no API), kept
  // only for the resolvedCache shape.
  // DEFAULT OFF (round 134, ADR 0010). This content script rewrites text nodes the
  // DSH React client owns, and the client reconciles those nodes IN PLACE: its next
  // streaming chunk writes to a node this script has already replaced, so the reply
  // freezes at the injected link and a later structural diff can throw inside React's
  // commit phase. A feature whose measured effect is corrupting the host UI does not
  // belong on by default — it stays available, explicitly, and the README no longer
  // claims the rewrite is safe.
  let cfg = { origin: DEFAULT_STUDIO_ORIGIN, enabled: false };
  /** path candidate -> { dir: string, at: number } */
  const resolvedCache = new Map();
  const RX_TTL_MS = 5 * 60 * 1000;

  async function loadCfg() {
    try {
      const st = await chrome.storage.local.get(["studioOrigin", "studioLinksEnabled"]);
      cfg.origin = httpsOrigin(st.studioOrigin || DEFAULT_STUDIO_ORIGIN) || DEFAULT_STUDIO_ORIGIN;
      cfg.enabled = st.studioLinksEnabled === true; // opt IN, never out
    } catch {
      /* extension context gone */
    }
    return cfg.enabled;
  }

  /** Resolve a raw path mention to a folder URL under the workspace base.
   *  Cache wrapper around the shared pure resolveDir (SOLID Round-96) —
   *  cache lifetime belongs to the page, the mapping is pinned in
   *  shared.js. Absolute paths map directly; relative paths are joined
   *  onto the base. Best-effort by design: code-server opens the folder
   *  even when a file in the mention does not exist. */
  function resolve(raw) {
    const hit = resolvedCache.get(raw);
    if (hit && Date.now() - hit.at < RX_TTL_MS) return hit.dir;
    const dir = resolveDir(raw);
    resolvedCache.set(raw, { dir, at: Date.now() });
    if (resolvedCache.size > 500) {
      const cutoff = Date.now() - RX_TTL_MS;
      for (const [k, v] of resolvedCache) if (v.at < cutoff) resolvedCache.delete(k);
    }
    return dir;
  }

  function deepUrl(dir, line) {
    // Shared (SOLID Round-96): the line rides in the tooltip, not the URL.
    return studioFolderUrl(cfg.origin, dir);
  }

  function makeLink(text, dir, line) {
    const a = document.createElement("a");
    a.href = deepUrl(dir, line);
    a.target = "_blank";
    a.rel = "noopener";
    a.className = "vs-studio-link";
    a.textContent = text;
    a.title = "在 code-server 中打开该目录" + (line ? ` · 提到第 ${line} 行` : "");
    return a;
  }

  // WHAT "ALREADY LOOKED AT" MEANS (round 144, X3). This used to be an attribute on
  // the text node's PARENT, set when a scan found no path — which encoded "this whole
  // ELEMENT is finished". A streaming reply keeps appending NEW text nodes into that
  // same element, and every one of them was skipped for the rest of the page's life,
  // so content that arrived after the first scan was never linkified. An attribute
  // cannot say "this text node has been examined" (text nodes carry no attributes), so
  // the set lives here; the attribute is still set on the spans WE create, where
  // "inside a linkified span" is exactly what it should mean.
  const examined = new WeakSet();

  function shouldSkip(node) {
    const el = node.parentElement;
    if (!el) return true;
    if (examined.has(node)) return true;
    if (el.closest("a, script, style, noscript, textarea")) return true;
    if (el.closest('[data-vs-processed="1"]')) return true;
    if (el.closest('[contenteditable="true"]')) return true; // never touch the composer
    return false;
  }

  function processRoot(rootEl) {
    const walker = document.createTreeWalker(rootEl, NodeFilter.SHOW_TEXT, {
      acceptNode: (n) =>
        n.nodeValue && n.nodeValue.trim().length > 3 && !shouldSkip(n)
          ? NodeFilter.FILTER_ACCEPT
          : NodeFilter.FILTER_REJECT,
    });
    const targets = [];
    let tn;
    while ((tn = walker.nextNode()) && targets.length < 400) targets.push(tn);

    for (const node of targets) {
      const text = node.nodeValue;
      // Shared matcher (SOLID Round-96): match → {raw, bare, lineNo, index}.
      const jobs = extractPathJobs(text);
      if (!jobs.length) {
        examined.add(node);
        continue;
      }

      const span = document.createElement("span");
      span.setAttribute("data-vs-processed", "1");
      let cursor = 0;
      for (const job of jobs) {
        if (job.index > cursor) span.append(document.createTextNode(text.slice(cursor, job.index)));
        const dir = resolve(job.bare);
        // A mention we cannot vouch for — outside the folder code-server can open, or
        // inside a credential home (`.ssh`, `.dsh`) — stays TEXT. Better fewer links
        // than wrong ones, and never a one-click link into the key directory because a
        // chat message asked for it.
        span.append(dir ? makeLink(job.raw, dir, job.lineNo) : document.createTextNode(job.raw));
        cursor = job.index + job.raw.length;
      }
      if (cursor < text.length) span.append(document.createTextNode(text.slice(cursor)));

      try {
        node.replaceWith(span);
      } catch {
        /* node vanished mid-stream */
      }
    }
  }

  let timer = null;
  const queue = new Set();
  function scheduleScan(el) {
    if (el) queue.add(el);
    if (timer) return;
    timer = setTimeout(() => {
      timer = null;
      const batch = [...queue];
      queue.clear();
      for (const el of batch) {
        if (el && el.isConnected) processRoot(el);
      }
    }, 400);
  }

  async function start() {
    if (!(await loadCfg())) return;
    scheduleScan(document.body);
    const mo = new MutationObserver((muts) => {
      for (const mu of muts) {
        if (mu.type === "characterData") {
          // The host rewrote an existing text node in place (React does this while a
          // reply streams). It is a different string now, so it is no longer
          // "examined" — without this arm the observer only ever saw childList and
          // such a node stayed unlinkified forever.
          examined.delete(mu.target);
          scheduleScan(mu.target.parentElement);
          continue;
        }
        if (mu.type !== "childList") continue;
        for (const n of mu.addedNodes) {
          if (n.nodeType === 1 && !n.closest?.('[data-vs-processed="1"]')) scheduleScan(n);
          else if (n.nodeType === 3) scheduleScan(n.parentElement);
        }
      }
    });
    mo.observe(document.body, { childList: true, characterData: true, subtree: true });

    // config changes apply without reload
    chrome.storage.onChanged.addListener((changes, area) => {
      if (area === "local" && (changes.studioOrigin || changes.studioLinksEnabled)) {
        resolvedCache.clear();
        loadCfg().then((on) => on && scheduleScan(document.body));
      }
    });
  }

  // inject a tiny stylesheet once
  const css = document.createElement("style");
  css.textContent =
    ".vs-studio-link{color:#0d9488;text-decoration:underline dotted;text-underline-offset:3px}" +
    ".vs-studio-link:hover{text-decoration-style:solid}";
  document.documentElement.append(css);

  start();
})();
