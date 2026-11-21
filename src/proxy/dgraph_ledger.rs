//! The durable provenance ledger over the real graph engine (HSP-196).
//!
//! The proxy's hot path reads freshness from the ledger of receipts and
//! nowhere else: the in-process caches (single-flight, negative, metrics)
//! are only optimizations, and what survives a process restart is exactly
//! what this ledger holds. Until now the ledger had only the Surreal
//! adapter (`crates/hodei-catalog-storage/src/surreal/provenance_ledger.rs`);
//! this module ports it onto Dgraph with the additive store pattern the
//! sibling slices established (`src/quota_dgraph.rs`, HSP-198; the catalog
//! Dgraph store, HSP-187): the slice declares its OWN predicates and type
//! with an idempotent `alter_schema`, never touching the versioned graph
//! schema.
//!
//! Guarantees this port keeps from the Surreal reference:
//!
//! - **Durability**: every receipt is one graph row, keyed by
//!   `{account}/receipts/{receipt_id}` and exact-indexed, so the receipt —
//!   digest, size, outcome and the validators (etag/last-modified) it
//!   recorded — survives a process restart and every later revalidation
//!   reads it back.
//! - **No duplication across repos or restarts**: the key carries the
//!   account, so two accounts never share a receipt row; writing a row
//!   whose key already stands is skipped by the transaction's condition —
//!   the graph's answer to the reference's `INSERT IGNORE`. A receipt the
//!   process died before finishing (the ledger write sits after the blob
//!   commit, so the GC collects that blob) is replayed on the next attempt
//!   with a FRESH identifier, exactly as in the reference.
//! - **The same closed failures**: a row this release cannot read is
//!   refused with an explicit corrupt error, never reinterpreted and never
//!   served as if it were fine.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_catalog::{
    CatalogError, ChecksumEvidence, ProvenanceLedger, ProvenanceReceipt, ReceiptOutcome,
};
use hodei_evidence::domain::content::{ContentDigest, DigestAlgorithm};
use hodei_graph_client::GraphClient;
use hodei_graph_client::port::{GraphMutationBlock, GraphMutationPort, GraphUpsertTransaction};
use hodei_graph_client::schema::dql_string;
use serde_json::Value;
use std::sync::Arc;

/// The type name of one receipt row in the graph.
pub const RECEIPT_TYPE: &str = "HodeiProxyReceipt";

/// The DQL text of the additive DDL slice this ledger declares. Idempotent
/// and additive: it only creates `hodei.proxy.receipt.*` predicates and the
/// one type above; nothing else in the graph names them.
pub const PROXY_RECEIPT_SCHEMA_DQL: &str = "
hodei.proxy.receipt.key: string @index(exact) @upsert .
hodei.proxy.receipt.subject: string @index(exact) .
hodei.proxy.receipt.account: string @index(exact) .
hodei.proxy.receipt.repository: string @index(exact) .
hodei.proxy.receipt.path: string @index(exact) .
hodei.proxy.receipt.id: string @index(exact) .
hodei.proxy.receipt.upstream_id: string .
hodei.proxy.receipt.source_url: string .
hodei.proxy.receipt.config_version: int .
hodei.proxy.receipt.digest_algorithm: string .
hodei.proxy.receipt.digest: string .
hodei.proxy.receipt.size_bytes: int .
hodei.proxy.receipt.checksum_kind: string .
hodei.proxy.receipt.checksum_header: string .
hodei.proxy.receipt.status: int .
hodei.proxy.receipt.redirects: int .
hodei.proxy.receipt.etag: string .
hodei.proxy.receipt.last_modified: string .
hodei.proxy.receipt.outcome: string .
hodei.proxy.receipt.actor: string .
hodei.proxy.receipt.correlation_id: string .
hodei.proxy.receipt.fetched_at: datetime @index(day) .
type HodeiProxyReceipt {
  hodei.proxy.receipt.key
  hodei.proxy.receipt.subject
  hodei.proxy.receipt.account
  hodei.proxy.receipt.repository
  hodei.proxy.receipt.path
  hodei.proxy.receipt.id
  hodei.proxy.receipt.upstream_id
  hodei.proxy.receipt.source_url
  hodei.proxy.receipt.config_version
  hodei.proxy.receipt.digest_algorithm
  hodei.proxy.receipt.digest
  hodei.proxy.receipt.size_bytes
  hodei.proxy.receipt.checksum_kind
  hodei.proxy.receipt.checksum_header
  hodei.proxy.receipt.status
  hodei.proxy.receipt.redirects
  hodei.proxy.receipt.etag
  hodei.proxy.receipt.last_modified
  hodei.proxy.receipt.outcome
  hodei.proxy.receipt.actor
  hodei.proxy.receipt.correlation_id
  hodei.proxy.receipt.fetched_at
}
";

