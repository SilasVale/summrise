//! The CDN worker's ROUTE TABLE — which of the six routes a request is, decided before any I/O.
//!
//! ## Why this is a separate function at all
//!
//! `index/src/index.js` is a chain of `if` statements, and each branch ends in a different KIND of
//! work: two are `ASSETS.fetch`, two are R2 reads, one is a GitHub proxy, and the last renders the
//! landing page. **Only the DECISION is portable.** So the decision is a pure function over a
//! pathname, and the I/O stays outside it — which is the same shape the manifest took, and the same
//! reason: the plan's criterion for this block is "同请求新旧响应字节可比", and a comparison that
//! needs two runtimes is not a comparison.
//!
//! ## The rules, and three of them are about what must NOT match
//!
//! * **A MISSING BINARY MUST 404, NEVER THE LANDING PAGE AS 200 HTML.** Devices once downloaded
//!   HTML as `SummriseAgent-Setup.exe` and the agent never started (the worker's own comment). So the
//!   landing page answers `/` and `/index.html` ONLY, and everything else that is not one of the five
//!   artifacts is `NotFound`.
//! * **THE VERSIONED NAMES ARE EXACT PATTERNS AND THE ALIASES ARE EXACT LITERALS.** The versionless
//!   `summrise-agent-latest.tgz` is matched by literal string, NOT by loosening the versioned regex
//!   to cover it — a download path is not a pattern.
//! * **`/api/version` IS ITS OWN ROUTE AND IT COMES FIRST**, before every artifact: the updater's
//!   poll must not be shadowed by a path that happens to end in `.tgz`.

/// What the worker should do with a request, before it does any of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// `GET /api/version` — the release manifest, derived from `version.json`.
    Manifest,
    /// Served straight from the ASSETS binding: the Windows installer.
    Installer,
    /// Served straight from ASSETS: the npm tarball.
    Tarball,
    /// Proxied from a PINNED GitHub release asset: the cloudflared tunnel binary.
    Cloudflared,
    /// Read from the R2 bucket: the electron runtime.
    Electron,
    /// Read from the R2 bucket: the playwright browser-tools bundle.
    Playwright,
    /// `/` and `/index.html`: the landing page.
    Landing,
    /// Anything else. A binary that is not there is a 404, never the page.
    NotFound,
}

impl Route {
    /// The pathname this route answers, or the pattern it is matched by — for the messages and for
    /// the tests that read the table rather than the branches.
    pub fn path(self) -> &'static str {
        match self {
            Route::Manifest => "/api/version",
            Route::Installer => "/summrise-agent/SummriseAgent-Setup-<v>.exe",
            Route::Tarball => "/summrise-agent/summrise-agent-<v>.tgz",
            Route::Cloudflared => "/summrise-agent/cloudflared.exe",
            Route::Electron => "/summrise-agent/electron-win32-x64.zip",
            Route::Playwright => "/summrise-agent/summrise-playwright.zip",
            Route::Landing => "/ and /index.html",
            Route::NotFound => "(everything else)",
        }
    }
}

/// `route(pathname)` — the whole table, in the worker's own order.
///
/// THE ORDER IS THE TABLE: `/api/version` is tested first, then the two ASSETS artifacts, then the
/// three big binaries, and the landing page LAST and behind the not-found guard. Reordering these is
/// not a refactor — `/api/version` must not be shadowed, and the landing page must not answer for a
/// path that names a file.
pub fn route(pathname: &str) -> Route {
    if pathname == "/api/version" {
        return Route::Manifest;
    }
    // `/^\/summrise-agent\/SummriseAgent-Setup-[0-9]+\.[0-9]+\.[0-9]+\.exe$/` OR the flat alias.
    if is_versioned_setup(pathname) || pathname == "/summrise-agent/SummriseAgent-Setup.exe" {
        return Route::Installer;
    }
    // `/^\/summrise-agent\/summrise-agent-[0-9]+\.[0-9]+\.[0-9]+\.tgz$/` OR the flat alias.
    if is_versioned_tarball(pathname) || pathname == "/summrise-agent/summrise-agent-latest.tgz" {
        return Route::Tarball;
    }
    match pathname {
        "/summrise-agent/cloudflared.exe" => Route::Cloudflared,
        "/summrise-agent/electron-win32-x64.zip" => Route::Electron,
        "/summrise-agent/summrise-playwright.zip" => Route::Playwright,
        // A missing binary must 404, not return the download PAGE as 200 HTML — devices silently
        // downloaded HTML as SummriseAgent-Setup.exe and the agent never started. Only "/" and
        // "/index.html" render the page.
        "/" | "/index.html" => Route::Landing,
        _ => Route::NotFound,
    }
}

