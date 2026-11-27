//! The execution receipt v1 and the adapter that exports it from PipelineK
//! (HSP-323 slice 8, origin HSP-37).
//!
//! A digest without the receipt of the execution that produced it is not
//! evidence of anything. The receipt is the binding between three things: the
//! digest of the artifact, the run that produced it and the source revision it
//! was built from.
//!
//! # What lives here
//!
//! - [`ExecutionReceiptV1`]: the closed contract. It has no free-form field, so
//!   nothing a pipeline printed (a secret, a token in a log line) can ride in
//!   it, and every field has a validated charset.
//! - [`export_from_pipelinek`]: the adapter. It reads the **public** output of a
//!   pinned PipelineK (`pipelinek version --json` and
//!   `pipelinek events --typed <runId>`) and copies only whitelisted fields.
//! - [`ReceiptIngestor`]: the admission of a receipt through the evidence
//!   pipeline that already exists (`EvidenceIngestion`), linking it to the
//!   artifact by its exact identity.
//!
//! # Trust model (what a receipt does and does not prove)
//!
//! PipelineK's events say that a run with an id finished with an outcome at a
//! time. They do **not** carry the digest of the produced artifact nor the
//! source revision: those are declared by the operator that exports the
//! receipt. Hodei does not take the digest on faith: the service re-hashes the
//! bytes it holds for the artifact and refuses the receipt when they differ.
//! The revision is a declaration of the exporter and is recorded as such.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_evidence::{
    ContentDigest, ContentStore, EvidenceIngestion, EvidenceType, EvidenceVerification,
    IngestionEnvelope, IngestionOutcome, IngestionRequest, Normalization, Producer, SourceRef,
    StoreError,
};
use hodei_evidence_storage::{
    DgraphEventLedger, DgraphEvidenceStore, HmacEnvelopeAuthenticator, ProducerCredentials,
};
use hodei_graph_client::GraphClient;
use hodei_hub::blob::{BlobError, BlobStore};
use kernel::Hrn;
use serde::{Deserialize, Serialize};

/// The version of PipelineK this adapter is pinned to.
///
/// The receipt spec says no consumer may negotiate compatibility from a
/// `0.x` producer version, so the adapter accepts exactly one version of the
/// producer and refuses any other until the pin is moved on purpose.
pub const PINNED_PIPELINEK_VERSION: &str = "0.97.0";

/// The schema identifier of the receipt.
pub const RECEIPT_SCHEMA: &str = "hodei.execution-receipt/1";

/// The producer name a receipt of this adapter carries.
pub const PRODUCER_NAME: &str = "pipelinek";

/// The producer of the receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptProducer {
    /// The producer name (always `pipelinek` in this release).
    pub name: String,
    /// The exact product version that ran the pipeline.
    pub version: String,
}

/// The execution receipt, version 1.
///
/// The field order is the serialization order: the canonical bytes of a
/// receipt are `serde_json::to_vec` of this struct, so the same receipt always
/// has the same bytes and the same digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionReceiptV1 {
    /// Always [`RECEIPT_SCHEMA`].
    pub schema: String,
    /// Who ran the pipeline.
    pub producer: ReceiptProducer,
    /// The run id the producer gave to the execution.
    pub run_id: String,
    /// How the run ended. Only `success` can be linked.
    pub outcome: String,
    /// The source revision the run built from, as declared by the exporter.
    pub source_revision: String,
    /// The digest of the artifact the run produced, `sha256:<64 hex>`.
    pub subject_digest: String,
    /// When the run finished (RFC 3339), as the producer recorded it.
    pub finished_at: String,
}

/// Why a receipt is not acceptable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReceiptError {
    /// The schema identifier is not the one this release speaks.
    #[error("the receipt schema is not {RECEIPT_SCHEMA}")]
    Schema,
    /// The producer is not the pinned PipelineK.
    #[error("the producer is not the pinned PipelineK {PINNED_PIPELINEK_VERSION}")]
    Producer,
    /// A field is malformed. The value is never echoed: it may be a secret.
    #[error("the field `{0}` is not well formed")]
    Field(&'static str),
    /// The run did not succeed, so it cannot back a trust lineage.
    #[error("only a run that succeeded can be linked")]
    NotSuccessful,
}

fn is_id_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '-'
}

