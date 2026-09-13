// options.js — origin + toggle for the DSH path rewriter.
//
// THE SECURITY MODEL, corrected in round 143 because this file CARRIED A SECOND COPY
// of the claim round 141 fixed in shared.js — the pair-defect again, caught this time
// by re-reading the file rather than by a test. code-server runs with `--auth none`
// (pm2 args: ["--auth","none","/home/zhengsaisi"]), so there is NO code-server
// password; Cloudflare Access is the only gate in front of a shell. No per-extension
// token either (ADR 0006).

const $ = (id) => document.getElementById(id);

async function load() {
  const st = await chrome.storage.local.get(["studioOrigin", "studioLinksEnabled"]);
  $("studioOrigin").value = st.studioOrigin || DEFAULT_STUDIO_ORIGIN;
  // OPT-IN, matching the content script's predicate (round 134 flipped it from
  // opt-OUT to `=== true`; this line kept the old default, so a user who never
  // touched the setting saw the box CHECKED while the feature was OFF — the UI
  // asserting a state that was not in effect).
  $("studioLinksEnabled").checked = st.studioLinksEnabled === true;
}

async function save() {
  const raw = ($("studioOrigin").value.trim() || DEFAULT_STUDIO_ORIGIN).replace(/\/+$/, "");
  const origin = httpsOrigin(raw);
  const el = $("status");
  if (!origin) {
    // REFUSE, DO NOT SUBSTITUTE (round 143). This used to fall back to
    // DEFAULT_STUDIO_ORIGIN, so typing an unusable origin stored a value the user
    // never typed, under their name, and reported it as saved. The box keeps what
    // they wrote and the message says why it was not stored.
    el.textContent = "不是有效的 https:// 地址,未保存";
    setTimeout(() => (el.textContent = ""), 4000);
    return;
  }
  await chrome.storage.local.set({
    studioOrigin: origin,
    studioLinksEnabled: $("studioLinksEnabled").checked,
  });
  el.textContent = `已保存: ${origin}`;
  setTimeout(() => (el.textContent = ""), 2500);
}

document.addEventListener("DOMContentLoaded", () => {
  load();
  $("save").addEventListener("click", save);
  $("studioLinksEnabled").addEventListener("change", (e) =>
    chrome.storage.local.set({ studioLinksEnabled: e.target.checked }),
  );
});
