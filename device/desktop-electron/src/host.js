// The thin host page. Two tabs, two placeholders, and the narrow preload bridge — nothing else.
//
// THIS FILE IS GLUE, NOT A PRODUCT SURFACE: it never fetches, never reads config.yaml, never builds a
// credential and never shapes device facts. Its only inputs are `window.summriseBrowser` /
// `window.summriseHarness` (the host-only bridges in `preload.ts`) and its own DOM. The native
// `WebContentsView`s are positioned over the `.slot` boxes from `getBoundingClientRect()`, and the
// main process owns every decision (the load doors, the slot's visibility, the harness origins).
//
// WHY THE BRIDGE IS CHECKED BEFORE ANYTHING ELSE: a host page without the preload has no way to place
// a view, and a half-wired page that silently does nothing looks like a broken shell. It says so once
// (`#bridge-missing`) and disables every control.
"use strict";

(function () {
  var browser = window.summriseBrowser;
  var harness = window.summriseHarness;

  function $(id) { return document.getElementById(id); }

  function hasBridge(api, members) {
    if (!api || typeof api !== "object") return false;
    for (var i = 0; i < members.length; i++) {
      if (typeof api[members[i]] !== "function") return false;
    }
    return true;
  }

  var missing = [];
  if (!hasBridge(browser, ["navigate", "back", "fwd", "reload", "zoom", "place", "state", "recover", "onNav", "onGone"])) {
    missing.push("summriseBrowser");
  }
  if (!hasBridge(harness, ["targets", "open", "place", "go", "state", "reload", "recover", "onGone"])) {
    missing.push("summriseHarness");
  }

  if (missing.length > 0) {
    var notice = $("bridge-missing");
    notice.hidden = false;
    notice.textContent = "The host bridge is missing (" + missing.join(" and ") + "). This window cannot drive the panes.";
    var controls = document.querySelectorAll("button, input, select");
    for (var i = 0; i < controls.length; i++) controls[i].disabled = true;
    return;
  }

  // ── State ───────────────────────────────────────────────────────────────────────────────────────
  // `?tab=harness` selects Harness; anything else selects Browser.
  var tab = new URLSearchParams(location.search).get("tab") === "harness" ? "harness" : "browser";
  // The user hid the pane with its own toggle: a resize must not undo that, but a tab switch does.
  var browserHidden = false;
  var harnessHidden = false;
  var harnessStarted = false;
  var targetsLoaded = false;
  // A URL the operator just asked for: `state()` keeps reporting the last COMMITTED url until the
  // new one commits, so the address bar holds this instead of flicking back for a moment.
  var pendingUrl = "";
  var pendingTimer = null;
  // One poll read at a time: a slow reply must not stack a new `state()` call every second.
  var refreshBusy = false;
  // The record carries the PAGE's load (`load`) beside the view's visibility (`visible`). They are
  // different facts: a visible view over a failed page is `failed`, never a success state.
  var browserRecord = { url: "", title: "", visible: false, canBack: false, canFwd: false, load: null };
  var harnessRecord = { url: "", title: "", visible: false, load: null };

  function report(error) {
    var text = error && error.message ? error.message : String(error || "unknown error");
    if (window.console && console.error) console.error("[summrise-host]", text);
    var harnessTab = tab === "harness";
    var state = harnessTab ? $("harness-status-state") : $("browser-status-state");
    var box = harnessTab ? $("harness-status") : $("browser-status");
    var detail = harnessTab ? $("harness-status-detail") : $("browser-status-detail");
    var retry = harnessTab ? $("harness-retry") : $("browser-retry");
    // The bridge is the one thing this page cannot paint around: its reply is how the record is read, so
    // a refused call is reported as such, in the pane the operator is looking at. The next successful
    // refresh repaints from the record — this text is not sticky.
    if (state) state.textContent = "gone (host bridge: " + text + ")";
    if (detail) detail.textContent = "";
    if (retry) retry.disabled = true;
    if (box) box.dataset.state = "gone";
  }

  // THE STATE WORD IS THE PAGE'S, NOT THE VIEW'S. `null` means no load was ever observed, which is not a
  // failure: a hidden slot is `hidden`, and a visible view with no page committed yet is `no page`.
  function loadPaint(record) {
    var load = record.load;
    if (!load) {
      return record.visible ? { state: "nopage", text: "no page" } : { state: "hidden", text: "hidden" };
    }
    if (load.status === "failed") return { state: "failed", text: "failed (" + load.code + ")" };
    if (load.status === "gone") return { state: "gone", text: "gone (" + (load.desc || "renderer stopped") + ")" };
    if (load.status === "ready") return { state: "ready", text: "ready" };
    return { state: "loading", text: "loading" };
  }

  // ── The native views' bounds ───────────────────────────────────────────────────────────────────
  // CSS pixels in the window's content coordinate space: the page never scrolls, so the viewport
  // rectangle IS that space. A zero-sized box (a hidden pane, a collapsed window) maps to `null`,
  // which tells the main process to hide the view rather than place it at 0×0.
  function boundsOf(id) {
    var slot = $(id);
    if (!slot) return null;
    var rect = slot.getBoundingClientRect();
    if (!(rect.width > 1 && rect.height > 1)) return null;
    return {
      x: Math.round(rect.left),
      y: Math.round(rect.top),
      width: Math.round(rect.width),
      height: Math.round(rect.height)
    };
  }

  // ── Browser pane ───────────────────────────────────────────────────────────────────────────────
  function paintBrowser() {
    var load = browserRecord.load;
    var paint = loadPaint(browserRecord);
    if ($("browser-status-state").textContent !== paint.text) $("browser-status-state").textContent = paint.text;
    $("browser-status").dataset.state = paint.state;
    $("browser-status-visibility").textContent = browserRecord.visible ? "View visible" : "View hidden";
    $("browser-status-detail").textContent = (load && load.desc) || "";
    $("browser-status-title").textContent = browserRecord.title || "";
    // THE TARGET IS NAMED EVEN WHEN THE VIEW COMMITTED NOTHING: a failed provisional load leaves
    // `record.url` empty (and the old page would say "(none)"), but `load.url` is the target that failed.
    $("browser-status-url").textContent = (load && load.url) || browserRecord.url || "(none)";
    $("browser-retry").disabled = !(load && load.status === "failed");
    $("browser-back").disabled = !browserRecord.canBack;
    $("browser-fwd").disabled = !browserRecord.canFwd;
    $("browser-toggle").textContent = browserRecord.visible ? "Hide" : "Show";
    var input = $("browser-url");
    if (document.activeElement !== input && browserRecord.url && input.value !== browserRecord.url) {
      input.value = browserRecord.url;
    }
  }

  function refreshBrowser() {
    return browser.state().then(function (s) {
      if (s) {
        if (pendingUrl && s.url === pendingUrl) pendingUrl = "";
        if (!pendingUrl) browserRecord.url = s.url || browserRecord.url;
        browserRecord.title = s.title || "";
        browserRecord.visible = !!s.visible;
        browserRecord.canBack = !!s.canBack;
        browserRecord.canFwd = !!s.canFwd;
        browserRecord.load = s.load || null;
      }
      paintBrowser();
    }).catch(report);
  }

  function rememberPending(url) {
    pendingUrl = url || "";
    if (pendingTimer) clearTimeout(pendingTimer);
    pendingTimer = setTimeout(function () {
      pendingUrl = "";
      void refreshBrowser();
    }, 6000);
  }

  function placeBrowser() {
    var bounds = browserHidden ? null : boundsOf("browser-slot");
    return browser.place(bounds).then(refreshBrowser).catch(report);
  }

  // ── Harness pane ───────────────────────────────────────────────────────────────────────────────
  function paintHarness() {
    var load = harnessRecord.load;
    var paint = loadPaint(harnessRecord);
    if ($("harness-status-state").textContent !== paint.text) $("harness-status-state").textContent = paint.text;
    $("harness-status").dataset.state = paint.state;
    $("harness-status-visibility").textContent = harnessRecord.visible ? "View visible" : "View hidden";
    // The harness is the pane whose failure the operator most needs explained: with no DSH on loopback
    // the page is blank but the view is visible, so the reason is spelled out rather than left as a code.
    // A failed LOAD means the DSH is not answering; a dead RENDERER is this view's own crash, so it is
    // named as such rather than blamed on the harness.
    var detail = "";
    if (load && load.desc) {
      if (load.status === "failed") detail = "DSH not answering (" + load.desc + ")";
      else if (load.status === "gone") detail = "Harness page gone (" + load.desc + ")";
    }
    $("harness-status-detail").textContent = detail;
    $("harness-status-title").textContent = harnessRecord.title || "";
    // The DSH's own record reports "" once its view is dead; `load.url` still names the target that failed.
    $("harness-status-url").textContent = (load && load.url) || harnessRecord.url || "(none)";
    $("harness-retry").disabled = !(load && load.status === "failed");
    $("harness-toggle").textContent = harnessRecord.visible ? "Hide" : "Show";
  }

  function refreshHarness() {
    return harness.state().then(function (s) {
      if (s) {
        harnessRecord.url = s.url || harnessRecord.url;
        harnessRecord.title = s.title || "";
        harnessRecord.visible = !!s.visible;
        harnessRecord.load = s.load || null;
      }
      paintHarness();
    }).catch(report);
  }

  function placeHarness() {
    var bounds = harnessHidden ? null : boundsOf("harness-slot");
    return harness.place(bounds).then(refreshHarness).catch(report);
  }

  // ── The one-second read ───────────────────────────────────────────────────────────────────────
  // The load record rides the EXISTING state() reply — no new bridge member and no new channel — so the
  // page asks for it instead of being pushed. One read per second, only while this page is visible and
  // only for the pane on screen (the browser is eager; the harness only after its pane started it).
  // `refreshBusy` is the poll's own guard: a reply slower than a tick must not stack callers.
  function refreshActive() {
    if (document.hidden || refreshBusy) return;
    var pending = null;
    if (tab === "harness") {
      if (harnessStarted) pending = refreshHarness();
    } else {
      pending = refreshBrowser();
    }
    if (!pending) return;
    refreshBusy = true;
    pending.then(function () { refreshBusy = false; }, function () { refreshBusy = false; });
  }

  setInterval(refreshActive, 1000);
  document.addEventListener("visibilitychange", function () {
    if (!document.hidden) refreshActive();
  });

  // `targets()` is the policy's door list — one loopback origin per reachable harness. It is fetched
  // once; the selector is a view of that list, never a device table this page invents.
  function loadTargets() {
    if (targetsLoaded) return Promise.resolve();
    return harness.targets().then(function (answer) {
      var rows = (answer && answer.targets) || [];
      var select = $("harness-target");
      while (select.firstChild) select.removeChild(select.firstChild);
      for (var i = 0; i < rows.length; i++) {
        var option = document.createElement("option");
        option.value = String(rows[i]);
        option.textContent = String(rows[i]);
        select.appendChild(option);
      }
      if (rows.length === 0) {
        var none = document.createElement("option");
        none.value = "";
        none.textContent = "no harness door";
        select.appendChild(none);
      }
      $("harness-go").disabled = rows.length === 0;
      targetsLoaded = true;
    }).catch(report);
  }

  // First activation: a view with no url has never been navigated, so open it (the main process
  // navigates the harness home through its own door) and then place it. A view that already has a
  // url is placed as it is — re-opening it would throw away whatever the operator was reading.
  // `about:blank` is NOT "already started": a view created without a navigation (a crash-path hide,
  // or a slot built by a page this one replaced) reports it, and opening is what gives the pane a
  // page. Without this, the first Harness open after such a view would show a blank pane.
  function startHarness() {
    harnessStarted = true;
    return harness.state().then(function (s) {
      if (!s || !s.url || s.url === "about:blank") return harness.open();
      return null;
    }).then(function () {
      return placeHarness();
    }).then(loadTargets).catch(report);
  }

  // ── Tabs ───────────────────────────────────────────────────────────────────────────────────────
  function paintTabs() {
    $("tab-browser").setAttribute("aria-selected", tab === "browser" ? "true" : "false");
    $("tab-harness").setAttribute("aria-selected", tab === "harness" ? "true" : "false");
    $("tab-browser").tabIndex = tab === "browser" ? 0 : -1;
    $("tab-harness").tabIndex = tab === "harness" ? 0 : -1;
    $("browser-pane").hidden = tab !== "browser";
    $("harness-pane").hidden = tab !== "harness";
  }

  function activate(next) {
    if (next === tab) {
      if (next === "harness") {
        // A TAB CLICK RE-SHOWS: an explicit Hide is not a permanent state, exactly as the Browser
        // branch below clears its own flag. Without this, Hide → Browser → Harness left the pane
        // active with `place(null)` hiding the view the operator just asked for.
        harnessHidden = false;
        void startHarness();
      } else {
        browserHidden = false;
        void placeBrowser();
      }
      return;
    }
    if (next === "harness") {
      // Hide the outgoing view BEFORE the new pane paints: one native view is always over the page,
      // never two, and never a stale one over a pane that no longer owns it.
      browserHidden = true;
      void browser.place(null).catch(report);
      // The harness's own Hide is cleared by the switch for the same reason: the pane is being
      // opened deliberately, so it is placed rather than left hidden.
      harnessHidden = false;
      tab = "harness";
      paintTabs();
      void startHarness();
    } else {
      if (harnessStarted) void harness.place(null).catch(report);
      tab = "browser";
      paintTabs();
      browserHidden = false;
      void placeBrowser();
    }
  }

  // ── Resize: the native view is glued to its placeholder ───────────────────────────────────────
  var resizePending = false;
  function rePlace() {
    if (resizePending) return;
    resizePending = true;
    requestAnimationFrame(function () {
      resizePending = false;
      if (tab === "harness") {
        if (harnessStarted) void placeHarness();
      } else {
        void placeBrowser();
      }
    });
  }

  var observer = new ResizeObserver(rePlace);
  observer.observe($("browser-slot"));
  observer.observe($("harness-slot"));
  window.addEventListener("resize", rePlace);

  // Leaving the page (a shell-driven reload, a crash recovery, the window closing) must not leave a
  // native view hanging over whatever paints next. The harness is only touched once it has started:
  // `place(null)` would otherwise create the lazily-built view just to hide it.
  window.addEventListener("pagehide", function () {
    try { browser.place(null).catch(function () { /* page is going away */ }); } catch (e) { /* page is going away */ }
    if (harnessStarted) {
      try { harness.place(null).catch(function () { /* page is going away */ }); } catch (e) { /* page is going away */ }
    }
  });

  // ── Controls: Browser ─────────────────────────────────────────────────────────────────────────
  $("tab-browser").addEventListener("click", function () { activate("browser"); });
  $("tab-harness").addEventListener("click", function () { activate("harness"); });
  function tabKeydown(event) {
    var next;
    if (event.key === "Home") next = "browser";
    else if (event.key === "End") next = "harness";
    else if (event.key === "ArrowRight" || event.key === "ArrowLeft" || event.key === "ArrowDown" || event.key === "ArrowUp") next = tab === "browser" ? "harness" : "browser";
    else return;
    event.preventDefault();
    activate(next);
    $("tab-" + next).focus();
  }
  $("tab-browser").addEventListener("keydown", tabKeydown);
  $("tab-harness").addEventListener("keydown", tabKeydown);

  function navigateBrowserTo(url) {
    if (!url) return;
    browserHidden = false;
    rememberPending(url);
    browserRecord.url = url;
    paintBrowser();
    return browser.navigate(url).then(function (answer) {
      var decided = (answer && answer.url) || url;
      rememberPending(decided);
      browserRecord.url = decided;
      paintBrowser();
      return placeBrowser();
    }).then(refreshBrowser).catch(report);
  }

  function navigateBrowser() {
    var url = $("browser-url").value.trim();
    if (url) void navigateBrowserTo(url);
  }

  $("browser-go").addEventListener("click", navigateBrowser);
  // Retry re-enters the SAME door the failed load went through — never a raw load — so the url policy
  // decides again and the record/preset path is the one every other navigation uses.
  $("browser-retry").addEventListener("click", function () {
    var load = browserRecord.load;
    if (!load || load.status !== "failed") return;
    void navigateBrowserTo(load.url);
  });
  $("browser-url").addEventListener("keydown", function (event) {
    if (event.key === "Enter") {
      event.preventDefault();
      navigateBrowser();
    }
  });
  $("browser-back").addEventListener("click", function () {
    browser.back().then(refreshBrowser).catch(report);
  });
  $("browser-fwd").addEventListener("click", function () {
    browser.fwd().then(refreshBrowser).catch(report);
  });
  $("browser-reload").addEventListener("click", function () {
    browser.reload().then(refreshBrowser).catch(report);
  });
  $("browser-zoom").addEventListener("change", function () {
    var factor = Number($("browser-zoom").value) || 1;
    browser.zoom(factor).then(refreshBrowser).catch(report);
  });
  $("browser-toggle").addEventListener("click", function () {
    browserHidden = !!browserRecord.visible;
    void placeBrowser();
  });
  $("browser-recover").addEventListener("click", function () {
    browser.recover().then(function () {
      browserHidden = false;
      return placeBrowser();
    }).then(refreshBrowser).catch(report);
  });

  // ── Controls: Harness ─────────────────────────────────────────────────────────────────────────
  function openHarness() {
    harnessStarted = true;
    harnessHidden = false;
    return harness.open().then(function () {
      return placeHarness();
    }).then(refreshHarness).catch(report);
  }

  $("harness-open").addEventListener("click", function () { void openHarness(); });
  // The harness door (`open()`) carries no url by design; Retry re-enters it exactly as Open does.
  $("harness-retry").addEventListener("click", function () {
    var load = harnessRecord.load;
    if (!load || load.status !== "failed") return;
    void openHarness();
  });
  $("harness-go").addEventListener("click", function () {
    var url = $("harness-target").value;
    if (!url) return;
    harnessHidden = false;
    harness.go(url).then(function () {
      return placeHarness();
    }).then(refreshHarness).catch(report);
  });
  $("harness-reload").addEventListener("click", function () {
    harness.reload().then(refreshHarness).catch(report);
  });
  $("harness-toggle").addEventListener("click", function () {
    harnessHidden = !!harnessRecord.visible;
    void placeHarness();
  });
  $("harness-recover").addEventListener("click", function () {
    harness.recover().then(function () {
      harnessHidden = false;
      return placeHarness();
    }).then(refreshHarness).catch(report);
  });

  // ── Event subscriptions: pushes from the main process ─────────────────────────────────────────
  browser.onNav(function (s) {
    if (!s) return;
    pendingUrl = "";
    if (pendingTimer) { clearTimeout(pendingTimer); pendingTimer = null; }
    browserRecord.url = s.url || browserRecord.url;
    browserRecord.title = s.title || "";
    browserRecord.canBack = !!s.canBack;
    browserRecord.canFwd = !!s.canFwd;
    paintBrowser();
  });
  // A crash push is a HINT to read the record, not a state of its own: `render-process-gone` has already
  // written `gone` into the slot, so the refresh below is the one truth and a fresh load clears it.
  browser.onGone(function () { void refreshBrowser(); });
  harness.onGone(function () { void refreshHarness(); });

  // ── Boot ──────────────────────────────────────────────────────────────────────────────────────
  paintTabs();
  paintBrowser();
  paintHarness();
  void refreshBrowser();
  void loadTargets();
  if (tab === "harness") {
    // Hide the browser view explicitly: the boot path bypasses `activate`, so a view placed by the
    // page this one replaced must not sit under the harness pane. `place(null)` is safe here — the
    // browser view is the eager one (the harness's is only ever touched by its own pane).
    void browser.place(null).catch(report);
    void startHarness();
  } else {
    void placeBrowser();
    // A harness view left visible by the page this one replaced is hidden here — but ONLY if it
    // exists. `state()` reads the slot's record without creating anything, and `place(null)` on the
    // lazy harness would build the view at about:blank just to hide it (the B2 review's state bug).
    harness.state().then(function (s) {
      if (s && s.visible) return harness.place(null);
      return null;
    }).catch(function () { /* no harness view to hide */ });
  }
})();
