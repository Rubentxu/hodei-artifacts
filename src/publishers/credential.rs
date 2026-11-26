//! The short publisher credential the exchange mints (HSP-321 tramo j).
//!
//! A publisher credential is a JWT the hub signs itself, bound to one
//! account, one repository and a closed set of actions, with a short
//! lifetime. Two properties keep it apart from the user bearer flow:
//!
//! 1. **Its own key.** The signing key is derived from a dedicated secret
//!    with a domain separator, and the token declares the issuer
//!    `hodei-publishers` and the header type `HODEI-PUBLISHER`. The user
//!    authenticator requires the issuer and key of the deployment, so a
//!    publisher credential presented to any user surface is not a
//!    credential there (401).
//! 2. **No secrets inside.** The claims carry identities and scope only.
//!    The OIDC token itself never travels in them.

use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, decode_header, encode,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The header `typ` that tells a publisher credential from a user token.
pub const TOKEN_TYPE: &str = "HODEI-PUBLISHER";
/// The issuer the hub puts on its own publisher credentials.
pub const CREDENTIAL_ISSUER: &str = "hodei-publishers";
/// The audience of a publisher credential: this service.
pub const CREDENTIAL_AUDIENCE: &str = "hodei-artifacts";
/// The only action a publisher credential can carry.
pub const UPLOAD_ACTION: &str = "upload";
/// The shortest secret the service accepts to derive the signing key.
pub const MIN_SECRET_BYTES: usize = 32;

const KEY_DOMAIN: &[u8] = b"hodei-publisher-credential-v1";

/// The claims of a publisher credential.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublisherClaims {
    pub iss: String,
    pub aud: String,
    /// The workload principal the trail attributes to.
    pub sub: String,
    /// The account the grant publishes into.
    pub account: String,
    /// The repository the credential is bound to.
    pub repo: String,
    /// The granted actions (`["upload"]`).
    pub acts: Vec<String>,
    /// The registry entry that authorized the exchange. The publish surface
    /// re-checks that it still exists, so removing it revokes at once.
    pub publisher: String,
    /// The fingerprint of the registry record at the exchange: a record that
    /// was deleted and recreated with other content does not resurrect the
    /// credential.
    pub fp: String,
    pub iat: i64,
    pub exp: i64,
    pub jti: String,
}

/// A publisher credential that passed signature and lifetime checks.
#[derive(Debug, Clone)]
pub struct VerifiedPublisher {
    pub claims: PublisherClaims,
}

impl VerifiedPublisher {
    /// Whether the credential may publish into `repository`.
    pub fn may_upload_to(&self, repository: &str) -> bool {
        self.claims.repo == repository && self.claims.acts.iter().any(|a| a == UPLOAD_ACTION)
    }
}

/// A token that is not a live publisher credential. It says nothing more:
/// the reason is not for the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidCredential;

/// The signing material of the hub for publisher credentials.
pub struct CredentialSigner {
    encoding: EncodingKey,
    decoding: DecodingKey,
}

impl CredentialSigner {
    /// Derive the signing key from the dedicated secret.
    pub fn derive(secret: &[u8]) -> Result<Self, String> {
        if secret.len() < MIN_SECRET_BYTES {
            return Err(format!(
                "the publisher credential secret must have at least {MIN_SECRET_BYTES} bytes"
            ));
        }
        let mut hasher = Sha256::new();
        hasher.update(KEY_DOMAIN);
        hasher.update([0u8]);
        hasher.update((secret.len() as u64).to_be_bytes());
        hasher.update(secret);
        let key = hasher.finalize();
        Ok(Self {
            encoding: EncodingKey::from_secret(&key),
            decoding: DecodingKey::from_secret(&key),
        })
    }

    /// Mint a credential for the claims.
    pub fn mint(&self, claims: &PublisherClaims) -> Result<String, String> {
        let mut header = Header::new(Algorithm::HS256);
        header.typ = Some(TOKEN_TYPE.to_string());
        encode(&header, claims, &self.encoding).map_err(|_| "cannot sign".to_string())
    }

    /// Verify a publisher credential: algorithm, type, signature, issuer,
    /// audience and lifetime (no leeway: the credential is ours).
    pub fn verify(&self, token: &str) -> Result<VerifiedPublisher, InvalidCredential> {
        let header = decode_header(token).map_err(|_| InvalidCredential)?;
        if header.alg != Algorithm::HS256 || header.typ.as_deref() != Some(TOKEN_TYPE) {
            return Err(InvalidCredential);
        }
        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_issuer(&[CREDENTIAL_ISSUER]);
        validation.set_audience(&[CREDENTIAL_AUDIENCE]);
        validation.leeway = 0;
        validation.validate_nbf = true;
        validation.required_spec_claims = ["exp", "iss", "aud", "sub"]
            .into_iter()
            .map(str::to_string)
            .collect();
        let data = decode::<PublisherClaims>(token, &self.decoding, &validation)
            .map_err(|_| InvalidCredential)?;
        Ok(VerifiedPublisher {
            claims: data.claims,
        })
    }
}

/// Whether a bearer token announces itself as a publisher credential.
///
/// It reads the header only: it answers which verifier looks at the token,
/// not whether the token is good.
pub fn announces_publisher(token: &str) -> bool {
    decode_header(token)
        .ok()
        .is_some_and(|header| header.typ.as_deref() == Some(TOKEN_TYPE))
}

/// The workload principal an exchange attributes to: derived from the
/// verified OIDC subject (sanitized, plus a digest so two subjects never
/// collapse into one principal).
pub fn workload_principal(account: &str, subject: &str) -> String {
    let sanitized: String = subject
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .take(64)
        .collect();
    let digest = Sha256::digest(subject.as_bytes());
    let short: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    format!("hrn:hodei:iam::{account}:Workload/{sanitized}-{short}")
}

/// The identifier of a new credential.
pub fn new_jti() -> String {
    let bytes: [u8; 16] = rand::random();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
