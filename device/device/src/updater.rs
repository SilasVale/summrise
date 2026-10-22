//! The device downloader: one signed document, one signed revocation list, one bounded artifact,
//! the existing stager.
//!
//! A device is told where the release document lives; it does not trust that address to be honest.
//! The document is parsed, its ed25519 signature is verified against the embedded release key, its
//! revocation list is fetched and verified, the channel and version policy is applied, and only then
//! is the artifact fetched. The artifact's sha256 is checked against the signed digest before
//! [`crate::update::stage`] sees the bytes, so a body that does not match the document never reaches
//! the canonical staged pair.
//!
//! All three bodies are bounded while they are read, not after: a response that is larger than its
//! cap is refused as soon as the next chunk would cross it. The same client, and therefore the same
//! connect and total timeouts, serves the document, the list and the artifact. A URL is fetched over
//! `https` unless its host is loopback, and a redirect must stay on the original request's host and
//! scheme: a TLS origin cannot hand the fetch to a plain-http or foreign host, and the chain is
//! capped by [`UpdateConfig::max_redirects`].

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ed25519_dalek::VerifyingKey;
use reqwest::redirect::Policy;
use reqwest::{Client, Url};
use summrise_core::manifest::Manifest;
use summrise_core::RolledBackArtifact;
use summrise_release::{Channel, ReleaseDocument, ReleaseError, RevocationError, Revocations};

use crate::update::{self, StageError, StagedRelease};

/// The default cap on the release document's size.
pub const DEFAULT_MAX_DOCUMENT_BYTES: u64 = 1024 * 1024;

/// The default cap on the release artifact's size (the Windows app binary).
pub const DEFAULT_MAX_ARTIFACT_BYTES: u64 = 256 * 1024 * 1024;

/// The default time allowed to establish the TCP (and TLS) connection.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// The default time allowed for one request, body included.
pub const DEFAULT_TOTAL_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// The default number of redirects one request may follow.
pub const DEFAULT_MAX_REDIRECTS: u8 = 5;

/// The downloader's bounds.
///
/// These are arguments rather than constants so a caller (and a test) can choose a cap or a
/// timeout smaller than the production value; the production values are [`Default`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpdateConfig {
    /// The largest release document this device will read.
    pub max_document_bytes: u64,
    /// The largest artifact this device will read.
    pub max_artifact_bytes: u64,
    /// How long one connection attempt may take.
    pub connect_timeout: Duration,
    /// How long one request may take, body included.
    pub total_timeout: Duration,
    /// How many redirects one request may follow.
    pub max_redirects: u8,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            max_document_bytes: DEFAULT_MAX_DOCUMENT_BYTES,
            max_artifact_bytes: DEFAULT_MAX_ARTIFACT_BYTES,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            total_timeout: DEFAULT_TOTAL_TIMEOUT,
            max_redirects: DEFAULT_MAX_REDIRECTS,
        }
    }
}

/// What this device remembers about artifacts that failed here, and the operator's one override.
///
/// The decision is keyed on the artifact digest, not the version: a rebuilt release with the same
/// version is a different artifact and is allowed. A digest in `rolled_back` is refused before the
/// artifact is downloaded; `force_reinstall` names the one digest an operator has explicitly allowed
/// again after fixing the cause. The caller audits an override; this type only decides.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReinstallPolicy<'a> {
    /// Artifacts the supervisor rolled back on this device, newest last.
    pub rolled_back: &'a [RolledBackArtifact],
    /// The exact digest the operator has allowed to be installed again, if any.
    pub force_reinstall: Option<&'a str>,
}

/// Why a release could not be downloaded or staged.
#[derive(Debug)]
pub enum UpdateError {
    /// The document URL or the artifact URL is not an absolute http(s) URL.
    InvalidUrl(String),
    /// The build's embedded release key cannot verify anything (a placeholder or malformed key).
    Key(update::KeyError),
    /// The HTTP request failed: DNS, connect, TLS, the total timeout, or an interrupted body.
    Http(reqwest::Error),
    /// The server answered 404 for the document, the artifact, or the revocation list.
    NotFound(String),
    /// The server answered a status outside 2xx (404 is [`UpdateError::NotFound`]; a revocation
    /// list's 403 is treated as absence by `is_missing_revocations`).
    Status {
        /// The URL that answered.
        url: String,
        /// The status code, as a number.
        status: u16,
    },
    /// A redirect chain was longer than [`UpdateConfig::max_redirects`].
    TooManyRedirects(u8),
    /// A redirect pointed at a scheme other than the one the request started on.
    RedirectScheme(String),
    /// A redirect pointed at a host other than the one the request started on.
    RedirectHost {
        /// The host the request started on.
        from: String,
        /// The host the redirect named.
        to: String,
    },
    /// The release document was larger than [`UpdateConfig::max_document_bytes`].
    DocumentTooLarge(u64),
    /// The artifact was larger than [`UpdateConfig::max_artifact_bytes`].
    ArtifactTooLarge(u64),
    /// The document is not JSON this build can read.
    Parse(serde_json::Error),
    /// The document's signature, or a signed fact's shape, is invalid.
    Verify(ReleaseError),
    /// The signed document is genuine but this device must not install it.
    Refused(ReleaseError),
    /// The revocations list was larger than the document's byte cap.
    RevocationsTooLarge(u64),
    /// The revocations list at its URL is not JSON this build can read.
    RevocationsParse(serde_json::Error),
    /// The revocations list is genuine but unusable: its signature, shape or age is wrong.
    Revocations(RevocationError),
    /// The signed list withdraws exactly this release.
    Revoked {
        /// The withdrawn version.
        version: String,
        /// The withdrawn artifact digest the document names.
        sha256: String,
        /// Why the list withdraws it, when the operator wrote one.
        reason: Option<String>,
    },
    /// This exact artifact rolled back on this device; an operator must override it by digest.
    RolledBack {
        /// The version the failed artifact claimed.
        version: String,
        /// The digest the document names (and the failed artifact had).
        sha256: String,
        /// Millisecond Unix time the supervisor answered the rollback.
        at_ms: u64,
    },
    /// The artifact body does not hash to the digest the document signed.
    Sha {
        /// The signed digest the body failed to match.
        expected: String,
    },
    /// The verified artifact could not be staged (usually an I/O failure).
    Stage(StageError),
    /// The running executable has no directory to stage beside.
    Io(io::Error),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpdateError::InvalidUrl(url) => write!(
                f,
                "the release URL {url:?} is not an absolute http or https URL"
            ),
            UpdateError::Key(error) => write!(f, "refusing the update: {error}"),
            UpdateError::Http(error) => write!(f, "the release could not be fetched: {error}"),
            UpdateError::NotFound(url) => write!(f, "the release was not found at {url} (404)"),
            UpdateError::Status { url, status } => {
                write!(f, "the release URL {url} answered HTTP {status}")
            }
            UpdateError::TooManyRedirects(max) => {
                write!(f, "the release URL redirected more than {max} times")
            }
            UpdateError::RedirectScheme(scheme) => write!(
                f,
                "the release URL redirected to the {scheme:?} scheme, not the scheme it started on"
            ),
            UpdateError::RedirectHost { from, to } => write!(
                f,
                "the release URL redirected from host {from} to a different host {to}"
            ),
            UpdateError::DocumentTooLarge(limit) => write!(
                f,
                "the release document is larger than the {limit}-byte cap"
            ),
            UpdateError::ArtifactTooLarge(limit) => {
                write!(
                    f,
                    "the release artifact is larger than the {limit}-byte cap"
                )
            }
            UpdateError::Parse(error) => write!(f, "the release document is not readable: {error}"),
            UpdateError::Verify(error) => write!(f, "the release document was refused: {error}"),
            UpdateError::Refused(error) => write!(f, "this device refuses the release: {error}"),
            UpdateError::RevocationsTooLarge(limit) => write!(
                f,
                "the revocations list is larger than the {limit}-byte cap"
            ),
            UpdateError::RevocationsParse(error) => {
                write!(f, "the revocations list is not readable: {error}")
            }
            UpdateError::Revocations(error) => {
                write!(f, "the revocations list was refused: {error}")
            }
            UpdateError::Revoked {
                version,
                sha256,
                reason,
            } => {
                let reason = reason
                    .as_deref()
                    .filter(|reason| !reason.is_empty())
                    .unwrap_or("no reason given");
                write!(
                    f,
                    "the release {version} (sha256 {sha256}) is revoked: {reason}"
                )
            }
            UpdateError::RolledBack {
                version,
                sha256,
                at_ms,
            } => write!(
                f,
                "the release {version} (sha256 {sha256}) rolled back on this device at {}; \
                 install it again only with --force-reinstall {sha256} after fixing the cause",
                crate::store::format_utc(at_ms / 1_000)
            ),
            UpdateError::Sha { expected } => {
                write!(
                    f,
                    "the artifact does not hash to the signed sha256 {expected}"
                )
            }
            UpdateError::Stage(error) => write!(f, "the release could not be staged: {error}"),
            UpdateError::Io(error) => write!(f, "the install directory is unknown: {error}"),
        }
    }
}

