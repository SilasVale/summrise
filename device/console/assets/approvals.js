// The approvals badge: how many destructive calls are waiting for a person to answer.
//
// The pages are server-rendered and static, and this is the one poll that runs on all of them — a
// held call waits only ten minutes, and an operator on another page would otherwise never see the
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

  const poll = async () => {
    if (document.hidden) return;
    try {
      const response = await fetch("/next/approvals/count", {
        headers: { accept: "application/json" },
        credentials: "same-origin"
      });
      if (!response.ok) return;
      const body = await response.json();
      paint(Number(body.pending) || 0);
    } catch (error) {
      // The console may be restarting; the next tick asks again.
    }
  };

  poll();
  setInterval(poll, 5000);
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) poll();
  });
})();
