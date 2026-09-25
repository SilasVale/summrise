#!/usr/bin/env node
// Panel SPA probe — what does the DESKTOP SPA actually hold and show?
//
// WHY THIS FILE EXISTS (round 89): the e2e suite's `panel` section fails stably with
//
//     PASS panel ai write  -- state=partial
//     FAIL panel xterm shows ai output  -- marker=PANEL-VIS-…
//
// and its own code proves the SPA target WAS found (it never printed "no desktop SPA target"),
// the rail button and the last session tab WERE clicked, and the visible `.term-host .xterm-rows`
// was read six times over twelve seconds. So the marker reaches the API and never reaches the
// SPA's terminal view. That leaves three answers, and this probe separates them:
//
//   (a) the SPA does not HAVE the session the AI opened  -> "tabs" count / tabtext lacks it
//   (b) it has it and does not SWITCH to it              -> the active tab is not the newest
//   (c) it switched and receives no STREAM               -> the visible xterm is empty
//
// IT IS A FILE, NOT A `node -e` ONE-LINER, and that is a measured decision: the first version of
// this probe was a PowerShell one-liner, PowerShell stripped the inner double quotes, and node
// received `fetch(http://127.0.0.1:9333/json/list)` and died with a syntax error. The repo already
// had the rule ("a 38 KB script must not be pasted into anything"); this is the same rule at 1 KB.
//
// TRANSPORT: same as e2e.js — published at
//   https://agent.saisi.online/summrise-agent/panel-spa-probe.js
// and fetched BY the device over an outbound GET. No inbound listener on either machine.
//
// USAGE (on the device):  node D:\Summrise\panel-spa-probe.js
// It reads the panel from the SPA's own CDP endpoint and prints one JSON verdict.
'use strict';

const CDP = process.env.SPA_CDP || 'http://127.0.0.1:9333/json/list';

const evaluate = async (ws, id, expression) => new Promise((resolve) => {
  const timer = setTimeout(() => resolve(null), 15000);
  const onMessage = (m) => {
    let o;
    try { o = JSON.parse(m.data); } catch (e) { return; }
    if (o.id === id) {
      clearTimeout(timer);
      ws.removeEventListener('message', onMessage);
      const r = o.result && o.result.result;
      resolve(r ? r.value : null);
    }
  };
  ws.addEventListener('message', onMessage);
  ws.send(JSON.stringify({ id, method: 'Runtime.evaluate', params: { expression, returnByValue: true } }));
});

(async () => {
  const verdict = { cdp: CDP, spaTarget: null, tabs: null, tabText: null, activeTab: null, visibleXterm: null, error: null };
  try {
    const list = await (await fetch(CDP)).json();
    const spa = (list || []).find((t) => (t.url || '').includes('/desktop/'));
    if (!spa) { verdict.error = 'no /desktop/ SPA target in the CDP list'; console.log(JSON.stringify(verdict, null, 2)); process.exit(0); }
    verdict.spaTarget = spa.url;

    const ws = new WebSocket(spa.webSocketDebuggerUrl);
    await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });

    const TABS = 'document.querySelectorAll("[role=tab]").length';
    const TABTEXT = 'Array.from(document.querySelectorAll("[role=tab]")).map(function(t){return t.textContent.trim()}).join("|")';
    const ACTIVE = '(function(){var t=document.querySelector("[role=tab][aria-selected=true]");return t?t.textContent.trim():"NO_ACTIVE_TAB"})()';
    // the VISIBLE term-host, exactly as the e2e section reads it (round-264 method)
    const XTERM = '(function(){var h=document.querySelectorAll(".term-host");for(var i=0;i<h.length;i++){var r=h[i].getBoundingClientRect();if(r.width>50&&r.height>50){var rows=h[i].querySelectorAll(".xterm-rows > div");var all="";for(var j=0;j<rows.length;j++){all+=rows[j].textContent+String.fromCharCode(10)}return all.slice(0,600)}}return "NO_VISIBLE_TERM_HOST"})()';

    verdict.tabs = await evaluate(ws, 1, TABS);
    verdict.tabText = await evaluate(ws, 2, TABTEXT);
    verdict.activeTab = await evaluate(ws, 3, ACTIVE);
    verdict.visibleXterm = await evaluate(ws, 4, XTERM);

    try { ws.close(); } catch (e) { /* the socket is going away anyway */ }
  } catch (e) {
    verdict.error = String((e && e.message) || e);
  }
  // (a) no tabs at all  -> the SPA has no sessions
  // (b) tabs but the active one is not the newest -> it has it and does not switch
  // (c) active tab but an empty/absent xterm -> it switched and receives no stream
  verdict.diagnosis = verdict.error ? 'INCONCLUSIVE'
    : (verdict.tabs === 0 || verdict.tabs === null) ? 'A: the SPA holds NO session tabs'
    : (verdict.visibleXterm === 'NO_VISIBLE_TERM_HOST') ? 'C: no visible term-host (nothing rendered to read)'
    : (String(verdict.visibleXterm || '').trim() === '') ? 'C: the visible xterm is EMPTY (switched, no stream)'
    : 'B-or-visible: the xterm HAS text -- compare it with the e2e marker';
  console.log(JSON.stringify(verdict, null, 2));
})();
