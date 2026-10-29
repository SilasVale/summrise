// The approvals badge: how many destructive calls are waiting for a person to answer.
//
// The pages are server-rendered and static, and this is the one poll that runs on all of them — a
// held call waits only ninety seconds, and an operator on another page would otherwise never see the
// question. It asks the console and nothing else: no state of its own, no mutation, and it pauses
// while the tab is hidden.
(() => {
  const badge = document.getElementById("approvals-badge");
  const announce = document.getElementById("approvals-announce");

  // **THE QUESTION HAS A CLOCK.** A held call is refused after ninety seconds, and the row said when
  // it started — leaving the operator to subtract from a paragraph. The server writes the deadline on
  // each row (`data-expires-at`, Unix ms); this counts it down, and at zero stops inviting a click
  // that the console would only refuse. Without JavaScript the same span is the absolute deadline,
  // which is still true.
  const clocks = Array.from(document.querySelectorAll("[data-expires-at]"));
  const tick = () => {
    const now = Date.now();
    for (const clock of clocks) {
      const expires = Number(clock.getAttribute("data-expires-at"));
      if (!Number.isFinite(expires)) continue;
      const left = Math.ceil((expires - now) / 1000);
      if (left > 0) {
        clock.textContent = left + "s left";
        continue;
      }
      clock.textContent = "expired";
      const row = clock.closest("tr");
      if (row && !row.classList.contains("expired")) {
        row.classList.add("expired");
        row.querySelectorAll("button").forEach((button) => {
          button.disabled = true;
        });
      }
    }
  };
  if (clocks.length > 0) {
    tick();
    setInterval(() => {
      if (!document.hidden) tick();
    }, 1000);
  }

  // **THE MENUS CLOSE THE WAY MENUS DO.** None of the page's disclosures had an Escape or an
  // outside click, so a keyboard user could open a row menu and leave it open over the next row's
  // controls. One delegated handler, no per-menu state.
  document.addEventListener("keydown", (event) => {
    if (event.key !== "Escape") return;
    const open = document.querySelectorAll("details[open]");
    if (open.length === 0) return;
    open.forEach((details) => details.removeAttribute("open"));
    if (open[open.length - 1].querySelector("summary")) open[open.length - 1].querySelector("summary").focus();
  });
  document.addEventListener("pointerdown", (event) => {
    document.querySelectorAll("details.row-menu[open]").forEach((details) => {
      if (!details.contains(event.target)) details.removeAttribute("open");
    });
  });
  if (!badge) return;
  const baseTitle = document.title;
  let waiting = -1;

  const paint = (pending) => {
    if (pending === waiting) return;
    waiting = pending;
    if (pending > 0) {
      badge.textContent = pending + (pending === 1 ? " call waiting" : " calls waiting");
      badge.hidden = false;
      document.title = "(" + pending + ") " + baseTitle;
      if (announce) {
        announce.textContent =
          pending + (pending === 1 ? " call is waiting" : " calls are waiting") + " for your approval";
      }
    } else {
      badge.hidden = true;
      document.title = baseTitle;
      if (announce) announce.textContent = "";
    }
  };

  let timer = null;
  const stop = () => {
    if (timer !== null) {
      clearInterval(timer);
      timer = null;
    }
    paint(0);
  };

  const poll = async () => {
    if (document.hidden) return;
    try {
      const response = await fetch("/next/approvals/count", {
        headers: { accept: "application/json" },
        credentials: "same-origin"
      });
      // A signed-out tab is not waiting for anything: stop asking and take the badge down, instead
      // of advertising a count nobody can act on.
      if (response.status === 401 || response.status === 403) {
        stop();
        return;
      }
      if (!response.ok) return;
      const body = await response.json();
      paint(Number(body.pending) || 0);
    } catch (error) {
      // The console may be restarting; the next tick asks again.
    }
  };

  poll();
  timer = setInterval(poll, 5000);
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) poll();
  });
})();
