//! Production JWT verification for the executable.
//!
//! Supports an HS256 shared secret or a local JWKS (RS256/ES256). Tokens must
//! pass signature, issuer, audience and expiration checks. The subject must be
//! a canonical HRN whose account matches the token's account claim;
//! `verify_principal` repeats that cross-check before producing a verified
//! principal.

use std::sync::Arc;

use async_trait::async_trait;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use jsonwebtoken::jwk::{JwkSet, KeyAlgorithm};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Claims {
    sub: String,
    account: String,
}

enum JwtKey {
    Secret(Arc<Vec<u8>>),
    Jwks(Arc<JwkSet>),
}

/// JWT authenticator configured from the security settings.
pub struct JwtAuthenticator {
    key: JwtKey,
    issuer: String,
    audience: String,
}

impl JwtAuthenticator {
    pub fn new(secret: impl Into<Vec<u8>>, issuer: String, audience: String) -> Self {
        Self {
            key: JwtKey::Secret(Arc::new(secret.into())),
            issuer,
            audience,
        }
    }

    pub fn from_jwks(jwks: JwkSet, issuer: String, audience: String) -> Self {
        Self {
            key: JwtKey::Jwks(Arc::new(jwks)),
            issuer,
            audience,
        }
    }

    fn decode(&self, token: &str) -> Result<Claims, AuthenticationError> {
        let header = decode_header(token).map_err(|_| AuthenticationError::InvalidCredentials)?;
        let (key, algorithm) = match &self.key {
            JwtKey::Secret(secret) => (
                DecodingKey::from_secret(secret.as_slice()),
                Algorithm::HS256,
            ),
            JwtKey::Jwks(jwks) => {
                let jwk = header
                    .kid
                    .as_deref()
                    .and_then(|kid| jwks.find(kid))
                    .or_else(|| (jwks.keys.len() == 1).then(|| &jwks.keys[0]))
                    .ok_or(AuthenticationError::InvalidCredentials)?;
                let algorithm = match jwk.common.key_algorithm {
                    Some(KeyAlgorithm::HS256) => Algorithm::HS256,
                    Some(KeyAlgorithm::HS384) => Algorithm::HS384,
                    Some(KeyAlgorithm::HS512) => Algorithm::HS512,
                    Some(KeyAlgorithm::ES256) => Algorithm::ES256,
                    Some(KeyAlgorithm::ES384) => Algorithm::ES384,
                    Some(KeyAlgorithm::RS256) | None => Algorithm::RS256,
                    Some(KeyAlgorithm::RS384) => Algorithm::RS384,
                    Some(KeyAlgorithm::RS512) => Algorithm::RS512,
                    Some(KeyAlgorithm::PS256) => Algorithm::PS256,
                    Some(KeyAlgorithm::PS384) => Algorithm::PS384,
                    Some(KeyAlgorithm::PS512) => Algorithm::PS512,
                    Some(_) => {
                        return Err(AuthenticationError::InvalidCredentials);
                    }
                };
                (
                    DecodingKey::from_jwk(jwk)
                        .map_err(|_| AuthenticationError::InvalidCredentials)?,
                    algorithm,
                )
            }
        };

        let mut validation = Validation::new(algorithm);
        validation.set_issuer(&[self.issuer.as_str()]);
        validation.set_audience(&[self.audience.as_str()]);
        validation.required_spec_claims = ["exp", "iss", "aud", "sub"]
            .into_iter()
            .map(str::to_string)
            .collect();

        decode::<Claims>(token, &key, &validation)
            .map(|data| data.claims)
            .map_err(|error| match error.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthenticationError::Expired,
                jsonwebtoken::errors::ErrorKind::InvalidAudience => {
                    AuthenticationError::WrongAudience
                }
                _ => AuthenticationError::InvalidCredentials,
            })
    }
}

#[async_trait]
impl PrincipalAuthenticator for JwtAuthenticator {
    async fn authenticate(
        &self,
        credentials: &Credentials,
    ) -> Result<PrincipalClaims, AuthenticationError> {
        let token = match credentials {
            Credentials::Bearer(token) => token.as_str(),
            _ => return Err(AuthenticationError::MissingCredentials),
        };
        let claims = self.decode(token)?;

        let hrn: hodei_authz::hodei_hrn::Hrn = claims
            .sub
            .parse()
            .map_err(|_| AuthenticationError::InvalidCredentials)?;
        if hrn.account() != claims.account {
            return Err(AuthenticationError::InvalidCredentials);
        }

        Ok(PrincipalClaims::new(
            PrincipalId::new(claims.sub).map_err(|_| AuthenticationError::InvalidCredentials)?,
            AccountId::new(claims.account).map_err(|_| AuthenticationError::InvalidCredentials)?,
        ))
    }
}
