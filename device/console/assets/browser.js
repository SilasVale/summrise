// The browser pane's frame pump: one screenshot a second through the console, so an operator watches
// the device's browser instead of looking at one stale capture. The first frame is server-rendered,
// so a page without this script still shows a picture; the script only replaces it as new ones come.
(() => {
  const img = document.getElementById("cast");
  if (!img) return;
  const device = img.dataset.device;
  const target = img.dataset.target;
  if (!device || !target) return;

  let stopped = false;
  let timer = null;
  // A device with a live stream is polled five times a second; one without (a build older than
  // `browser.cast`) once a second, and the console is told to stop trying the stream at all.
  let streaming = true;
  const interval = () => (streaming ? 200 : 1000);

  // One capture at a time: each frame is a device call that holds a channel slot and a queue place
  // until it answers, so a device slower than the interval must not accumulate them (and an older
  // frame must not land after a newer one). The next tick is scheduled from the END of this one.
  const tick = async () => {
    timer = null;
    if (stopped || document.hidden) {
      return;
    }
    try {
      const response = await fetch(
        `/next/browser/${encodeURIComponent(device)}/frame?target=${encodeURIComponent(target)}` +
          `&stream=${streaming ? 1 : 0}`,
        { headers: { accept: "application/json" }, credentials: "same-origin" }
      );
      // A signed-out tab stops asking rather than turning every second into a login page request.
      if (response.status === 401 || response.status === 403) {
        stopped = true;
        return;
      }
      if (response.ok) {
        const body = await response.json();
        streaming = body.stream === true;
        if (body && body.data) {
          img.src = `data:image/${body.format || "png"};base64,${body.data}`;
        }
      }
    } catch (error) {
      // The console or the device is away; the next tick asks again.
    } finally {
      if (!stopped && timer === null) {
        timer = setTimeout(tick, interval());
      }
    }
  };

  timer = setTimeout(tick, interval());
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden && timer === null && !stopped) {
      timer = setTimeout(tick, 0);
    }
  });
})();
