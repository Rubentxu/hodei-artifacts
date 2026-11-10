//! Production JWT/OIDC-style credential verification for the executable.
//!
//! Tokens are validated with an HS256 secret, issuer, audience and expiration.
//! The subject must be a canonical HRN and its account must match the token's
//! account claim; `verify_principal` repeats that cross-check before producing
//! a verified principal.

use std::sync::Arc;

use async_trait::async_trait;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Claims {
    sub: String,
    account: String,
}

/// JWT authenticator configured from the security settings.
pub struct JwtAuthenticator {
    secret: Arc<Vec<u8>>,
    issuer: String,
    audience: String,
}

impl JwtAuthenticator {
    pub fn new(secret: impl Into<Vec<u8>>, issuer: String, audience: String) -> Self {
        Self {
            secret: Arc::new(secret.into()),
            issuer,
            audience,
        }
    }

    fn decode(&self, token: &str) -> Result<Claims, AuthenticationError> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[self.issuer.as_str()]);
        validation.set_audience(&[self.audience.as_str()]);
        validation.required_spec_claims = ["exp", "iss", "aud", "sub"]
            .into_iter()
            .map(str::to_string)
            .collect();

        decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.secret.as_slice()),
            &validation,
        )
        .map(|data| data.claims)
        .map_err(|error| match error.kind() {
            jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthenticationError::Expired,
            jsonwebtoken::errors::ErrorKind::InvalidAudience => AuthenticationError::WrongAudience,
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
        if hrn.tenant_id != claims.account {
            return Err(AuthenticationError::InvalidCredentials);
        }

        Ok(PrincipalClaims::new(
            PrincipalId::new(claims.sub).map_err(|_| AuthenticationError::InvalidCredentials)?,
            AccountId::new(claims.account).map_err(|_| AuthenticationError::InvalidCredentials)?,
        ))
    }
}
