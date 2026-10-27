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
  const tick = async () => {
    if (stopped || document.hidden) return;
    try {
      const response = await fetch(
        `/next/browser/${encodeURIComponent(device)}/frame?target=${encodeURIComponent(target)}`,
        { headers: { accept: "application/json" }, credentials: "same-origin" }
      );
      // A signed-out tab stops asking rather than turning every second into a login page request.
      if (response.status === 401 || response.status === 403) {
        stopped = true;
        return;
      }
      if (!response.ok) return;
      const body = await response.json();
      if (body && body.data) {
        img.src = `data:image/${body.format || "png"};base64,${body.data}`;
      }
    } catch (error) {
      // The console or the device is away; the next tick asks again.
    }
  };

  setInterval(tick, 1000);
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) tick();
  });
})();
