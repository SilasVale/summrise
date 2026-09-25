//! Cloudflared tunnel provisioning for the Settings-page Gateway card
//! (POST /api/gateway/connect → the optional free tunnel). Moved verbatim
//! out of web.rs so the HTTP surface stays auth + dispatch and the
//! cloudflared/Cloudflare infra lives in its own module seam. The RUNNING
//! cloudflared child is NOT owned here — main.rs's supervisor task owns the
//! single child; this module rewrites tunnel.yml and signals the restart via
//! [`crate::tunnel_ctl`].

use sha2::{Digest, Sha256};
use std::path::Path;

// ── Pinned cloudflared release (on-demand download integrity) ────────────
//
// The agent downloads cloudflared.exe on demand, but ONLY when
// InstallDir\tools\cloudflared.exe is absent. NOTE (verified 2026-09-06):
// the published npm tgz currently boxes NO binary (1.2.297 tgz holds only
// the exe + electron shell + summrise.js), so THIS download is the live channel
// devices actually use — `summrise setup` / `agent_update`'s boxed-staging arms
// are dormant until the release flow packs the binary. That makes the pin
// below load-bearing, not belt-and-braces: a wrong constant breaks ALL
// tunnel provisioning, so it was measured against the exact bytes the
// gateway proxy serves (see CLOUDFLARED_SHA256). The old gate — a
// versionless `latest` URL plus a "bigger than 1MB" size check — is NOT an
// integrity story: upstream can ship new bytes under the same URL at any
// time, and a compromised proxy would hand us an executable we then run at
// SYSTEM. So the download
// is pinned AND hash-gated, mirroring `agent_update`'s ver&&sha double bar
// (round-119): a versioned immutable URL + a sha256 constant, verified
// BEFORE the bytes are written or executed. ANY mismatch fails CLOSED
// (clear error string + error log, no write, no spawn).
//
// HOW TO UPDATE THE PIN (release flow — do all three together):
//   1. On a trusted machine, download the versioned asset for the new
//      release and record its hash:
//        curl -sL -o cloudflared-windows-amd64.exe \
//          https://github.com/cloudflare/cloudflared/releases/download/<NEW_VERSION>/cloudflared-windows-amd64.exe
//        sha256sum cloudflared-windows-amd64.exe
//   2. Set CLOUDFLARED_VERSION to <NEW_VERSION> and CLOUDFLARED_SHA256 to the
//      hash below.
//   3. If the release flow boxes the binary (gitignored staging file
//      `agent/summrise-agent-npm/cloudflared.exe`, packed into the tgz),
//      stage the EXACT same bytes and confirm `cloudflared --version`
//      prints the pinned version (today the tgz carries no binary, so
//      this step is a no-op — the download path below is the channel).
//   4. `cargo test` — the unit tests below cover match, mismatch-rejection,
//      and the versioned-URL shape; `cargo clippy -- -D warnings` must stay
//      clean.
///
/// Pinned cloudflared release (`main.Version=2026.8.3`,
/// `BuildTime=2026-08-31T02:48 UTC` per its ldflags).
const CLOUDFLARED_VERSION: &str = "2026.8.3";
/// sha256 of the pinned `cloudflared-windows-amd64.exe` asset, measured
/// 2026-09-06 from the bytes the gateway proxy serves TODAY (the proxy
/// streams the upstream `latest` asset unmodified, so this equals the
/// versioned-asset bytes while `latest` stays 2026.8.3). Re-measure per the
/// steps above whenever CLOUDFLARED_VERSION moves — a stale pin fails CLOSED
/// (that refusal IS the drift signal, not a bug).
const CLOUDFLARED_SHA256: &str = "83e726ed18ea78c5ad5213c4c3a3a27051393950d2bc8ed4de69bec12d14eaae";

/// Immutable upstream asset for the pinned release (GitHub release assets
/// never change under a versioned path — unlike `.../releases/latest/...`).
fn cloudflared_download_url() -> String {
    format!(
        "https://github.com/cloudflare/cloudflared/releases/download/{CLOUDFLARED_VERSION}/cloudflared-windows-amd64.exe"
    )
}