impl std::error::Error for UpdateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            UpdateError::Key(error) => Some(error),
            UpdateError::Http(error) => Some(error),
            UpdateError::Parse(error) => Some(error),
            UpdateError::Verify(error) | UpdateError::Refused(error) => Some(error),
            UpdateError::RevocationsParse(error) => Some(error),
            UpdateError::Revocations(error) => Some(error),
            UpdateError::Stage(error) => Some(error),
            UpdateError::Io(error) => Some(error),
            _ => None,
        }
    }
}

/// Why the redirect policy stopped a request.
///
/// The policy runs inside `reqwest`; its error comes back as a `reqwest::Error` whose source is this
/// value, so [`map_http_error`] can name the limit, the scheme or the host instead of reporting an
/// opaque transport failure.
#[derive(Debug)]
enum RedirectRefusal {
    /// The chain was longer than the configured cap.
    TooMany(u8),
    /// The next URL changed the scheme.
    Scheme(String),
    /// The next URL changed the host.
    Host {
        /// The host the request started on.
        from: String,
        /// The host the redirect named.
        to: String,
    },
}

impl fmt::Display for RedirectRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RedirectRefusal::TooMany(max) => {
                write!(f, "the release URL redirected more than {max} times")
            }
            RedirectRefusal::Scheme(scheme) => write!(
                f,
                "the release URL redirected to the {scheme:?} scheme, not the scheme it started on"
            ),
            RedirectRefusal::Host { from, to } => write!(
                f,
                "the release URL redirected from host {from} to a different host {to}"
            ),
        }
    }
}

impl std::error::Error for RedirectRefusal {}

/// Recover this crate's typed redirect refusal from a `reqwest::Error`, if that is what it was.
fn map_http_error(error: reqwest::Error) -> UpdateError {
    if error.is_redirect() {
        if let Some(source) = std::error::Error::source(&error) {
            if let Some(refusal) = source.downcast_ref::<RedirectRefusal>() {
                return match refusal {
                    RedirectRefusal::TooMany(max) => UpdateError::TooManyRedirects(*max),
                    RedirectRefusal::Scheme(scheme) => UpdateError::RedirectScheme(scheme.clone()),
                    RedirectRefusal::Host { from, to } => UpdateError::RedirectHost {
                        from: from.clone(),
                        to: to.clone(),
                    },
                };
            }
        }
    }
    UpdateError::Http(error)
}

/// The install root a [`check_update`] call stages beside: the directory the running app lives in.
///
/// This is the same rule the hidden `--stage` mode uses when `--install` is absent: an installed
/// app always lives at `<install>/summrise-device.exe`.
pub(crate) fn install_root() -> Result<PathBuf, UpdateError> {
    std::env::current_exe()
        .map_err(UpdateError::Io)?
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            UpdateError::Io(io::Error::other(
                "the running executable has no parent directory",
            ))
        })
}

/// What one completed check decided.
///
/// "Nothing to do" is an answer, not an error: the document verified, and it names the release this
/// device already runs. The caller reports `up_to_date <version>` and exits 0 without asking the
/// supervisor for a swap, so re-running a check is a safe no-op.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckOutcome {
    /// The document names the installed version. Nothing was fetched beyond the document and
    /// nothing on disk changed.
    UpToDate,
    /// The artifact is staged and the canonical pair is ready for the supervisor.
    Staged(StagedRelease),
}

/// Fetch, verify and stage the release document at `url`.
///
/// The install root is the directory the running app lives in ([`install_root`]); the release key
/// is the build's embedded key ([`crate::update::release_pubkey`]). This is the production entry
/// point; [`check_update_with`] is its explicit-key, explicit-install half.
pub async fn check_update(
    url: &str,
    revocations_url: Option<&str>,
    channel: Channel,
    installed: &str,
    cfg: &UpdateConfig,
    policy: ReinstallPolicy<'_>,
) -> Result<CheckOutcome, UpdateError> {
    let pubkey = update::release_pubkey().map_err(UpdateError::Key)?;
    let install = install_root()?;
    check_update_with(
        url,
        revocations_url,
        channel,
        installed,
        &install,
        cfg,
        policy,
        &pubkey,
    )
    .await
}

/// The testable and CLI half of [`check_update`]: the install root and the key are arguments.
///
/// `revocations_url` is the operator's `--revocations`; when it is `None` the list is the sibling
/// `revocations.json` of the document's **artifactUrl** directory. The artifact is the thing a
/// release publishes, so its directory is where the release's own withdrawal notice belongs; a
/// document may live on a console or metadata host that never sees the artifact bytes. The explicit
/// URL exists for the case where the list must live on a third host.
#[allow(clippy::too_many_arguments)] // the update's seams: URL, list, channel, version, install, bounds, policy, key
pub(crate) async fn check_update_with(
    url: &str,
    revocations_url: Option<&str>,
    channel: Channel,
    installed: &str,
    install: &Path,
    cfg: &UpdateConfig,
    policy: ReinstallPolicy<'_>,
    pubkey: &VerifyingKey,
) -> Result<CheckOutcome, UpdateError> {
    let client = build_client(cfg)?;
    let document_url = http_url(url)?;
    let explicit_list_url = revocations_url.map(http_url).transpose()?;
    let document_bytes = get_body(
        &client,
        &document_url,
        cfg.max_document_bytes,
        UpdateError::DocumentTooLarge,
    )
    .await?;
    let document: ReleaseDocument =
        serde_json::from_slice(&document_bytes).map_err(UpdateError::Parse)?;

    // The signature proves who signed the release; the channel and version policy decides whether
    // this device is its audience. Both are settled before the artifact is requested, so a forged
    // or wrong-channel document costs one small GET and no bandwidth.
    summrise_release::verify(&document, pubkey).map_err(UpdateError::Verify)?;

    // The revocation list is checked before the policy answer and before the artifact download: a
    // withdrawn release must not be staged even if it is newer than the installed version, and a
    // malformed list found at its URL is a hard failure rather than "nothing revoked". A 404 and a
    // 403 both mean the list does not exist (a bucket that refuses GET on an absent key answers
    // 403); every other status and every parse error refuses the update.
    let list_url = match explicit_list_url {
        Some(url) => url,
        None => sibling_revocations_url(&document.artifact_url)?,
    };
    if let Some(list) = fetch_revocations(&client, &list_url, cfg, pubkey).await? {
        if let Some(entry) = list.revoked_release(&document.version, &document.sha256) {
            return Err(UpdateError::Revoked {
                version: document.version.clone(),
                sha256: document.sha256.clone(),
                reason: entry.reason.clone(),
            });
        }
    }

    match summrise_release::select(&document, channel, installed).map_err(UpdateError::Refused)? {
        summrise_release::Selection::UpToDate => return Ok(CheckOutcome::UpToDate),
        summrise_release::Selection::Update => {}
    }

    // The artifact-level safeguard: this exact build failed here once, so a retry is refused before
    // the body is downloaded. The digest, not the version, is the identity — a rebuilt release with
    // the same version is a different artifact. `--force-reinstall <sha256>` names the one digest an
    // operator has explicitly allowed again.
    if let Some(artifact) = policy
        .rolled_back
        .iter()
        .find(|artifact| artifact.sha256.eq_ignore_ascii_case(&document.sha256))
    {
        let forced = policy
            .force_reinstall
            .is_some_and(|sha256| sha256.eq_ignore_ascii_case(&document.sha256));
        if !forced {
            return Err(UpdateError::RolledBack {
                version: artifact.version.clone(),
                sha256: document.sha256.clone(),
                at_ms: artifact.at_ms,
            });
        }
    }

    let artifact_url = http_url(&document.artifact_url)?;
    let artifact = get_body(
        &client,
        &artifact_url,
        cfg.max_artifact_bytes,
        UpdateError::ArtifactTooLarge,
    )
    .await?;
    let digest = summrise_release::sha256_hex(&artifact);
    if !digest.eq_ignore_ascii_case(&document.sha256) {
        return Err(UpdateError::Sha {
            expected: document.sha256.clone(),
        });
    }

    update::stage(&artifact, &manifest_of(&document), pubkey, install)
        .map(CheckOutcome::Staged)
        .map_err(UpdateError::Stage)
}

