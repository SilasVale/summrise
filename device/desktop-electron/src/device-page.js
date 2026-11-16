"use strict";
// Which page belongs to the device itself.
//
// The default window and the tray's transcript are the device's pages now (`/activity`, `/panel`), so
// the tripwire needs one decision the URL policy crate cannot make: "is this URL one of the device's
// own pages, on the origin the agent actually listens on?" It is parsed, not prefix-matched — a foreign
// port, a lookalike host, or userinfo carrying the base host must not pass.
function isDevicePage(url, base) {
  let page;
  let origin;
  try {
    page = new URL(url);
    origin = new URL(base);
  } catch {
    return false;
  }
  if (page.username !== "" || page.password !== "") return false;
  if (page.origin !== origin.origin) return false;
  return page.pathname === "/activity" || page.pathname === "/panel";
}

module.exports = { isDevicePage };