/// The predicates a receipt row is read back with.
const RECEIPT_FIELDS: &str = "hodei.proxy.receipt.account \
  hodei.proxy.receipt.repository hodei.proxy.receipt.path hodei.proxy.receipt.id \
  hodei.proxy.receipt.upstream_id hodei.proxy.receipt.source_url \
  hodei.proxy.receipt.config_version hodei.proxy.receipt.digest_algorithm \
  hodei.proxy.receipt.digest hodei.proxy.receipt.size_bytes \
  hodei.proxy.receipt.checksum_kind hodei.proxy.receipt.checksum_header \
  hodei.proxy.receipt.status hodei.proxy.receipt.redirects hodei.proxy.receipt.etag \
  hodei.proxy.receipt.last_modified hodei.proxy.receipt.outcome \
  hodei.proxy.receipt.actor hodei.proxy.receipt.correlation_id \
  hodei.proxy.receipt.fetched_at";

/// The graph key of one receipt: scoped to the account, so two accounts
/// never collide and one repo's receipts never live under another's key.
pub fn receipt_key(account: &str, receipt_id: &str) -> String {
    format!("{account}/receipts/{receipt_id}")
}

/// The subject of the list index of one path: `{account}/{repository}/
/// {asset_path}` — the same triple the hot path asks receipts for.
pub fn path_subject(account: &str, repository: &str, asset_path: &str) -> String {
    format!("{account}/{repository}/{asset_path}")
}

/// The charset a receipt identifier must respect to become a graph key.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.starts_with(|c: char| c.is_ascii_alphanumeric())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '@'))
}