/// Reachability fallback for devices where GitHub is blocked (GFW etc.) — the deployment's own proxy of the official
/// release, at the same host that serves the agent's updates. STILL hash-gated: it tracks `latest`, so once upstream
/// moves past the pin a proxied download fails CLOSED with the mismatch log below (the signal to run the HOW-TO-UPDATE
/// steps above), exactly like a tampered binary would.
///
/// DERIVED FROM CONFIGURATION, NOT HARDCODED (round 45 of the standing goal). This was a `const` carrying the
/// production host, which made the repository a second place that host was written down — the first being the device's
/// own config, where a deployment's URLs belong. A build with no `download_url` configured now has no proxy fallback at
/// all, which is the honest behaviour: the upstream GitHub URL is the primary candidate, and a device that has not been
/// told where its download site is cannot be told where a proxy of it is either.
fn cloudflared_proxy_url(download_url: &str) -> String {
    format!(
        "{}/summrise-agent/cloudflared.exe",
        download_url.trim_end_matches('/')
    )
}

/// True only when `bytes` hash to `expected_hex`. Malformed expectations
/// (wrong length, non-hex) NEVER match — fail closed, never fail open.
pub(crate) fn verify_cloudflared_bytes(bytes: &[u8], expected_hex: &str) -> bool {
    if expected_hex.len() != 64 || !expected_hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }
    crate::hex_encode(&Sha256::digest(bytes)).eq_ignore_ascii_case(expected_hex)
}

/// Download one candidate URL and gate it: 2xx + sane size + pinned sha256.
/// Err carries the human-readable reason (surfaced in the API status string
/// and the error log — an operator must see WHY provisioning refused).
async fn download_and_verify(client: &reqwest::Client, url: &str) -> Result<bytes::Bytes, String> {
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    let resp = resp
        .error_for_status()
        .map_err(|e| format!("bad status: {e}"))?;
    let bytes = resp.bytes().await.map_err(|e| format!("read error: {e}"))?;
    if bytes.len() <= 1_000_000 {
        return Err(format!("unexpected small payload ({} bytes)", bytes.len()));
    }
    if !verify_cloudflared_bytes(&bytes, CLOUDFLARED_SHA256) {
        tracing::error!(
            "[summrise-agent] provision_tunnel: cloudflared sha256 MISMATCH from {url} \
             (want pinned {CLOUDFLARED_VERSION} {CLOUDFLARED_SHA256}, got {}) — \
             refusing unverifiable binary (no write, no spawn)",
            crate::hex_encode(&Sha256::digest(&bytes)),
        );
        return Err("sha256 mismatch — refusing unverifiable binary".to_string());
    }
    Ok(bytes)
}

/// Write already-verified bytes into place (atomic: a kill mid-write must not
/// leave a half-written exe that the supervisor would then spawn). The hash
/// is re-checked here so NO caller can stage unverified bytes by accident —
/// production passes CLOUDFLARED_SHA256; tests pass their fixture digest.
pub(crate) fn write_verified_bytes(
    dest: &Path,
    bytes: &[u8],
    expected_sha256_hex: &str,
) -> Result<(), String> {
    if !verify_cloudflared_bytes(bytes, expected_sha256_hex) {
        tracing::error!(
            "[summrise-agent] provision_tunnel: refusing to write {} — \
             integrity check failed (no write performed)",
            dest.display(),
        );
        return Err(
            "cloudflared integrity check failed — refusing unverifiable binary".to_string(),
        );
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("parent dir create failed: {e}"))?;
    }
    crate::bootstrap::atomic_write(dest, bytes)
        .map_err(|e| format!("cloudflared download write failed: {e}"))?;
    Ok(())
}