/// The stager's view of a verified document.
///
/// The stager verifies the older three-part message, so it receives `artifactSignature`, not the
/// policy signature `verify` checked above.
fn manifest_of(document: &ReleaseDocument) -> Manifest {
    Manifest {
        version: document.version.clone(),
        channel: document.channel.as_str().to_owned(),
        sha256: document.sha256.clone(),
        signature: document.artifact_signature.clone(),
        min_version: document.min_version.clone(),
    }
}

/// Build the one HTTP client both requests use, with the configured timeouts and redirect rule.
///
/// A redirect must keep the scheme and the host of the URL the request started on. A CDN may hand
/// off to another path on the same origin, but not to a foreign host and never from `https` to
/// `http`; the count is capped by [`UpdateConfig::max_redirects`]. A refusal is raised from inside
/// the policy so the error is typed rather than discovered after a hop was already taken.
fn build_client(cfg: &UpdateConfig) -> Result<Client, UpdateError> {
    let max_redirects = cfg.max_redirects;
    let policy = Policy::custom(move |attempt| {
        match redirect_refusal(attempt.previous(), attempt.url(), max_redirects) {
            Some(refusal) => attempt.error(refusal),
            None => attempt.follow(),
        }
    });

    Client::builder()
        .connect_timeout(cfg.connect_timeout)
        .timeout(cfg.total_timeout)
        .redirect(policy)
        .build()
        .map_err(UpdateError::Http)
}

/// The redirect rule, separated from `reqwest` so it can be tested without a TLS server.
///
/// `previous` is the chain so far and includes the URL the request started on; `next` is the
/// redirect target. The first URL is the origin every later URL is compared against.
fn redirect_refusal(previous: &[Url], next: &Url, max_redirects: u8) -> Option<RedirectRefusal> {
    if previous.len() > usize::from(max_redirects) {
        return Some(RedirectRefusal::TooMany(max_redirects));
    }
    let origin = previous.first()?;
    if next.scheme() != origin.scheme() {
        return Some(RedirectRefusal::Scheme(next.scheme().to_owned()));
    }
    let from = origin.host_str().unwrap_or_default();
    let to = next.host_str().unwrap_or_default();
    if to != from {
        return Some(RedirectRefusal::Host {
            from: from.to_owned(),
            to: to.to_owned(),
        });
    }
    None
}

/// Parse a URL this device is allowed to fetch: absolute, https unless loopback, with a host.
pub(crate) fn http_url(text: &str) -> Result<Url, UpdateError> {
    summrise_release::parse_release_url(text).map_err(|_| UpdateError::InvalidUrl(text.to_owned()))
}

/// The default list URL: `revocations.json` in the artifact URL's directory.
///
/// `Url::join` replaces the artifact's last path segment, keeps the scheme and host (including the
/// loopback-http exception), and drops any query or fragment. The result is then fetched with the
/// same client, redirect rule and byte cap as the document, exactly as if the operator had passed it
/// with `--revocations`.
fn sibling_revocations_url(artifact_url: &str) -> Result<Url, UpdateError> {
    let artifact = http_url(artifact_url)?;
    artifact
        .join("revocations.json")
        .map_err(|_| UpdateError::InvalidUrl(artifact_url.to_owned()))
}

/// Whether the list's fetch means "not published": a 404 (no object) or a 403 (no GET permission
/// on the absent key). Every other non-2xx is a real failure, not an absence.
fn is_missing_revocations(error: &UpdateError) -> bool {
    matches!(
        error,
        UpdateError::NotFound(_) | UpdateError::Status { status: 403, .. }
    )
}

/// Fetch and verify the revocation list, treating a 403 or 404 as "nothing revoked".
///
/// A missing list is logged and the update continues: most releases have nothing withdrawn. Both
/// statuses are the absence of the object in practice: an object store answers 404 for a missing key
/// and 403 when it will not GET one. Every other non-2xx (a 5xx, a 429, a bad redirect) is a hard
/// failure with the status in the error. A body that is present but unreadable, too large, badly
/// signed, stale or malformed is returned as an error and the caller stages nothing — a broken list
/// is not the same fact as no list.
async fn fetch_revocations(
    client: &Client,
    url: &Url,
    cfg: &UpdateConfig,
    pubkey: &VerifyingKey,
) -> Result<Option<Revocations>, UpdateError> {
    let body = match get_body(
        client,
        url,
        cfg.max_document_bytes,
        UpdateError::RevocationsTooLarge,
    )
    .await
    {
        Ok(body) => body,
        Err(error) if is_missing_revocations(&error) => {
            eprintln!(
                "summrise-device: no revocations list at {url}; treating the release as not revoked"
            );
            return Ok(None);
        }
        Err(error) => return Err(error),
    };

    let list: Revocations = serde_json::from_slice(&body).map_err(UpdateError::RevocationsParse)?;
    list.verify(pubkey).map_err(UpdateError::Revocations)?;
    Ok(Some(list))
}

