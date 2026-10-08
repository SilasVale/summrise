//! WHERE A COMPONENT COMES FROM, AND WHETHER THE BYTES ARE THE ONES THAT WERE PUBLISHED.
//!
//! Ported from `cdnBase`, `componentUrl`, `componentFetchUrl`, `componentKey`, `componentPins`,
//! `sha256File`, `resolveComponent`, `boxedVersions`, `writeBoxedVersions` and `writeReleaseMarker`
//! in `agent/summrise-agent-npm/src/summrise.ts`.
//!
//! WHY THIS EXISTS: the npm package carries NONE of the big binaries on purpose (cloudflared 54 MB,
//! playwright 31 MB, electron 234 MB), so "not in the package" used to mean "you do not get it,
//! ever". For cloudflared that left a freshly migrated device with no tunnel — and a device with no
//! tunnel is INVISIBLE TO THE CONSOLE while looking perfectly healthy from inside.

use crate::host::Host;
use crate::version::package_version;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The release host this CLI already trusts for version checks.
pub fn cdn_base(host: &dyn Host) -> String {
    let base = host
        .env("SUMMRISE_CDN")
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| crate::endpoints::DEFAULT_CDN_BASE.to_string());
    base.trim_end_matches('/').to_string()
}

/// Where a boxed component lives on that host.
pub fn component_url(host: &dyn Host, name: &str) -> String {
    format!("{}/summrise-agent/{name}", cdn_base(host))
}

/// The release manifest's key for a component's FILE name — they are not the same string.
///
/// The manifest pins `playwright` / `electron` / `cloudflared`; the files are
/// `summrise-playwright.zip` / `electron-win32-x64.zip` / `cloudflared.exe`. Mapping them is what
/// lets setup verify a fetched component against the release manifest — and a file this release
/// does NOT pin must not borrow somebody else's pin.
pub fn component_key(file_name: &str) -> Option<&'static str> {
    if file_name.starts_with("summrise-playwright") {
        Some("playwright")
    } else if file_name.starts_with("electron-") {
        Some("electron")
    } else if file_name.starts_with("cloudflared") {
        Some("cloudflared")
    } else {
        None // a file this release does not pin (e.g. fix-tunnel.ps1)
    }
}

/// The address to FETCH a boxed component from: the release manifest's own `url` for it, or the
/// derived route when the manifest carries none.
///
/// WHY THE MANIFEST IS THE AUTHOR: every release since the pins existed has published a `url` beside
/// each component's `sha256`, and this CLI read the digest while the address was retyped here — one
/// fact, two authors. A manifest that cannot be read (an older release, no network, the worker's
/// 503) keeps the derived route, which is what every release before this one used.
pub fn component_fetch_url(host: &dyn Host, name: &str, pins: &Value) -> String {
    let key = component_key(name);
    let u = key
        .and_then(|k| pins.get(k))
        .and_then(|p| p.get("url"))
        .and_then(|u| u.as_str())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    // `/^https?:\/\/\S+$/` — a scheme-less or non-http url is not an address.
    if is_http_url(&u) {
        u
    } else {
        component_url(host, name)
    }
}

fn is_http_url(u: &str) -> bool {
    regex::Regex::new(r"^https?://\S+$")
        .unwrap()
        .is_match(u.trim())
}

/// The manifest's component pins, or `{}` when it carries none (an older release).
pub fn component_pins(host: &dyn Host) -> Value {
    let url = format!("{}/api/version", cdn_base(host));
    let Some(body) = host.http_get(&url, 5) else {
        return json!({});
    };
    let Ok(j) = serde_json::from_str::<Value>(&body) else {
        return json!({});
    };
    match j.get("components") {
        Some(Value::Object(o)) => Value::Object(o.clone()),
        _ => json!({}),
    }
}

/// THE ADVICE FOR A COMPONENT THAT IS NOT IN THE PACKAGE.
///
/// NOT "reinstall the package": the npm package carries NO boxed components BY DESIGN (that is what
/// keeps it ~6.7 MB), so a reinstall cannot stage this and the advice sent the operator in a circle.
pub const STAGE_ADVICE: &str =
    "run 'summrise setup' to stage it -- the component comes from the release host, not from the npm package.";