/// Provision the free cloudflared tunnel from the Settings-page Gateway card:
/// login with the token, create the tunnel, route DNS, write tunnel.yml, and
/// spawn cloudflared (agent-owned, spawn-if-absent model). Returns a status
/// string for the API response. Best-effort — failures are reported, not fatal.
/// `port` is the agent's configured bind port — the ingress must point where
/// the agent actually listens (a hardcoded 18080 502s custom-port installs).
pub(crate) async fn provision_tunnel(
    cf_token: &str,
    port: u16,
    download_url: Option<&str>,
) -> String {
    let cf = crate::paths::cloudflared_bin();
    if !cf.exists() {
        // components\cloudflared.exe absent (the boxed tgz binary normally covers
        // this) — download the PINNED official Windows binary on demand
        // (one-time). Pinned version + sha256 (see the constants above): the
        // bytes are verified BEFORE they are written or executed, mirroring
        // agent_update's ver&&sha bar. Fail closed on any mismatch.
        tracing::info!(
            "[summrise-agent] provision_tunnel: downloading pinned cloudflared {CLOUDFLARED_VERSION}"
        );
        let client = match reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
        {
            Ok(c) => c,
            Err(_) => return "cloudflared download client build failed".to_string(),
        };
        // Versioned upstream first; the gateway proxy (same host the old
        // code used) as the reachability fallback. EVERY candidate is
        // hash-gated inside download_and_verify — an unverified binary can
        // never reach the write below.
        let mut verified: Option<bytes::Bytes> = None;
        let mut last_err = String::new();
        let mut candidates = vec![cloudflared_download_url()];
        if let Some(site) = download_url.filter(|s| !s.trim().is_empty()) {
            candidates.push(cloudflared_proxy_url(site));
        }
        for url in candidates {
            match download_and_verify(&client, &url).await {
                Ok(b) => {
                    verified = Some(b);
                    break;
                }
                Err(e) => {
                    tracing::warn!(
                        "[summrise-agent] provision_tunnel: cloudflared candidate failed ({url}): {e}"
                    );
                    last_err = format!("{url}: {e}");
                }
            }
        }
        let bytes = match verified {
            Some(b) => b,
            None => return format!("cloudflared download failed ({last_err})"),
        };
        if let Err(e) = write_verified_bytes(&cf, &bytes, CLOUDFLARED_SHA256) {
            return e;
        }
        tracing::info!(
            "[summrise-agent] provision_tunnel: cloudflared {CLOUDFLARED_VERSION} verified (sha256 ok, {} bytes)",
            bytes.len()
        );
    }
    let hostname = std::fs::read_to_string(crate::paths::hostname_file())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    // Supervision audit #5: hostname flows into cloudflared ARGV and an
    // unquoted YAML line. A value starting with '-' becomes a FLAG, an
    // embedded newline injects keys (e.g. a different `service:` target).
    // Validate to bare subdomain charset before anything else touches it.
    let host_ok = |v: &str| -> bool {
        !v.is_empty()
            && v.len() <= 253
            && !v.starts_with('-')
            && v.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
    };
    if !host_ok(&hostname) {
        return "cannot provision: summrise-agent.hostname missing or invalid (set it via `summrise setup --hostname <sub>` first)".to_string();
    }
    if !host_ok(cf_token) {
        return "cannot provision: gateway returned a malformed API token".to_string();
    }
    let tunnel_name = format!(
        "summrise-agent-{}",
        hostname.split('.').next().unwrap_or("device")
    );
    // 1. login with token. cloudflared writes cert.pem to %USERPROFILE%\.cloudflared\
    //    — under the SYSTEM service that is systemprofile, and `tunnel login
    //    --token` may not write it there reliably. After login, ensure the
    //    credentials exist: copy from a real user profile if missing.
    // `cloudflared.exe` is a console-subsystem binary — the no-console rule,
    // and the flag it names, live in `crate::spawn::hidden`. EVERY site in this
    // file asks (login, list, create, list again, route dns).
    //
    // THE COMMAND IS BUILT IN THE OPEN RATHER THAN AS A CHAIN, which is the
    // ask's requirement and not a style choice: the whole-tree gate that counts
    // these asks (`spawn::console_subsystem_spawn_sites_ask_for_the_flag`)
    // reads the gap between one spawn expression and the NEXT, so an ask placed
    // after the last `.arg(...)` of a chain falls outside the gap it belongs to.
    // `main.rs`'s fix-tunnel site took the same shape for the same reason.
    let mut login_cmd = tokio::process::Command::new(&cf);
    login_cmd.args(["tunnel", "login", "--token", cf_token]);
    crate::spawn::hidden(&mut login_cmd);
    let login = login_cmd.output().await;
    let login_ok = login.map(|o| o.status.success()).unwrap_or(false);
    if !login_ok {
        return "cloudflared login failed".to_string();
    }
    ensure_cf_credentials().await;
    // 2. create tunnel (idempotent-ish: list first). The tunnel ID is a
    //    canonical UUID — parse it with the dash-delimited regex from the
    //    `tunnel list` output; `tunnel create` prints the full ID on success,
    //    so if the list parse fails (table truncation etc.) grab it from the
    //    create output directly.
    let mut list_cmd = tokio::process::Command::new(&cf);
    list_cmd.args(["tunnel", "list"]);
    crate::spawn::hidden(&mut list_cmd); // same no-console ask as the login spawn above
    let list = list_cmd.output().await;
    let list_text = list
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let mut tunnel_id = find_tunnel_id_by_name(&list_text, &tunnel_name);
    if tunnel_id.is_none() {
        let mut create_cmd = tokio::process::Command::new(&cf);
        create_cmd.args(["tunnel", "create", &tunnel_name]);
        crate::spawn::hidden(&mut create_cmd); // same no-console ask as the login spawn above
        let created = create_cmd.output().await;
        let (created_text, created_err) = match created {
            Ok(o) => (
                String::from_utf8_lossy(&o.stdout).to_string(),
                String::from_utf8_lossy(&o.stderr).to_string(),
            ),
            Err(_) => (String::new(), String::new()),
        };
        tunnel_id = parse_tunnel_id(&created_text).or_else(|| parse_tunnel_id(&created_err));
        if tunnel_id.is_none() {
            let mut list2_cmd = tokio::process::Command::new(&cf);
            list2_cmd.args(["tunnel", "list"]);
            crate::spawn::hidden(&mut list2_cmd); // same no-console ask as the login spawn above
            let list2 = list2_cmd.output().await;
            let list2_text = list2
                .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
                .unwrap_or_default();
            tunnel_id = find_tunnel_id_by_name(&list2_text, &tunnel_name);
        }
    }
    let Some(id) = tunnel_id else {
        // Include the raw list output in the error so a device report
        // pinpoints WHY parsing failed (auth? empty list? different format?).
        let diag = format!(
            "could not determine tunnel id for '{tunnel_name}'. login_ok={} list_out={:?}",
            login_ok,
            &list_text[..list_text.len().min(400)],
        );
        return diag;
    };
    // 3. DNS route (best-effort)
    let mut route_cmd = tokio::process::Command::new(&cf);
    route_cmd.args(["tunnel", "route", "dns", &tunnel_name, &hostname]);
    crate::spawn::hidden(&mut route_cmd); // same no-console ask as the login spawn above
    let _ = route_cmd.output().await;
    // 3b. Update the tunnel's REMOTE config via the Cloudflare API — cloudflared
    //     prefers the remote config when one exists, and a stale remote (old
    //     127.0.0.2 ingress) would override the local tunnel.yml. Point the
    //     remote ingress at 127.0.0.1 so both agree.
    let remote = update_remote_config(cf_token, &id, &hostname, port).await;
    match &remote {
        RemoteConfig::Failed(reason) => {
            tracing::warn!("[summrise-agent] provision_tunnel: remote config NOT updated: {reason}")
        }
        RemoteConfig::Updated => {
            tracing::info!("[summrise-agent] provision_tunnel: remote config update ok=true")
        }
    }
    // 4. write tunnel.yml (single location, agent spawns it on boot)
    let cred = std::env::var("USERPROFILE")
        .map(|u| format!(r"{u}\.cloudflared\{id}.json"))
        .unwrap_or_else(|_| format!(".cloudflared/{id}.json"));
    let yml = format!(
        "tunnel: {id}\ncredentials-file: {cred}\nallow-remote-config: false\ningress:\n  - hostname: {hostname}\n    service: {}\n  - service: http_status:404\n",
        ingress_service(port)
    );
    let cfg_path = crate::paths::tunnel_file();
    // Supervision audit #5: atomic (the boot-spawned cloudflared may be
    // mid-read) — and #1: DO NOT spawn a second tunnel here; the supervisor
    // task owns the single child and restarts on the generation bump.
    // A FAILED WRITE MUST NOT REPORT SUCCESS — AND MUST NOT REQUEST THE RESTART.
    //
    // This was `let _ = atomic_write(...)`: the failure vanished, the generation still
    // bumped, and the Gateway card answered `ok (host)` because that string is what the
    // caller returns. The supervisor then relaunched cloudflared against a `tunnel.yml`
    // that was never written — a device that comes up on a STALE ingress, or not at all,
    // while every surface the operator can see says it is connected.
    //
    // Restarting against a config we failed to write is worse than not restarting at all:
    // the old file may still be the working one, and this way it keeps being used.
    // THE OPERATOR'S VERDICT IS DECIDED BY A PURE FUNCTION (tunnel_outcome), not assembled
    // here. Assembling it here is exactly what left the FAILED branch with no coverage:
    // round 112 measured that mutating this tail back to a bare `format!("ok ({hostname})")`
    // kept the WHOLE suite green, because nothing could call into the decision.
    tunnel_outcome(install_tunnel_config(&cfg_path, &yml), remote, &hostname)
}