/// A server-side refusal travels inside the JSON: it is never read as a
/// successful answer.
fn require_no_errors(json: &Value) -> Result<(), CatalogError> {
    let empty: Vec<Value> = Vec::new();
    let errors = json
        .get("errors")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    if errors.is_empty() {
        return Ok(());
    }
    let text = errors
        .iter()
        .filter_map(|error| error.get("message").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("; ");
    Err(CatalogError::unavailable(if text.is_empty() {
        format!("{errors:?}")
    } else {
        text
    }))
}

/// Reads one node back into the domain receipt. A field this release
/// cannot read refuses the row: an unreadable receipt asserts nothing and
/// serves nothing — the same fail-closed rule as the Surreal reference.
pub fn the_receipt(node: &Value) -> Result<ProvenanceReceipt, CatalogError> {
    let id = text_of(node, "hodei.proxy.receipt.id")
        .ok_or_else(|| CatalogError::corrupt("receipt", "?", "a receipt row without an id"))?
        .to_string();
    let fail = |detail: String| CatalogError::corrupt("receipt", id.clone(), detail);
    let required = |predicate: &str| -> Result<String, CatalogError> {
        text_of(node, predicate)
            .map(str::to_owned)
            .ok_or_else(|| fail(format!("the predicate {predicate} is missing")))
    };
    let algorithm = match required("hodei.proxy.receipt.digest_algorithm")?.as_str() {
        "sha256" => DigestAlgorithm::Sha256,
        "sha512" => DigestAlgorithm::Sha512,
        other => return Err(fail(format!("unknown digest algorithm {other:?}"))),
    };
    let checksum = match (
        required("hodei.proxy.receipt.checksum_kind")?.as_str(),
        text_of(node, "hodei.proxy.receipt.checksum_header").map(str::to_owned),
    ) {
        ("declared", Some(header)) => ChecksumEvidence::Declared { header },
        ("computed_only", None) => ChecksumEvidence::ComputedOnly,
        _ => {
            return Err(fail(
                "a provenance receipt carries a checksum evidence this release cannot read"
                    .to_owned(),
            ));
        }
    };
    // A receipt without an outcome predates the cache: it was fetched. An
    // outcome this build does not know is refused, never reinterpreted.
    let outcome = match text_of(node, "hodei.proxy.receipt.outcome") {
        None => ReceiptOutcome::Fetched,
        Some(raw) => ReceiptOutcome::parse(raw).ok_or_else(|| {
            fail("a provenance receipt carries an outcome this release cannot read".to_owned())
        })?,
    };
    let fetched_at =
        DateTime::parse_from_rfc3339(required("hodei.proxy.receipt.fetched_at")?.as_str())
            .map(|when| when.with_timezone(&Utc))
            .map_err(|_| {
                fail("the fetched_at of the receipt is not a readable datetime".to_owned())
            })?;
    let integer = |predicate: &str| -> Result<i64, CatalogError> {
        node.get(predicate)
            .and_then(Value::as_i64)
            .ok_or_else(|| fail(format!("the predicate {predicate} is missing")))
    };
    Ok(ProvenanceReceipt {
        id: id.clone(),
        account: required("hodei.proxy.receipt.account")?,
        repository: required("hodei.proxy.receipt.repository")?,
        asset_path: required("hodei.proxy.receipt.path")?,
        upstream_id: required("hodei.proxy.receipt.upstream_id")?,
        source_url: required("hodei.proxy.receipt.source_url")?,
        config_version: u64::try_from(integer("hodei.proxy.receipt.config_version")?)
            .unwrap_or_default(),
        digest: ContentDigest {
            algorithm,
            value: required("hodei.proxy.receipt.digest")?,
        },
        size_bytes: u64::try_from(integer("hodei.proxy.receipt.size_bytes")?).unwrap_or_default(),
        checksum,
        status: u16::try_from(integer("hodei.proxy.receipt.status")?).unwrap_or_default(),
        redirects: u8::try_from(integer("hodei.proxy.receipt.redirects")?).unwrap_or_default(),
        etag: text_of(node, "hodei.proxy.receipt.etag").map(str::to_owned),
        last_modified: text_of(node, "hodei.proxy.receipt.last_modified").map(str::to_owned),
        outcome,
        actor: required("hodei.proxy.receipt.actor")?,
        correlation_id: required("hodei.proxy.receipt.correlation_id")?,
        fetched_at,
    })
}

fn text_of<'a>(node: &'a Value, predicate: &str) -> Option<&'a str> {
    node.get(predicate).and_then(Value::as_str)
}

/// The ledger of proxy provenance receipts over the Dgraph engine.
///
/// Cheap to clone: it shares the connected client. Every method is a
/// single round trip: the gated once-only write of `record`, the
/// exact-indexed list of `receipts_of`, the bounded window of
/// `latest_receipts`.
pub struct DgraphProvenanceLedger {
    client: Arc<GraphClient>,
}

impl std::fmt::Debug for DgraphProvenanceLedger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DgraphProvenanceLedger")
            .finish_non_exhaustive()
    }
}

impl DgraphProvenanceLedger {
    /// The ledger over a client the deployment already connected.
    pub fn new(client: Arc<GraphClient>) -> Self {
        Self { client }
    }