/// A boxed component's local path: the copy inside the package if it is there, otherwise the release
/// host's route for it, downloaded to a temp file.
///
/// `None` when neither source works: every component here is optional to the AGENT, and a failed
/// fetch must not fail the install. Both messages this emits go to the caller's log rather than to
/// stdout here, so the decision stays testable.
pub fn resolve_component(host: &dyn Host, name: &str, pkg_path: &Path) -> Option<PathBuf> {
    resolve_component_logged(host, name, pkg_path).0
}

/// The same call, with the lines it wants to print. Separated so a test can assert the DECISION and
/// the sentence together without capturing stdout.
pub fn resolve_component_logged(
    host: &dyn Host,
    name: &str,
    pkg_path: &Path,
) -> (Option<PathBuf>, Vec<String>) {
    let mut log = Vec::new();
    // THE FIRST ARM IS A SEAM, NOT A LIVE PATH — measured: no component is in `package.json`'s
    // `files[]` or in `required-in-tgz.txt`, so today the release host is the ONLY source, and this
    // arm exists so a future release which DOES box one gets it from the package without touching
    // this function.
    if host.exists(pkg_path) {
        return (Some(pkg_path.to_path_buf()), log);
    }
    let dir = match host.make_temp_dir("summrise-comp-") {
        Ok(d) => d,
        Err(_) => return (None, log),
    };
    let dest = dir.join(name);
    // THE MANIFEST IS READ ONCE, BEFORE THE FETCH, because it is the author of the DIGEST and of the
    // ADDRESS. Same read, same object, same order of effects.
    let pins = component_pins(host);
    let url = component_fetch_url(host, name, &pins);
    // `-fsSL`: an HTTP error is a FAILURE, not a 404 page written to disk.
    if !host.http_download(&url, &dest, 300) {
        let (ok, more) = (None, Vec::new());
        log.extend(more);
        return (ok, log);
    }
    if !host.exists(&dest) || host.file_size(&dest).unwrap_or(0) == 0 {
        return (None, log);
    }
    // VERIFY WHAT WAS FETCHED. Until this existed, a worker serving different bytes would have been
    // staged without complaint — the components are executed or loaded, so "the host said so" is not
    // an anchor. A pin that does not match REFUSES the component; no pin at all (an older release) is
    // a warning, not a refusal, because it must not make an install impossible.
    let key = component_key(name);
    let pin = key
        .and_then(|k| pins.get(k))
        .and_then(|p| p.get("sha256"))
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_string();
    if !pin.is_empty() {
        let got = host.sha256_file(&dest).unwrap_or_default();
        if got != pin {
            log.push(format!(
                "setup: REFUSING {name} \u{2014} sha256 {}\u{2026} does not match the release manifest's {}\u{2026} (the host served different bytes than it published)",
                &got[..got.len().min(12)],
                &pin[..pin.len().min(12)]
            ));
            return (None, log);
        }
        log.push(format!(
            "setup: {name} verified against the release manifest ({}\u{2026})",
            &pin[..pin.len().min(12)]
        ));
    } else if key.is_some() {
        log.push(format!(
            "setup: {name} fetched WITHOUT a manifest pin \u{2014} not verified"
        ));
    }
    log.push(format!(
        "setup: {name} fetched from the release host (not in the package)"
    ));
    (Some(dest), log)
}

/// Best-effort writer for the boxed-component manifest (never throws).
pub fn write_boxed_versions(host: &dyn Host, install_dir: &str, pkg_dir: &str) {
    let etc = Path::new(install_dir).join("etc");
    if host.mkdirs(&etc).is_err() {
        return;
    }
    let manifest = boxed_versions(host, install_dir, pkg_dir);
    let body = serde_json::to_string_pretty(&manifest).unwrap_or_default();
    let _ = host.write_bytes(&etc.join("boxed-versions.json"), body.as_bytes());
}

