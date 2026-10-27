// The approvals badge: how many destructive calls are waiting for a person to answer.
//
// The pages are server-rendered and static, and this is the one poll that runs on all of them — a
// held call waits only ninety seconds, and an operator on another page would otherwise never see the
// question. It asks the console and nothing else: no state of its own, no mutation, and it pauses
// while the tab is hidden.
(() => {
  const badge = document.getElementById("approvals-badge");
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
    } else {
      badge.hidden = true;
      document.title = baseTitle;
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