    /// Opens the ledger on a connected, logged-in client: declares the
    /// additive DDL slice, bounded and idempotent — a concurrent boot that
    /// finds the indexes settling retries instead of failing.
    pub async fn open(client: Arc<GraphClient>) -> Result<Self, CatalogError> {
        let mut last = String::new();
        for attempt in 1..=3 {
            match client.alter_schema(PROXY_RECEIPT_SCHEMA_DQL).await {
                Ok(()) => return Ok(Self::new(client)),
                Err(error) if attempt < 3 => {
                    last = error.to_string();
                    tokio::time::sleep(std::time::Duration::from_millis(250 * attempt)).await;
                }
                Err(error) => return Err(CatalogError::unavailable(error.to_string())),
            }
        }
        Err(CatalogError::unavailable(format!(
            "the proxy receipt DDL slice did not settle after 3 attempts: {last}"
        )))
    }

    /// The N-Quads that write this receipt, in a canonical field order:
    /// the same receipt always yields the same text. Optional strings are
    /// written only when present — a blank string and an absent predicate
    /// are two different facts, and the read path distinguishes them.
    pub fn receipt_nquads(receipt: &ProvenanceReceipt) -> String {
        let key = receipt_key(&receipt.account, &receipt.id);
        let subject = path_subject(&receipt.account, &receipt.repository, &receipt.asset_path);
        let (kind, header) = match &receipt.checksum {
            ChecksumEvidence::Declared { header } => ("declared", Some(header.clone())),
            ChecksumEvidence::ComputedOnly => ("computed_only", None),
        };
        let mut quads = format!(
            "_:rec <dgraph.type> \"{RECEIPT_TYPE}\" .\n\
             _:rec <hodei.proxy.receipt.key> {} .\n\
             _:rec <hodei.proxy.receipt.subject> {} .\n\
             _:rec <hodei.proxy.receipt.account> {} .\n\
             _:rec <hodei.proxy.receipt.repository> {} .\n\
             _:rec <hodei.proxy.receipt.path> {} .\n\
             _:rec <hodei.proxy.receipt.id> {} .\n\
             _:rec <hodei.proxy.receipt.upstream_id> {} .\n\
             _:rec <hodei.proxy.receipt.source_url> {} .\n\
             _:rec <hodei.proxy.receipt.config_version> \"{}\" .\n\
             _:rec <hodei.proxy.receipt.digest_algorithm> {} .\n\
             _:rec <hodei.proxy.receipt.digest> {} .\n\
             _:rec <hodei.proxy.receipt.size_bytes> \"{}\" .\n\
             _:rec <hodei.proxy.receipt.checksum_kind> {} .\n",
            dql_string(&key),
            dql_string(&subject),
            dql_string(&receipt.account),
            dql_string(&receipt.repository),
            dql_string(&receipt.asset_path),
            dql_string(&receipt.id),
            dql_string(&receipt.upstream_id),
            dql_string(&receipt.source_url),
            receipt.config_version,
            dql_string(&receipt.digest.algorithm.to_string()),
            dql_string(&receipt.digest.value),
            receipt.size_bytes,
            dql_string(kind),
        );
        if let Some(header) = header {
            quads.push_str(&format!(
                "_:rec <hodei.proxy.receipt.checksum_header> {} .\n",
                dql_string(&header)
            ));
        }
        quads.push_str(&format!(
            "_:rec <hodei.proxy.receipt.status> \"{}\" .\n\
             _:rec <hodei.proxy.receipt.redirects> \"{}\" .\n",
            receipt.status, receipt.redirects,
        ));
        if let Some(etag) = &receipt.etag {
            quads.push_str(&format!(
                "_:rec <hodei.proxy.receipt.etag> {} .\n",
                dql_string(etag)
            ));
        }
        if let Some(last_modified) = &receipt.last_modified {
            quads.push_str(&format!(
                "_:rec <hodei.proxy.receipt.last_modified> {} .\n",
                dql_string(last_modified)
            ));
        }
        quads.push_str(&format!(
            "_:rec <hodei.proxy.receipt.outcome> {} .\n\
             _:rec <hodei.proxy.receipt.actor> {} .\n\
             _:rec <hodei.proxy.receipt.correlation_id> {} .\n\
             _:rec <hodei.proxy.receipt.fetched_at> {} .\n",
            dql_string(receipt.outcome.as_str()),
            dql_string(&receipt.actor),
            dql_string(&receipt.correlation_id),
            dql_string(&receipt.fetched_at.to_rfc3339()),
        ));
        quads
    }

