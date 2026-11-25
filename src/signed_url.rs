//! Short-lived, signed capability URLs.
//!
//! Some real clients download an artifact with a plain HTTP client that
//! carries no credential: the Terraform CLI does so for the archive of a
//! provider network mirror, whatever the `credentials` block says. A hub
//! that authenticates every read cannot serve that request, and serving it
//! anonymously would publish the archive to anybody who can guess its path.
//!
//! The way out is a capability: when the hub answers an **authenticated**
//! request that names an archive (the version document of the mirror), it
//! hands out a URL for that archive alone, signed with a key only the hub
//! holds and valid for a few minutes. Redeeming the URL needs no
//! `Authorization` header, and nothing else.
//!
//! What a signature binds, and why each field is there:
//!
//! - the **method** (`GET`; `HEAD` is the same read), so a URL for a read is
//!   never a URL for a write;
//! - the **path** and the **repository**, so the URL serves that archive of
//!   that repository and no other;
//! - the **account** and the **principal** it was issued to, so it cannot be
//!   replayed against the catalog of another account and the audit trail
//!   names who asked for it;
//! - the **expiry**, so the capability dies by itself;
//! - the **key id**, so a key can be rotated without cutting the URLs that
//!   are already in flight.
//!
//! The hub signs with the FIRST key of the ring and verifies with ALL of
//! them. Without a key there is nothing to sign with: the hub refuses to
//! issue (fail closed). It never falls back to a default key and never
//! derives one from the JWT or JWKS material.

use std::fmt;

use base64::Engine as _;
use ring::hmac;

/// The shortest secret a key may have, in bytes.
pub const MINIMUM_SECRET_BYTES: usize = 32;

/// The lifetime of a URL when the deployment does not say.
pub const DEFAULT_TTL_SECS: u64 = 300;

/// The longest lifetime a deployment may configure, and the farthest an
/// expiry may lie in the future when a URL is verified.
pub const MAXIMUM_TTL_SECS: u64 = 3_600;

/// The clock difference between replicas that verification tolerates, in
/// seconds: an expired URL is still honoured this long, and an expiry this
/// far beyond the maximum lifetime is still believed. Nothing larger.
pub const CLOCK_SKEW_SECS: i64 = 5;

/// The domain separator of the signed message: a signature made for another
/// purpose with the same key can never verify here.
const DOMAIN: &[u8] = b"hodei-signed-url-v1";

/// The query parameters of a signed URL.
const EXPIRY: &str = "hodei_exp";
const KEY_ID: &str = "hodei_kid";
const ACCOUNT: &str = "hodei_acct";
const SUBJECT: &str = "hodei_sub";
const SIGNATURE: &str = "hodei_sig";

/// Why a URL could not be issued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueError {
    /// The deployment configured no signing key.
    NotConfigured,
}

impl fmt::Display for IssueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotConfigured => formatter.write_str(
                "no signing key is configured: add a key to [signing] to let the mirror issue archive URLs",
            ),
        }
    }
}

impl std::error::Error for IssueError {}

/// Why a signed URL was not accepted.
///
/// The variants exist for the tests and the logs of the operator; the answer
/// to the client is the same for all of them, so that a probe learns nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyError {
    /// The deployment has no key to verify with.
    NotConfigured,
    /// The query is not exactly the signed-URL parameters.
    Malformed,
    /// The key id names no key of the ring.
    UnknownKey,
    /// The signature does not match what was signed.
    BadSignature,
    /// The URL has expired.
    Expired,
    /// The expiry lies further in the future than any URL may live.
    TooFarInTheFuture,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NotConfigured => "no signing key is configured",
            Self::Malformed => "the signed URL is malformed",
            Self::UnknownKey => "the signed URL names an unknown key",
            Self::BadSignature => "the signature does not match",
            Self::Expired => "the signed URL has expired",
            Self::TooFarInTheFuture => "the expiry of the signed URL is too far away",
        })
    }
}

impl std::error::Error for VerifyError {}

/// One key of the ring.
///
/// The secret is held as an HMAC key and nowhere else; `Debug` never prints
/// it.
#[derive(Clone)]
pub struct SigningKey {
    id: String,
    key: hmac::Key,
}

