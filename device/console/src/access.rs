//! Trusting Cloudflare Access, when the operator says so.
//!
//! The console has one credential of its own: the operator token. That is correct for a box on a
//! tailnet, and inconvenient for a page on the internet — so an operator who has already put
//! Cloudflare Access in front of the console can let Access be the identity, and skip the token box.
//!
//! **This is off unless three flags name the trust**: the team's certificates, the Access application's
//! audience tag, and the issuer. A JWT that does not verify, or verifies for the wrong audience, falls
//! back to the token — a security feature that fails open is not a security feature.
//!
//! The certificates are pinned in a file rather than fetched at run time: the console does not carry an
//! HTTP client for its own auth path, and a trust anchor that arrives over the network is a trust
//! anchor an attacker on that network can move. Rotation is a file replacement and a restart.

use std::path::Path;

use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use serde::Deserialize;

/// The header Cloudflare Access sets on a request it has authenticated.
pub const ACCESS_HEADER: &str = "cf-access-jwt-assertion";

/// What the console needs to verify an Access JWT.
pub struct Access {
    keys: Vec<(Option<String>, DecodingKey)>,
    validation: Validation,
}

/// The claims the console reads. Everything else is Access's business.
#[derive(Debug, Clone, Deserialize)]
pub struct AccessClaims {
    /// The verified email of the person Access admitted.
    pub email: Option<String>,
}

/// Why an Access JWT was refused.
#[derive(Debug, PartialEq, Eq)]
pub enum AccessError {
    /// The file the certificates should be in could not be read or was not a JWKS.
    Certificates(String),
    /// The token did not verify, expired, or was for another audience or issuer.
    Refused(String),
}

impl std::fmt::Display for AccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccessError::Certificates(detail) => {
                write!(f, "the Access certificates are unusable: {detail}")
            }
            AccessError::Refused(detail) => write!(f, "the Access token was refused: {detail}"),
        }
    }
}

/// One key of a JWKS, as Access publishes them.
#[derive(Deserialize)]
struct Jwk {
    #[serde(default)]
    kid: Option<String>,
    kty: String,
    n: String,
    e: String,
}

#[derive(Deserialize)]
struct Jwks {
    keys: Vec<Jwk>,
}

impl Access {
    /// Build the verifier from the team's JWKS file, the application's audience and the issuer.
    pub fn from_jwks_file(
        path: &Path,
        audience: &str,
        issuer: &str,
    ) -> Result<Access, AccessError> {
        let bytes = std::fs::read(path)
            .map_err(|error| AccessError::Certificates(format!("{}: {error}", path.display())))?;
        Access::from_jwks(&bytes, audience, issuer)
    }

    /// The testable half: a JWKS document in memory.
    pub fn from_jwks(jwks: &[u8], audience: &str, issuer: &str) -> Result<Access, AccessError> {
        let parsed: Jwks = serde_json::from_slice(jwks)
            .map_err(|error| AccessError::Certificates(error.to_string()))?;
        if parsed.keys.is_empty() {
            return Err(AccessError::Certificates(
                "the JWKS holds no keys".to_owned(),
            ));
        }
        let mut keys = Vec::new();
        for jwk in parsed.keys {
            if jwk.kty != "RSA" {
                continue;
            }
            let key = DecodingKey::from_rsa_components(&jwk.n, &jwk.e)
                .map_err(|error| AccessError::Certificates(error.to_string()))?;
            keys.push((jwk.kid, key));
        }
        if keys.is_empty() {
            return Err(AccessError::Certificates(
                "the JWKS holds no RSA keys".to_owned(),
            ));
        }

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[audience]);
        validation.set_issuer(&[issuer]);
        validation.validate_exp = true;

