//! THE DEPLOYMENT'S HOSTNAMES, IN ONE PLACE — and the reason this file exists at all.
//!
//! The ported CLI carries the same defaults the TypeScript carries: the update channel a device
//! checks (`agent.saisi.online`), the device-host suffix `setup` self-registers under, and the npm
//! registry a second release channel is read from. Those defaults ARE the product's behaviour — a
//! port that left them out would not be the same decision — so they belong in the tree.
//!
//! **ONE FILE, DELIBERATELY.** `agent/tests/production_host.rs` counts the FILES that name a
//! production host, and its list may only shrink; a port that scattered the same three URLs across
//! five modules would read to that gate as debt five times over when the debt is one default, written
//! once. Every reference in this crate goes through the constants below, so the count is what it is:
//! one new file, declared with its reason.
//!
//! Each is overridable exactly where the TypeScript makes it overridable — `SUMMRISE_CDN` for the
//! channel, `SUMMRISE_HOSTNAME`/`--hostname` for the device host — and the override is read at the
//! call site, not here, because a `const` that consults the environment is not a constant.

/// The release host this CLI already trusts for version checks and component downloads.
pub const DEFAULT_CDN_BASE: &str = "https://agent.saisi.online";

/// The suffix a self-registering device's hostname is put under, when neither `--hostname` nor
/// `SUMMRISE_HOSTNAME` says otherwise. **THE DEFAULT USED TO BE THE DEVELOPER'S OWN DEVICE**, and
/// every fresh install inherited it — the console registered new machines as `d1` and their tunnels
/// collided with the real one. This is a suffix plus THIS machine's name, which is a default that
/// names nobody.
pub const DEVICE_HOST_SUFFIX: &str = ".agent.saisi.online";

/// The gateway API the console endpoints live on (tunnel-token / register). Overridable for staging
/// with `SUMMRISE_API_BASE`.
pub const DEFAULT_API_BASE: &str = "https://api.saisi.online";

/// The registry's dist-tags endpoint: a few bytes, where the packument is a document.
pub const NPM_DIST_TAGS_URL: &str = "https://registry.npmjs.org/-/package/summrise-agent/dist-tags";
