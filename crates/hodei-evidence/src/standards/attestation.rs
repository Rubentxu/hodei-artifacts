//! The normalizer that reads an in-toto attestation, and verifies it.
//!
//! The attestation is read **and verified**. Copying the document is not
//! reading it: an attestation whose signature was not checked says what
//! its author wanted to say, and an attestation whose signature was checked
//! says what its issuer said. The domain keeps the difference, and a policy
//! reads it.
//!
//! # The two checks
//!
//! 1. **The signature.** The DSSE envelope is checked against the key of
//!    its issuer. A signature that does not hold is an invalid attestation
//!    with a reason, never a silent acceptance.
//! 2. **The subject.** The attestation is bound to the thing it is offered
//!    for. A provenance that speaks about a different digest is not
//!    evidence about this artifact, and saying so is part of reading it.
//!
//! # What is not checked
//!
//! The verification does not talk to a transparency log and it does not
//! renew a trust root. Those are online verification and they are another
//! milestone. What this gives is the difference the domain needs.

use crate::domain::content::ContentDigest;
use crate::domain::standards::attestation::{
    Attestation, AttestationStatement, AttestationSubject, AttestationVerification,
    IN_TOTO_PAYLOAD_TYPE, IN_TOTO_STATEMENT_TYPE, SignatureFailure, SlsaProvenance,
};
use crate::standards::dsse::{KeyRing, verify_envelope};
use serde_json::Value;
use std::collections::BTreeMap;

/// The version of the attestation normalizer.
pub const ATTESTATION_NORMALIZER_VERSION: &str = "1.0.0";

/// Why an attestation could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AttestationError {
    /// The envelope is not JSON.
    #[error("the envelope is not JSON: {0}")]
    MalformedJson(String),
    /// The envelope is not a DSSE envelope.
    #[error("the envelope is not a DSSE envelope: {0}")]
    MalformedEnvelope(String),
    /// The payload is not an in-toto statement.
    #[error("the payload is not an in-toto statement: {0}")]
    MalformedStatement(String),
    /// The statement is of a kind this version does not read.
    #[error("the statement is of a kind this version does not read: {0}")]
    UnsupportedStatement(String),
}

/// Reads an attestation from a DSSE envelope and verifies it.
///
/// The attestation is returned even when the verification fails: the
/// document is evidence in its own right, and the raw verification result
/// has to stay inspectable. What changes is the verification: a failed
/// signature is an invalid attestation with a reason.
pub fn read_attestation(envelope: &str, keys: &KeyRing) -> Result<Attestation, AttestationError> {
    let value: Value = serde_json::from_str(envelope)
        .map_err(|error| AttestationError::MalformedJson(error.to_string()))?;

    let payload_type = value
        .get("payloadType")
        .and_then(Value::as_str)
        .ok_or_else(|| AttestationError::MalformedEnvelope("it has no payload type".to_string()))?
        .to_string();
    let payload = value
        .get("payload")
        .and_then(Value::as_str)
        .ok_or_else(|| AttestationError::MalformedEnvelope("it has no payload".to_string()))?
        .to_string();
    let signatures = value
        .get("signatures")
        .and_then(Value::as_array)
        .map(|signatures| {
            signatures
                .iter()
                .map(
                    |signature| crate::domain::standards::attestation::DsseSignature {
                        keyid: signature
                            .get("keyid")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        sig: signature
                            .get("sig")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                    },
                )
                .collect()
        })
        .unwrap_or_default();

    let dsse = crate::domain::standards::attestation::DsseEnvelope {
        payload_type: payload_type.clone(),
        payload,
        signatures,
    };

    // The verification comes first: an envelope whose signature does not
    // hold is not worth reading, and the reason is more useful than a
    // complaint about the shape of the payload.
    let outcome = verify_envelope(&dsse, keys);

    let Some(payload_bytes) = outcome.payload else {
        return Ok(Attestation {
            statement: AttestationStatement {
                statement_type: String::new(),
                predicate_type: String::new(),
                subject: Vec::new(),
                predicate: SlsaProvenance {
                    build_type: String::new(),
                    builder_id: String::new(),
                    invocation_id: String::new(),
                    started_on: None,
                    finished_on: None,
                    resolved_dependencies: Vec::new(),
                },
            },
            verification: outcome.verification,
            source_digest: ContentDigest::of_bytes(envelope.as_bytes()),
        });
    };

    if payload_type != IN_TOTO_PAYLOAD_TYPE {
        return Ok(Attestation {
            statement: empty_statement(),
            verification: AttestationVerification::Invalid {
                reason: SignatureFailure::MalformedEnvelope(format!(
                    "the payload type {payload_type} is not the one of an in-toto statement"
                )),
            },
            source_digest: ContentDigest::of_bytes(envelope.as_bytes()),
        });
    }

    let statement: Value = serde_json::from_slice(&payload_bytes)
        .map_err(|error| AttestationError::MalformedStatement(error.to_string()))?;

    let statement_type = statement
        .get("_type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if statement_type != IN_TOTO_STATEMENT_TYPE {
        return Ok(Attestation {
            statement: empty_statement(),
            verification: AttestationVerification::Invalid {
                reason: SignatureFailure::UnsupportedStatement(statement_type),
            },
            source_digest: ContentDigest::of_bytes(envelope.as_bytes()),
        });
    }

    let parsed = parse_statement(&statement)?;
    let source_digest = ContentDigest::of_bytes(envelope.as_bytes());

    // The verification of the signature stands, unless the statement could
    // not be read at all: then the failure is the reading, not the
    // signature.
    let verification = match outcome.verification {
        AttestationVerification::Verified { key_id } => {
            AttestationVerification::Verified { key_id }
        }
        other => other,
    };

    Ok(Attestation {
        statement: parsed,
        verification,
        source_digest,
    })
}

