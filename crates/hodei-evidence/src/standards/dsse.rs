//! The DSSE envelope and the verification of its signatures.
//!
//! The envelope is the one Sigstore and in-toto use: a payload type, a
//! payload and the signatures over it. The signatures are over the **PAE**
//! --the pre-authentication encoding-- and not over the payload bytes, so
//! that the payload type is part of what is signed.
//!
//! # Why the PAE and not the payload
//!
//! A signature over the raw payload bytes would prove that somebody signed
//! *some* bytes. The payload type is what says what those bytes mean, and
//! without it in the signed material the same bytes could be replayed as
//! something else. The PAE puts the type, and its length, in front of the
//! payload, so that what is signed says both.
//!
//! # What is verified, and what is not
//!
//! The verification checks the signature against the key of its issuer and
//! nothing more. It does not talk to a transparency log, it does not renew
//! a trust root, and it does not decide whether the issuer was allowed to
//! sign. Those are online verification, and they are another milestone.
//! What this gives is the difference the domain needs: an attestation whose
//! signature holds, and one whose does not, with the reason.

use crate::domain::standards::attestation::{
    AttestationVerification, DsseEnvelope, SignatureFailure,
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use std::collections::BTreeMap;

/// The bytes that are signed: the pre-authentication encoding of a payload.
///
/// The encoding is the one DSSE defines:
///
/// ```text
/// DSSEv1 <len(payloadType)> <payloadType> <len(payload)> <payload>
/// ```
///
/// The lengths are the number of **bytes**, and they are written out so
/// that the boundary between the type and the payload cannot be moved.
pub fn pae(payload_type: &str, payload: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(payload.len() + payload_type.len() + 32);
    bytes.extend_from_slice(b"DSSEv1 ");
    bytes.extend_from_slice(payload_type.len().to_string().as_bytes());
    bytes.push(b' ');
    bytes.extend_from_slice(payload_type.as_bytes());
    bytes.push(b' ');
    bytes.extend_from_slice(payload.len().to_string().as_bytes());
    bytes.push(b' ');
    bytes.extend_from_slice(payload);
    bytes
}

/// A public key that can verify a signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationKey {
    /// The identifier of the key. It is what a signature names.
    pub key_id: String,
    /// The raw uncompressed point of an ECDSA P-256 key: 65 bytes, starting
    /// with `0x04`.
    pub point: Vec<u8>,
}

/// The keys a deployment knows.
///
/// The keys come from outside Hodei: they are the trust roots of the
/// deployment, and the fixtures use a key that exists only for the tests.
/// A key that Hodei does not know is not a failure of the signature: it is
/// a failure of the trust, and it is reported as such.
#[derive(Debug, Clone, Default)]
pub struct KeyRing {
    keys: BTreeMap<String, VerificationKey>,
}

impl KeyRing {
    /// A ring with no keys in it.
    pub fn new() -> Self {
        Self::default()
    }

    /// A ring with one key in it.
    pub fn with_key(key: VerificationKey) -> Self {
        let mut ring = Self::new();
        ring.add(key);
        ring
    }

    /// Adds a key to the ring.
    pub fn add(&mut self, key: VerificationKey) {
        self.keys.insert(key.key_id.clone(), key);
    }

    /// The key with that identifier, when the ring knows it.
    pub fn get(&self, key_id: &str) -> Option<&VerificationKey> {
        self.keys.get(key_id)
    }

    /// How many keys the ring holds.
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    /// Whether the ring holds no keys.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

/// The outcome of verifying the signatures of an envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationOutcome {
    /// How far the verification goes.
    pub verification: AttestationVerification,
    /// The bytes of the payload, when the envelope could be read.
    pub payload: Option<Vec<u8>>,
}

/// Verifies the signatures of a DSSE envelope.
///
/// The envelope is checked against the keys of the ring. The first
/// signature that verifies decides the outcome; a signature that does not
/// verify is reported with its reason, and an envelope whose key the ring
/// does not know is reported as an unknown key rather than as a bad
/// signature -- those are different failures and they lead to different
/// repairs.
pub fn verify_envelope(envelope: &DsseEnvelope, keys: &KeyRing) -> VerificationOutcome {
    let payload = match decode_payload(envelope) {
        Ok(payload) => payload,
        Err(reason) => {
            return VerificationOutcome {
                verification: AttestationVerification::Invalid { reason },
                payload: None,
            };
        }
    };

    if envelope.signatures.is_empty() {
        return VerificationOutcome {
            verification: AttestationVerification::Unverified {
                reason: SignatureFailure::NoSignature,
            },
            payload: Some(payload),
        };
    }

    let signed = pae(&envelope.payload_type, &payload);
    let mut failure: Option<AttestationVerification> = None;

    // Every signature of the envelope is tried, and the first one that
    // holds decides. A signature that does not hold is a failure, and the
    // first failure is the one that is reported: it is the first thing
    // that went wrong, and it is the one a repair starts from.
    for signature in &envelope.signatures {
        let Some(key) = keys.get(&signature.keyid) else {
            // The key is not one the deployment knows. That is a failure of
            // the trust and not a bad signature, and the two lead to
            // different repairs.
            failure.get_or_insert(AttestationVerification::Unverified {
                reason: SignatureFailure::UnknownKey(signature.keyid.clone()),
            });
            continue;
        };

        let bytes = match BASE64.decode(signature.sig.as_bytes()) {
            Ok(bytes) => bytes,
            Err(error) => {
                failure.get_or_insert(AttestationVerification::Invalid {
                    reason: SignatureFailure::MalformedSignature(error.to_string()),
                });
                continue;
            }
        };

        match verify_signature(&signed, &bytes, key) {
            Ok(()) => {
                return VerificationOutcome {
                    verification: AttestationVerification::Verified {
                        key_id: signature.keyid.clone(),
                    },
                    payload: Some(payload),
                };
            }
            Err(reason) => {
                failure.get_or_insert(AttestationVerification::Invalid { reason });
            }
        }
    }

    VerificationOutcome {
        verification: failure.unwrap_or(AttestationVerification::Unverified {
            reason: SignatureFailure::NoSignature,
        }),
        payload: Some(payload),
    }
}

/// Verifies one ECDSA P-256 signature over the bytes, with a key.
fn verify_signature(
    message: &[u8],
    signature: &[u8],
    key: &VerificationKey,
) -> Result<(), SignatureFailure> {
    use ring::signature;

    if key.point.len() != 65 || key.point[0] != 0x04 {
        return Err(SignatureFailure::MalformedSignature(
            "the public key is not an uncompressed P-256 point".to_string(),
        ));
    }

    let verifier =
        signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_ASN1, key.point.as_slice());
    verifier
        .verify(message, signature)
        .map_err(|_| SignatureFailure::SignatureDoesNotMatch)
}

/// Decodes the payload of an envelope.
fn decode_payload(envelope: &DsseEnvelope) -> Result<Vec<u8>, SignatureFailure> {
    BASE64
        .decode(envelope.payload.as_bytes())
        .map_err(|error| SignatureFailure::MalformedPayload(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pae_puts_the_type_and_its_length_in_front_of_the_payload() {
        let encoded = pae("application/vnd.in-toto+json", b"hello");
        assert_eq!(
            String::from_utf8_lossy(&encoded),
            "DSSEv1 28 application/vnd.in-toto+json 5 hello"
        );
    }

    #[test]
    fn two_payloads_with_the_same_bytes_and_different_types_have_different_encodings() {
        let one = pae("a", b"bc");
        let other = pae("ab", b"c");
        assert_ne!(one, other);
    }
}
