//! Evidence metadata on Dgraph; raw bytes remain in the existing CAS.

use std::sync::Arc;

use async_trait::async_trait;
use hodei_evidence::{EvidenceId, EvidenceRecord, EvidenceStore, Producer, StoreError};
use hodei_graph_client::{
    GraphClient, GraphMutationBlock, GraphMutationPort, GraphUpsertTransaction, dql_string,
    schema::ensure_ready,
};
use sha2::{Digest, Sha256};

/// The Dgraph type of immutable evidence metadata nodes.
pub const EVIDENCE_TYPE: &str = "HodeiEvidence";
/// The Dgraph type of the current producer/external-id lookup nodes.
pub const EVIDENCE_EXTERNAL_TYPE: &str = "HodeiEvidenceExternal";
const MAX_WRITE_ATTEMPTS: usize = 8;

/// Why an evidence store could not be opened on the graph.
#[derive(Debug, thiserror::Error)]
pub enum DgraphEvidenceError {
    /// The graph schema is incompatible or unavailable.
    #[error("the evidence store could not open on the graph: {0}")]
    Schema(#[source] Box<hodei_graph_client::schema::SchemaBootError>),
}

/// The accepted evidence metadata store over the graph authority.
///
/// Evidence bytes are deliberately absent from this adapter. Their digest
/// and opaque content reference live in the evidence record; the bytes stay
/// in the product's existing content-addressed store.
#[derive(Debug)]
pub struct DgraphEvidenceStore {
    client: Arc<GraphClient>,
}

impl DgraphEvidenceStore {
    /// Opens the adapter after validating and incrementally applying the
    /// versioned graph schema.
    pub async fn open(client: Arc<GraphClient>) -> Result<Self, DgraphEvidenceError> {
        ensure_ready(&client)
            .await
            .map_err(|error| DgraphEvidenceError::Schema(Box::new(error)))?;
        Ok(Self { client })
    }

    fn evidence_key(id: &str) -> String {
        sha256_key(&format!("evidence#{id}"))
    }

    fn external_key(producer: &str, external_id: &str) -> String {
        sha256_key(&format!("external#{producer}#{external_id}"))
    }

    async fn get_by_key(&self, key: &str) -> Result<Option<EvidenceRecord>, StoreError> {
        let query = format!(
            "{{ q(func: eq(hodei.evidence.key, {})) {{ hodei.evidence.key hodei.evidence.record }} }}",
            dql_string(key),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| StoreError::Unavailable(error.to_string()))?;
        decode_record_nodes(&response.json, "q", key)
    }

    fn put_transaction(evidence: &EvidenceRecord) -> GraphUpsertTransaction {
        let evidence_key = Self::evidence_key(evidence.id().as_str());
        let index_key = Self::external_key(
            &evidence.producer().id,
            evidence.source().external_id.as_deref().unwrap_or_default(),
        );
        let serialized = serde_json::to_string(evidence).unwrap_or_default();
        let tenant = evidence.tenant_hrn().to_string();
        let account = evidence.tenant_hrn().account_id().to_string();
        let subjects = evidence.subject_refs();
        let type_name = evidence.evidence_type().to_string();
        let digest = evidence.content().digest.to_string();
        let producer = evidence.producer().id.as_str();
        let external_id = evidence.source().external_id.as_deref().unwrap_or_default();
        let key_literal = dql_string(&evidence_key);
        let index_literal = dql_string(&index_key);
        let mut record_quads = format!(
            "_:e <dgraph.type> \"{EVIDENCE_TYPE}\" .\n\
             _:e <hodei.evidence.key> {key_literal} .\n\
             _:e <hodei.evidence.tenant> {} .\n\
             _:e <hodei.evidence.producer_id> {} .\n\
             _:e <hodei.evidence.external_id> {} .\n\
             _:e <hodei.evidence.type> {} .\n\
             _:e <hodei.evidence.content_digest> {} .\n\
             _:e <hodei.evidence.account> {} .\n\
             _:e <hodei.evidence.record> {} .\n",
            dql_string(&tenant),
            dql_string(producer),
            dql_string(external_id),
            dql_string(&type_name),
            dql_string(&digest),
            dql_string(&account),
            dql_string(&serialized),
        );
        for subject in subjects {
            record_quads.push_str(&format!(
                "_:e <hodei.evidence.subject> {} .\n",
                dql_string(&subject.to_string()),
            ));
        }
        let index_create = format!(
            "_:x <dgraph.type> \"{EVIDENCE_EXTERNAL_TYPE}\" .\n\
             _:x <hodei.evidence.external_key> {index_literal} .\n\
             _:x <hodei.evidence.external_evidence_id> {} .\n",
            dql_string(evidence.id().as_str()),
        );
        let index_update = format!(
            "uid(x) <hodei.evidence.external_evidence_id> {} .\n",
            dql_string(evidence.id().as_str()),
        );
        GraphUpsertTransaction {
            query: format!(
                "{{ e as var(func: eq(hodei.evidence.key, {key_literal}))\n\
                   x as var(func: eq(hodei.evidence.external_key, {index_literal})) }}"
            ),
            blocks: vec![
                GraphMutationBlock::set_if("eq(len(e), 0)", record_quads),
                GraphMutationBlock::set_if("eq(len(x), 0)", index_create),
                GraphMutationBlock::set_if("eq(len(x), 1)", index_update),
            ],
            commit_now: true,
        }
    }
}

#[async_trait]
impl EvidenceStore for DgraphEvidenceStore {
    /// Stores an immutable record and points its external index at it.
    async fn put(&self, evidence: EvidenceRecord) -> Result<EvidenceId, StoreError> {
        let key = Self::evidence_key(evidence.id().as_str());
        for attempt in 0..MAX_WRITE_ATTEMPTS {
            if let Some(existing) = self.get_by_key(&key).await? {
                if existing != evidence {
                    return Err(StoreError::InvalidRecord(format!(
                        "evidence identity '{}' already has different immutable metadata",
                        evidence.id()
                    )));
                }
                return Ok(evidence.id().clone());
            }
            match self.client.upsert(Self::put_transaction(&evidence)).await {
                Ok(_) => {
                    let stored = self.get_by_key(&key).await?.ok_or_else(|| {
                        StoreError::InvalidRecord(format!(
                            "evidence '{}' was acknowledged but is absent",
                            evidence.id()
                        ))
                    })?;
                    if stored != evidence {
                        return Err(StoreError::InvalidRecord(format!(
                            "evidence identity '{}' resolved to different immutable metadata",
                            evidence.id()
                        )));
                    }
                    return Ok(evidence.id().clone());
                }
                Err(hodei_graph_client::GraphClientError::UpsertConflict { .. })
                    if attempt + 1 < MAX_WRITE_ATTEMPTS => {}
                Err(error) => return Err(StoreError::Unavailable(error.to_string())),
            }
        }
        Err(StoreError::Unavailable(format!(
            "evidence '{}' did not commit after {MAX_WRITE_ATTEMPTS} attempts",
            evidence.id()
        )))
    }