    async fn list_receipts(
        &self,
        subject: &str,
        limit: Option<usize>,
    ) -> Result<Vec<ProvenanceReceipt>, CatalogError> {
        // Dgraph answers with `first` and `order` at the root; the
        // timestamp order matches the reference's `ORDER BY fetched_at
        // DESC`. The id tie-break of the reference is applied below,
        // client-side over the window the server returned.
        let mut func = format!("eq(hodei.proxy.receipt.subject, {})", dql_string(subject));
        if let Some(limit) = limit {
            let limit = limit.min(i32::MAX as usize);
            // This server dialect takes `orderdesc: <predicate>` at the
            // root, not the `order: desc(...)` form: the window and its
            // order are both arguments of the root function.
            func.push_str(&format!(
                ", first: {limit}, orderdesc: hodei.proxy.receipt.fetched_at"
            ));
        }
        let query = format!("{{\n  q(func: {func}) {{\n    {RECEIPT_FIELDS}\n  }}\n}}");
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        require_no_errors(&response.json)?;
        let empty: Vec<Value> = Vec::new();
        let nodes = response
            .json
            .get("q")
            .and_then(Value::as_array)
            .unwrap_or(&empty);
        nodes.iter().map(the_receipt).collect()
    }

    /// Orders newest-first by (fetched_at, id), the reference's order.
    fn sort_newest(receipts: &mut [ProvenanceReceipt]) {
        receipts.sort_by(|a, b| {
            b.fetched_at
                .cmp(&a.fetched_at)
                .then_with(|| b.id.cmp(&a.id))
        });
    }
}

#[async_trait]
impl ProvenanceLedger for DgraphProvenanceLedger {
    async fn record(&self, receipt: &ProvenanceReceipt) -> Result<(), CatalogError> {
        if !valid_id(&receipt.id) {
            return Err(CatalogError::invalid(format!(
                "the receipt id {:?} cannot become a graph key",
                receipt.id
            )));
        }
        let key = receipt_key(&receipt.account, &receipt.id);
        let nquads = Self::receipt_nquads(receipt);
        // One round trip per attempt, no read first: the condition binds
        // the unique key and the block writes only when nothing stands.
        // Two racers both ask; the `@upsert` index makes one of them abort
        // with a typed conflict, the retry finds the row standing, the
        // condition skips the write and the replay answers Ok. Nothing
        // already written is ever rewritten or duplicated.
        for attempt in 1..=3 {
            let transaction = GraphUpsertTransaction {
                query: format!(
                    "{{\n  dupe as var(func: eq(hodei.proxy.receipt.key, {}))\n}}",
                    dql_string(&key)
                ),
                blocks: vec![GraphMutationBlock::set_if(
                    "eq(len(dupe), 0)",
                    nquads.clone(),
                )],
                commit_now: true,
            };
            match self.client.upsert(transaction).await {
                Ok(response) => {
                    require_no_errors(&response.json)?;
                    return Ok(());
                }
                Err(hodei_graph_client::GraphClientError::UpsertConflict { .. }) if attempt < 3 => {
                    tokio::time::sleep(std::time::Duration::from_millis(50 * attempt)).await;
                }
                Err(error) => return Err(CatalogError::unavailable(error.to_string())),
            }
        }
        Err(CatalogError::unavailable(
            "the receipt key kept racing other writers after 3 attempts",
        ))
    }

    async fn receipts_of(
        &self,
        account: &str,
        repository: &str,
        asset_path: &str,
    ) -> Result<Vec<ProvenanceReceipt>, CatalogError> {
        let subject = path_subject(account, repository, asset_path);
        let mut receipts = self.list_receipts(&subject, None).await?;
        Self::sort_newest(&mut receipts);
        Ok(receipts)
    }