/// Write the tunnel config, and bump the restart signal ONLY if it landed.
///
/// Split out so the ORDERING CLAIM is testable rather than asserted in a comment: a
/// best-effort write plus an unconditional restart is exactly how a device ends up
/// relaunching against a config that does not exist.
fn install_tunnel_config(path: &std::path::Path, yml: &str) -> Result<(), String> {
    crate::bootstrap::atomic_write(path, yml.as_bytes())
        .map_err(|e| format!("could not write {} ({e})", path.display()))?;
    crate::tunnel_ctl::request_restart();
    Ok(())
}

// Tunnel-ID output parsers (round-426: hoisted to module level from
// provision_tunnel verbatim so the hand-rolled scanner is unit-testable;
// the only callers are the two spots above).
fn parse_tunnel_id(text: &str) -> Option<String> {
    // Canonical UUID with dashes: 8-4-4-4-12 hex. Scan char windows to
    // avoid pulling in the regex crate (cargo-xwin build stays lean).
    let bytes = text.as_bytes();
    let is_hex = |c: u8| c.is_ascii_hexdigit();
    let mut i = 0;
    while i + 36 <= bytes.len() {
        let seg = [8usize, 4, 4, 4, 12];
        let mut ok = true;
        let mut pos = i;
        for (si, len) in seg.iter().enumerate() {
            for _ in 0..*len {
                if !is_hex(bytes[pos]) {
                    ok = false;
                    break;
                }
                pos += 1;
            }
            if !ok {
                break;
            }
            if si < seg.len() - 1 {
                if bytes[pos] != b'-' {
                    ok = false;
                    break;
                }
                pos += 1;
            }
        }
        if ok {
            return Some(text[i..i + 36].to_string());
        }
        i += 1;
    }
    None
}
// `tunnel list` WITHOUT --name: the --name filter behaves differently
// across cloudflared versions and can return empty — match the NAME
// column ourselves (ID is col 1, NAME is col 2 in the table).
fn find_tunnel_id_by_name(text: &str, name: &str) -> Option<String> {
    for line in text.lines() {
        let toks: Vec<&str> = line.split_whitespace().collect();
        if toks.len() >= 2 && toks[1] == name {
            if let Some(id) = parse_tunnel_id(toks[0]) {
                return Some(id);
            }
        }
    }
    None
}

