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
}