impl SigningKey {
    /// A key from its id and its secret.
    ///
    /// # Errors
    ///
    /// A message when the id is not usable or the secret is shorter than
    /// [`MINIMUM_SECRET_BYTES`]. The message never contains the secret.
    pub fn new(id: &str, secret: &[u8]) -> Result<Self, String> {
        if !is_valid_key_id(id) {
            return Err(format!(
                "the signing key id `{id}` is not valid: use 1 to 32 characters among letters, digits, `-` and `_`"
            ));
        }
        if secret.len() < MINIMUM_SECRET_BYTES {
            return Err(format!(
                "the secret of the signing key `{id}` has {} bytes; at least {MINIMUM_SECRET_BYTES} are required (generate one with `openssl rand -hex 32`)",
                secret.len()
            ));
        }
        Ok(Self {
            id: id.to_string(),
            key: hmac::Key::new(hmac::HMAC_SHA256, secret),
        })
    }

    /// The id of the key.
    pub fn id(&self) -> &str {
        &self.id
    }
}

impl fmt::Debug for SigningKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SigningKey")
            .field("id", &self.id)
            .field("secret", &"<redacted>")
            .finish()
    }
}

/// A key id is short and boring: it travels in a URL.
pub fn is_valid_key_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// What a signed URL is a capability for.
#[derive(Debug, Clone, Copy)]
pub struct Scope<'a> {
    /// The HTTP method of the read; `HEAD` is signed as `GET`.
    pub method: &'a str,
    /// The path of the URL, as the client sends it, without the query.
    pub path: &'a str,
    /// The key of the repository that serves the path.
    pub repository: &'a str,
    /// The account the URL was issued in.
    pub account: &'a str,
    /// The principal the URL was issued to.
    pub subject: &'a str,
}

/// The parameters a signed URL carries.
#[derive(Clone, PartialEq, Eq)]
pub struct Claims {
    /// The instant after which the URL is dead, in seconds since the epoch.
    pub expires_at: i64,
    /// The id of the key that signed.
    pub key_id: String,
    /// The account the URL was issued in.
    pub account: String,
    /// The principal the URL was issued to.
    pub subject: String,
    signature: Vec<u8>,
}

impl fmt::Debug for Claims {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Claims")
            .field("expires_at", &self.expires_at)
            .field("key_id", &self.key_id)
            .field("account", &self.account)
            .field("subject", &self.subject)
            .field("signature", &"<redacted>")
            .finish()
    }
}

impl Claims {
    /// Whether the query of a request carries a signature at all.
    pub fn is_present(query: &str) -> bool {
        url::form_urlencoded::parse(query.as_bytes()).any(|(name, _)| name == SIGNATURE)
    }

    /// The claims of a query.
    ///
    /// The query has to be exactly the five parameters of a signed URL,
    /// each once: an extra or a repeated parameter is a different request
    /// than the one that was signed, and is refused.
    ///
    /// # Errors
    ///
    /// [`VerifyError::Malformed`] when the query is anything else.
    pub fn parse(query: &str) -> Result<Self, VerifyError> {
        let (mut expiry, mut key_id, mut account, mut subject, mut signature) =
            (None, None, None, None, None);
        for (name, value) in url::form_urlencoded::parse(query.as_bytes()) {
            let slot = match name.as_ref() {
                EXPIRY => &mut expiry,
                KEY_ID => &mut key_id,
                ACCOUNT => &mut account,
                SUBJECT => &mut subject,
                SIGNATURE => &mut signature,
                _ => return Err(VerifyError::Malformed),
            };
            if slot.replace(value.into_owned()).is_some() {
                return Err(VerifyError::Malformed);
            }
        }
        let (Some(expiry), Some(key_id), Some(account), Some(subject), Some(signature)) =
            (expiry, key_id, account, subject, signature)
        else {
            return Err(VerifyError::Malformed);
        };
        Ok(Self {
            expires_at: expiry.parse().map_err(|_| VerifyError::Malformed)?,
            key_id,
            account,
            subject,
            signature: base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(signature)
                .map_err(|_| VerifyError::Malformed)?,
        })
    }

    /// The query string of the claims.
    pub fn to_query(&self) -> String {
        url::form_urlencoded::Serializer::new(String::new())
            .append_pair(EXPIRY, &self.expires_at.to_string())
            .append_pair(KEY_ID, &self.key_id)
            .append_pair(ACCOUNT, &self.account)
            .append_pair(SUBJECT, &self.subject)
            .append_pair(
                SIGNATURE,
                &base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&self.signature),
            )
            .finish()
    }
}