/// Make sure the SYSTEM agent's cloudflared credentials exist. `tunnel
/// login --token` under SYSTEM writes to systemprofile\.cloudflared — if
/// that failed, copy cert.pem + tunnel credentials from a real user profile
/// (Administrator runs the console/install flows and already has them).
async fn ensure_cf_credentials() {
    let sys_cf = std::env::var("USERPROFILE")
        .map(|u| std::path::PathBuf::from(u).join(".cloudflared"))
        .unwrap_or_default();
    if sys_cf.join("cert.pem").exists() {
        return; // already authenticated
    }
    // Candidate user profiles to copy from.
    for user in ["Administrator", "admin", "user"] {
        let src = std::path::PathBuf::from(r"C:\Users")
            .join(user)
            .join(".cloudflared");
        let cert = src.join("cert.pem");
        if cert.exists() {
            let _ = std::fs::create_dir_all(&sys_cf);
            if std::fs::copy(&cert, sys_cf.join("cert.pem")).is_ok() {
                // Copy all *.<uuid>.json credentials too.
                if let Ok(rd) = std::fs::read_dir(&src) {
                    for e in rd.flatten() {
                        let name = e.file_name().to_string_lossy().to_string();
                        if name.ends_with(".json") && e.path().is_file() {
                            let _ = std::fs::copy(e.path(), sys_cf.join(&name));
                        }
                    }
                }
                tracing::info!(
                    "[summrise-agent] provision_tunnel: copied cloudflared credentials from {user}"
                );
                return;
            }
        }
    }
    tracing::warn!("[summrise-agent] provision_tunnel: no cert.pem found in any user profile — tunnel auth may fail");
}

