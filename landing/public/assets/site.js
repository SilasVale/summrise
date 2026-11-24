/* Summrise landing: local language choice and the latest published release.
   Static English and release-page links remain useful without JavaScript or a successful lookup. */
(function () {
  "use strict";

  var ZH = window.SUMMRISE_ZH || {};
  var nodes = document.querySelectorAll("[data-i18n]");
  var en = {};
  for (var i = 0; i < nodes.length; i++) {
    var key = nodes[i].getAttribute("data-i18n");
    if (!(key in en)) en[key] = nodes[i].innerHTML;
  }
  var attributes = [];
  ["alt", "aria-label"].forEach(function (attribute) {
    var matches = document.querySelectorAll("[data-i18n-" + attribute + "]");
    for (var i = 0; i < matches.length; i++) {
      attributes.push({ node: matches[i], name: attribute, key: matches[i].getAttribute("data-i18n-" + attribute), en: matches[i].getAttribute(attribute) });
    }
  });
  var enTitle = document.title;
  var description = document.querySelector('meta[name="description"]');
  var enDescription = description ? description.content : "";
  var storeKey = "summrise.lang";
  var query = new URLSearchParams(location.search).get("lang");
  var lang = query === "zh" || query === "en" ? query : "en";
  if (query !== "zh" && query !== "en") {
    try {
      var saved = localStorage.getItem(storeKey);
      if (saved === "zh" || saved === "en") lang = saved;
    } catch (e) { /* Storage is optional; the query and default language still apply. */ }
  }

  var releasePage = "https://github.com/SilasVale/summrise/releases";
  var releaseState = "pending";
  var releaseTag = null;
  var releaseURL = releasePage;
  var assets = { installer: null, doc: null, sums: null };

  function renderRelease() {
    var zh = lang === "zh";
    var states = zh ? {
      pending: ZH["dl.statePending"], error: ZH["dl.stateError"], empty: ZH["dl.stateEmpty"],
      partial: ZH["dl.statePartial"], success: ZH["dl.stateSuccess"]
    } : {
      pending: "Checking published releases. The release-page links remain available while we check.",
      error: "Release information could not be loaded. Open the releases page to check available downloads.",
      empty: "No published release was returned. Open the releases page to check what is available.",
      partial: "A release was found, but some files are unavailable. Those links open the release page.",
      success: "Release files are available. Downloads below link to the files in this published release."
    };
    var status = document.getElementById("release-status");
    if (status) { status.dataset.state = releaseState; status.textContent = states[releaseState]; }
    var tag = document.getElementById("release-tag");
    if (tag) tag.textContent = releaseTag || (zh ? ZH["dl.noteFallback"] : "see the releases page");
    var download = zh ? ZH["dl.download"] : "Download";
    var fallback = zh ? ZH["dl.viewRelease"] : "View release";
    function action(id, asset) {
      var node = document.getElementById(id);
      if (node) { node.href = asset || releaseURL; node.textContent = asset ? download : fallback; }
    }
    action("art-installer", assets.installer);
    action("art-doc", assets.doc);
    action("art-sums", assets.sums);
    var installer = document.getElementById("dl-installer");
    var installerLabel = document.getElementById("dl-installer-label");
    if (installer) installer.href = assets.installer || releaseURL;
    if (installerLabel) installerLabel.textContent = assets.installer
      ? (zh ? ZH["dl.downloadWindows"] : "Download for Windows")
      : (zh ? ZH["dl.cta1"] : en["dl.cta1"]);
    var notes = document.getElementById("dl-notes");
    if (notes) notes.href = releaseURL;
  }

  function applyLang(next) {
    lang = next === "zh" ? "zh" : "en";
    var zh = lang === "zh";
    document.documentElement.lang = zh ? "zh-CN" : "en";
    document.title = zh ? ZH["meta.title"] || enTitle : enTitle;
    if (description) description.content = zh ? ZH["meta.description"] || enDescription : enDescription;
    for (var j = 0; j < nodes.length; j++) {
      var key = nodes[j].getAttribute("data-i18n");
      nodes[j].innerHTML = zh && ZH[key] ? ZH[key] : en[key];
    }
    attributes.forEach(function (attribute) {
      attribute.node.setAttribute(attribute.name, zh && ZH[attribute.key] ? ZH[attribute.key] : attribute.en);
    });
    var button = document.getElementById("lang-toggle");
    var label = document.getElementById("lang-label");
    if (button && label) {
      button.hidden = false;
      label.textContent = zh ? "EN" : "中文";
      label.lang = zh ? "en" : "zh-CN";
      button.setAttribute("aria-pressed", zh ? "true" : "false");
      button.setAttribute("aria-label", zh ? "Switch to English" : "切换到中文");
    }
    renderRelease();
    try { localStorage.setItem(storeKey, lang); } catch (e) { /* Language changes do not require storage. */ }
  }
  var toggle = document.getElementById("lang-toggle");
  if (toggle) toggle.addEventListener("click", function () { applyLang(lang === "zh" ? "en" : "zh"); });
  applyLang(lang);

  var screenshot = document.getElementById("console-shot");
  var screenshotFallback = document.getElementById("shot-fallback");
  if (screenshot && screenshotFallback) {
    function missingScreenshot() { screenshot.hidden = true; screenshotFallback.hidden = false; }
    screenshot.addEventListener("error", missingScreenshot);
    if (screenshot.complete && screenshot.naturalWidth === 0) missingScreenshot();
  }

  function findAsset(releaseAssets, pattern) {
    var found = null;
    for (var i = 0; i < releaseAssets.length; i++) {
      var asset = releaseAssets[i];
      if (asset && typeof asset.name === "string" && pattern.test(asset.name) && typeof asset.browser_download_url === "string" && asset.browser_download_url) {
        if (found) return null; // Ambiguous roles keep the release-page fallback, regardless of upload order.
        found = asset;
      }
    }
    return found;
  }
  if (typeof fetch !== "function") { releaseState = "error"; renderRelease(); return; }
  fetch("https://api.github.com/repos/SilasVale/summrise/releases?per_page=10", { headers: { Accept: "application/vnd.github+json" } })
    .then(function (response) {
      if (!response.ok) throw new Error("releases " + response.status);
      return response.json();
    })
    .then(function (releases) {
      if (!Array.isArray(releases)) throw new Error("invalid release list");
      var release = null;
      for (var i = 0; i < releases.length; i++) {
        if (releases[i] && !releases[i].draft) { release = releases[i]; break; }
      }
      if (!release) { releaseState = "empty"; renderRelease(); return; }
      if (typeof release.tag_name !== "string" || !release.tag_name || typeof release.html_url !== "string" || !release.html_url) throw new Error("invalid published release");
      var releaseAssets = Array.isArray(release.assets) ? release.assets : [];
      releaseTag = release.tag_name;
      releaseURL = release.html_url;
      // Current bundles include a channel; older publishes used version-only names.
      // Match the selected tag exactly so sidecars, inventories and other versions cannot become downloads.
      var version = /^v?((?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*))$/.exec(release.tag_name);
      var installer = null, doc = null;
      if (version) {
        var escapedVersion = version[1].replace(/\./g, "\\.");
        installer = findAsset(releaseAssets, new RegExp("^SummriseSetup-" + escapedVersion + "(?:-(?:stable|canary))?\\.exe$"));
        doc = findAsset(releaseAssets, new RegExp("^release-" + escapedVersion + "(?:-(?:stable|canary))?\\.json$"));
        var installerChannel = installer && /-(stable|canary)\.exe$/.exec(installer.name);
        var docChannel = doc && /-(stable|canary)\.json$/.exec(doc.name);
        if (installerChannel && docChannel && installerChannel[1] !== docChannel[1]) doc = null;
      }
      var sums = findAsset(releaseAssets, /^SHA256SUMS$/);
      assets.installer = installer ? installer.browser_download_url : null;
      assets.doc = doc ? doc.browser_download_url : null;
      assets.sums = sums ? sums.browser_download_url : null;
      releaseState = assets.installer && assets.doc && assets.sums ? "success" : "partial";
      renderRelease();
    })
    .catch(function () { releaseState = "error"; renderRelease(); });
})();