/// The ring of keys and the lifetime of what it signs.
///
/// An empty ring is a valid value: it is a deployment that has no key, and
/// it neither issues nor verifies anything.
#[derive(Clone, Debug)]
pub struct SignedUrls {
    keys: Vec<SigningKey>,
    ttl_secs: u64,
}

impl Default for SignedUrls {
    fn default() -> Self {
        Self::unconfigured()
    }
}

impl SignedUrls {
    /// A deployment without keys: it issues and verifies nothing.
    pub fn unconfigured() -> Self {
        Self {
            keys: Vec::new(),
            ttl_secs: DEFAULT_TTL_SECS,
        }
    }

    /// A ring. The first key signs, all of them verify.
    ///
    /// # Errors
    ///
    /// A message when the lifetime is outside `1..=`[`MAXIMUM_TTL_SECS`] or
    /// two keys share an id.
    pub fn new(keys: Vec<SigningKey>, ttl_secs: u64) -> Result<Self, String> {
        if !(1..=MAXIMUM_TTL_SECS).contains(&ttl_secs) {
            return Err(format!(
                "the lifetime of a signed URL must be between 1 and {MAXIMUM_TTL_SECS} seconds, not {ttl_secs}"
            ));
        }
        for (index, key) in keys.iter().enumerate() {
            if keys[..index].iter().any(|other| other.id == key.id) {
                return Err(format!("the signing key id `{}` is repeated", key.id));
            }
        }
        Ok(Self { keys, ttl_secs })
    }

    /// Whether the deployment can issue URLs.
    pub fn is_configured(&self) -> bool {
        !self.keys.is_empty()
    }

    /// The lifetime of the URLs it issues, in seconds.
    pub fn ttl_secs(&self) -> u64 {
        self.ttl_secs
    }

    /// Sign a capability for `scope`, valid from `now` for the lifetime of
    /// the ring (`now` is seconds since the epoch).
    ///
    /// # Errors
    ///
    /// [`IssueError::NotConfigured`] when the ring is empty: nothing is
    /// signed with a default key.
    pub fn issue(&self, scope: &Scope<'_>, now: i64) -> Result<Claims, IssueError> {
        let key = self.keys.first().ok_or(IssueError::NotConfigured)?;
        let expires_at = now.saturating_add(self.ttl_secs as i64);
        let message = the_message(scope, expires_at, &key.id);
        let tag = hmac::sign(&key.key, &message);
        Ok(Claims {
            expires_at,
            key_id: key.id.clone(),
            account: scope.account.to_string(),
            subject: scope.subject.to_string(),
            signature: tag.as_ref().to_vec(),
        })
    }

    /// Check that `claims` are a capability for `scope`, at `now`.
    ///
    /// The account and the principal of `scope` come from the claims
    /// themselves: they are only trusted because the signature covers them.
    ///
    /// The signature is checked first and in constant time; only then is the
    /// expiry looked at. Nothing is read from any store before this returns.
    ///
    /// # Errors
    ///
    /// The reason the capability is not valid; callers must answer all of
    /// them alike.
    pub fn verify(
        &self,
        method: &str,
        path: &str,
        repository: &str,
        claims: &Claims,
        now: i64,
    ) -> Result<(), VerifyError> {
        if !self.is_configured() {
            return Err(VerifyError::NotConfigured);
        }
        let key = self
            .keys
            .iter()
            .find(|key| key.id == claims.key_id)
            .ok_or(VerifyError::UnknownKey)?;
        let scope = Scope {
            method,
            path,
            repository,
            account: &claims.account,
            subject: &claims.subject,
        };
        let message = the_message(&scope, claims.expires_at, &key.id);
        // `ring` compares the tag in constant time.
        hmac::verify(&key.key, &message, &claims.signature)
            .map_err(|_| VerifyError::BadSignature)?;
        if claims.expires_at.saturating_add(CLOCK_SKEW_SECS) < now {
            return Err(VerifyError::Expired);
        }
        let farthest = now
            .saturating_add(MAXIMUM_TTL_SECS as i64)
            .saturating_add(CLOCK_SKEW_SECS);
        if claims.expires_at > farthest {
            return Err(VerifyError::TooFarInTheFuture);
        }
        Ok(())
    }
}

