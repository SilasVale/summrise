//! The release manifest (design §6) and the version ordering the update decision needs.
//!
//! [`Manifest`] is the release document's device-facing subset. The release file also carries
//! `notes`, and will grow fields this build has not heard of; unknown keys are therefore tolerated
//! rather than refused. A device that cannot read a future release's manifest is a device that stops
//! updating, which is the failure this document exists to prevent.

use std::fmt;

use serde::{Deserialize, Serialize};

/// The release document's device-facing fields (design §6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// The release's version, as `major.minor.patch`.
    pub version: String,
    /// The channel the release belongs to; the update decision compares it with the device's.
    pub channel: String,
    /// Hex-encoded SHA-256 of the artifact.
    pub sha256: String,
    /// The ed25519 signature over `version\nchannel\nsha256`.
    pub signature: String,
    /// The oldest version this release will let a device stay on; below it, the update is forced.
    pub min_version: String,
}

/// A release version: three non-negative numbers, compared numerically.
///
/// The derived [`Ord`] compares fields in declaration order — major, then minor, then patch — so the
/// struct's field order *is* the version order and a reordering is a comparison change.
///
/// A pre-release label (`2.0.3-rc.1`) is not part of this type yet: phase 1 ships plain releases, and
/// a label needs a deliberate ordering rule rather than a library's default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    /// The major number.
    pub major: u64,
    /// The minor number.
    pub minor: u64,
    /// The patch number.
    pub patch: u64,
}

impl Version {
    /// Parse `major.minor.patch`.
    ///
    /// Strict on purpose: exactly three components, each a `u64`, no prefixes or suffixes. A version
    /// string this build cannot read is refused as [`VersionError::Shape`] or
    /// [`VersionError::Component`] rather than guessed at.
    pub fn parse(text: &str) -> Result<Version, VersionError> {
        let mut parts = text.split('.');
        let major = parts.next().ok_or(VersionError::Shape)?;
        let minor = parts.next().ok_or(VersionError::Shape)?;
        let patch = parts.next().ok_or(VersionError::Shape)?;
        if parts.next().is_some() {
            return Err(VersionError::Shape);
        }

        Ok(Version {
            major: component(major)?,
            minor: component(minor)?,
            patch: component(patch)?,
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Parse one dot-separated component.
fn component(value: &str) -> Result<u64, VersionError> {
    value.parse::<u64>().map_err(|_| VersionError::Component {
        value: value.to_owned(),
    })
}

/// Why a version string was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VersionError {
    /// The string is not exactly three dot-separated components.
    Shape,
    /// One component is not a non-negative integer that fits in a `u64`.
    Component {
        /// The component as it was written, for the error message.
        value: String,
    },
}

impl fmt::Display for VersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VersionError::Shape => write!(f, "a version is exactly major.minor.patch"),
            VersionError::Component { value } => {
                write!(f, "'{value}' is not a non-negative integer")
            }
        }
    }
}

impl std::error::Error for VersionError {}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use super::*;

    /// The design's release document (§6), with the signature and digest filled in.
    const RELEASE: &str = r#"{
      "version": "2.0.3",
      "channel": "stable",
      "sha256": "b1946ac92492d2347c6235b4d2611184a5a35f7e9a2f3c2d1e0f9a8b7c6d5e4f",
      "signature": "00ff11ee22dd33cc44bb55aa66",
      "minVersion": "2.0.0",
      "notes": "the first release of the two runtimes"
    }"#;

    #[test]
    fn the_designs_release_document_parses() {
        let manifest: Manifest =
            serde_json::from_str(RELEASE).expect("the design's release document is a manifest");

        assert_eq!(manifest.version, "2.0.3");
        assert_eq!(manifest.channel, "stable");
        assert_eq!(
            manifest.sha256,
            "b1946ac92492d2347c6235b4d2611184a5a35f7e9a2f3c2d1e0f9a8b7c6d5e4f"
        );
        assert_eq!(manifest.signature, "00ff11ee22dd33cc44bb55aa66");
        assert_eq!(manifest.min_version, "2.0.0");
    }

    #[test]
    fn min_version_is_camel_case_on_the_wire() {
        let manifest: Manifest =
            serde_json::from_str(RELEASE).expect("the design's release document is a manifest");
        let document = serde_json::to_value(&manifest).expect("a manifest serializes");

        assert_eq!(document["minVersion"], "2.0.0");
        assert!(
            document.get("min_version").is_none(),
            "the wire spelling is minVersion: {document}"
        );
        assert!(
            document.get("notes").is_none(),
            "notes is not the device's fact"
        );
    }

    #[test]
    fn a_version_parses_into_three_numbers() {
        assert_eq!(
            Version::parse("2.0.3"),
            Ok(Version {
                major: 2,
                minor: 0,
                patch: 3
            })
        );
        assert_eq!(
            Version::parse("2.0.3").expect("parses").to_string(),
            "2.0.3"
        );
        assert_eq!(
            Version::parse("10.20.30").expect("parses"),
            Version {
                major: 10,
                minor: 20,
                patch: 30
            }
        );
    }

    #[test]
    fn versions_compare_numerically_not_lexically() {
        let cases = [
            ("2.0.10", "2.0.9", Ordering::Greater),
            ("2.10.0", "2.9.9", Ordering::Greater),
            ("10.0.0", "9.99.99", Ordering::Greater),
            ("2.0.3", "2.0.3", Ordering::Equal),
            ("2.0.2", "2.0.3", Ordering::Less),
            ("1.99.99", "2.0.0", Ordering::Less),
        ];

        for (left, right, expected) in cases {
            let left = Version::parse(left).expect("the left version parses");
            let right = Version::parse(right).expect("the right version parses");
            assert_eq!(left.cmp(&right), expected, "{left} vs {right}");
        }
    }

    #[test]
    fn a_malformed_version_names_what_is_wrong() {
        let cases = [
            ("", VersionError::Shape),
            ("2", VersionError::Shape),
            ("2.0", VersionError::Shape),
            ("2.0.3.1", VersionError::Shape),
            (
                "v2.0.3",
                VersionError::Component {
                    value: "v2".to_owned(),
                },
            ),
            (
                "2.x.3",
                VersionError::Component {
                    value: "x".to_owned(),
                },
            ),
            (
                "2..3",
                VersionError::Component {
                    value: String::new(),
                },
            ),
            (
                "2.0.-1",
                VersionError::Component {
                    value: "-1".to_owned(),
                },
            ),
            (
                "2.0.99999999999999999999",
                VersionError::Component {
                    value: "99999999999999999999".to_owned(),
                },
            ),
        ];

        for (text, error) in cases {
            assert_eq!(Version::parse(text), Err(error), "{text:?}");
        }
    }
}