        Ok(Access { keys, validation })
    }

    /// The issuer this verifier pins: the team's own URL, and therefore where its sign-out lives.
    /// The configuration sets exactly one, so the set's single member is the answer.
    pub fn issuer(&self) -> Option<&str> {
        self.validation
            .iss
            .as_ref()?
            .iter()
            .next()
            .map(String::as_str)
    }

    /// Verify a token and return the claims Access signed.
    pub fn verify(&self, token: &str) -> Result<AccessClaims, AccessError> {
        let header =
            decode_header(token).map_err(|error| AccessError::Refused(error.to_string()))?;
        let candidates: Vec<&DecodingKey> = match &header.kid {
            // A `kid` names the key; a token signed by a key we do not have is refused here rather
            // than by trying every key in turn.
            Some(kid) => self
                .keys
                .iter()
                .filter(|(key_id, _)| key_id.as_deref() == Some(kid.as_str()))
                .map(|(_, key)| key)
                .collect(),
            None => self.keys.iter().map(|(_, key)| key).collect(),
        };
        if candidates.is_empty() {
            return Err(AccessError::Refused(
                "the token names a key this console does not have".to_owned(),
            ));
        }

        let mut last = String::new();
        for key in candidates {
            match decode::<AccessClaims>(token, key, &self.validation) {
                Ok(data) => return Ok(data.claims),
                Err(error) => last = error.to_string(),
            }
        }
        Err(AccessError::Refused(last))
    }
}

/// The token from a request's Access header, when it carries one.
pub fn token_from_headers(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(ACCESS_HEADER)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};
    use serde::Serialize;

    /// A throwaway key pair, generated for these tests and used nowhere else.
    const PRIVATE_PEM: &str = include_str!("../tests/fixtures/access-test-key.pem");
    const JWKS: &str = include_str!("../tests/fixtures/access-test-jwks.json");
    const AUDIENCE: &str = "test-audience-tag";
    const ISSUER: &str = "https://test-team.cloudflareaccess.com";

    #[derive(Serialize)]
    struct Claims<'a> {
        aud: &'a str,
        iss: &'a str,
        exp: u64,
        email: &'a str,
    }

    fn token(claims: Claims<'_>, kid: Option<&str>) -> String {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = kid.map(str::to_owned);
        let key = EncodingKey::from_rsa_pem(PRIVATE_PEM.as_bytes()).expect("the fixture key loads");
        encode(&header, &claims, &key).expect("the token is signed")
    }

    fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }

    #[test]
    fn a_token_for_this_audience_and_issuer_is_accepted() {
        let access = Access::from_jwks(JWKS.as_bytes(), AUDIENCE, ISSUER).expect("the JWKS loads");
        let claims = token(
            Claims {
                aud: AUDIENCE,
                iss: ISSUER,
                exp: now() + 60,
                email: "operator@example.test",
            },
            Some("test-key"),
        );
        let verified = access.verify(&claims).expect("the token verifies");
        assert_eq!(verified.email.as_deref(), Some("operator@example.test"));
    }

    /// The three ways an Access token must fail: another audience, another issuer, and an expired one.
    #[test]
    fn a_token_for_another_audience_issuer_or_time_is_refused() {
        let access = Access::from_jwks(JWKS.as_bytes(), AUDIENCE, ISSUER).expect("the JWKS loads");
        for (label, claims) in [
            (
                "audience",
                Claims {
                    aud: "someone-else",
                    iss: ISSUER,
                    exp: now() + 60,
                    email: "a@b",
                },
            ),
            (
                "issuer",
                Claims {
                    aud: AUDIENCE,
                    iss: "https://other.cloudflareaccess.com",
                    exp: now() + 60,
                    email: "a@b",
                },
            ),
            (
                // Well past jsonwebtoken's default 60 s of leeway for clock skew.
                "expiry",
                Claims {
                    aud: AUDIENCE,
                    iss: ISSUER,
                    exp: now() - 120,
                    email: "a@b",
                },
            ),
        ] {
            assert!(
                access.verify(&token(claims, Some("test-key"))).is_err(),
                "{label} must be refused"
            );
        }
    }

    #[test]
    fn a_token_naming_an_unknown_key_is_refused() {
        let access = Access::from_jwks(JWKS.as_bytes(), AUDIENCE, ISSUER).expect("the JWKS loads");
        let claims = token(
            Claims {
                aud: AUDIENCE,
                iss: ISSUER,
                exp: now() + 60,
                email: "a@b",
            },
            Some("some-other-key"),
        );
        assert!(matches!(
            access.verify(&claims),
            Err(AccessError::Refused(_))
        ));
    }

    #[test]
    fn an_unusable_jwks_is_reported_at_load_time() {
        assert!(matches!(
            Access::from_jwks(b"not json", AUDIENCE, ISSUER),
            Err(AccessError::Certificates(_))
        ));
        assert!(matches!(
            Access::from_jwks(br#"{"keys":[]}"#, AUDIENCE, ISSUER),
            Err(AccessError::Certificates(_))
        ));
    }
}