/// GET `url`, refuse any non-success status, and read the body up to `limit` bytes.
///
/// The cap is applied while the body is read, not to a buffer that was already accumulated. A
/// declared content length over the cap is refused before the first byte; a chunked body is refused
/// as soon as the next chunk would cross the cap. `over` names the error for the caller's cap.
async fn get_body(
    client: &Client,
    url: &Url,
    limit: u64,
    over: fn(u64) -> UpdateError,
) -> Result<Vec<u8>, UpdateError> {
    let mut response = client
        .get(url.clone())
        .send()
        .await
        .map_err(map_http_error)?;
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(UpdateError::NotFound(url.to_string()));
    }
    if !status.is_success() {
        return Err(UpdateError::Status {
            url: url.to_string(),
            status: status.as_u16(),
        });
    }
    if let Some(length) = response.content_length() {
        if length > limit {
            return Err(over(limit));
        }
    }

    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(UpdateError::Http)? {
        let next = body.len() as u64 + chunk.len() as u64;
        if next > limit {
            return Err(over(limit));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::Ordering;

    use axum::extract::{Path as AxumPath, State};
    use axum::http::StatusCode;
    use axum::response::Redirect;
    use axum::{routing::get, Json, Router};
    use ed25519_dalek::SigningKey;
    use std::sync::atomic::AtomicUsize;
    use std::sync::Arc;

    use summrise_release::sign::{sign_document, SignRequest};
    use summrise_release::{RevocationError, Revocations, MAX_REVOCATIONS_AGE_MS};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use crate::test_support::{serve_release, TempDir};
    use crate::update::fixtures::TEST_SEED;

    use super::*;

    /// A signed revocation list from literal `(version, sha256, reason)` entries.
    fn revocation_list(
        key: &SigningKey,
        issued_at_ms: u64,
        entries: &[(&str, &str, Option<&str>)],
    ) -> Revocations {
        let revoked = entries
            .iter()
            .map(
                |(version, sha256, reason)| summrise_release::RevokedRelease {
                    version: (*version).to_owned(),
                    sha256: (*sha256).to_owned(),
                    reason: reason.map(str::to_owned),
                },
            )
            .collect();
        Revocations::sign(key, issued_at_ms, revoked).expect("the fixture list signs")
    }

    /// How the fixture answers the revocations route.
    enum ListFixture {
        /// No route at all, so the server answers 404.
        Absent,
        /// A 200 with these bytes.
        Body { path: String, bytes: Vec<u8> },
        /// A non-success status with no body (403 for an absent bucket key, 500 for a broken host).
        Status { path: String, status: StatusCode },
    }

    /// Serve one document, its artifact, and a revocations route described by `list`.
    ///
    /// The artifact URL is `{base}/artifact`, so the list's sibling default is
    /// `{base}/revocations.json`; a test can place it elsewhere and pass the explicit URL instead.
    async fn serve_release_with_list(
        key: &SigningKey,
        version: &str,
        channel: Channel,
        min_version: &str,
        artifact: Vec<u8>,
        list: ListFixture,
    ) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback listener");
        let base = format!("http://{}", listener.local_addr().expect("an address"));
        let artifact_url = format!("{base}/artifact");
        let document = sign_document(
            key,
            &artifact,
            &SignRequest {
                version,
                channel,
                artifact_url: &artifact_url,
                min_version,
                issued_at_ms: summrise_release::now_ms(),
            },
        )
        .expect("the fixture document signs");
        let hits = Arc::new(AtomicUsize::new(0));
        let handler_hits = Arc::clone(&hits);
        let app = Router::new()
            .route(
                "/release.json",
                get(move || {
                    let document = document.clone();
                    async move { Json(document) }
                }),
            )
            .route(
                "/artifact",
                get(move || {
                    handler_hits.fetch_add(1, Ordering::SeqCst);
                    let artifact = artifact.clone();
                    async move { artifact }
                }),
            );
        let app = match list {
            ListFixture::Body { path, bytes } => app.route(
                &path,
                get(move || {
                    let bytes = bytes.clone();
                    async move { bytes }
                }),
            ),
            ListFixture::Status { path, status } => {
                app.route(&path, get(move || async move { status }))
            }
            ListFixture::Absent => app,
        };
        let _handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        (base, hits)
    }

    // Not covered: a real connect or total timeout. Exercising either costs its configured wall
    // time, and the brief's test list did not include one; the builder calls in `build_client` are
    // the only unsampled lines.

    /// Serve one document and nothing else; the artifact URL can point anywhere.
    async fn serve_document(document: ReleaseDocument) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback listener");
        let base = format!("http://{}", listener.local_addr().expect("an address"));
        let app = Router::new().route(
            "/release.json",
            get(move || {
                let document = document.clone();
                async move { Json(document) }
            }),
        );
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("{base}/release.json"), handle)
    }

    /// Stream a chunked body larger than any cap, one chunk at a time, and report how many body
    /// bytes the server actually wrote before the client closed the connection.
    ///
    /// The pause between chunks is what makes the count meaningful: a client that reads the whole
    /// body lets the server write all of it, while a client that refuses at the cap closes the
    /// socket and the next write fails long before `total` is reached.
    async fn serve_streaming_body(total: usize) -> (String, tokio::task::JoinHandle<usize>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback listener");
        let address = listener.local_addr().expect("an address");
        let handle = tokio::spawn(async move {
            let Ok((mut socket, _)) = listener.accept().await else {
                return 0;
            };
            // Read the request before answering it. A socket closed with unread request bytes is
            // reset by the kernel, and that reset can discard the response the client has not read
            // yet; reading first makes the close a FIN and the test measure the cap, not TCP.
            let mut request = Vec::new();
            let mut buffer = [0u8; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                match socket.read(&mut buffer).await {
                    Ok(0) | Err(_) => return 0,
                    Ok(read) => request.extend_from_slice(&buffer[..read]),
                }
            }
            if socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n\
                      transfer-encoding: chunked\r\nconnection: close\r\n\r\n",
                )
                .await
                .is_err()
            {
                return 0;
            }
            let chunk = vec![b'x'; 8 * 1024];
            let mut written = 0;
            while written < total {
                let frame = format!("{:x}\r\n", chunk.len());
                if socket.write_all(frame.as_bytes()).await.is_err()
                    || socket.write_all(&chunk).await.is_err()
                    || socket.write_all(b"\r\n").await.is_err()
                {
                    break;
                }
                written += chunk.len();
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            written
        });
        (format!("http://{address}/stream"), handle)
    }

    /// The redirect chain's shared state: the port to return to, and how many hops it has.
    #[derive(Clone, Copy)]
    struct HopState {
        port: u16,
        redirects: u8,
        cross_host: bool,
    }

    /// One link of a chain that changes host (or not) and then lands on the document.
    async fn hop(AxumPath(step): AxumPath<u8>, State(state): State<HopState>) -> Redirect {
        if step >= state.redirects {
            return Redirect::temporary(&format!("http://127.0.0.1:{}/release.json", state.port));
        }
        let host = if state.cross_host && step % 2 == 1 {
            "127.0.0.2"
        } else {
            "127.0.0.1"
        };
        Redirect::temporary(&format!("http://{host}:{}/hop/{}", state.port, step + 1))
    }

    /// Serve a document whose first URL is `/hop/1`; each hop changes host when `cross_host` is set,
    /// then lands on the document. `redirects` is both the number of hops and the number of redirects
    /// followed. `cross_host` is false on Windows, where 127.0.0.2 is not portable.
    async fn serve_redirects(
        key: &SigningKey,
        redirects: u8,
        artifact: Vec<u8>,
        cross_host: bool,
    ) -> (String, tokio::task::JoinHandle<()>) {
        // Binding every loopback address makes 127.0.0.1 and 127.0.0.2 the same server, so a
        // redirect can genuinely change the host. The portable path binds one address.
        let address = if cross_host {
            "0.0.0.0:0"
        } else {
            "127.0.0.1:0"
        };
        let listener = TcpListener::bind(address)
            .await
            .expect("a loopback listener");
        let port = listener.local_addr().expect("an address").port();
        let artifact_url = format!("http://127.0.0.1:{port}/artifact");
        let document = sign_document(
            key,
            &artifact,
            &SignRequest {
                version: "2.0.3",
                channel: Channel::Stable,
                artifact_url: &artifact_url,
                min_version: "2.0.0",
                issued_at_ms: summrise_release::now_ms(),
            },
        )
        .expect("the fixture document signs");
        let app = Router::new()
            .route(
                "/release.json",
                get(move || {
                    let document = document.clone();
                    async move { Json(document) }
                }),
            )
            .route("/artifact", get(move || async move { artifact }))
            .route("/hop/:step", get(hop))
            .with_state(HopState {
                port,
                redirects,
                cross_host,
            });
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://127.0.0.1:{port}/hop/1"), handle)
    }

    #[tokio::test]
    async fn a_valid_document_and_artifact_are_staged() {
        let dir = TempDir::new("check-update-valid");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"the release artifact".to_vec();
        let server = serve_release(&key, "2.0.3", Channel::Stable, "2.0.0", artifact.clone()).await;

        let outcome = check_update_with(
            &server.document_url,
            None,
            Channel::Stable,
            "2.0.2",
            &install,
            &UpdateConfig::default(),
            ReinstallPolicy::default(),
            &key.verifying_key(),
        )
        .await
        .expect("a valid release is staged");
        let CheckOutcome::Staged(staged) = outcome else {
            panic!("a newer release must stage, not report up to date: {outcome:?}");
        };

        assert_eq!(staged.version, "2.0.3");
        assert_eq!(staged.sha256, summrise_release::sha256_hex(&artifact));
        assert_eq!(
            fs::read(install.join(update::STAGED_EXE)).expect("the artifact is staged"),
            artifact
        );
        let sidecar: summrise_launch::Sidecar = serde_json::from_slice(
            &fs::read(install.join(update::STAGED_SIDECAR)).expect("the sidecar is staged"),
        )
        .expect("the sidecar is the launcher's type");
        assert_eq!(sidecar.version, staged.version);
        assert_eq!(sidecar.sha256, staged.sha256);
        assert_eq!(sidecar.channel, "stable");
        assert_eq!(
            server.artifact_hits.load(Ordering::SeqCst),
            1,
            "one artifact download, after the document verified"
        );
    }

    #[tokio::test]
    async fn a_bad_signature_is_refused_before_the_artifact_is_fetched() {
        let dir = TempDir::new("check-update-bad-signature");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let other = SigningKey::from_bytes(&[8u8; 32]);
        let artifact = b"the release artifact".to_vec();
        let server = serve_release(&key, "2.0.3", Channel::Stable, "2.0.0", artifact).await;

        let result = check_update_with(
            &server.document_url,
            None,
            Channel::Stable,
            "2.0.2",
            &install,
            &UpdateConfig::default(),
            ReinstallPolicy::default(),
            &other.verifying_key(),
        )
        .await;

        assert!(
            matches!(result, Err(UpdateError::Verify(ReleaseError::Signature))),
            "{result:?}"
        );
        assert_eq!(
            server.artifact_hits.load(Ordering::SeqCst),
            0,
            "a document that does not verify must not cause an artifact download"
        );
        assert!(
            !install.exists(),
            "a refused update must not create the install directory"
        );
    }

    #[tokio::test]
    async fn an_expired_document_is_refused_before_the_artifact_is_fetched() {
        let dir = TempDir::new("check-update-expired");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let document = sign_document(
            &key,
            b"the release artifact",
            &SignRequest {
                version: "2.0.3",
                channel: Channel::Stable,
                artifact_url: "https://example.test/summrise-device.exe",
                min_version: "2.0.0",
                issued_at_ms: summrise_release::now_ms()
                    .saturating_sub(summrise_release::MAX_ISSUED_AGE_MS + 1),
            },
        )
        .expect("the fixture document signs");
        let (document_url, server) = serve_document(document).await;

        let result = check_update_with(
            &document_url,
            None,
            Channel::Stable,
            "2.0.2",
            &install,
            &UpdateConfig::default(),
            ReinstallPolicy::default(),
            &key.verifying_key(),
        )
        .await;

        assert!(
            matches!(
                result,
                Err(UpdateError::Verify(ReleaseError::Expired { .. }))
            ),
            "{result:?}"
        );
        assert!(!install.exists());
        server.abort();
    }

    #[tokio::test]
    async fn a_missing_document_is_not_found() {
        let dir = TempDir::new("check-update-404");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback listener");
        let base = format!("http://{}", listener.local_addr().expect("an address"));
        let app = Router::new().route(
            "/missing",
            get(|| async { axum::http::StatusCode::NOT_FOUND }),
        );
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let url = format!("{base}/missing");

        let result = check_update_with(
            &url,
            None,
            Channel::Stable,
            "2.0.2",
            &install,
            &UpdateConfig::default(),
            ReinstallPolicy::default(),
            &key.verifying_key(),
        )
        .await;

        assert!(
            matches!(result, Err(UpdateError::NotFound(ref missing)) if missing == &url),
            "{result:?}"
        );
        assert!(!install.exists());
        handle.abort();
    }

    #[tokio::test]
    async fn a_stable_device_refuses_a_canary_document() {
        let dir = TempDir::new("check-update-canary");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"the canary artifact".to_vec();
        let server = serve_release(&key, "2.0.3", Channel::Canary, "2.0.0", artifact).await;

        let result = check_update_with(
            &server.document_url,
            None,
            Channel::Stable,
            "2.0.2",
            &install,
            &UpdateConfig::default(),
            ReinstallPolicy::default(),
            &key.verifying_key(),
        )
        .await;

        assert!(
            matches!(result, Err(UpdateError::Refused(ReleaseError::Channel))),
            "{result:?}"
        );
        assert_eq!(
            server.artifact_hits.load(Ordering::SeqCst),
            0,
            "the channel is decided before the artifact is downloaded"
        );
        assert!(!install.exists());
    }

    #[tokio::test]
    async fn a_downgrade_is_refused_before_the_artifact_is_fetched() {
        let dir = TempDir::new("check-update-downgrade");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"an older release".to_vec();
        let server = serve_release(&key, "2.0.2", Channel::Stable, "2.0.0", artifact).await;

        let result = check_update_with(
            &server.document_url,
            None,
            Channel::Stable,
            "2.0.3",
            &install,
            &UpdateConfig::default(),
            ReinstallPolicy::default(),
            &key.verifying_key(),
        )
        .await;

        assert!(
            matches!(
                result,
                Err(UpdateError::Refused(ReleaseError::OlderThanInstalled))
            ),
            "{result:?}"
        );
        assert_eq!(server.artifact_hits.load(Ordering::SeqCst), 0);
        assert!(!install.exists());
    }

    #[tokio::test]
    async fn the_installed_version_is_up_to_date_and_stages_nothing() {
        let dir = TempDir::new("check-update-up-to-date");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"the release artifact".to_vec();
        let server = serve_release(&key, "2.0.3", Channel::Stable, "2.0.0", artifact).await;

        let outcome = check_update_with(
            &server.document_url,
            None,
            Channel::Stable,
            "2.0.3",
            &install,
            &UpdateConfig::default(),
            ReinstallPolicy::default(),
            &key.verifying_key(),
        )
        .await
        .expect("the document still verifies; it simply names the installed version");

        assert_eq!(
            outcome,
            CheckOutcome::UpToDate,
            "equal versions are an answer, not a downgrade error"
        );
        assert_eq!(
            server.artifact_hits.load(Ordering::SeqCst),
            0,
            "nothing to do means no artifact download"
        );
        assert!(
            !install.exists(),
            "up to date must not create the install directory"
        );
    }

    #[tokio::test]
    async fn a_document_over_the_cap_is_refused_while_reading() {
        let dir = TempDir::new("check-update-document-cap");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let (url, server) = serve_streaming_body(2 * 1024 * 1024).await;
        let cfg = UpdateConfig {
            max_document_bytes: 4 * 1024,
            ..UpdateConfig::default()
        };

        let result = tokio::time::timeout(
            Duration::from_secs(10),
            check_update_with(
                &url,
                None,
                Channel::Stable,
                "2.0.2",
                &install,
                &cfg,
                ReinstallPolicy::default(),
                &key.verifying_key(),
            ),
        )
        .await
        .expect("a capped read does not wait for the whole body");

        assert!(
            matches!(result, Err(UpdateError::DocumentTooLarge(4096))),
            "{result:?}"
        );
        let written = tokio::time::timeout(Duration::from_secs(10), server)
            .await
            .expect("the streaming server stops")
            .expect("the streaming task did not panic");
        assert!(
            written < 512 * 1024,
            "the client must stop before the whole 2 MiB body is written; the server wrote {written} bytes"
        );
        assert!(!install.exists());
    }

    #[tokio::test]
    async fn an_artifact_over_the_cap_is_refused_while_reading() {
        let dir = TempDir::new("check-update-artifact-cap");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let (artifact_url, server) = serve_streaming_body(2 * 1024 * 1024).await;
        let document = sign_document(
            &key,
            b"the bytes the document promises",
            &SignRequest {
                version: "2.0.3",
                channel: Channel::Stable,
                artifact_url: &artifact_url,
                min_version: "2.0.0",
                issued_at_ms: summrise_release::now_ms(),
            },
        )
        .expect("the fixture document signs");
        let (document_url, document_server) = serve_document(document).await;
        // The artifact's sibling would land on the streaming fixture, whose whole purpose is to
        // answer any GET with an oversized body. Point the list at the document server, which
        // answers 404 for everything but /release.json: "no list" is the point of this test.
        let no_list = document_url.replace("release.json", "revocations.json");
        let cfg = UpdateConfig {
            max_artifact_bytes: 4 * 1024,
            ..UpdateConfig::default()
        };

        let result = tokio::time::timeout(
            Duration::from_secs(10),
            check_update_with(
                &document_url,
                Some(&no_list),
                Channel::Stable,
                "2.0.2",
                &install,
                &cfg,
                ReinstallPolicy::default(),
                &key.verifying_key(),
            ),
        )
        .await
        .expect("a capped read does not wait for the whole body");

        assert!(
            matches!(result, Err(UpdateError::ArtifactTooLarge(4096))),
            "{result:?}"
        );
        let written = tokio::time::timeout(Duration::from_secs(10), server)
            .await
            .expect("the streaming server stops")
            .expect("the streaming task did not panic");
        assert!(
            written < 512 * 1024,
            "the client must stop before the whole 2 MiB body is written; the server wrote {written} bytes"
        );
        assert!(!install.exists());
        document_server.abort();
    }

    #[tokio::test]
    async fn same_host_redirects_are_followed_up_to_the_cap() {
        let dir = TempDir::new("check-update-redirects-same-host");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"the release artifact".to_vec();
        let (url, server) = serve_redirects(&key, 3, artifact.clone(), false).await;
        let cfg = UpdateConfig {
            max_redirects: 3,
            ..UpdateConfig::default()
        };

        let outcome = check_update_with(
            &url,
            None,
            Channel::Stable,
            "2.0.2",
            &install,
            &cfg,
            ReinstallPolicy::default(),
            &key.verifying_key(),
        )
        .await
        .expect("three redirects are within the cap");
        let CheckOutcome::Staged(staged) = outcome else {
            panic!("a newer release must stage, not report up to date: {outcome:?}");
        };

        assert_eq!(staged.version, "2.0.3");
        assert_eq!(
            fs::read(install.join(update::STAGED_EXE)).expect("the artifact is staged"),
            artifact
        );
        server.abort();
    }

    // 127.0.0.2 is not portable to Windows; the refusal is still the portable rule, so this test
    // proves it on the platform where the second loopback address exists.
    #[cfg(unix)]
    #[tokio::test]
    async fn redirects_to_another_host_are_refused() {
        let dir = TempDir::new("check-update-redirects-cross-host");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"the release artifact".to_vec();
        let (url, server) = serve_redirects(&key, 3, artifact, true).await;

        let result = check_update_with(
            &url,
            None,
            Channel::Stable,
            "2.0.2",
            &install,
            &UpdateConfig::default(),
            ReinstallPolicy::default(),
            &key.verifying_key(),
        )
        .await;

        assert!(
            matches!(
                result,
                Err(UpdateError::RedirectHost { ref from, ref to })
                    if from == "127.0.0.1" && to == "127.0.0.2"
            ),
            "{result:?}"
        );
        assert!(!install.exists());
        server.abort();
    }

    #[test]
    fn the_redirect_rule_keeps_the_origin_scheme_and_host() {
        let https = Url::parse("https://example.test/release.json").expect("a URL");
        let same = Url::parse("https://example.test/other.json").expect("a URL");
        let downgrade = Url::parse("http://example.test/release.json").expect("a URL");
        let foreign = Url::parse("https://cdn.example.test/release.json").expect("a URL");
        let previous = std::slice::from_ref(&https);

        assert!(redirect_refusal(previous, &same, 5).is_none());
        assert!(matches!(
            redirect_refusal(previous, &downgrade, 5),
            Some(RedirectRefusal::Scheme(ref scheme)) if scheme == "http"
        ));
        assert!(matches!(
            redirect_refusal(previous, &foreign, 5),
            Some(RedirectRefusal::Host { ref from, ref to })
                if from == "example.test" && to == "cdn.example.test"
        ));
        assert!(matches!(
            redirect_refusal(previous, &same, 0),
            Some(RedirectRefusal::TooMany(0))
        ));
    }

    #[test]
    fn only_https_or_loopback_http_urls_are_fetchable() {
        for url in [
            "https://releases.example.test/release.json",
            "http://127.0.0.1:8080/release.json",
            "http://[::1]:8080/release.json",
        ] {
            assert!(http_url(url).is_ok(), "{url} must be fetchable");
        }

        for url in [
            "",
            "not a url",
            "ftp://releases.example.test/release.json",
            "http://releases.example.test/release.json",
        ] {
            assert!(
                matches!(http_url(url), Err(UpdateError::InvalidUrl(ref refused)) if refused == url),
                "{url:?} must be refused"
            );
        }
    }

    #[tokio::test]
    async fn a_redirect_past_the_cap_is_refused() {
        let dir = TempDir::new("check-update-redirects-cap");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let (url, server) = serve_redirects(&key, 4, b"the release artifact".to_vec(), false).await;
        let cfg = UpdateConfig {
            max_redirects: 3,
            ..UpdateConfig::default()
        };

        let result = check_update_with(
            &url,
            None,
            Channel::Stable,
            "2.0.2",
            &install,
            &cfg,
            ReinstallPolicy::default(),
            &key.verifying_key(),
        )
        .await;

        assert!(
            matches!(result, Err(UpdateError::TooManyRedirects(3))),
            "{result:?}"
        );
        assert!(!install.exists());
        server.abort();
    }

    #[tokio::test]
    async fn a_redirect_off_http_is_refused_even_when_it_is_within_the_cap() {
        let dir = TempDir::new("check-update-redirect-scheme");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback listener");
        let base = format!("http://{}", listener.local_addr().expect("an address"));
        let app = Router::new().route(
            "/off-http",
            get(|| async { Redirect::temporary("ftp://example.test/release.json") }),
        );
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let result = check_update_with(
            &format!("{base}/off-http"),
            None,
            Channel::Stable,
            "2.0.2",
            &install,
            &UpdateConfig::default(),
            ReinstallPolicy::default(),
            &key.verifying_key(),
        )
        .await;

        assert!(
            matches!(result, Err(UpdateError::RedirectScheme(ref scheme)) if scheme == "ftp"),
            "{result:?}"
        );
        assert!(!install.exists());
        server.abort();
    }

    #[tokio::test]
    async fn a_body_that_does_not_match_the_signed_digest_is_refused() {
        let dir = TempDir::new("check-update-sha");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback listener");
        let base = format!("http://{}", listener.local_addr().expect("an address"));
        let artifact_url = format!("{base}/artifact");
        let document = sign_document(
            &key,
            b"the bytes the document promises",
            &SignRequest {
                version: "2.0.3",
                channel: Channel::Stable,
                artifact_url: &artifact_url,
                min_version: "2.0.0",
                issued_at_ms: summrise_release::now_ms(),
            },
        )
        .expect("the fixture document signs");
        let expected = document.sha256.clone();
        let app = Router::new()
            .route(
                "/release.json",
                get(move || {
                    let document = document.clone();
                    async move { Json(document) }
                }),
            )
            .route("/artifact", get(|| async { b"tampered bytes".to_vec() }));
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let result = check_update_with(
            &format!("{base}/release.json"),
            None,
            Channel::Stable,
            "2.0.2",
            &install,
            &UpdateConfig::default(),
            ReinstallPolicy::default(),
            &key.verifying_key(),
        )
        .await;

        assert!(
            matches!(result, Err(UpdateError::Sha { expected: ref signed }) if signed == &expected),
            "{result:?}"
        );
        assert!(!install.exists());
        server.abort();
    }

    #[tokio::test]
    async fn a_document_that_is_not_json_is_a_parse_error() {
        let dir = TempDir::new("check-update-parse");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback listener");
        let base = format!("http://{}", listener.local_addr().expect("an address"));
        let app = Router::new().route(
            "/release.json",
            get(|| async { "{ not the document this build reads" }),
        );
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let result = check_update_with(
            &format!("{base}/release.json"),
            None,
            Channel::Stable,
            "2.0.2",
            &install,
            &UpdateConfig::default(),
            ReinstallPolicy::default(),
            &key.verifying_key(),
        )
        .await;

        assert!(matches!(result, Err(UpdateError::Parse(_))), "{result:?}");
        assert!(!install.exists());
        server.abort();
    }

    /// The artifact-level safeguard: a digest the supervisor rolled back on this device is refused
    /// before the artifact is downloaded, and the refusal names the version and when it failed.
    #[tokio::test]
    async fn a_rolled_back_artifact_is_refused_with_its_version_and_time() {
        let dir = TempDir::new("check-update-rolled-back");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"the artifact that failed here".to_vec();
        let server = serve_release(&key, "2.0.3", Channel::Stable, "2.0.0", artifact.clone()).await;
        let sha256 = summrise_release::sha256_hex(&artifact);
        let policy = ReinstallPolicy {
            rolled_back: &[RolledBackArtifact {
                sha256: sha256.clone(),
                version: "2.0.3".to_owned(),
                at_ms: 1_700_000_000_000,
            }],
            force_reinstall: None,
        };

        let result = check_update_with(
            &server.document_url,
            None,
            Channel::Stable,
            "2.0.2",
            &install,
            &UpdateConfig::default(),
            policy,
            &key.verifying_key(),
        )
        .await;

        match &result {
            Err(UpdateError::RolledBack {
                version,
                at_ms,
                sha256: refused,
            }) => {
                assert_eq!(version, "2.0.3");
                assert_eq!(*at_ms, 1_700_000_000_000);
                assert_eq!(refused, &sha256);
            }
            other => panic!("a rolled-back digest must be a typed refusal: {other:?}"),
        }
        let message = result.expect_err("the result is a refusal").to_string();
        assert!(
            message.contains("2.0.3"),
            "the refusal names the version: {message}"
        );
        assert!(
            message.contains("2023-11-14T22:13:20Z"),
            "the refusal names when it failed: {message}"
        );
        assert_eq!(
            server.artifact_hits.load(Ordering::SeqCst),
            0,
            "a digest this device already refused must not be downloaded again"
        );
        assert!(!install.exists());
    }

    /// The point of keying on the digest: a rebuilt artifact with the same version is not the build
    /// that failed, so it is allowed.
    #[tokio::test]
    async fn the_same_version_with_a_different_digest_is_allowed() {
        let dir = TempDir::new("check-update-rebuilt");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"the rebuilt artifact".to_vec();
        let server = serve_release(&key, "2.0.3", Channel::Stable, "2.0.0", artifact).await;
        let policy = ReinstallPolicy {
            rolled_back: &[RolledBackArtifact {
                sha256: "ab".repeat(32),
                version: "2.0.3".to_owned(),
                at_ms: 1_700_000_000_000,
            }],
            force_reinstall: None,
        };

        let outcome = check_update_with(
            &server.document_url,
            None,
            Channel::Stable,
            "2.0.2",
            &install,
            &UpdateConfig::default(),
            policy,
            &key.verifying_key(),
        )
        .await
        .expect("a different artifact is not the one that failed");

        assert!(
            matches!(outcome, CheckOutcome::Staged(_)),
            "the rebuilt 2.0.3 is staged: {outcome:?}"
        );
        assert_eq!(server.artifact_hits.load(Ordering::SeqCst), 1);
    }

    /// An operator who has fixed the cause names the exact digest; only that digest is overridden.
    #[tokio::test]
    async fn force_reinstall_overrides_the_rolled_back_refusal() {
        let dir = TempDir::new("check-update-force");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"the artifact the operator fixed".to_vec();
        let server = serve_release(&key, "2.0.3", Channel::Stable, "2.0.0", artifact.clone()).await;
        let sha256 = summrise_release::sha256_hex(&artifact);
        let policy = ReinstallPolicy {
            rolled_back: &[RolledBackArtifact {
                sha256: sha256.clone(),
                version: "2.0.3".to_owned(),
                at_ms: 1_700_000_000_000,
            }],
            force_reinstall: Some(&sha256),
        };

        let outcome = check_update_with(
            &server.document_url,
            None,
            Channel::Stable,
            "2.0.2",
            &install,
            &UpdateConfig::default(),
            policy,
            &key.verifying_key(),
        )
        .await
        .expect("the operator's exact digest overrides the refusal");

        assert!(matches!(outcome, CheckOutcome::Staged(_)));
        assert_eq!(server.artifact_hits.load(Ordering::SeqCst), 1);
    }

    /// The override is a digest, not a blanket permission: naming another artifact does not let the
    /// failed one through.
    #[tokio::test]
    async fn force_reinstall_for_another_digest_does_not_override() {
        let dir = TempDir::new("check-update-force-other");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"the artifact that failed here".to_vec();
        let server = serve_release(&key, "2.0.3", Channel::Stable, "2.0.0", artifact.clone()).await;
        let sha256 = summrise_release::sha256_hex(&artifact);
        let policy = ReinstallPolicy {
            rolled_back: &[RolledBackArtifact {
                sha256: sha256.clone(),
                version: "2.0.3".to_owned(),
                at_ms: 1_700_000_000_000,
            }],
            force_reinstall: Some(&"cd".repeat(32)),
        };

        let result = check_update_with(
            &server.document_url,
            None,
            Channel::Stable,
            "2.0.2",
            &install,
            &UpdateConfig::default(),
            policy,
            &key.verifying_key(),
        )
        .await;

        assert!(
            matches!(result, Err(UpdateError::RolledBack { .. })),
            "the failed artifact is still refused: {result:?}"
        );
        assert_eq!(server.artifact_hits.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_listed_artifact_is_refused_with_its_reason() {
        let dir = TempDir::new("check-update-revoked");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"the withdrawn artifact".to_vec();
        let sha256 = summrise_release::sha256_hex(&artifact);
        let list = revocation_list(
            &key,
            summrise_release::now_ms(),
            &[("2.0.3", &sha256, Some("boot loop on Windows 11"))],
        );
        let (base, hits) = serve_release_with_list(
            &key,
            "2.0.3",
            Channel::Stable,
            "2.0.0",
            artifact,
            ListFixture::Body {
                path: "/revocations.json".to_owned(),
                bytes: serde_json::to_vec(&list).expect("the list serializes"),
            },
        )
        .await;

        let result = check_update_with(
            &format!("{base}/release.json"),
            None,
            Channel::Stable,
            "2.0.2",
            &install,
            &UpdateConfig::default(),
            ReinstallPolicy::default(),
            &key.verifying_key(),
        )
        .await;

        let error = result.expect_err("a revoked artifact must be refused");
        match &error {
            UpdateError::Revoked {
                version,
                sha256: refused,
                reason: Some(reason),
            } => {
                assert_eq!(version, "2.0.3");
                assert_eq!(refused, &sha256);
                assert_eq!(reason, "boot loop on Windows 11");
            }
            other => panic!("a listed artifact must be a typed refusal: {other:?}"),
        }
        let message = error.to_string();
        assert!(
            message.contains("2.0.3"),
            "the message names the version: {message}"
        );
        assert!(
            message.contains("boot loop on Windows 11"),
            "the message names the reason: {message}"
        );
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "a revoked release is refused before its artifact is downloaded"
        );
        assert!(!install.exists(), "a revocation refusal stages nothing");
    }

    #[tokio::test]
    async fn the_same_version_with_a_different_digest_is_not_revoked() {
        let dir = TempDir::new("check-update-revoked-rebuilt");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"the rebuilt artifact".to_vec();
        let list = revocation_list(
            &key,
            summrise_release::now_ms(),
            &[("2.0.3", &"ab".repeat(32), Some("the failed build"))],
        );
        let (base, hits) = serve_release_with_list(
            &key,
            "2.0.3",
            Channel::Stable,
            "2.0.0",
            artifact,
            ListFixture::Body {
                path: "/revocations.json".to_owned(),
                bytes: serde_json::to_vec(&list).expect("the list serializes"),
            },
        )
        .await;

        let outcome = check_update_with(
            &format!("{base}/release.json"),
            None,
            Channel::Stable,
            "2.0.2",
            &install,
            &UpdateConfig::default(),
            ReinstallPolicy::default(),
            &key.verifying_key(),
        )
        .await
        .expect("a rebuilt release with the same version is a different artifact");

        assert!(matches!(outcome, CheckOutcome::Staged(_)), "{outcome:?}");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn an_empty_list_admits_everything() {
        let dir = TempDir::new("check-update-revoked-empty");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let list = revocation_list(&key, summrise_release::now_ms(), &[]);
        let (base, hits) = serve_release_with_list(
            &key,
            "2.0.3",
            Channel::Stable,
            "2.0.0",
            b"the release artifact".to_vec(),
            ListFixture::Body {
                path: "/revocations.json".to_owned(),
                bytes: serde_json::to_vec(&list).expect("the list serializes"),
            },
        )
        .await;

        let outcome = check_update_with(
            &format!("{base}/release.json"),
            None,
            Channel::Stable,
            "2.0.2",
            &install,
            &UpdateConfig::default(),
            ReinstallPolicy::default(),
            &key.verifying_key(),
        )
        .await
        .expect("an empty, valid list withdraws nothing");

        assert!(matches!(outcome, CheckOutcome::Staged(_)), "{outcome:?}");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_tampered_list_fails_verification_and_stages_nothing() {
        let dir = TempDir::new("check-update-revocations-tampered");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"the release artifact".to_vec();
        let sha256 = summrise_release::sha256_hex(&artifact);
        let mut list = revocation_list(
            &key,
            summrise_release::now_ms(),
            &[("2.0.3", &sha256, Some("withdrawn"))],
        );
        // Edit a signed fact after signing: the signature covers version and digest, so this must
        // never be accepted as a list that withdraws a different release.
        list.revoked[0].version = "2.0.4".to_owned();
        let (base, hits) = serve_release_with_list(
            &key,
            "2.0.3",
            Channel::Stable,
            "2.0.0",
            artifact,
            ListFixture::Body {
                path: "/revocations.json".to_owned(),
                bytes: serde_json::to_vec(&list).expect("the list serializes"),
            },
        )
        .await;

        let result = check_update_with(
            &format!("{base}/release.json"),
            None,
            Channel::Stable,
            "2.0.2",
            &install,
            &UpdateConfig::default(),
            ReinstallPolicy::default(),
            &key.verifying_key(),
        )
        .await;

        assert!(
            matches!(
                result,
                Err(UpdateError::Revocations(RevocationError::Signature))
            ),
            "a tampered list is a hard failure: {result:?}"
        );
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "the list is checked before the artifact is downloaded"
        );
        assert!(!install.exists(), "nothing may be staged");
    }

    #[tokio::test]
    async fn a_malformed_list_is_a_hard_failure() {
        let dir = TempDir::new("check-update-revocations-malformed");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let (base, hits) = serve_release_with_list(
            &key,
            "2.0.3",
            Channel::Stable,
            "2.0.0",
            b"the release artifact".to_vec(),
            ListFixture::Body {
                path: "/revocations.json".to_owned(),
                bytes: b"{ not the list this build reads".to_vec(),
            },
        )
        .await;

        let result = check_update_with(
            &format!("{base}/release.json"),
            None,
            Channel::Stable,
            "2.0.2",
            &install,
            &UpdateConfig::default(),
            ReinstallPolicy::default(),
            &key.verifying_key(),
        )
        .await;

        assert!(
            matches!(result, Err(UpdateError::RevocationsParse(_))),
            "a malformed list found at the URL is a hard failure: {result:?}"
        );
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        assert!(!install.exists());
    }

    #[tokio::test]
    async fn a_404_list_logs_and_proceeds() {
        let dir = TempDir::new("check-update-revocations-missing");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let (base, hits) = serve_release_with_list(
            &key,
            "2.0.3",
            Channel::Stable,
            "2.0.0",
            b"the release artifact".to_vec(),
            ListFixture::Absent,
        )
        .await;

        let outcome = check_update_with(
            &format!("{base}/release.json"),
            None,
            Channel::Stable,
            "2.0.2",
            &install,
            &UpdateConfig::default(),
            ReinstallPolicy::default(),
            &key.verifying_key(),
        )
        .await
        .expect("a 404 for the list means nothing is revoked");

        assert!(matches!(outcome, CheckOutcome::Staged(_)), "{outcome:?}");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    /// A bucket that has no key for the list often answers 403, not 404. That is still "no list
    /// published", not a broken device: the update proceeds, and only a present-but-unusable list
    /// fails closed.
    #[tokio::test]
    async fn a_403_list_is_also_treated_as_missing() {
        let dir = TempDir::new("check-update-revocations-403");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let (base, hits) = serve_release_with_list(
            &key,
            "2.0.3",
            Channel::Stable,
            "2.0.0",
            b"the release artifact".to_vec(),
            ListFixture::Status {
                path: "/revocations.json".to_owned(),
                status: StatusCode::FORBIDDEN,
            },
        )
        .await;

        let outcome = check_update_with(
            &format!("{base}/release.json"),
            None,
            Channel::Stable,
            "2.0.2",
            &install,
            &UpdateConfig::default(),
            ReinstallPolicy::default(),
            &key.verifying_key(),
        )
        .await
        .expect("a 403 for an absent list means nothing is revoked");

        assert!(matches!(outcome, CheckOutcome::Staged(_)), "{outcome:?}");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_500_list_is_a_hard_failure_with_its_status() {
        let dir = TempDir::new("check-update-revocations-500");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let (base, hits) = serve_release_with_list(
            &key,
            "2.0.3",
            Channel::Stable,
            "2.0.0",
            b"the release artifact".to_vec(),
            ListFixture::Status {
                path: "/revocations.json".to_owned(),
                status: StatusCode::INTERNAL_SERVER_ERROR,
            },
        )
        .await;

        let result = check_update_with(
            &format!("{base}/release.json"),
            None,
            Channel::Stable,
            "2.0.2",
            &install,
            &UpdateConfig::default(),
            ReinstallPolicy::default(),
            &key.verifying_key(),
        )
        .await;

        let error = result.expect_err("a 500 is not 'no list'");
        match &error {
            UpdateError::Status { status, .. } => assert_eq!(*status, 500),
            other => panic!("a 500 must stay a status failure: {other:?}"),
        }
        assert!(
            error.to_string().contains("500"),
            "the refusal names the HTTP status: {error}"
        );
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        assert!(!install.exists(), "nothing may be staged");
    }

    #[tokio::test]
    async fn a_stale_list_is_refused() {
        let dir = TempDir::new("check-update-revocations-stale");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let issued = summrise_release::now_ms().saturating_sub(MAX_REVOCATIONS_AGE_MS + 60_000);
        let list = revocation_list(&key, issued, &[]);
        let (base, hits) = serve_release_with_list(
            &key,
            "2.0.3",
            Channel::Stable,
            "2.0.0",
            b"the release artifact".to_vec(),
            ListFixture::Body {
                path: "/revocations.json".to_owned(),
                bytes: serde_json::to_vec(&list).expect("the list serializes"),
            },
        )
        .await;

        let result = check_update_with(
            &format!("{base}/release.json"),
            None,
            Channel::Stable,
            "2.0.2",
            &install,
            &UpdateConfig::default(),
            ReinstallPolicy::default(),
            &key.verifying_key(),
        )
        .await;

        assert!(
            matches!(
                result,
                Err(UpdateError::Revocations(RevocationError::Stale { .. }))
            ),
            "a stale list is refused rather than trusted: {result:?}"
        );
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        assert!(!install.exists());
    }

    #[tokio::test]
    async fn an_explicit_list_url_wins_over_the_artifact_sibling() {
        let dir = TempDir::new("check-update-revocations-explicit");
        let install = dir.join("install");
        let key = SigningKey::from_bytes(&TEST_SEED);
        let artifact = b"the withdrawn artifact".to_vec();
        let sha256 = summrise_release::sha256_hex(&artifact);
        let list = revocation_list(
            &key,
            summrise_release::now_ms(),
            &[("2.0.3", &sha256, Some("withdrawn"))],
        );
        let (base, hits) = serve_release_with_list(
            &key,
            "2.0.3",
            Channel::Stable,
            "2.0.0",
            artifact,
            ListFixture::Body {
                path: "/elsewhere.json".to_owned(),
                bytes: serde_json::to_vec(&list).expect("the list serializes"),
            },
        )
        .await;
        let list_url = format!("{base}/elsewhere.json");

        let result = check_update_with(
            &format!("{base}/release.json"),
            Some(&list_url),
            Channel::Stable,
            "2.0.2",
            &install,
            &UpdateConfig::default(),
            ReinstallPolicy::default(),
            &key.verifying_key(),
        )
        .await;

        assert!(
            matches!(result, Err(UpdateError::Revoked { ref version, .. }) if version == "2.0.3"),
            "the explicit list is the one that is read: {result:?}"
        );
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        assert!(!install.exists());
    }
}