    /// Reads evidence by its stable domain identity.
    async fn get(&self, id: &EvidenceId) -> Result<Option<EvidenceRecord>, StoreError> {
        self.get_by_key(&Self::evidence_key(id.as_str())).await
    }

    /// Finds the most recent record for a producer's external identity.
    async fn find_by_external_id(
        &self,
        producer: &Producer,
        external_id: &str,
    ) -> Result<Option<EvidenceRecord>, StoreError> {
        let index_key = Self::external_key(&producer.id, external_id);
        let query = format!(
            "{{ x(func: eq(hodei.evidence.external_key, {})) {{ hodei.evidence.external_evidence_id }} }}",
            dql_string(&index_key),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| StoreError::Unavailable(error.to_string()))?;
        let nodes = response
            .json
            .get("x")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        match nodes.as_slice() {
            [] => Ok(None),
            [node] => {
                let id = node
                    .get("hodei.evidence.external_evidence_id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        StoreError::InvalidRecord(
                            "external evidence index has no identity".to_owned(),
                        )
                    })?;
                self.get(&EvidenceId::new(id)).await
            }
            many => Err(StoreError::InvalidRecord(format!(
                "{} external evidence index nodes match one key",
                many.len()
            ))),
        }
    }

    /// Reads evidence for a subject without crossing the requested account.
    async fn evidence_of_subject(
        &self,
        account: &str,
        subject: &kernel::Hrn,
    ) -> Result<Vec<EvidenceRecord>, StoreError> {
        let query = format!(
            "{{ q(func: eq(hodei.evidence.subject, {}))\n\
               @filter(eq(hodei.evidence.account, {}))\n\
               {{ hodei.evidence.key hodei.evidence.record }} }}",
            dql_string(&subject.to_string()),
            dql_string(account),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| StoreError::Unavailable(error.to_string()))?;
        let nodes = response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut records = Vec::with_capacity(nodes.len());
        for node in nodes {
            let key = node
                .get("hodei.evidence.key")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("<missing key>");
            records.push(decode_record_node(&node, key)?);
        }
        Ok(records)
    }
}

fn decode_record_nodes(
    json: &serde_json::Value,
    root: &str,
    key: &str,
) -> Result<Option<EvidenceRecord>, StoreError> {
    let nodes = json
        .get(root)
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    match nodes.as_slice() {
        [] => Ok(None),
        [node] => decode_record_node(node, key).map(Some),
        many => Err(StoreError::InvalidRecord(format!(
            "{} evidence nodes carry key '{key}'",
            many.len()
        ))),
    }
}

fn decode_record_node(node: &serde_json::Value, key: &str) -> Result<EvidenceRecord, StoreError> {
    let record = node
        .get("hodei.evidence.record")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| StoreError::InvalidRecord(format!("evidence node '{key}' has no record")))?;
    serde_json::from_str(record).map_err(|error| {
        StoreError::InvalidRecord(format!("evidence node '{key}' is corrupt: {error}"))
    })
}

fn sha256_key(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}