/// The bytes that are signed: the domain, then every field prefixed with its
/// length, so that no two different scopes produce the same message.
fn the_message(scope: &Scope<'_>, expires_at: i64, key_id: &str) -> Vec<u8> {
    // A `HEAD` is the same read as a `GET`.
    let method = if scope.method.eq_ignore_ascii_case("HEAD") {
        "GET".to_string()
    } else {
        scope.method.to_ascii_uppercase()
    };
    let expiry = expires_at.to_string();
    let mut message = Vec::new();
    message.extend_from_slice(DOMAIN);
    for field in [
        method.as_str(),
        scope.path,
        scope.repository,
        scope.account,
        scope.subject,
        expiry.as_str(),
        key_id,
    ] {
        message.extend_from_slice(&(field.len() as u32).to_be_bytes());
        message.extend_from_slice(field.as_bytes());
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;

    fn key(id: &str, fill: u8) -> SigningKey {
        SigningKey::new(id, &[fill; 32]).expect("a valid key")
    }

    fn ring(keys: Vec<SigningKey>) -> SignedUrls {
        SignedUrls::new(keys, 300).expect("a valid ring")
    }

    fn scope() -> Scope<'static> {
        Scope {
            method: "GET",
            path: "/terraform/mirror/registry.terraform.io/hashicorp/null/null_3.2.1_linux_amd64.zip",
            repository: "mirror",
            account: "acct-1",
            subject: "alice",
        }
    }

    fn verify(
        urls: &SignedUrls,
        scope: &Scope<'_>,
        claims: &Claims,
        now: i64,
    ) -> Result<(), VerifyError> {
        urls.verify(scope.method, scope.path, scope.repository, claims, now)
    }

    #[test]
    fn a_fresh_url_verifies() {
        let urls = ring(vec![key("a", 1)]);
        let claims = urls.issue(&scope(), NOW).expect("issued");
        assert_eq!(verify(&urls, &scope(), &claims, NOW), Ok(()));
        assert_eq!(verify(&urls, &scope(), &claims, NOW + 299), Ok(()));
    }

    #[test]
    fn a_head_reads_what_a_get_was_signed_for() {
        let urls = ring(vec![key("a", 1)]);
        let claims = urls.issue(&scope(), NOW).expect("issued");
        let head = Scope {
            method: "HEAD",
            ..scope()
        };
        assert_eq!(verify(&urls, &head, &claims, NOW), Ok(()));
    }

    #[test]
    fn the_query_round_trips() {
        let urls = ring(vec![key("a", 1)]);
        let claims = urls.issue(&scope(), NOW).expect("issued");
        let parsed = Claims::parse(&claims.to_query()).expect("the query parses");
        assert_eq!(parsed, claims);
        assert_eq!(verify(&urls, &scope(), &parsed, NOW), Ok(()));
    }

    #[test]
    fn the_expiry_is_honoured_within_the_skew_and_not_beyond() {
        let urls = ring(vec![key("a", 1)]);
        let claims = urls.issue(&scope(), NOW).expect("issued");
        let end = NOW + 300;
        assert_eq!(verify(&urls, &scope(), &claims, end), Ok(()));
        assert_eq!(
            verify(&urls, &scope(), &claims, end + CLOCK_SKEW_SECS),
            Ok(())
        );
        assert_eq!(
            verify(&urls, &scope(), &claims, end + CLOCK_SKEW_SECS + 1),
            Err(VerifyError::Expired)
        );
        assert_eq!(
            verify(&urls, &scope(), &claims, end + 3_600),
            Err(VerifyError::Expired)
        );
    }

    #[test]
    fn an_expiry_beyond_the_longest_lifetime_is_not_believed() {
        // A holder of the key that signs a far-future expiry is not a
        // client of this hub: the bound is checked even with a valid
        // signature.
        let urls = ring(vec![key("a", 1)]);
        let long = SignedUrls::new(vec![key("a", 1)], MAXIMUM_TTL_SECS).expect("ring");
        let claims = long.issue(&scope(), NOW).expect("issued");
        assert_eq!(verify(&urls, &scope(), &claims, NOW), Ok(()));
        // The same key, an issuer whose clock is far ahead of the verifier.
        let ahead = long.issue(&scope(), NOW + 3_600).expect("issued");
        assert_eq!(
            verify(&urls, &scope(), &ahead, NOW),
            Err(VerifyError::TooFarInTheFuture)
        );
        // The bound is the lifetime plus the skew, and not a second more.
        let edge = long.issue(&scope(), NOW + CLOCK_SKEW_SECS).expect("issued");
        assert_eq!(verify(&urls, &scope(), &edge, NOW), Ok(()));
        let past_edge = long
            .issue(&scope(), NOW + CLOCK_SKEW_SECS + 1)
            .expect("issued");
        assert_eq!(
            verify(&urls, &scope(), &past_edge, NOW),
            Err(VerifyError::TooFarInTheFuture)
        );
    }

    #[test]
    fn the_signature_is_bound_to_path_repository_account_and_subject() {
        let urls = ring(vec![key("a", 1)]);
        let claims = urls.issue(&scope(), NOW).expect("issued");

        let other_path = Scope {
            path: "/terraform/mirror/registry.terraform.io/hashicorp/null/other.zip",
            ..scope()
        };
        assert_eq!(
            verify(&urls, &other_path, &claims, NOW),
            Err(VerifyError::BadSignature)
        );

        let other_repository = Scope {
            repository: "mirror-2",
            ..scope()
        };
        assert_eq!(
            verify(&urls, &other_repository, &claims, NOW),
            Err(VerifyError::BadSignature)
        );

        let other_method = Scope {
            method: "POST",
            ..scope()
        };
        assert_eq!(
            verify(&urls, &other_method, &claims, NOW),
            Err(VerifyError::BadSignature)
        );

        let mut other_account = claims.clone();
        other_account.account = "acct-2".to_string();
        assert_eq!(
            verify(&urls, &scope(), &other_account, NOW),
            Err(VerifyError::BadSignature)
        );

        let mut other_subject = claims.clone();
        other_subject.subject = "mallory".to_string();
        assert_eq!(
            verify(&urls, &scope(), &other_subject, NOW),
            Err(VerifyError::BadSignature)
        );
    }

    #[test]
    fn a_changed_expiry_or_key_id_or_signature_is_refused() {
        let urls = ring(vec![key("a", 1), key("b", 2)]);
        let claims = urls.issue(&scope(), NOW).expect("issued");

        let mut later = claims.clone();
        later.expires_at += 60;
        assert_eq!(
            verify(&urls, &scope(), &later, NOW),
            Err(VerifyError::BadSignature)
        );

        // Another key of the ring: the key id is part of the message and
        // the key is another one.
        let mut other_key = claims.clone();
        other_key.key_id = "b".to_string();
        assert_eq!(
            verify(&urls, &scope(), &other_key, NOW),
            Err(VerifyError::BadSignature)
        );

        let mut unknown = claims.clone();
        unknown.key_id = "zzz".to_string();
        assert_eq!(
            verify(&urls, &scope(), &unknown, NOW),
            Err(VerifyError::UnknownKey)
        );

        let mut flipped = claims.clone();
        flipped.signature[0] ^= 1;
        assert_eq!(
            verify(&urls, &scope(), &flipped, NOW),
            Err(VerifyError::BadSignature)
        );

        let mut truncated = claims.clone();
        truncated.signature.truncate(16);
        assert_eq!(
            verify(&urls, &scope(), &truncated, NOW),
            Err(VerifyError::BadSignature)
        );

        let mut empty = claims;
        empty.signature.clear();
        assert_eq!(
            verify(&urls, &scope(), &empty, NOW),
            Err(VerifyError::BadSignature)
        );
    }

    #[test]
    fn the_fields_cannot_be_shifted_into_each_other() {
        // Length-prefixing: moving a character from the account to the
        // subject must not produce the same message.
        let urls = ring(vec![key("a", 1)]);
        let one = Scope {
            account: "ab",
            subject: "c",
            ..scope()
        };
        let claims = urls.issue(&one, NOW).expect("issued");
        let mut shifted = claims;
        shifted.account = "a".to_string();
        shifted.subject = "bc".to_string();
        assert_eq!(
            verify(&urls, &scope(), &shifted, NOW),
            Err(VerifyError::BadSignature)
        );
    }

    #[test]
    fn a_rotation_keeps_the_urls_in_flight_alive() {
        // Step 1: the new key B joins behind A, A still signs.
        // Step 2: B moves to the front: it signs, A still verifies.
        let old = ring(vec![key("a", 1)]);
        let in_flight = old.issue(&scope(), NOW).expect("issued by A");
        let rotated = ring(vec![key("b", 2), key("a", 1)]);
        assert_eq!(verify(&rotated, &scope(), &in_flight, NOW), Ok(()));
        let fresh = rotated.issue(&scope(), NOW).expect("issued by B");
        assert_eq!(fresh.key_id, "b");
        assert_eq!(verify(&rotated, &scope(), &fresh, NOW), Ok(()));
        // Step 3: A leaves the ring and what it signed dies with it.
        let retired = ring(vec![key("b", 2)]);
        assert_eq!(
            verify(&retired, &scope(), &in_flight, NOW),
            Err(VerifyError::UnknownKey)
        );
        assert_eq!(verify(&retired, &scope(), &fresh, NOW), Ok(()));
    }

    #[test]
    fn a_key_with_the_same_id_and_another_secret_does_not_verify() {
        let issuer = ring(vec![key("a", 1)]);
        let claims = issuer.issue(&scope(), NOW).expect("issued");
        let impostor = ring(vec![key("a", 9)]);
        assert_eq!(
            verify(&impostor, &scope(), &claims, NOW),
            Err(VerifyError::BadSignature)
        );
    }

    #[test]
    fn without_a_key_nothing_is_issued_or_verified() {
        let none = SignedUrls::unconfigured();
        assert!(!none.is_configured());
        assert_eq!(none.issue(&scope(), NOW), Err(IssueError::NotConfigured));
        let claims = ring(vec![key("a", 1)])
            .issue(&scope(), NOW)
            .expect("issued");
        assert_eq!(
            verify(&none, &scope(), &claims, NOW),
            Err(VerifyError::NotConfigured)
        );
    }

    #[test]
    fn a_short_secret_or_a_bad_id_is_refused_without_echoing_the_secret() {
        let error = SigningKey::new("a", b"short-secret-value").expect_err("too short");
        assert!(error.contains("at least 32"), "{error}");
        assert!(!error.contains("short-secret-value"), "{error}");
        assert!(SigningKey::new("a", &[0u8; 31]).is_err());
        assert!(SigningKey::new("a", &[0u8; 32]).is_ok());
        for bad in ["", "a b", "a/b", "a?b", &"x".repeat(33)] {
            assert!(SigningKey::new(bad, &[0u8; 32]).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_ring_refuses_duplicate_ids_and_a_lifetime_outside_the_bounds() {
        assert!(SignedUrls::new(vec![key("a", 1), key("a", 2)], 300).is_err());
        assert!(SignedUrls::new(vec![key("a", 1)], 0).is_err());
        assert!(SignedUrls::new(vec![key("a", 1)], MAXIMUM_TTL_SECS + 1).is_err());
        assert!(SignedUrls::new(vec![key("a", 1)], MAXIMUM_TTL_SECS).is_ok());
    }

    #[test]
    fn the_query_must_be_exactly_the_signed_parameters() {
        let urls = ring(vec![key("a", 1)]);
        let query = urls.issue(&scope(), NOW).expect("issued").to_query();
        assert!(Claims::parse(&query).is_ok());
        assert_eq!(
            Claims::parse(&format!("{query}&extra=1")),
            Err(VerifyError::Malformed)
        );
        assert_eq!(
            Claims::parse(&format!("{query}&hodei_exp=1")),
            Err(VerifyError::Malformed)
        );
        assert_eq!(Claims::parse(""), Err(VerifyError::Malformed));
        assert_eq!(Claims::parse("hodei_sig=abc"), Err(VerifyError::Malformed));
        assert_eq!(
            Claims::parse(&query.replace("hodei_exp=", "hodei_exp=x")),
            Err(VerifyError::Malformed)
        );
        assert!(Claims::is_present(&query));
        assert!(!Claims::is_present("a=1"));
    }

    #[test]
    fn the_debug_output_holds_no_secret_and_no_signature() {
        let urls = ring(vec![key("a", 7)]);
        let claims = urls.issue(&scope(), NOW).expect("issued");
        let text = format!("{urls:?} {claims:?}");
        assert!(text.contains("<redacted>"));
        let signature = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(&claims.signature);
        assert!(!text.contains(&signature), "{text}");
    }
}
