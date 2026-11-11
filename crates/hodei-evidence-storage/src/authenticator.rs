//! The authentication of the producer of an envelope.
//!
//! The envelope carries a signature over its canonical form, and this
//! adapter checks it with a secret that belongs to the producer. The
//! producer is vouched for BEFORE anything is accepted: an envelope that
//! nobody vouched for must not reach the store.
//!
//! The secret is per producer, and the accounts a producer may speak for are
//! declared with it. A producer that sends for an account it does not own is
//! refused, even when its signature is correct: a valid signature says who
//! sent the envelope, not that it was allowed to send it.

use hodei_evidence::{AuthenticationError, EnvelopeAuthenticator, IngestionEnvelope, Producer};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// The credentials of a producer.
#[derive(Clone)]
pub struct ProducerCredentials {
    /// The secret the producer signs with.
    pub secret: Vec<u8>,
    /// The accounts the producer may speak for.
    pub accounts: Vec<String>,
}

/// The authenticator that knows a set of producers.
#[derive(Clone, Default)]
pub struct HmacEnvelopeAuthenticator {
    credentials: Arc<RwLock<HashMap<String, ProducerCredentials>>>,
}

impl HmacEnvelopeAuthenticator {
    /// An authenticator that knows nobody yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers the credentials of a producer.
    pub fn register(&self, producer_id: impl Into<String>, credentials: ProducerCredentials) {
        self.credentials
            .write()
            .expect("the credentials lock")
            .insert(producer_id.into(), credentials);
    }

    /// The signature an envelope would have with these credentials.
    pub fn sign(&self, envelope: &IngestionEnvelope) -> Option<String> {
        let credentials = self
            .credentials
            .read()
            .expect("the credentials lock")
            .get(&envelope.producer.id)
            .cloned()?;
        Some(sign(&credentials.secret, &envelope.canonical_bytes()))
    }
}

/// The signature of the bytes with the secret of the producer.
fn sign(secret: &[u8], bytes: &[u8]) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let mut mac =
        <Hmac<Sha256> as Mac>::new_from_slice(secret).expect("a key of any length works for HMAC");
    mac.update(bytes);
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[async_trait::async_trait]
impl EnvelopeAuthenticator for HmacEnvelopeAuthenticator {
    /// Authenticates an envelope.
    async fn authenticate(&self, envelope: &IngestionEnvelope) -> Result<(), AuthenticationError> {
        let credentials = self
            .credentials
            .read()
            .expect("the credentials lock")
            .get(&envelope.producer.id)
            .cloned()
            .ok_or(AuthenticationError::UnknownProducer)?;

        let expected = sign(&credentials.secret, &envelope.canonical_bytes());
        if !constant_time_eq(expected.as_bytes(), envelope.signature.as_bytes()) {
            return Err(AuthenticationError::InvalidSignature);
        }

        let account = envelope.tenant_hrn.account_id();
        if !credentials
            .accounts
            .iter()
            .any(|allowed| allowed == account)
        {
            return Err(AuthenticationError::NotAllowedForAccount);
        }

        Ok(())
    }
}

/// Compares two byte sequences without letting the time depend on where they
/// differ.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0u8;
    for (left, right) in left.iter().zip(right.iter()) {
        difference |= left ^ right;
    }
    difference == 0
}

/// The producer of a delivery, for the tests and the wiring.
pub fn producer(id: &str) -> Producer {
    Producer::new("connector", id).with_version("1.0.0")
}