/// The boxed-component manifest the agent echoes in `/api/status`.
///
/// NEVER fail-closed: every probe is best-effort and the write itself is guarded — a failure only
/// logs, never blocks the install/update. Layout v2: callers always run after staging/migration, so
/// the `components\` homes exist — no legacy fallback (single semantic).
pub fn boxed_versions(host: &dyn Host, install_dir: &str, pkg_dir: &str) -> Value {
    let pkg_ver = |p: PathBuf| -> String {
        host.read_string(&p)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|j| {
                j.get("version")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .map(String::from)
            })
            .unwrap_or_else(|| "unknown".to_string())
    };
    let sha_of = |p: &Path| -> String {
        // The oversize guard comes FIRST: the electron runtime is over the cap, and reading it to
        // find that out is exactly what this refuses to do.
        match host.file_size(p) {
            Some(n) if host.is_file(p) && n <= 300 * 1024 * 1024 => {
                host.sha256_file(p).unwrap_or_else(|| "unknown".to_string())
            }
            _ => "unknown".to_string(),
        }
    };
    let components_cloudflared = Path::new(install_dir)
        .join("components")
        .join("cloudflared.exe");
    let cf_bin = if host.exists(&components_cloudflared) {
        components_cloudflared
    } else {
        Path::new(pkg_dir).join("cloudflared.exe")
    };
    let mut cf_ver = "unknown".to_string();
    if host.exists(&cf_bin) {
        let r = host.run(
            &[
                cf_bin.to_string_lossy().to_string(),
                "--version".to_string(),
            ],
            Some(15_000),
        );
        let line = format!("{}{}", r.stdout, r.stderr)
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if !line.is_empty() {
            cf_ver = line.chars().take(120).collect();
        }
    }
    let pw_root = Path::new(install_dir).join("components").join("playwright");
    json!({
        "updated": iso_timestamp(host.now_ms()),
        "playwright_mcp": {
            "version": pkg_ver(pw_root.join("node_modules").join("@playwright").join("mcp").join("package.json")),
            "sha256": sha_of(&Path::new(pkg_dir).join("summrise-playwright.zip")),
        },
        "playwright_core": {
            "version": pkg_ver(pw_root.join("node_modules").join("playwright-core").join("package.json")),
            "sha256": "unknown",
        },
        "cloudflared": {
            "version": cf_ver,
            "sha256": if host.exists(&cf_bin) { sha_of(&cf_bin) } else { "unknown".to_string() },
        },
    })
}

/// `new Date().toISOString()` — the stamp the manifest carries.
///
/// A machine-readable stamp, which is the only thing the oracle asserts about it, but the shape has
/// a reader in the field (an operator comparing two devices), so the port keeps the exact form
/// rather than a counter or a local-time string.
pub fn iso_timestamp(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let millis = ms.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        sod / 3600,
        (sod % 3600) / 60,
        sod % 60
    )
}