    async fn latest_receipts(
        &self,
        account: &str,
        repository: &str,
        asset_path: &str,
        limit: usize,
    ) -> Result<Vec<ProvenanceReceipt>, CatalogError> {
        let subject = path_subject(account, repository, asset_path);
        let mut receipts = self.list_receipts(&subject, Some(limit)).await?;
        Self::sort_newest(&mut receipts);
        Ok(receipts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receipt(id: &str, account: &str) -> ProvenanceReceipt {
        ProvenanceReceipt {
            id: id.to_string(),
            account: account.to_string(),
            repository: "central".to_string(),
            asset_path: "org/a.jar".to_string(),
            upstream_id: "lab".to_string(),
            source_url: "https://upstream.example/org/a.jar".to_string(),
            config_version: 4,
            digest: ContentDigest::sha256("a".repeat(64)),
            size_bytes: 1234,
            checksum: ChecksumEvidence::Declared {
                header: "sha256-abc".to_string(),
            },
            status: 200,
            redirects: 1,
            etag: Some("\"E1\"".to_string()),
            last_modified: Some("Wed, 21 Oct 2015 07:28:00 GMT".to_string()),
            outcome: ReceiptOutcome::Fetched,
            actor: "hrn:hodei:iam:acme:User/alice".to_string(),
            correlation_id: "corr-1".to_string(),
            fetched_at: DateTime::<Utc>::from_timestamp(1_700_000_000, 0).expect("a datetime"),
        }
    }

    #[test]
    fn keys_and_subjects_scope_the_row() {
        assert_eq!(
            receipt_key("acme", "prv-1"),
            "acme/receipts/prv-1".to_string()
        );
        assert_eq!(
            path_subject("acme", "central", "org/a.jar"),
            "acme/central/org/a.jar".to_string()
        );
        // Two accounts never share a row key, even with the same receipt id.
        assert_ne!(receipt_key("a", "prv-1"), receipt_key("b", "prv-1"));
    }

    #[test]
    fn the_identifier_charset_is_the_graph_key_charset() {
        assert!(valid_id("prv-9f8a"));
        assert!(valid_id("a.b:c@d_e"));
        assert!(!valid_id(""));
        assert!(!valid_id("-leading"));
        assert!(!valid_id("has space"));
        assert!(!valid_id("pipe|here"));
        assert!(!valid_id(&"x".repeat(129)));
    }

    #[test]
    fn nquads_are_canonical_and_optional_fields_only_stand_when_present() {
        let first = DgraphProvenanceLedger::receipt_nquads(&receipt("prv-1", "acme"));
        let again = DgraphProvenanceLedger::receipt_nquads(&receipt("prv-1", "acme"));
        assert_eq!(first, again, "the same receipt writes the same text");
        assert!(first.contains(&format!(
            "_:rec <hodei.proxy.receipt.key> {:?} .",
            "acme/receipts/prv-1"
        )));
        assert!(first.contains("sha256-abc"));
        assert!(first.contains(&dql_string("\"E1\"")));

        let mut sparse = receipt("prv-2", "acme");
        sparse.checksum = ChecksumEvidence::ComputedOnly;
        sparse.etag = None;
        sparse.last_modified = None;
        let text = DgraphProvenanceLedger::receipt_nquads(&sparse);
        assert!(!text.contains("checksum_header"));
        assert!(!text.contains("hodei.proxy.receipt.etag"));
        assert!(!text.contains("hodei.proxy.receipt.last_modified"));
        assert!(text.contains("computed_only"));
    }

    #[test]
    fn a_row_read_back_equals_the_receipt_that_wrote_it() {
        let original = receipt("prv-3", "acme");
        let node = serde_json::json!({
            "hodei.proxy.receipt.account": "acme",
            "hodei.proxy.receipt.repository": "central",
            "hodei.proxy.receipt.path": "org/a.jar",
            "hodei.proxy.receipt.id": "prv-3",
            "hodei.proxy.receipt.upstream_id": "lab",
            "hodei.proxy.receipt.source_url": "https://upstream.example/org/a.jar",
            "hodei.proxy.receipt.config_version": 4,
            "hodei.proxy.receipt.digest_algorithm": "sha256",
            "hodei.proxy.receipt.digest": "a".repeat(64),
            "hodei.proxy.receipt.size_bytes": 1234,
            "hodei.proxy.receipt.checksum_kind": "declared",
            "hodei.proxy.receipt.checksum_header": "sha256-abc",
            "hodei.proxy.receipt.status": 200,
            "hodei.proxy.receipt.redirects": 1,
            "hodei.proxy.receipt.etag": "\"E1\"",
            "hodei.proxy.receipt.last_modified": "Wed, 21 Oct 2015 07:28:00 GMT",
            "hodei.proxy.receipt.outcome": "fetched",
            "hodei.proxy.receipt.actor": "hrn:hodei:iam:acme:User/alice",
            "hodei.proxy.receipt.correlation_id": "corr-1",
            "hodei.proxy.receipt.fetched_at": original.fetched_at.to_rfc3339(),
        });
        let read_back = the_receipt(&node).expect("the row reads back");
        assert_eq!(read_back, original);
    }

    #[test]
    fn an_unreadable_row_is_refused_not_reinterpreted() {
        let mut node = serde_json::json!({
            "hodei.proxy.receipt.id": "prv-4",
            "hodei.proxy.receipt.outcome": "teleported",
            "hodei.proxy.receipt.account": "acme",
            "hodei.proxy.receipt.repository": "central",
            "hodei.proxy.receipt.path": "a",
            "hodei.proxy.receipt.upstream_id": "lab",
            "hodei.proxy.receipt.source_url": "u",
            "hodei.proxy.receipt.config_version": 1,
            "hodei.proxy.receipt.digest_algorithm": "sha256",
            "hodei.proxy.receipt.digest": "b".repeat(64),
            "hodei.proxy.receipt.size_bytes": 1,
            "hodei.proxy.receipt.checksum_kind": "computed_only",
            "hodei.proxy.receipt.status": 200,
            "hodei.proxy.receipt.redirects": 0,
            "hodei.proxy.receipt.actor": "a",
            "hodei.proxy.receipt.correlation_id": "c",
            "hodei.proxy.receipt.fetched_at": "2026-01-01T00:00:00+00:00",
        });
        assert!(the_receipt(&node).is_err(), "an unknown outcome refuses");
        node["hodei.proxy.receipt.outcome"] = serde_json::Value::String("quarantined".to_string());
        let receipt = the_receipt(&node).expect("a known outcome reads");
        assert_eq!(receipt.outcome, ReceiptOutcome::Quarantined);
    }

    #[test]
    fn a_missing_outcome_is_read_as_fetched_like_the_reference() {
        let mut node = serde_json::json!({
            "hodei.proxy.receipt.id": "prv-5",
            "hodei.proxy.receipt.account": "acme",
            "hodei.proxy.receipt.repository": "central",
            "hodei.proxy.receipt.path": "a",
            "hodei.proxy.receipt.upstream_id": "lab",
            "hodei.proxy.receipt.source_url": "u",
            "hodei.proxy.receipt.config_version": 1,
            "hodei.proxy.receipt.digest_algorithm": "sha256",
            "hodei.proxy.receipt.digest": "b".repeat(64),
            "hodei.proxy.receipt.size_bytes": 1,
            "hodei.proxy.receipt.checksum_kind": "computed_only",
            "hodei.proxy.receipt.status": 200,
            "hodei.proxy.receipt.redirects": 0,
            "hodei.proxy.receipt.actor": "a",
            "hodei.proxy.receipt.correlation_id": "c",
            "hodei.proxy.receipt.fetched_at": "2026-01-01T00:00:00+00:00",
        });
        node.as_object_mut()
            .expect("object")
            .remove("hodei.proxy.receipt.outcome");
        let read = the_receipt(&node).expect("pre-cache rows read as fetched");
        assert_eq!(read.outcome, ReceiptOutcome::Fetched);
    }
}