impl ExecutionReceiptV1 {
    /// Checks the receipt against the contract, failing closed.
    ///
    /// The checks are external to any JSON Schema: the producer identity, the
    /// pinned version and the charset of every field. A value that is not in
    /// the expected charset is refused and never echoed in the error.
    pub fn validate(&self) -> Result<(), ReceiptError> {
        if self.schema != RECEIPT_SCHEMA {
            return Err(ReceiptError::Schema);
        }
        if self.producer.name != PRODUCER_NAME || self.producer.version != PINNED_PIPELINEK_VERSION
        {
            return Err(ReceiptError::Producer);
        }
        if !(8..=64).contains(&self.run_id.len()) || !self.run_id.chars().all(is_id_char) {
            return Err(ReceiptError::Field("run_id"));
        }
        let revision = &self.source_revision;
        if !matches!(revision.len(), 40 | 64)
            || !revision
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        {
            return Err(ReceiptError::Field("source_revision"));
        }
        if self.digest().is_none() {
            return Err(ReceiptError::Field("subject_digest"));
        }
        if DateTime::parse_from_rfc3339(&self.finished_at).is_err() {
            return Err(ReceiptError::Field("finished_at"));
        }
        if self.outcome != "success" {
            return Err(
                if self.outcome.chars().all(is_id_char) && self.outcome.len() < 32 {
                    ReceiptError::NotSuccessful
                } else {
                    ReceiptError::Field("outcome")
                },
            );
        }
        Ok(())
    }

    /// The declared digest of the artifact, when it is a well-formed sha256.
    pub fn digest(&self) -> Option<ContentDigest> {
        let value = self.subject_digest.strip_prefix("sha256:")?;
        let digest = ContentDigest::sha256(value);
        (value == digest.value && digest.is_well_formed()).then_some(digest)
    }

    /// The canonical bytes of the receipt (the evidence content).
    pub fn canonical_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("a receipt always serializes")
    }
}

/// Why the export from PipelineK failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExportError {
    /// The PipelineK that produced the events is not the pinned one.
    #[error("PipelineK {found} is not the pinned version {PINNED_PIPELINEK_VERSION}")]
    UnpinnedVersion {
        /// The version that was reported.
        found: String,
    },
    /// The event stream is not a complete, consistent run.
    #[error("the PipelineK event stream is not a complete run: {0}")]
    Events(&'static str),
    /// The receipt the export produced does not satisfy the contract.
    #[error(transparent)]
    Receipt(#[from] ReceiptError),
}

/// The input of the adapter: what the public PipelineK surface printed, and
/// what the exporter declares.
#[derive(Debug, Clone, Copy)]
pub struct PipelinekExport<'a> {
    /// `productVersion` of `pipelinek version --json`.
    pub pipelinek_version: &'a str,
    /// The stdout of `pipelinek events --db <db> --typed <runId>`.
    pub events_jsonl: &'a str,
    /// The stderr of that same command: the `evt-cursor-v1:<runId>:<last
    /// sequence>` trailer PipelineK prints after the last event.
    pub cursor: &'a str,
    /// The digest of the artifact the run produced (`sha256:<hex>`).
    pub subject_digest: &'a str,
    /// The source revision the run built from.
    pub source_revision: &'a str,
}