/// `SummriseAgent-Setup-<x.y.z>.exe` — three dotted numeric groups, nothing else. A path, a
/// four-group version, a leading `v`, or a `-rc` suffix are all NOT this route: an exact-pattern
/// discipline is what keeps a request for one file from being answered with another.
fn is_versioned_setup(pathname: &str) -> bool {
    let Some(rest) = pathname.strip_prefix("/summrise-agent/SummriseAgent-Setup-") else {
        return false;
    };
    dotted_version(rest.strip_suffix(".exe").unwrap_or(""))
}

/// `summrise-agent-<x.y.z>.tgz` — the same shape, and the versionless alias is NOT matched here (it is
/// a literal in the table above, and the worker's comment says loosening the regex to cover it is
/// exactly what must not be done).
fn is_versioned_tarball(pathname: &str) -> bool {
    let Some(rest) = pathname.strip_prefix("/summrise-agent/summrise-agent-") else {
        return false;
    };
    dotted_version(rest.strip_suffix(".tgz").unwrap_or(""))
}

/// `[0-9]+\.[0-9]+\.[0-9]+` — and the suffix must be CONSUMED, which is why the callers hand it the
/// already-stripped remainder: a string that merely CONTAINS a version is not a version.
fn dotted_version(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

#[cfg(test)]
// SAME ALLOW, SAME REASON as the page module's: the name below asserts the behaviour a reader expects
// NOT to find — that a path which ALMOST names an artifact is `NotFound` and not the landing page —
// and the capitals are the note. Flattening it would make the surprising case read like an ordinary one.
#[allow(non_snake_case)]
mod tests {
    use super::*;

    #[test]
    fn every_route_answers_its_own_path() {
        let cases: &[(&str, Route)] = &[
            ("/api/version", Route::Manifest),
            ("/summrise-agent/SummriseAgent-Setup.exe", Route::Installer),
            (
                "/summrise-agent/SummriseAgent-Setup-1.2.297.exe",
                Route::Installer,
            ),
            ("/summrise-agent/summrise-agent-latest.tgz", Route::Tarball),
            ("/summrise-agent/summrise-agent-1.2.297.tgz", Route::Tarball),
            ("/summrise-agent/cloudflared.exe", Route::Cloudflared),
            ("/summrise-agent/electron-win32-x64.zip", Route::Electron),
            ("/summrise-agent/summrise-playwright.zip", Route::Playwright),
            ("/", Route::Landing),
            ("/index.html", Route::Landing),
        ];
        for (path, want) in cases {
            assert_eq!(route(path), *want, "{path}");
        }
    }

    #[test]
    fn a_path_that_ALMOST_names_a_binary_is_not_found_rather_than_the_page() {
        // The defect this whole guard exists for: a device asking for an artifact that is not there
        // got the LANDING PAGE as 200 HTML, downloaded it as the installer, and the agent never
        // started. So every near-miss is `NotFound`, and only the two page paths are `Landing`.
        for path in [
            "/summrise-agent/",
            "/summrise-agent",
            "/summrise-agent/summrise-agent-1.2.tgz",
            "/summrise-agent/summrise-agent-1.2.297.1.tgz",
            "/summrise-agent/summrise-agent-v1.2.297.tgz",
            "/summrise-agent/summrise-agent-1.2.297.tgz.bak",
            "/summrise-agent/SummriseAgent-Setup-1.2.exe",
            "/summrise-agent/SummriseAgent-Setup-1.2.297.1.exe",
            "/summrise-agent/SummriseAgent-Setup-x.y.z.exe",
            "/summrise-agent/cloudflared.exe.bak",
            "/api/version/",
            "/api/version.json",
            "/index.htm",
            "/robots.txt",
            "/../etc/passwd",
            "/summrise-agent/../../etc/passwd",
        ] {
            assert_eq!(route(path), Route::NotFound, "{path}");
        }
    }

    #[test]
    fn the_manifest_route_is_not_shadowed_by_anything() {
        // It is tested FIRST in the worker, and a path that ends in a versioned name must not reach
        // the download routes.
        assert_eq!(route("/api/version"), Route::Manifest);
        assert_eq!(
            route("/api/version/../summrise-agent/summrise-agent-1.2.297.tgz"),
            Route::NotFound
        );
    }
}