/// Ingress service URL for the agent's configured port (custom ports must
/// reach the agent where it actually listens — a hardcoded 18080 here 502s
/// every non-default install).
fn ingress_service(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

/// The verdict of the REMOTE config PUT — deliberately NOT the same thing as the local
/// `tunnel.yml` write. cloudflared prefers a remote configuration whenever one exists, so a
/// PUT that did not land can override a local file that was written perfectly.
#[derive(Debug)]
enum RemoteConfig {
    /// The PUT landed: the remote ingress points where the local file points.
    Updated,
    /// The PUT did not land, WITH the reason. Whether this tunnel HAS a remote
    /// configuration cannot be known from here — and if it does, it wins over the local file.
    Failed(String),
}

/// The ONE line the Gateway card shows, decided from BOTH verdicts.
///
/// Pure on purpose. This is the branch the operator actually reads, and while it lived inline
/// in `provision_tunnel` it had no coverage at all (round 112: mutating it back to a bare `ok`
/// kept the suite green). As a value-taking function, every case is testable.
fn tunnel_outcome(local: Result<(), String>, remote: RemoteConfig, hostname: &str) -> String {
    if let Err(e) = local {
        // A failed local write is never dressed up as a partial success: the OLD tunnel.yml
        // is still the file cloudflared will read.
        return format!("FAILED: {e} — the tunnel was NOT (re)configured and was left as it was");
    }
    match remote {
        RemoteConfig::Updated => format!("ok ({hostname})"),
        // NO SILENT SUCCESS: the local file is right, but cloudflared prefers a remote
        // configuration when one exists. The operator gets the consequence AND the cause —
        // "it failed" without a reason is not actionable.
        RemoteConfig::Failed(reason) => format!(
            "PARTIAL ({hostname}): local config written, remote config NOT updated ({reason}) — \
             a remote configuration OVERRIDES the local file, so verify this tunnel before trusting it"
        ),
    }
}

/// Update a tunnel's REMOTE config (Cloudflare API) so its ingress points at
/// the agent's configured port. cloudflared prefers the remote config over the local file
/// when one exists; a stale remote (e.g. an old 127.0.0.2 ingress) would keep
/// proxying to a dead address (502) no matter what tunnel.yml says.
///
/// It RETURNS its verdict. It used to return `()` through eight bare `return`s, which made a
/// failed remote update invisible everywhere while the local file claimed 127.0.0.1.
async fn update_remote_config(
    cf_token: &str,
    tunnel_id: &str,
    hostname: &str,
    port: u16,
) -> RemoteConfig {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => return RemoteConfig::Failed(format!("no HTTP client ({e})")),
    };
    // 1. Resolve the account id from the token.
    let acc = match client
        .get("https://api.cloudflare.com/client/v4/accounts")
        .header("authorization", format!("Bearer {cf_token}"))
        .send()
        .await
    {
        Ok(r) => match r.json::<serde_json::Value>().await {
            Ok(j) => j,
            Err(e) => {
                return RemoteConfig::Failed(format!("the accounts response was not JSON ({e})"))
            }
        },
        Err(e) => {
            return RemoteConfig::Failed(format!(
                "the Cloudflare API is unreachable (accounts: {e})"
            ))
        }
    };
    let account_id = match acc["result"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|x| x["id"].as_str())
    {
        Some(v) => v.to_string(),
        None => return RemoteConfig::Failed("the token returned no account id".to_string()),
    };
    // 2. PUT the ingress config.
    let body = serde_json::json!({
        "config": {
            "ingress": [
                { "hostname": hostname, "service": ingress_service(port) },
                { "service": "http_status:404" }
            ]
        }
    });
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{account_id}/cfd_tunnel/{tunnel_id}/configurations"
    );
    match client
        .put(&url)
        .header("authorization", format!("Bearer {cf_token}"))
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .await
    {
        Ok(r) => {
            let status = r.status();
            if status.is_success() {
                RemoteConfig::Updated
            } else {
                RemoteConfig::Failed(format!("Cloudflare answered HTTP {status}"))
            }
        }
        Err(e) => RemoteConfig::Failed(format!(
            "the Cloudflare API is unreachable (configurations: {e})"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingress_service_follows_the_configured_port() {
        assert_eq!(ingress_service(18080), "http://127.0.0.1:18080");
        assert_eq!(ingress_service(7740), "http://127.0.0.1:7740");
        // A custom port must never silently fall back to the default.
        assert!(!ingress_service(7740).contains("18080"));
    }

    /// sha256("abc") — FIPS vector. Hardcodes the digest so the hex-encode +
    /// compare path is NOT tautological (a test that recomputes the expected
    /// with the same code could never catch an encoding bug).
    const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn verify_accepts_exact_digest() {
        assert!(verify_cloudflared_bytes(b"abc", ABC_SHA256));
    }

    #[test]
    fn verify_is_case_insensitive_on_hex() {
        assert!(verify_cloudflared_bytes(b"abc", &ABC_SHA256.to_uppercase()));
    }

    #[test]
    fn verify_rejects_tampered_and_malformed() {
        // One-bit payload change.
        assert!(!verify_cloudflared_bytes(b"abd", ABC_SHA256));
        // All-zero digest (wrong value, right shape).
        assert!(!verify_cloudflared_bytes(
            b"abc",
            "0000000000000000000000000000000000000000000000000000000000000000"
        ));
        // Malformed expectations fail CLOSED, never open.
        assert!(!verify_cloudflared_bytes(b"abc", ""));
        assert!(!verify_cloudflared_bytes(b"abc", "not-hex"));
        assert!(!verify_cloudflared_bytes(b"abc", &ABC_SHA256[..63]));
    }

    #[test]
    fn pinned_constants_are_well_formed() {
        // A malformed constant would fail CLOSED on every provision (brick
        // the tunnel path) — pin the shape here so a bad edit fails `cargo
        // test`, not a device at midnight.
        assert!(!CLOUDFLARED_VERSION.is_empty());
        assert!(!CLOUDFLARED_VERSION.contains("latest"));
        assert_eq!(CLOUDFLARED_SHA256.len(), 64);
        assert!(CLOUDFLARED_SHA256.bytes().all(|b| b.is_ascii_hexdigit()));
    }

    /// THE FALLBACK IS THE CONFIGURED SITE, JOINED — and the join is where a double slash would live: a deployment that
    /// writes `https://host/` in its config must not produce `https://host//summrise-agent/...`. Pinned here because the
    /// value now comes from a human's config file rather than from a constant this file controls.
    #[test]
    fn the_proxy_fallback_joins_the_configured_site() {
        assert_eq!(
            cloudflared_proxy_url("https://cdn.example.com"),
            "https://cdn.example.com/summrise-agent/cloudflared.exe"
        );
        assert_eq!(
            cloudflared_proxy_url("https://cdn.example.com/"),
            "https://cdn.example.com/summrise-agent/cloudflared.exe",
            "a trailing slash in the config must not double up"
        );
    }

    #[test]
    fn download_url_is_versioned_and_immutable() {
        let url = cloudflared_download_url();
        assert!(
            url.contains(CLOUDFLARED_VERSION),
            "versioned URL must name the pin: {url}"
        );
        assert!(
            url.ends_with("cloudflared-windows-amd64.exe"),
            "official asset name: {url}"
        );
        assert!(!url.contains("latest"), "never the mutable latest: {url}");
    }

    fn test_dir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("summrise-tunnel-cf-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn write_rejects_tampered_bytes_without_touching_disk() {
        let dir = test_dir("reject");
        let dest = dir.join("tools").join("cloudflared.exe");
        let err = write_verified_bytes(&dest, b"tampered-bytes", ABC_SHA256).unwrap_err();
        assert!(
            err.contains("integrity check failed"),
            "clear fail-closed message: {err}"
        );
        assert!(!dest.exists(), "mismatched bytes must never be written");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_stores_verified_bytes_intact() {
        // Happy path: the expected digest is the TRUE digest of the fixture
        // (computed with sha2 directly — this test covers the write + the
        // verify-then-write wiring, while verify_* above covers the digest
        // itself against the hardcoded FIPS vector).
        let dir = test_dir("accept");
        let dest = dir.join("tools").join("cloudflared.exe");
        let fixture = b"summrise-test-cloudflared-fixture-bytes";
        let expected = crate::hex_encode(&Sha256::digest(fixture));
        write_verified_bytes(&dest, fixture, &expected).expect("matching bytes must stage");
        assert_eq!(
            std::fs::read(&dest).expect("staged file readable"),
            fixture,
            "staged bytes must equal the verified download"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_tunnel_id_finds_canonical_uuid() {
        let id = "f47ac10b-58cc-4372-a567-0e02b2c3d479";
        assert_eq!(
            parse_tunnel_id(&format!("created tunnel {id} with id")),
            Some(id.to_string())
        );
        // table row shape
        assert_eq!(
            parse_tunnel_id(&format!("{id}  summrise-d1  2026-01-01")),
            Some(id.to_string())
        );
    }

    #[test]
    fn parse_tunnel_id_rejects_non_uuid() {
        assert_eq!(parse_tunnel_id("no uuid here"), None);
        assert_eq!(parse_tunnel_id(""), None);
        // dashless 32-hex is not canonical
        assert_eq!(parse_tunnel_id("f47ac10b58cc4372a5670e02b2c3d479"), None);
        // truncated
        assert_eq!(parse_tunnel_id("f47ac10b-58cc-4372-a567"), None);
        // non-hex in a dash-shaped slot
        assert_eq!(
            parse_tunnel_id("f47ac10b-58cc-4372-a567-0e02b2c3d47z"),
            None
        );
    }

    #[test]
    fn find_tunnel_id_by_name_matches_name_column() {
        let id = "f47ac10b-58cc-4372-a567-0e02b2c3d479";
        let other = "aaaaaaaa-1111-2222-3333-444444444444";
        let table = format!("ID  NAME  CREATED\n{other}  other-tunnel  x\n{id}  summrise-d1  y\n");
        assert_eq!(
            find_tunnel_id_by_name(&table, "summrise-d1"),
            Some(id.to_string())
        );
        assert_eq!(find_tunnel_id_by_name(&table, "missing"), None);
        // header row itself never matches (NAME != a real name… unless asked)
        assert_eq!(find_tunnel_id_by_name("ID  NAME\n", "NAME"), None);
        // name match with a garbage id column is skipped, not returned
        assert_eq!(
            find_tunnel_id_by_name("oops  summrise-d1\n", "summrise-d1"),
            None
        );
    }
    #[test]
    fn a_config_write_that_fails_neither_reports_success_nor_asks_for_a_restart() {
        // The tie between the two halves is the whole point: a best-effort write plus an
        // unconditional `request_restart()` is how a device relaunches cloudflared against
        // a tunnel.yml that was never written — a stale ingress, with the Gateway card
        // saying "connected". The generation is the observable: it must MOVE on success and
        // STAY on failure.
        let dir = std::env::temp_dir().join(format!("summrise-tcfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // The failure is real, not simulated: the target's parent is a FILE.
        let blocker = dir.join("blocker");
        std::fs::write(&blocker, b"not a directory").unwrap();
        let bad = blocker.join("tunnel.yml");

        let before = crate::tunnel_ctl::generation();
        let err = install_tunnel_config(&bad, "tunnel: x\n").unwrap_err();
        assert!(err.contains("could not write"), "{err}");
        assert_eq!(
            crate::tunnel_ctl::generation(),
            before,
            "a failed write must NOT request a restart"
        );

        let good = dir.join("tunnel.yml");
        install_tunnel_config(&good, "tunnel: x\n").unwrap();
        assert_eq!(std::fs::read_to_string(&good).unwrap(), "tunnel: x\n");
        assert_ne!(
            crate::tunnel_ctl::generation(),
            before,
            "a written config must restart"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_gateway_card_says_which_of_the_two_tunnel_configs_landed() {
        // The LOCAL write failed: today's exact words, and never dressed up as a success —
        // the old tunnel.yml is still the file cloudflared reads.
        assert_eq!(
            tunnel_outcome(
                Err("could not write D:\\Summrise\\etc\\tunnel.yml (access denied)".into()),
                RemoteConfig::Updated,
                "d1.agent.saisi.online",
            ),
            "FAILED: could not write D:\\Summrise\\etc\\tunnel.yml (access denied) — the tunnel was NOT (re)configured and was left as it was"
        );

        // Both landed — the only case allowed to be a plain success.
        assert_eq!(
            tunnel_outcome(Ok(()), RemoteConfig::Updated, "d1.agent.saisi.online"),
            "ok (d1.agent.saisi.online)"
        );

        // THE CASE THIS EXISTS FOR: the local file is correct and the REMOTE config is not,
        // and a remote configuration OVERRIDES the local file. The operator gets the
        // consequence AND the cause; "it failed" without a reason is not actionable.
        let partial = tunnel_outcome(
            Ok(()),
            RemoteConfig::Failed("Cloudflare answered HTTP 403".into()),
            "d1.agent.saisi.online",
        );
        assert!(partial.starts_with("PARTIAL"), "{partial}");
        assert!(partial.contains("d1.agent.saisi.online"), "{partial}");
        assert!(
            partial.contains("Cloudflare answered HTTP 403"),
            "the cause must survive into the card: {partial}"
        );
        assert!(
            partial.contains("OVERRIDES"),
            "the consequence must be stated, not implied: {partial}"
        );

        // The two verdicts are independent and the WORSE one wins.
        let both = tunnel_outcome(
            Err("nope".into()),
            RemoteConfig::Failed("also nope".into()),
            "h",
        );
        assert!(both.starts_with("FAILED: "), "{both}");
        assert!(!both.contains("PARTIAL"), "{both}");
    }

    #[test]
    fn the_remote_verdict_is_passed_through_not_invented_at_the_call_site() {
        // `tunnel_outcome` is pure and tested, but a pure function only helps if the REAL
        // verdict reaches it. Passing a literal there is the round-111 defect (a discarded
        // verdict) wearing new clothes, and no behavioural test can see it: the caller does
        // network I/O, so the wiring is pinned at the source — the way this repo already pins
        // the boot-task contract and the pre-v2 path rule.
        let src = include_str!("tunnel.rs");
        let body = src
            .split("pub(crate) async fn provision_tunnel")
            .nth(1)
            .expect("provision_tunnel must still exist")
            .split("async fn update_remote_config")
            .next()
            .unwrap();
        let call = body
            .split("tunnel_outcome(install_tunnel_config")
            .nth(1)
            .expect("provision_tunnel must decide the outcome from install_tunnel_config's result")
            .split(';')
            .next()
            .unwrap();
        assert!(
            call.contains("remote"),
            "the verdict the API returned must REACH the card — passing a literal here is the \
             discarded-verdict defect again: tunnel_outcome(install_tunnel_config{call}"
        );
    }
}
