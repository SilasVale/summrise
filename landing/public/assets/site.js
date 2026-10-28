/* Summrise landing page — language switch, release lookup, and nothing else.
   English is the default because the product's documents are English; the switch is for the
   operator's own audience. A page that claims a version must read it from somewhere, so the
   download block asks GitHub for the newest published release and falls back to the releases
   page rather than printing a version somebody forgot to bump. */
(function () {
  "use strict";

  var ZH = window.SUMMRISE_ZH || {};

  var en = {}; // key -> original English innerHTML, captured before any switch
  var nodes = document.querySelectorAll("[data-i18n]");
  for (var i = 0; i < nodes.length; i++) {
    var key = nodes[i].getAttribute("data-i18n");
    if (!(key in en)) en[key] = nodes[i].innerHTML;
  }

  var storeKey = "summrise.lang";
  var lang = "en";
  try {
    var q = new URLSearchParams(location.search).get("lang");
    var saved = localStorage.getItem(storeKey);
    if (q === "zh" || q === "en") lang = q;
    else if (saved === "zh" || saved === "en") lang = saved;
  } catch (e) {
    /* a blocked storage API must not decide the page's language */
  }

  var tagNode = document.getElementById("release-tag");
  var releaseTag = null;

  function applyLang(next) {
    lang = next === "zh" ? "zh" : "en";
    var zh = lang === "zh";

    document.documentElement.lang = zh ? "zh-CN" : "en";
    document.title = zh ? "Summrise — 让你的 AI 脚踩真机器" : "Summrise — a real machine under your AI";

    var desc = document.querySelector('meta[name="description"]');
    if (desc) {
      desc.setAttribute("content", zh
        ? "Summrise 把一台 Windows 机器变成 AI 可操作的工位：终端、SSH、串口、浏览器、文件，全部通过 MCP；一个控制台负责观看、批准和随时收回控制权。"
        : "Summrise turns a Windows machine into an AI-operable workspace: terminal, SSH, serial, browser and files over MCP — with one console to watch, approve and take back control from anywhere.");
    }

    for (var j = 0; j < nodes.length; j++) {
      var k = nodes[j].getAttribute("data-i18n");
      nodes[j].innerHTML = zh && ZH[k] ? ZH[k] : en[k];
    }

    var btn = document.getElementById("lang-toggle");
    var label = document.getElementById("lang-label");
    if (btn && label) {
      label.textContent = zh ? "EN" : "中文";
      btn.setAttribute("aria-pressed", zh ? "true" : "false");
      btn.setAttribute("aria-label", zh ? "Switch to English" : "切换到中文");
    }

    if (tagNode) tagNode.textContent = releaseTag || (zh ? ZH["dl.noteFallback"] : "see the releases page");

    try { localStorage.setItem(storeKey, lang); } catch (e) {}
  }

  var toggle = document.getElementById("lang-toggle");
  if (toggle) {
    toggle.addEventListener("click", function () {
      applyLang(lang === "zh" ? "en" : "zh");
    });
  }

  applyLang(lang);

  /* ---------- newest published release ---------- */

  function setHref(id, url) {
    var el = document.getElementById(id);
    if (el && url) el.href = url;
  }

  function findAsset(release, prefix) {
    for (var a = 0; a < release.assets.length; a++) {
      if (release.assets[a].name.indexOf(prefix) === 0) return release.assets[a].browser_download_url;
    }
    return null;
  }

  fetch("https://api.github.com/repos/SilasVale/summrise/releases?per_page=10", { headers: { Accept: "application/vnd.github+json" } })
    .then(function (r) {
      if (!r.ok) throw new Error("releases " + r.status);
      return r.json();
    })
    .then(function (releases) {
      var release = null;
      for (var i = 0; i < releases.length; i++) {
        if (!releases[i].draft) { release = releases[i]; break; }
      }
      if (!release) return;
      releaseTag = release.tag_name;
      if (tagNode) tagNode.textContent = releaseTag;

      var installer = findAsset(release, "SummriseSetup-");
      var doc = findAsset(release, "release-");
      var sums = findAsset(release, "SHA256SUMS");

      setHref("dl-installer", installer || release.html_url);
      setHref("art-installer", installer || release.html_url);
      setHref("art-doc", doc || release.html_url);
      setHref("art-sums", sums || release.html_url);
      setHref("dl-notes", release.html_url);
    })
    .catch(function () {
      /* offline, or the API is rate-limiting: the links already point at the releases page */
    });

  /* ---------- sticky nav hairline ---------- */

  var nav = document.getElementById("nav");
  if (nav) {
    var onScroll = function () {
      nav.classList.toggle("scrolled", window.scrollY > 8);
    };
    window.addEventListener("scroll", onScroll, { passive: true });
    onScroll();
  }
})();
