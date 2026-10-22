//! The transport rule for release documents and artifacts.
//!
//! A release is fetched over TLS. Plain `http` is allowed only for a loopback host, because there is
//! no network to protect and the lane's fixtures are loopback servers. The signer and the device
//! share this one predicate: a document the signer accepts is a document the device will try to
//! fetch, and a signer cannot produce an address the device refuses.

use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};

use url::{Host, Url};

/// Why a URL is not one a release may be fetched from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UrlError {
    /// The string is empty.
    Empty,
    /// The string is not an absolute URL.
    NotAbsolute,
    /// The URL has no host.
    NoHost,
    /// The scheme is neither `http` nor `https`.
    Scheme(String),
    /// The URL is plain `http` on a host that is not loopback.
    InsecureHttp(String),
}

impl fmt::Display for UrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UrlError::Empty => f.write_str("the URL is empty"),
            UrlError::NotAbsolute => f.write_str("the URL is not absolute"),
            UrlError::NoHost => f.write_str("the URL has no host"),
            UrlError::Scheme(scheme) => {
                write!(f, "the URL uses the {scheme:?} scheme, not http or https")
            }
            UrlError::InsecureHttp(url) => write!(
                f,
                "the URL {url:?} is plain http on a host that is not loopback; use https"
            ),
        }
    }
}

impl std::error::Error for UrlError {}

/// Parse a URL a release may be fetched from: absolute, http(s), and https unless loopback.
pub fn parse_release_url(text: &str) -> Result<Url, UrlError> {
    if text.is_empty() {
        return Err(UrlError::Empty);
    }
    let url = Url::parse(text).map_err(|_| UrlError::NotAbsolute)?;
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(UrlError::Scheme(scheme.to_owned()));
    }
    let Some(host) = url.host() else {
        return Err(UrlError::NoHost);
    };
    if scheme == "http" && !is_loopback_host(host) {
        return Err(UrlError::InsecureHttp(text.to_owned()));
    }
    Ok(url)
}

/// Parse a URL, allowing plaintext http when the operator says the transport is a tunnel.
///
/// The strict rule exists because a document's bytes are trusted by signature, not by transport —
/// but a device that has been told to fetch from a private mirror must be able to name it. The flag
/// widens the scheme and nothing else: the address must still be absolute, with a host and without
/// userinfo, query or fragment.
pub fn parse_release_url_allowing_plaintext(
    text: &str,
    allow_plaintext: bool,
) -> Result<Url, UrlError> {
    match parse_release_url(text) {
        Ok(url) => Ok(url),
        Err(error) if allow_plaintext => {
            let url = match Url::parse(text) {
                Ok(url) => url,
                Err(_) => return Err(error),
            };
            if url.scheme() != "http" || url.host().is_none() {
                return Err(error);
            }
            if !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(error);
            }
            Ok(url)
        }
        Err(error) => Err(error),
    }
}

/// Whether `host` is a loopback host: `127.0.0.1`, `::1`, or the name `localhost`.
fn is_loopback_host(host: Host<&str>) -> bool {
    match host {
        Host::Ipv4(address) => address == Ipv4Addr::LOCALHOST,
        Host::Ipv6(address) => address == Ipv6Addr::LOCALHOST,
        Host::Domain(name) => name.eq_ignore_ascii_case("localhost"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_is_required_except_on_loopback() {
        for url in [
            "https://example.test/release.json",
            "http://127.0.0.1/release.json",
            "http://[::1]/release.json",
            "http://localhost/release.json",
        ] {
            assert_eq!(parse_release_url(url), Ok(Url::parse(url).expect("a URL")));
        }

        assert_eq!(
            parse_release_url("http://example.test/release.json"),
            Err(UrlError::InsecureHttp(
                "http://example.test/release.json".to_owned()
            ))
        );
        assert_eq!(
            parse_release_url("http://127.0.0.2/release.json"),
            Err(UrlError::InsecureHttp(
                "http://127.0.0.2/release.json".to_owned()
            ))
        );
    }

    #[test]
    fn only_absolute_http_and_https_urls_are_accepted() {
        assert_eq!(parse_release_url(""), Err(UrlError::Empty));
        assert_eq!(parse_release_url("not a url"), Err(UrlError::NotAbsolute));
        assert_eq!(parse_release_url("https://"), Err(UrlError::NotAbsolute));
        assert_eq!(
            parse_release_url("ftp://example.test/release.json"),
            Err(UrlError::Scheme("ftp".to_owned()))
        );
        assert_eq!(
            parse_release_url("file:///release.json"),
            Err(UrlError::Scheme("file".to_owned()))
        );
    }

    #[test]
    fn plaintext_is_an_explicit_widening_of_the_scheme() {
        let url = "http://mirror.example.test/release.json";
        assert!(parse_release_url_allowing_plaintext(url, false).is_err());
        let allowed = parse_release_url_allowing_plaintext(url, true).expect("the flag allows it");
        assert_eq!(allowed.scheme(), "http");
        assert!(
            parse_release_url_allowing_plaintext("http://u:p@mirror.test/r.json", true).is_err()
        );
        assert!(
            parse_release_url_allowing_plaintext("http://mirror.test/r.json?x=1", true).is_err()
        );
        assert!(parse_release_url_allowing_plaintext("http://mirror.test/r.json#f", true).is_err());
        assert!(parse_release_url_allowing_plaintext("ftp://mirror.test/r.json", true).is_err());
    }
}