/// Exports the receipt of a finished PipelineK run.
///
/// The adapter reads only the public event stream (typed JSON lines plus the
/// cursor trailer) and copies four things from it: the run id, the
/// terminal outcome, the time the run finished and nothing else. Everything a
/// pipeline printed stays where it was.
///
/// The stream is checked as a whole before anything is trusted: every event
/// belongs to the same run, the sequence is contiguous from 1, the run started
/// first and finished last, and the cursor trailer names that run and that
/// last sequence. A truncated or spliced stream does not export.
pub fn export_from_pipelinek(
    input: PipelinekExport<'_>,
) -> Result<ExecutionReceiptV1, ExportError> {
    if input.pipelinek_version != PINNED_PIPELINEK_VERSION {
        return Err(ExportError::UnpinnedVersion {
            found: input
                .pipelinek_version
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+'))
                .take(32)
                .collect(),
        });
    }

    let mut run_id: Option<String> = None;
    let mut expected_sequence = 1u64;
    let mut kinds: Vec<String> = Vec::new();
    let mut finished: Option<(String, String)> = None;

    for line in input.events_jsonl.lines().filter(|l| !l.trim().is_empty()) {
        let event: serde_json::Value = serde_json::from_str(line)
            .map_err(|_| ExportError::Events("a line is not a JSON event"))?;
        let event_run = event
            .get("runId")
            .and_then(|v| v.as_str())
            .ok_or(ExportError::Events("an event has no runId"))?;
        match &run_id {
            None => run_id = Some(event_run.to_string()),
            Some(known) if known == event_run => {}
            Some(_) => return Err(ExportError::Events("events of more than one run")),
        }
        let sequence = event
            .get("sequence")
            .and_then(|v| v.as_u64())
            .ok_or(ExportError::Events("an event has no sequence"))?;
        if sequence != expected_sequence {
            return Err(ExportError::Events("the sequence is not contiguous"));
        }
        expected_sequence += 1;
        let kind = event
            .get("kind")
            .and_then(|v| v.as_str())
            .ok_or(ExportError::Events("an event has no kind"))?;
        if kind == "RunFinished" {
            if finished.is_some() {
                return Err(ExportError::Events("the run finished more than once"));
            }
            let outcome = event
                .get("outcome")
                .and_then(|v| v.as_str())
                .ok_or(ExportError::Events("the terminal event has no outcome"))?;
            let at = event
                .get("occurredAt")
                .and_then(|v| v.as_str())
                .ok_or(ExportError::Events("the terminal event has no time"))?;
            finished = Some((outcome.to_ascii_lowercase(), at.to_string()));
        } else if finished.is_some() {
            return Err(ExportError::Events("events after the terminal event"));
        }
        kinds.push(kind.to_string());
    }

    let run_id = run_id.ok_or(ExportError::Events("the stream is empty"))?;
    if !kinds.iter().any(|k| k == "RunStarted") {
        return Err(ExportError::Events("the run never started"));
    }
    let (outcome, finished_at) = finished.ok_or(ExportError::Events("the run did not finish"))?;
    let last_sequence = expected_sequence - 1;
    let cursor = input
        .cursor
        .trim()
        .strip_prefix("evt-cursor-v1:")
        .and_then(|rest| rest.rsplit_once(':'))
        .and_then(|(run, sequence)| Some((run, sequence.parse::<u64>().ok()?)));
    match cursor {
        Some((cursor_run, sequence)) if cursor_run == run_id && sequence == last_sequence => {}
        _ => {
            return Err(ExportError::Events(
                "the cursor trailer does not match the run",
            ));
        }
    }

    let receipt = ExecutionReceiptV1 {
        schema: RECEIPT_SCHEMA.to_string(),
        producer: ReceiptProducer {
            name: PRODUCER_NAME.to_string(),
            version: PINNED_PIPELINEK_VERSION.to_string(),
        },
        run_id,
        outcome,
        source_revision: input.source_revision.to_string(),
        subject_digest: input.subject_digest.to_string(),
        finished_at,
    };
    receipt.validate()?;
    Ok(receipt)
}

/// How an ingestion ended, in the terms of the surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Linked {
    /// The receipt was admitted for the first time.
    Created(String),
    /// The same receipt had been admitted: nothing was written.
    Existing(String),
    /// A different receipt for the same run and artifact replaced the reading;
    /// the previous record is kept in the history.
    Revised(String),
}

impl Linked {
    /// The id of the evidence record.
    pub fn evidence_id(&self) -> &str {
        match self {
            Self::Created(id) | Self::Existing(id) | Self::Revised(id) => id,
        }
    }
}

/// Why the admission of a receipt failed (the surface maps it to a status).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IngestError {
    /// The evidence pipeline refused the delivery.
    #[error("the evidence pipeline refused the receipt")]
    Refused,
    /// A store was not available.
    #[error("the evidence store is not available")]
    Unavailable,
}

/// The content store of the evidence over the blob store of the service.
struct BlobBackedContent(Arc<dyn BlobStore>);

#[async_trait]
impl ContentStore for BlobBackedContent {
    async fn put(&self, content: Vec<u8>) -> Result<ContentDigest, StoreError> {
        self.0
            .put(&content)
            .await
            .map_err(|error| StoreError::Unavailable(error.to_string()))
    }

    async fn get(&self, digest: &ContentDigest) -> Result<Option<Vec<u8>>, StoreError> {
        match self.0.get(digest).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(BlobError::NotFound) => Ok(None),
            Err(error) => Err(StoreError::Unavailable(error.to_string())),
        }
    }

    async fn size(&self, digest: &ContentDigest) -> Result<Option<u64>, StoreError> {
        match self.0.length(digest).await {
            Ok(length) => Ok(Some(length)),
            Err(BlobError::NotFound) => Ok(None),
            Err(error) => Err(StoreError::Unavailable(error.to_string())),
        }
    }
}