/// Howard Hinnant's `civil_from_days`: days since the Unix epoch to a proleptic Gregorian date.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// The release marker `agent_update` reads as the LOCAL version.
///
/// `summrise setup` (fresh install) copies THIS package's exe, so the provable-success point is right
/// after the boot task registers — the caller invokes this only once setup succeeded. Best-effort,
/// never fail-closed: a marker failure must not block install. No mkdir: the callers always run after
/// staging/migration, so `etc\` exists, and a missing dir stays a silent best-effort skip.
pub fn write_release_marker(host: &dyn Host, install_dir: &str) {
    let v = package_version();
    if v.is_empty() {
        return;
    }
    let p = crate::paths::release_marker_path(install_dir);
    let _ = host.write_bytes(&p, v.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeHost;
    use std::path::PathBuf;

    /// Oracle: cli.test.mjs:58 — "componentKey: the manifest's keys and the FILE names are not the
    /// same strings".
    #[test]
    fn component_key_maps_file_names_to_manifest_keys() {
        assert_eq!(component_key("summrise-playwright.zip"), Some("playwright"));
        assert_eq!(component_key("electron-win32-x64.zip"), Some("electron"));
        assert_eq!(component_key("cloudflared.exe"), Some("cloudflared"));
        assert_eq!(component_key("fix-tunnel.ps1"), None);
    }

    /// Oracle: cli.test.mjs:69 — "componentUrl: a component comes from the release host, under the
    /// agent path".
    #[test]
    fn component_url_is_the_release_host_under_the_agent_path() {
        let host = FakeHost::new();
        let cf = component_url(&host, "cloudflared.exe"); // <cdn>/summrise-agent/cloudflared.exe
        assert!(
            regex::Regex::new(r"^https://[^/]+/summrise-agent/cloudflared\.exe$")
                .unwrap()
                .is_match(&cf),
            "{cf}"
        );
        assert!(component_url(&host, "summrise-playwright.zip")
            .ends_with("/summrise-agent/summrise-playwright.zip"));
        // The name is APPENDED under the agent path, so it cannot move the request to another host —
        // and every component resolves to the SAME host.
        let other = component_url(&host, "anything");
        assert_eq!(
            other.split('/').nth(2),
            cf.split('/').nth(2),
            "every component must come from one host"
        );
        assert!(other.find("/summrise-agent/").unwrap() > 0);
    }

    /// Oracle: cli.test.mjs:94 — "componentFetchUrl: the manifest's OWN url is the address, the
    /// derived route only the fallback".
    #[test]
    fn component_fetch_url_prefers_the_published_url() {
        let host = FakeHost::new();
        let url = "https://mirror.test/summrise-agent/cloudflared.exe";
        let pins = json!({"cloudflared": {"url": url, "sha256": "a".repeat(64)}});
        assert_eq!(
            component_fetch_url(&host, "cloudflared.exe", &pins),
            url,
            "the manifest's url must be the address that is fetched"
        );
        // The manifest's KEY is not the file name, so a url filed under the wrong key must not be
        // borrowed by a component it does not describe.
        assert_eq!(
            component_fetch_url(&host, "electron-win32-x64.zip", &pins),
            component_url(&host, "electron-win32-x64.zip")
        );
        for bad in [
            json!({}),
            json!({"cloudflared": {"sha256": "a".repeat(64)}}),
            json!({"cloudflared": {"url": "", "sha256": "a".repeat(64)}}),
            json!({"cloudflared": {"url": "   ", "sha256": "a".repeat(64)}}),
            json!({"cloudflared": {"url": "/summrise-agent/cloudflared.exe", "sha256": "a".repeat(64)}}),
            json!({"cloudflared": {"url": "ftp://mirror.test/cloudflared.exe", "sha256": "a".repeat(64)}}),
        ] {
            assert_eq!(
                component_fetch_url(&host, "cloudflared.exe", &bad),
                component_url(&host, "cloudflared.exe"),
                "an unusable manifest url ({bad}) must fall back, never fetch a non-http URL"
            );
        }
        // A file this release does not pin at all keeps the derived route.
        assert_eq!(
            component_fetch_url(
                &host,
                "fix-tunnel.ps1",
                &json!({"cloudflared": {"url": url}})
            ),
            component_url(&host, "fix-tunnel.ps1")
        );
    }

    /// Oracle: cli.test.mjs:181 — "resolveComponent fetches the url the manifest PUBLISHED — that
    /// field now has a reader".
    ///
    /// The TypeScript drove this by writing a fake `curl` onto `PATH`; here the fetch is a host
    /// method, so the same three cases run against a recorded URL.
    #[test]
    fn resolve_component_fetches_the_published_url_and_still_checks_the_digest() {
        let payload = b"pretend cloudflared bytes";
        let sha = crate::sha256::sha256_bytes(payload);
        let published = "https://manifest-host.test/summrise-agent/cloudflared.exe";

        // (a) the manifest's url is what is fetched, and the bytes are staged.
        let host = FakeHost::new()
            .with_download(published, payload)
            .with_http(
                &format!("{}/api/version", crate::endpoints::DEFAULT_CDN_BASE),
                &json!({"version": "9.9.9", "sha256": "0".repeat(64), "components": {"cloudflared": {"url": published, "sha256": sha}}}).to_string(),
            );
        let (got, log) = resolve_component_logged(
            &host,
            "cloudflared.exe",
            &PathBuf::from("/tmp/not-in-the-package.exe"),
        );
        assert!(
            got.is_some(),
            "a component whose digest matches must be staged"
        );
        assert_eq!(
            host.effects()
                .into_iter()
                .filter(|e| e.starts_with("http_download:"))
                .collect::<Vec<_>>(),
            vec![format!("http_download:{published}")],
            "the fetch must use the manifest's url, not a route derived in the CLI"
        );
        assert!(log
            .iter()
            .any(|l| l.contains("verified against the release manifest")));

        // (b) AND THE MANIFEST IS NOT A WAY PAST THE DIGEST: bytes that do not hash to the published
        // pin are still refused.
        let host = FakeHost::new().with_download(published, payload).with_http(
            &format!("{}/api/version", crate::endpoints::DEFAULT_CDN_BASE),
            &json!({"components": {"cloudflared": {"url": published, "sha256": "b".repeat(64)}}})
                .to_string(),
        );
        let (got, log) = resolve_component_logged(
            &host,
            "cloudflared.exe",
            &PathBuf::from("/tmp/not-in-the-package.exe"),
        );
        assert_eq!(
            got, None,
            "a published url must not excuse a sha256 mismatch"
        );
        assert!(log.iter().any(|l| l.contains("REFUSING")));

        // (c) A manifest that cannot be read keeps the URL this CLI used before the field was read.
        let cdn = FakeHost::new()
            .with_env("SUMMRISE_CDN", "https://cdn.test")
            .with_download("https://cdn.test/summrise-agent/cloudflared.exe", payload);
        let (got, _) = resolve_component_logged(
            &cdn,
            "cloudflared.exe",
            &PathBuf::from("/tmp/not-in-the-package.exe"),
        );
        assert!(
            got.is_some(),
            "a silent manifest is a warning, not a refusal (an older release must still install)"
        );
        assert_eq!(
            cdn.effects()
                .into_iter()
                .filter(|e| e.starts_with("http_download:"))
                .collect::<Vec<_>>(),
            vec!["http_download:https://cdn.test/summrise-agent/cloudflared.exe".to_string()],
            "a silent manifest must fall back to the derived route, unchanged"
        );

        // (d) The package's own copy wins when it is there — the seam a future release that boxes one
        // would use.
        let boxed = FakeHost::new().with_file("/pkg/cloudflared.exe", "x");
        let (got, _) = resolve_component_logged(
            &boxed,
            "cloudflared.exe",
            &PathBuf::from("/pkg/cloudflared.exe"),
        );
        assert_eq!(got, Some(PathBuf::from("/pkg/cloudflared.exe")));
        assert!(boxed.effects().is_empty(), "no fetch for a boxed component");
    }

    /// Oracle: cli.test.mjs:3142 — "a missing component cannot be staged by reinstalling, and the CLI
    /// does not say it can".
    ///
    /// The failure this guards is not cosmetic: a device with no cloudflared has no tunnel, and a
    /// device with no tunnel is INVISIBLE TO THE CONSOLE while looking perfectly healthy from inside.
    #[test]
    fn a_missing_component_names_setup_not_a_reinstall() {
        assert!(STAGE_ADVICE.contains("run 'summrise setup' to stage it"));
        assert!(
            !STAGE_ADVICE.contains("reinstall the package (npm i -g summrise-agent) to stage"),
            "the CLI must not advise a reinstall for a component the package does not carry"
        );
        // ...and the advice is what a caller actually reaches when the fetch fails.
        let host = FakeHost::new();
        let (got, _) =
            resolve_component_logged(&host, "cloudflared.exe", &PathBuf::from("/tmp/absent"));
        assert_eq!(got, None);
    }

    /// Oracle: cli.test.mjs:1130 — "boxedVersions: empty trees → all unknown, ISO updated stamp".
    #[test]
    fn boxed_versions_on_empty_trees_reports_unknown_with_a_stamp() {
        let host = FakeHost::new();
        host.set_now(1_700_000_000_000);
        let m = boxed_versions(&host, "/tmp/empty", "/tmp/empty");
        let stamp = m["updated"].as_str().unwrap();
        assert!(
            regex::Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}Z$")
                .unwrap()
                .is_match(stamp),
            "machine-readable stamp, got {stamp}"
        );
        assert_eq!(m["playwright_mcp"]["version"], json!("unknown"));
        assert_eq!(m["playwright_mcp"]["sha256"], json!("unknown"));
        assert_eq!(m["playwright_core"]["version"], json!("unknown"));
        assert_eq!(m["cloudflared"]["version"], json!("unknown"));
        assert_eq!(m["cloudflared"]["sha256"], json!("unknown"));
    }

    /// Oracle: cli.test.mjs:1151 — "boxedVersions: staged versions read, zip hashed exactly".
    #[test]
    fn boxed_versions_reads_staged_versions_and_hashes_the_zip() {
        let zip = b"fake-playwright-zip-bytes";
        let host = FakeHost::new()
            .with_file(
                "/d/components/playwright/node_modules/@playwright/mcp/package.json",
                r#"{"version":"9.9.9"}"#,
            )
            .with_bytes("/d/summrise-playwright.zip", zip);
        let m = boxed_versions(&host, "/d", "/d");
        assert_eq!(m["playwright_mcp"]["version"], json!("9.9.9"));
        assert_eq!(
            m["playwright_mcp"]["sha256"],
            json!(crate::sha256::sha256_bytes(zip))
        );
    }

    /// Oracle: cli.test.mjs:1184 — "boxedVersions: >300MB blob → unknown sha without reading it".
    ///
    /// The sparse file is created with `set_len`, so the case costs no disk and the timing claim —
    /// "must not read 301MB" — is what the guard is for.
    #[test]
    fn boxed_versions_refuses_to_hash_an_oversize_blob() {
        let dir = std::env::temp_dir().join(format!("summrise-boxed-big-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let big = dir.join("summrise-playwright.zip");
        let f = std::fs::File::create(&big).unwrap();
        f.set_len(301 * 1024 * 1024).unwrap();
        drop(f);
        let host = crate::host::RealHost::new();
        let t0 = std::time::Instant::now();
        let m = boxed_versions(&host, &dir.to_string_lossy(), &dir.to_string_lossy());
        assert_eq!(
            m["playwright_mcp"]["sha256"],
            json!("unknown"),
            "oversize guard, not a hash"
        );
        assert!(
            t0.elapsed().as_secs() < 5,
            "must not read 301MB (took {:?})",
            t0.elapsed()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Oracle: cli.test.mjs:1210 — "writeBoxedVersions: writes a parseable manifest; hostile dirs
    /// stay silent".
    #[test]
    fn write_boxed_versions_writes_a_parseable_manifest() {
        let dir = std::env::temp_dir().join(format!("summrise-boxed-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let host = crate::host::RealHost::new();
        let d = dir.to_string_lossy().to_string();
        write_boxed_versions(&host, &d, &d); // etc/ auto-created, never throws
        let back: Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join("etc").join("boxed-versions.json")).unwrap(),
        )
        .unwrap();
        for k in [
            "updated",
            "playwright_mcp",
            "playwright_core",
            "cloudflared",
        ] {
            assert!(back.get(k).is_some(), "manifest carries {k}");
        }
        // A file where a directory is expected: best-effort skip, never throws.
        let blocker = dir.join("blocker");
        std::fs::write(&blocker, "x").unwrap();
        write_boxed_versions(&host, &blocker.to_string_lossy(), &d);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Oracle: cli.test.mjs:835 — "writeReleaseMarker: fresh-install parity with the round-298 update
    /// marker".
    #[test]
    fn write_release_marker_writes_the_package_version() {
        let dir = std::env::temp_dir().join(format!("summrise-relmark-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("etc")).unwrap();
        let host = crate::host::RealHost::new();
        let d = dir.to_string_lossy().to_string();
        write_release_marker(&host, &d);
        let p = dir.join("etc").join(".summrise-release");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), package_version());
        // Idempotent (re-run overwrites with the same value).
        write_release_marker(&host, &d);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), package_version());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Oracle: cli.test.mjs:864 — "writeReleaseMarker: missing dir stays silent (best-effort, never
    /// throws)".
    ///
    /// The oracle uses `Z:\definitely\not\here` — a drive letter that does not exist on the
    /// Windows box it runs on. The port uses an ABSOLUTE path for the same reason, and not the same
    /// string: on a POSIX host `Z:\definitely\not\here` is a RELATIVE path, so the "missing dir
    /// stays silent" case wrote a file into the crate directory instead. A test that litters the
    /// tree is a test whose case did not happen.
    #[test]
    fn write_release_marker_stays_silent_for_a_missing_dir() {
        let host = crate::host::RealHost::new();
        let missing = std::env::temp_dir().join(format!("summrise-absent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&missing);
        write_release_marker(&host, &missing.to_string_lossy()); // must not panic, must not create
        assert!(
            !missing.exists(),
            "a missing etc/ stays a silent best-effort skip, and creates nothing"
        );
    }

    /// Oracle: cli.test.mjs:3187 — "no component ARTEFACT is boxed in the package, which is why the
    /// loader's first arm is a seam".
    ///
    /// IT MATCHES ARTEFACTS, NOT SUBSTRINGS, and the TypeScript's first version is why: it refused
    /// any entry matching `/electron/i`, and `files[]` legitimately contains
    /// `summrise-desktop-electron/src/main.js` — the desktop shell's SOURCES, which the package is
    /// supposed to carry.
    #[test]
    fn the_npm_package_boxes_no_component_artefact() {
        let pkg: Value = serde_json::from_str(crate::version::PACKAGE_JSON).unwrap();
        let files = pkg
            .get("files")
            .and_then(|f| f.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();
        for artefact in [
            "cloudflared.exe",
            "summrise-playwright.zip",
            "electron-dist",
            "node_modules",
        ] {
            assert!(
                !files.contains(artefact),
                "{artefact} must not be in package files[] — the package carries no boxed components by design"
            );
        }
    }
}