/// Binds an attestation to the digest it is offered for.
///
/// The binding is the second half of reading an attestation: the signature
/// says the issuer said it, and the subject says what the issuer said it
/// about. An attestation about another artifact is not evidence about this
/// one, and the reason says so.
pub fn bind_to_subject(
    attestation: &mut Attestation,
    algorithm: &str,
    digest: &str,
) -> Result<(), SignatureFailure> {
    if attestation.speaks_about(algorithm, digest) {
        return Ok(());
    }
    Err(SignatureFailure::SubjectMismatch {
        expected: format!("{algorithm}:{digest}"),
    })
}

/// Reads the statement of an attestation.
fn parse_statement(statement: &Value) -> Result<AttestationStatement, AttestationError> {
    let statement_type = statement
        .get("_type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let predicate_type = statement
        .get("predicateType")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let subject = statement
        .get("subject")
        .and_then(Value::as_array)
        .map(|subjects| {
            subjects
                .iter()
                .map(|subject| AttestationSubject {
                    name: subject
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    digests: subject
                        .get("digest")
                        .and_then(Value::as_object)
                        .map(|digests| {
                            digests
                                .iter()
                                .filter_map(|(algorithm, value)| {
                                    value
                                        .as_str()
                                        .map(|value| (algorithm.clone(), value.to_string()))
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default();

    let predicate = statement.get("predicate").cloned().unwrap_or(Value::Null);
    let build_definition = predicate
        .get("buildDefinition")
        .cloned()
        .unwrap_or(Value::Null);
    let run_details = predicate.get("runDetails").cloned().unwrap_or(Value::Null);

    let resolved_dependencies = build_definition
        .get("resolvedDependencies")
        .and_then(Value::as_array)
        .map(|dependencies| {
            dependencies
                .iter()
                .map(
                    |dependency| crate::domain::standards::attestation::ResolvedDependency {
                        uri: dependency
                            .get("uri")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        digests: dependency
                            .get("digest")
                            .and_then(Value::as_object)
                            .map(|digests| {
                                digests
                                    .iter()
                                    .filter_map(|(algorithm, value)| {
                                        value
                                            .as_str()
                                            .map(|value| (algorithm.clone(), value.to_string()))
                                    })
                                    .collect::<BTreeMap<String, String>>()
                            })
                            .unwrap_or_default(),
                    },
                )
                .collect()
        })
        .unwrap_or_default();

    Ok(AttestationStatement {
        statement_type,
        predicate_type,
        subject,
        predicate: SlsaProvenance {
            build_type: build_definition
                .get("buildType")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            builder_id: run_details
                .get("builder")
                .and_then(|builder| builder.get("id"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            invocation_id: run_details
                .get("metadata")
                .and_then(|metadata| metadata.get("invocationId"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            started_on: run_details
                .get("metadata")
                .and_then(|metadata| metadata.get("startedOn"))
                .and_then(Value::as_str)
                .map(str::to_string),
            finished_on: run_details
                .get("metadata")
                .and_then(|metadata| metadata.get("finishedOn"))
                .and_then(Value::as_str)
                .map(str::to_string),
            resolved_dependencies,
        },
    })
}

/// A statement with nothing in it, for the envelopes that could not be
/// read.
fn empty_statement() -> AttestationStatement {
    AttestationStatement {
        statement_type: String::new(),
        predicate_type: String::new(),
        subject: Vec::new(),
        predicate: SlsaProvenance {
            build_type: String::new(),
            builder_id: String::new(),
            invocation_id: String::new(),
            started_on: None,
            finished_on: None,
            resolved_dependencies: Vec::new(),
        },
    }
}