/// The admission of execution receipts over the evidence pipeline.
///
/// The service has already authenticated the caller, authorized the action and
/// verified the digest against the bytes when it gets here. The evidence
/// pipeline asks for a vouched envelope, so the service vouches for it with a
/// secret that exists only for this one delivery and only for this account.
#[derive(Clone)]
pub struct ReceiptIngestor {
    evidence: Arc<DgraphEvidenceStore>,
    client: Arc<GraphClient>,
}

impl ReceiptIngestor {
    /// An ingestor over the evidence store and the graph client of the profile.
    pub fn new(evidence: Arc<DgraphEvidenceStore>, client: Arc<GraphClient>) -> Self {
        Self { evidence, client }
    }

    /// Admits a **validated** receipt as evidence about `subject`.
    ///
    /// The external identity names the account, the run and the digest: two
    /// accounts that happen to share a run id never share a record, a run
    /// that produced two artifacts yields two independent records, and two
    /// runs that produced the same digest yield two independent records.
    pub async fn ingest(
        &self,
        account: &str,
        subject: Hrn,
        receipt: &ExecutionReceiptV1,
        content: Arc<dyn BlobStore>,
    ) -> Result<Linked, IngestError> {
        let digest = receipt.digest().ok_or(IngestError::Refused)?;
        let bytes = receipt.canonical_bytes();
        let observed_at = DateTime::parse_from_rfc3339(&receipt.finished_at)
            .map_err(|_| IngestError::Refused)?
            .with_timezone(&Utc);

        let ledger = DgraphEventLedger::open(self.client.clone(), account)
            .await
            .map_err(|_| IngestError::Unavailable)?;
        let authenticator = Arc::new(HmacEnvelopeAuthenticator::new());
        let secret: [u8; 32] = rand::random();
        authenticator.register(
            PRODUCER_NAME,
            ProducerCredentials {
                secret: secret.to_vec(),
                accounts: vec![account.to_string()],
            },
        );
        let ingestion = EvidenceIngestion::new(
            Arc::new(BlobBackedContent(content)),
            self.evidence.clone(),
            Arc::new(ledger),
            authenticator.clone(),
        );

        let mut envelope = IngestionEnvelope {
            producer: Producer::new("connector", PRODUCER_NAME)
                .with_version(receipt.producer.version.clone()),
            tenant_hrn: subject.clone(),
            subject_refs: vec![subject],
            evidence_type: EvidenceType::BuildMetadata,
            schema_uri: Some(RECEIPT_SCHEMA.to_string()),
            schema_version: Some("1".to_string()),
            source: SourceRef::by_external_id(format!("{account}/{}/{digest}", receipt.run_id)),
            content_digest: ContentDigest::of_bytes(&bytes),
            observed_at,
            valid_from: None,
            verification: EvidenceVerification::verified("sha256-rehash-of-stored-bytes"),
            normalization: Normalization::new("hodei-execution-receipt", "1.0.0"),
            correlation_id: None,
            relation_claim: None,
            signature: String::new(),
        };
        envelope.signature = authenticator.sign(&envelope).ok_or(IngestError::Refused)?;

        let outcome = ingestion
            .ingest(IngestionRequest {
                envelope,
                content: bytes,
            })
            .await
            .map_err(|_| IngestError::Unavailable)?;
        match outcome {
            IngestionOutcome::Accepted { evidence_id, .. } => {
                Ok(Linked::Created(evidence_id.to_string()))
            }
            IngestionOutcome::AlreadyAccepted { evidence_id } => {
                Ok(Linked::Existing(evidence_id.to_string()))
            }
            IngestionOutcome::Revised { evidence_id, .. } => {
                Ok(Linked::Revised(evidence_id.to_string()))
            }
            IngestionOutcome::Rejected { .. } => Err(IngestError::Refused),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt() -> ExecutionReceiptV1 {
        ExecutionReceiptV1 {
            schema: RECEIPT_SCHEMA.into(),
            producer: ReceiptProducer {
                name: PRODUCER_NAME.into(),
                version: PINNED_PIPELINEK_VERSION.into(),
            },
            run_id: "cfe8ac79-950d-4696-84ac-a36a241e3ce5".into(),
            outcome: "success".into(),
            source_revision: "a".repeat(40),
            subject_digest: format!("sha256:{}", "b".repeat(64)),
            finished_at: "2026-11-27T07:09:05.256682164Z".into(),
        }
    }

    fn stream(run: &str, outcome: &str) -> String {
        format!(
            "{{\"runId\":\"{run}\",\"sequence\":1,\"kind\":\"RunCorrelated\"}}\n\
             {{\"runId\":\"{run}\",\"sequence\":2,\"kind\":\"RunStarted\"}}\n\
             {{\"runId\":\"{run}\",\"sequence\":3,\"kind\":\"RunFinished\",\"outcome\":\"{outcome}\",\"occurredAt\":\"2026-11-27T07:09:05.256682164Z\"}}\n"
        )
    }

    const RUN: &str = "cfe8ac79-950d-4696-84ac-a36a241e3ce5";

    fn input<'a>(events: &'a str, version: &'a str) -> PipelinekExport<'a> {
        input_with(
            events,
            version,
            "evt-cursor-v1:cfe8ac79-950d-4696-84ac-a36a241e3ce5:3\n",
        )
    }

    fn input_with<'a>(events: &'a str, version: &'a str, cursor: &'a str) -> PipelinekExport<'a> {
        PipelinekExport {
            pipelinek_version: version,
            events_jsonl: events,
            cursor,
            subject_digest: "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            source_revision: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        }
    }

    #[test]
    fn a_complete_stream_exports_a_valid_receipt() {
        let events = stream("cfe8ac79-950d-4696-84ac-a36a241e3ce5", "success");
        let exported = export_from_pipelinek(input(&events, PINNED_PIPELINEK_VERSION)).unwrap();
        assert_eq!(exported, receipt());
    }

    #[test]
    fn an_unpinned_version_does_not_export() {
        let events = stream("cfe8ac79-950d-4696-84ac-a36a241e3ce5", "success");
        assert!(matches!(
            export_from_pipelinek(input(&events, "0.98.0")),
            Err(ExportError::UnpinnedVersion { .. })
        ));
    }

    #[test]
    fn a_failed_run_never_becomes_a_linkable_receipt() {
        let events = stream("cfe8ac79-950d-4696-84ac-a36a241e3ce5", "failure");
        assert_eq!(
            export_from_pipelinek(input(&events, PINNED_PIPELINEK_VERSION)),
            Err(ExportError::Receipt(ReceiptError::NotSuccessful))
        );
    }

    #[test]
    fn a_truncated_or_spliced_stream_does_not_export() {
        let run = "cfe8ac79-950d-4696-84ac-a36a241e3ce5";
        let full = stream(run, "success");
        // Without the terminal event.
        let cut: String = full.lines().take(2).collect::<Vec<_>>().join("\n");
        assert!(export_from_pipelinek(input(&cut, PINNED_PIPELINEK_VERSION)).is_err());
        // A cursor of another run.
        assert!(
            export_from_pipelinek(input_with(
                &full,
                PINNED_PIPELINEK_VERSION,
                "evt-cursor-v1:aaaaaaaa-950d-4696-84ac-a36a241e3ce5:3"
            ))
            .is_err()
        );
        // A cursor that is missing, or that names another last sequence.
        assert!(export_from_pipelinek(input_with(&full, PINNED_PIPELINEK_VERSION, "")).is_err());
        assert!(
            export_from_pipelinek(input_with(
                &full,
                PINNED_PIPELINEK_VERSION,
                &format!("evt-cursor-v1:{RUN}:2")
            ))
            .is_err()
        );
        // A gap in the sequence.
        let gap = full.replace("\"sequence\":2", "\"sequence\":5");
        assert!(export_from_pipelinek(input(&gap, PINNED_PIPELINEK_VERSION)).is_err());
    }

    #[test]
    fn a_secret_in_any_field_is_refused_and_never_echoed() {
        let secret = "ghp_SuperSecretToken123456";
        let mut bad = receipt();
        bad.source_revision = secret.into();
        let error = bad.validate().unwrap_err();
        assert!(!error.to_string().contains(secret));
        let mut bad = receipt();
        bad.run_id = format!("run {secret}");
        assert!(bad.validate().is_err());
        // Unknown fields cannot smuggle a value either.
        let smuggled = serde_json::to_string(&receipt())
            .unwrap()
            .replace("\"outcome\"", "\"note\":\"x\",\"outcome\"");
        assert!(serde_json::from_str::<ExecutionReceiptV1>(&smuggled).is_err());
    }

    #[test]
    fn the_canonical_bytes_are_stable() {
        assert_eq!(receipt().canonical_bytes(), receipt().canonical_bytes());
        let mut other = receipt();
        other.run_id = "cfe8ac79-950d-4696-84ac-a36a241e3ce6".into();
        assert_ne!(receipt().canonical_bytes(), other.canonical_bytes());
    }
}
