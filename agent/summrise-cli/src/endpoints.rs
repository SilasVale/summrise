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

/// The hostname `init_tunnel` falls back to when `--tunnel`/`--hostname` carries nothing.
///
/// **A KNOWN DEFECT, CARRIED ACROSS RATHER THAN FIXED** (landing 4b moved the CONSTANT here, not the
/// behaviour): it is the literal DEVELOPER'S DEVICE, so a fresh install that reaches this fallback
/// claims `d1` and collides with the real one. The TypeScript does the same, and a port that quietly
/// changed it would be a behaviour change wearing a port's commit message. [`DEVICE_HOST_SUFFIX`] is
/// the default that names nobody, and `setup` computes the right host one function away
/// ([`crate::tunnel::device_host`]) — fixing this is a separate decision, with a device to measure it
/// on.
///
/// **IT LIVES HERE RATHER THAN IN `tunnel.rs`, AND THAT IS A MEASURED MOVE.** This file is the ONE
/// path `agent/tests/production_host.rs` declares for the CLI's hostname defaults, so the constant
/// sitting in `tunnel.rs` read to that gate as the debt twice over when it is one default written
/// once — the shape this module's own header says does not happen. The value is unchanged, character
/// for character, and the pin below is what keeps the carried defect visible.
pub const DEFAULT_TUNNEL_HOST: &str = "d1.agent.saisi.online";

/// The registry's dist-tags endpoint: a few bytes, where the packument is a document.
pub const NPM_DIST_TAGS_URL: &str = "https://registry.npmjs.org/-/package/summrise-agent/dist-tags";

#[cfg(test)]
mod tests {
    use super::*;

    /// THE DEFAULT IS CARRIED, NOT FIXED. This case exists so the carried defect is VISIBLE: a
    /// future commit that changes the default has to change this assertion and say why.
    ///
    /// It moved here WITH the constant (landing 4b) so the literal stays inside the one file
    /// `production_host.rs` declares — the pin and the thing it pins, in one place.
    #[test]
    fn the_default_tunnel_host_is_still_the_developers_device() {
        assert_eq!(DEFAULT_TUNNEL_HOST, "d1.agent.saisi.online");
    }

    /// And the suffix is the answer that names nobody — the two constants are different answers to
    /// one question, which is what makes the defect above worth pinning.
    #[test]
    fn the_device_host_suffix_names_nobody() {
        assert_eq!(DEVICE_HOST_SUFFIX, ".agent.saisi.online");
        assert!(
            !DEVICE_HOST_SUFFIX.starts_with("d1"),
            "the suffix must not carry the developer's device name"
        );
    }
}
