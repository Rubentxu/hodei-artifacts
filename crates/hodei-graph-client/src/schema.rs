//! The versioned schema of the Hodei graph, applied at boot.
//!
//! The graph is never opened "as found": the store boots through
//! [`ensure_ready`], which reads the schema metadata node persisted in the
//! server itself, compares it against the schema this binary was built
//! with, and either applies the schema (fresh graph or older version),
//! accepts it as-is (same version and hash) or **refuses to open** (a
//! stored version newer than the binary, a stored hash that does not
//! match the declared schema, or required predicates missing or wrongly
//! declared in the live server). There is no fallback and no repair in
//! place of the operator: an incompatible server fails the boot with a
//! typed error, so no write can ever land on an unvalidated graph.
//!
//! The schema itself is minimal and additive:
//!
//! - the identity node (see [`crate::identity`]) with its unique
//!   composite key, its canonical HRN, its deterministic digest and the
//!   CAS revision guard;
//! - the shared scope guard and the idempotent operation registry (see
//!   [`crate::guard`] and [`crate::idempotency`]), the transactional
//!   guard a writer of tags, references, GC holds or quotas runs under;
//! - two native edge predicates (`owner`, `member_of`) with reverse
//!   indexes, so references are traversable in both directions;
//! - the schema metadata node that makes the schema versioned *in the
//!   server*, not only in the binary.
//!
//! Predicates carry the indexes the upserts need: `exact` indexes on the
//! uniqueness-bearing strings plus `@upsert`, so the server itself
//! rejects concurrent writes over the same key; `int` predicates that
//! take part in CAS increments are indexed and `@upsert` too.

use std::time::Duration;

use sha2::{Digest, Sha256};
use tracing::{debug, info};

use crate::client::GraphClient;
use crate::error::GraphClientError;
use crate::port::{GraphMutationPort, GraphUpsertTransaction};

/// Version of the schema this binary carries. Any server whose stored
/// schema is *newer* than this must not be opened by this binary: it was
/// upgraded by a build that knows more than we do.
///
/// v2 adds the transactional guard of scopes ([`crate::guard`]) and the
/// idempotent operation registry ([`crate::idempotency`]): the shared
/// conflict key per scope (`hodei.guard.*`) and the once-only operation
/// marker (`hodei.operation.*`), both `@upsert` guarded so the server
/// itself forces the conflict between concurrent writers.
///
/// v3 adds the sequenced evidence ledger and IAM carriers. v4 adds
/// immutable evidence metadata and its producer/external-identity index;
/// content bytes remain in the existing CAS and are never stored here.
pub const SCHEMA_VERSION: u64 = 4;

/// How many times a schema metadata write may lose the write race before
/// giving up (same policy as the identity upserts).
const MAX_META_ATTEMPTS: usize = 5;

/// Base delay between metadata-write retries; it doubles per attempt.
const META_RETRY_BASE: Duration = Duration::from_millis(10);

/// One predicate declaration of the schema, with everything the live
/// server must expose for the store to accept it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PredicateSpec {
    /// The predicate name (`hodei.identity.key`).
    pub name: &'static str,
    /// The declared DQL line, as sent to `alter` and hashed.
    pub declaration: &'static str,
    /// The value type the live schema must report (`string`, `int`,
    /// `datetime`, `uid`).
    pub expected_type: &'static str,
    /// The index the live schema must report, when one is required.
    pub expected_index: Option<&'static str>,
    /// Whether the predicate is a DQL list (`[uid]`).
    pub expected_list: bool,
    /// Whether `@reverse` must be declared.
    pub expected_reverse: bool,
    /// Whether `@upsert` must be declared.
    pub expected_upsert: bool,
}

/// The schema this binary carries: its version, its predicates and its
/// node types, plus the hash of the canonical text used as the
/// integrity check at boot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HodeiSchema {
    version: u64,
    predicates: Vec<PredicateSpec>,
    type_declarations: Vec<String>,
}

impl HodeiSchema {
    /// Builds a schema from its parts. Callers in this crate use
    /// [`hodei_schema`]; the constructor is public so tests can build
    /// variants (e.g. a schema that expects a predicate the server does
    /// not have) against a real server.
    pub fn new(
        version: u64,
        predicates: Vec<PredicateSpec>,
        type_declarations: Vec<String>,
    ) -> Self {
        Self {
            version,
            predicates,
            type_declarations,
        }
    }

    /// The schema version this instance declares.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// The predicate specifications, in declaration order.
    pub fn predicates(&self) -> &[PredicateSpec] {
        &self.predicates
    }

    /// The predicate specifications, mutable, so test variants can derive
    /// an incompatible schema from the declared one (e.g. one that expects
    /// a predicate the server does not have) and prove the refusal.
    pub fn predicates_mut(&mut self) -> &mut Vec<PredicateSpec> {
        &mut self.predicates
    }

    /// The canonical DQL text of the whole schema: predicates first, then
    /// the node types, in declaration order. Deterministic by
    /// construction: the same schema always renders the same text, and
    /// re-sending an identical text to `alter` is accepted by the server
    /// as a no-op, which is what makes booting idempotent.
    pub fn canonical_dql(&self) -> String {
        let mut text = String::new();
        for predicate in &self.predicates {
            text.push_str(predicate.declaration);
            text.push('\n');
        }
        for type_declaration in &self.type_declarations {
            text.push_str(type_declaration);
            text.push('\n');
        }
        text
    }

    /// SHA-256 of the canonical DQL text, hex encoded. Stored in the
    /// server with the version number; at boot both must match what this
    /// binary declares, or the server is refused.
    pub fn hash(&self) -> String {
        let digest = Sha256::digest(self.canonical_dql().as_bytes());
        hex::encode(digest)
    }

    /// The `schema(pred: [...])` query listing every required predicate.
    /// Predicate names in this query are bare (no quotes): the schema
    /// query is not a data query.
    fn verification_query(&self) -> String {
        let names: Vec<&str> = self.predicates.iter().map(|spec| spec.name).collect();
        format!("schema(pred: [{}]) {{}}", names.join(", "))
    }
}

/// The schema this binary carries (see the module documentation for the
/// design). Predicate by predicate:
///
/// - `hodei.identity.key` — the composite `tenant|canonical-HRN` key:
///   the uniqueness anchor, indexed `exact` and `@upsert` so the server
///   itself rejects concurrent duplicates;
/// - `hodei.identity.hrn` and `hodei.identity.tenant` — the readable
///   halves of the key, indexed for lookups (the HRN is *not* unique by
///   itself: two tenants may name their resources identically);
/// - `hodei.identity.digest` — the deterministic public identity (SHA-256
///   of the key): unique per graph, `@upsert` guarded;
/// - `hodei.identity.name` — the display name (the one two different
///   tenants may share freely); no index: names are not lookup keys;
/// - `hodei.identity.revision` — the CAS guard of the aggregate: indexed
///   and `@upsert`, so concurrent increments conflict instead of
///   silently overwriting each other;
/// - `hodei.identity.created_at` — typed timestamp, no index;
/// - `hodei.identity.owner` — a single native edge to the owner node,
///   reverse indexed (cardinality 1: re-setting it replaces it);
/// - `hodei.identity.member_of` — a native list edge to the groups, in
///   both directions (cardinality N);
/// - `hodei.schema.*` — the metadata node itself, with the version and
///   hash `@upsert` guarded so two bootstrappers cannot both claim it;
/// - `hodei.guard.*` — the shared scope guard ([`crate::guard`]): one
///   node per scope (`hodei.guard.key`, unique and `@upsert` guarded)
///   whose revision every writer of the scope advances inside its own
///   transaction. The advance writes the scope-qualified token of the
///   next revision (`hodei.guard.token`, exact-indexed and `@upsert`):
///   two writers advancing to the same revision write the SAME new
///   token, and the server forces one of them to abort — snapshot
///   isolation is not serialization, so the conflict is the server's
///   decision, not a client's hope;
/// - `hodei.operation.*` — the idempotent operation registry
///   ([`crate::idempotency`]): one node per operation id, committed
///   atomically WITH its effect, so a retried operation finds its
///   marker and replays the already-committed result instead of
///   applying the effect twice.
pub fn hodei_schema() -> HodeiSchema {
    let spec = |name,
                declaration,
                expected_type,
                expected_index,
                expected_list,
                expected_reverse,
                expected_upsert| PredicateSpec {
        name,
        declaration,
        expected_type,
        expected_index,
        expected_list,
        expected_reverse,
        expected_upsert,
    };
    HodeiSchema::new(
        SCHEMA_VERSION,
        vec![
            spec(
                "hodei.identity.key",
                "hodei.identity.key: string @index(exact) @upsert .",
                "string",
                Some("exact"),
                false,
                false,
                true,
            ),
            spec(
                "hodei.identity.hrn",
                "hodei.identity.hrn: string @index(exact) .",
                "string",
                Some("exact"),
                false,
                false,
                false,
            ),
            spec(
                "hodei.identity.tenant",
                "hodei.identity.tenant: string @index(exact) .",
                "string",
                Some("exact"),
                false,
                false,
                false,
            ),
            spec(
                "hodei.identity.digest",
                "hodei.identity.digest: string @index(exact) @upsert .",
                "string",
                Some("exact"),
                false,
                false,
                true,
            ),
            spec(
                "hodei.identity.name",
                "hodei.identity.name: string @index(exact) .",
                "string",
                Some("exact"),
                false,
                false,
                false,
            ),
            spec(
                "hodei.identity.revision",
                "hodei.identity.revision: int @index(int) @upsert .",
                "int",
                Some("int"),
                false,
                false,
                true,
            ),
            spec(
                "hodei.identity.created_at",
                "hodei.identity.created_at: datetime .",
                "datetime",
                None,
                false,
                false,
                false,
            ),
            spec(
                "hodei.identity.owner",
                "hodei.identity.owner: uid @reverse .",
                "uid",
                None,
                false,
                true,
                false,
            ),
            spec(
                "hodei.identity.member_of",
                "hodei.identity.member_of: [uid] @reverse .",
                "uid",
                None,
                true,
                true,
                false,
            ),
            spec(
                "hodei.schema.version",
                "hodei.schema.version: int @index(int) @upsert .",
                "int",
                Some("int"),
                false,
                false,
                true,
            ),
            spec(
                "hodei.schema.hash",
                "hodei.schema.hash: string @index(exact) @upsert .",
                "string",
                Some("exact"),
                false,
                false,
                true,
            ),
            spec(
                "hodei.schema.applied_at",
                "hodei.schema.applied_at: datetime .",
                "datetime",
                None,
                false,
                false,
                false,
            ),
            spec(
                "hodei.guard.key",
                "hodei.guard.key: string @index(exact) @upsert .",
                "string",
                Some("exact"),
                false,
                false,
                true,
            ),
            spec(
                "hodei.guard.revision",
                "hodei.guard.revision: int @index(int) @upsert .",
                "int",
                Some("int"),
                false,
                false,
                true,
            ),
            spec(
                "hodei.guard.token",
                "hodei.guard.token: string @index(exact) @upsert .",
                "string",
                Some("exact"),
                false,
                false,
                true,
            ),
            spec(
                "hodei.guard.updated_at",
                "hodei.guard.updated_at: datetime .",
                "datetime",
                None,
                false,
                false,
                false,
            ),
            spec(
                "hodei.operation.id",
                "hodei.operation.id: string @index(exact) @upsert .",
                "string",
                Some("exact"),
                false,
                false,
                true,
            ),
            spec(
                "hodei.operation.scope",
                "hodei.operation.scope: string @index(exact) .",
                "string",
                Some("exact"),
                false,
                false,
                false,
            ),
            spec(
                "hodei.operation.result",
                "hodei.operation.result: string .",
                "string",
                None,
                false,
                false,
                false,
            ),
            spec(
                "hodei.operation.committed_at",
                "hodei.operation.committed_at: datetime .",
                "datetime",
                None,
                false,
                false,
                false,
            ),
            // HSP-185: the sequenced evidence ledger. One node per
            // stream position; the composite key (`digest#position`) is
            // exact-indexed and `@upsert`, so the server itself refuses
            // two nodes claiming the same position. The head of the
            // stream is the guard's revision, not a stored row.
            spec(
                "hodei.ledger.key",
                "hodei.ledger.key: string @index(exact) @upsert .",
                "string",
                Some("exact"),
                false,
                false,
                true,
            ),
            spec(
                "hodei.ledger.stream_id",
                "hodei.ledger.stream_id: string @index(exact) .",
                "string",
                Some("exact"),
                false,
                false,
                false,
            ),
            spec(
                "hodei.ledger.position",
                "hodei.ledger.position: string .",
                "string",
                None,
                false,
                false,
                false,
            ),
            spec(
                "hodei.ledger.event_id",
                "hodei.ledger.event_id: string @index(exact) .",
                "string",
                Some("exact"),
                false,
                false,
                false,
            ),
            spec(
                "hodei.ledger.tenant_hrn",
                "hodei.ledger.tenant_hrn: string .",
                "string",
                None,
                false,
                false,
                false,
            ),
            spec(
                "hodei.ledger.event_type",
                "hodei.ledger.event_type: string .",
                "string",
                None,
                false,
                false,
                false,
            ),
            spec(
                "hodei.ledger.schema_version",
                "hodei.ledger.schema_version: string .",
                "string",
                None,
                false,
                false,
                false,
            ),
            spec(
                "hodei.ledger.occurred_at",
                "hodei.ledger.occurred_at: string .",
                "string",
                None,
                false,
                false,
                false,
            ),
            spec(
                "hodei.ledger.recorded_at",
                "hodei.ledger.recorded_at: string .",
                "string",
                None,
                false,
                false,
                false,
            ),
            spec(
                "hodei.ledger.producer",
                "hodei.ledger.producer: string .",
                "string",
                None,
                false,
                false,
                false,
            ),
            spec(
                "hodei.ledger.correlation_id",
                "hodei.ledger.correlation_id: string .",
                "string",
                None,
                false,
                false,
                false,
            ),
            spec(
                "hodei.ledger.causation_id",
                "hodei.ledger.causation_id: string .",
                "string",
                None,
                false,
                false,
                false,
            ),
            spec(
                "hodei.ledger.idempotency_key",
                "hodei.ledger.idempotency_key: string .",
                "string",
                None,
                false,
                false,
                false,
            ),
            spec(
                "hodei.ledger.payload",
                "hodei.ledger.payload: string .",
                "string",
                None,
                false,
                false,
                false,
            ),
            // IAM carriers (v3): the discriminator plus the fields the
            // IAM adapters of `hodei-iam` store. The IAM nodes reuse the
            // identity key (unique, `@upsert`) and the identity revision
            // (CAS) — there is no second uniqueness or versioning anchor.
            // `hodei.iam.hrn` is exact-indexed (no `@upsert`): the same
            // resource HRN under a different tenant key is legal and
            // must be able to coexist; the tenant|HRN composite stays
            // the uniqueness anchor.
            spec(
                "hodei.iam.kind",
                "hodei.iam.kind: string @index(exact) .",
                "string",
                Some("exact"),
                false,
                false,
                false,
            ),
            spec(
                "hodei.iam.hrn",
                "hodei.iam.hrn: string @index(exact) .",
                "string",
                Some("exact"),
                false,
                false,
                false,
            ),
            spec(
                "hodei.iam.content",
                "hodei.iam.content: string .",
                "string",
                None,
                false,
                false,
                false,
            ),
            spec(
                "hodei.iam.description",
                "hodei.iam.description: string .",
                "string",
                None,
                false,
                false,
                false,
            ),
            spec(
                "hodei.iam.email",
                "hodei.iam.email: string .",
                "string",
                None,
                false,
                false,
                false,
            ),
            spec(
                "hodei.iam.tags",
                "hodei.iam.tags: [string] .",
                "string",
                None,
                true,
                false,
                false,
            ),
            spec(
                "hodei.iam.attachment",
                "hodei.iam.attachment: [string] @index(exact) .",
                "string",
                Some("exact"),
                true,
                false,
                false,
            ),
            spec(
                "hodei.iam.created_at",
                "hodei.iam.created_at: string .",
                "string",
                None,
                false,
                false,
                false,
            ),
            spec(
                "hodei.iam.updated_at",
                "hodei.iam.updated_at: string .",
                "string",
                None,
                false,
                false,
                false,
            ),
            spec(
                "hodei.evidence.key",
                "hodei.evidence.key: string @index(exact) @upsert .",
                "string",
                Some("exact"),
                false,
                false,
                true,
            ),
            spec(
                "hodei.evidence.tenant",
                "hodei.evidence.tenant: string @index(exact) .",
                "string",
                Some("exact"),
                false,
                false,
                false,
            ),
            spec(
                "hodei.evidence.producer_id",
                "hodei.evidence.producer_id: string .",
                "string",
                None,
                false,
                false,
                false,
            ),
            spec(
                "hodei.evidence.external_id",
                "hodei.evidence.external_id: string .",
                "string",
                None,
                false,
                false,
                false,
            ),
            spec(
                "hodei.evidence.type",
                "hodei.evidence.type: string .",
                "string",
                None,
                false,
                false,
                false,
            ),
            spec(
                "hodei.evidence.content_digest",
                "hodei.evidence.content_digest: string .",
                "string",
                None,
                false,
                false,
                false,
            ),
            spec(
                "hodei.evidence.account",
                "hodei.evidence.account: string @index(exact) .",
                "string",
                Some("exact"),
                false,
                false,
                false,
            ),
            spec(
                "hodei.evidence.subject",
                "hodei.evidence.subject: [string] @index(exact) .",
                "string",
                Some("exact"),
                true,
                false,
                false,
            ),
            spec(
                "hodei.evidence.record",
                "hodei.evidence.record: string .",
                "string",
                None,
                false,
                false,
                false,
            ),
            spec(
                "hodei.evidence.external_key",
                "hodei.evidence.external_key: string @index(exact) @upsert .",
                "string",
                Some("exact"),
                false,
                false,
                true,
            ),
            spec(
                "hodei.evidence.external_evidence_id",
                "hodei.evidence.external_evidence_id: string .",
                "string",
                None,
                false,
                false,
                false,
            ),
        ],
        vec![
            // The type declarations put every field on its own line:
            // the DQL grammar reads fields line by line inside `type`.
            "type HodeiIdentity {\n  hodei.identity.key\n  hodei.identity.hrn\n  \
             hodei.identity.tenant\n  hodei.identity.digest\n  hodei.identity.name\n  \
             hodei.identity.revision\n  hodei.identity.created_at\n  hodei.identity.owner\n  \
             hodei.identity.member_of\n}"
                .to_owned(),
            // The fixture-bearing node types reuse identity predicates:
            // no new ontology, only typed carriers for the native edges.
            "type HodeiOwner {\n  hodei.identity.tenant\n  hodei.identity.name\n}".to_owned(),
            "type HodeiGroup {\n  hodei.identity.tenant\n  hodei.identity.name\n}".to_owned(),
            "type HodeiSchemaMeta {\n  hodei.schema.version\n  hodei.schema.hash\n  \
             hodei.schema.applied_at\n}"
                .to_owned(),
            "type HodeiGuard {\n  hodei.guard.key\n  hodei.guard.revision\n  \
             hodei.guard.token\n  hodei.guard.updated_at\n}"
                .to_owned(),
            "type HodeiOperation {\n  hodei.operation.id\n  hodei.operation.scope\n  \
             hodei.operation.result\n  hodei.operation.committed_at\n}"
                .to_owned(),
            "type HodeiLedgerEvent {\n  hodei.ledger.key\n  hodei.ledger.stream_id\n  \
             hodei.ledger.position\n  hodei.ledger.event_id\n  hodei.ledger.tenant_hrn\n  \
             hodei.ledger.event_type\n  hodei.ledger.schema_version\n  hodei.ledger.occurred_at\n  \
             hodei.ledger.recorded_at\n  hodei.ledger.producer\n  hodei.ledger.correlation_id\n  \
             hodei.ledger.causation_id\n  hodei.ledger.idempotency_key\n  hodei.ledger.payload\n}"
                .to_owned(),
            // The IAM carriers are identity nodes: they keep the composite
            // key and the CAS revision (the uniqueness and versioning
            // anchors) and add the IAM payload.
            "type HodeiIam {\n  hodei.identity.key\n  hodei.identity.hrn\n  \
             hodei.identity.tenant\n  hodei.identity.digest\n  hodei.identity.name\n  \
             hodei.identity.revision\n  hodei.identity.created_at\n  hodei.iam.kind\n  \
             hodei.iam.hrn\n  hodei.iam.content\n  hodei.iam.description\n  \
             hodei.iam.email\n  hodei.iam.tags\n  hodei.iam.attachment\n  \
             hodei.iam.created_at\n  hodei.iam.updated_at\n}"
                .to_owned(),
            "type HodeiEvidence {\n  hodei.evidence.key\n  hodei.evidence.tenant\n  hodei.evidence.producer_id\n  hodei.evidence.external_id\n  hodei.evidence.type\n  hodei.evidence.content_digest\n  hodei.evidence.account\n  hodei.evidence.subject\n  hodei.evidence.record\n}"
                .to_owned(),
            "type HodeiEvidenceExternal {\n  hodei.evidence.external_key\n  hodei.evidence.external_evidence_id\n}"
                .to_owned(),
        ],
    )
}

/// Why the live server cannot be opened by this binary.
#[derive(Debug, thiserror::Error)]
pub enum SchemaBootError {
    /// A transport, auth or protocol failure of the underlying client
    /// while reading, altering or writing the schema.
    #[error(transparent)]
    Graph(#[from] GraphClientError),
    /// The stored schema is newer than the one this binary carries:
    /// opening would silently operate on predicates and semantics this
    /// build does not know. The operator must either upgrade this
    /// process or roll the server back.
    #[error(
        "the stored schema version {stored} is newer than the schema of this binary ({binary}): \
         refusing to open a graph upgraded by a newer build"
    )]
    NewerThanBinary {
        /// The version stored in the server.
        stored: u64,
        /// The version this binary declares.
        binary: u64,
    },
    /// The stored version matches but the stored hash does not: the
    /// server carries a schema that was never declared by any build of
    /// this lineage, so its semantics are unknown.
    #[error(
        "the stored schema hash '{stored_hash}' does not match the hash of the declared schema \
         '{binary_hash}' at the same version {version}: refusing to open a graph with a foreign \
         schema"
    )]
    HashMismatch {
        /// The version stored in the server.
        version: u64,
        /// The hash stored in the server.
        stored_hash: String,
        /// The hash this binary computes for its own schema text.
        binary_hash: String,
    },
    /// A predicate the schema requires does not exist in the live server.
    #[error("the required predicate '{predicate}' is missing in the live server schema")]
    PredicateMissing {
        /// The predicate that is not there.
        predicate: &'static str,
    },
    /// A predicate exists but its declared type differs.
    #[error(
        "the predicate '{predicate}' is declared as '{found}' but the schema requires '{expected}'"
    )]
    PredicateTypeMismatch {
        /// The predicate whose type differs.
        predicate: &'static str,
        /// The type the schema requires.
        expected: &'static str,
        /// The type the server reports.
        found: String,
    },
    /// A predicate exists but the index the upserts rely on is absent.
    #[error(
        "the predicate '{predicate}' is missing the required index '{expected}': upserts and \
         uniqueness would not be enforced"
    )]
    PredicateIndexMissing {
        /// The predicate whose index differs.
        predicate: &'static str,
        /// The index the schema requires.
        expected: &'static str,
    },
    /// A predicate exists but a structural flag (`@reverse`, `@upsert`,
    /// list cardinality) differs from the declaration.
    #[error(
        "the predicate '{predicate}' does not match its declaration: {detail} (expected \
         list = {expected_list}, reverse = {expected_reverse}, upsert = {expected_upsert})"
    )]
    PredicateFlagsMismatch {
        /// The predicate whose flags differ.
        predicate: &'static str,
        /// The server's own detail of what differs.
        detail: String,
        /// Whether the schema requires list cardinality.
        expected_list: bool,
        /// Whether the schema requires `@reverse`.
        expected_reverse: bool,
        /// Whether the schema requires `@upsert`.
        expected_upsert: bool,
    },
    /// The metadata node is duplicated or unreadable: the schema book
    ///keeping of the server is inconsistent and the graph is refused.
    #[error("the schema metadata state of the server is inconsistent: {detail}")]
    CorruptMeta {
        /// What is wrong with the metadata nodes.
        detail: String,
    },
}

/// The decision boot makes from the stored metadata versus the binary
/// schema. Pure and testable: no server is involved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootDecision {
    /// Nothing is stored: the graph is fresh. Apply the full schema and
    /// write the metadata node.
    ApplyFresh,
    /// The stored version and hash match this binary. Nothing to apply;
    /// the live predicates are still verified.
    AcceptCurrent,
    /// The stored version is older than the binary's: apply the schema
    /// (idempotent by construction) and update the metadata node.
    UpgradeFrom {
        /// The version that was stored.
        stored: u64,
    },
}

/// Pure boot decision from the stored metadata against the binary
/// schema. Every incompatible case is an `Err`, never a fallback.
pub fn decide_boot(
    stored: Option<(u64, String)>,
    binary_version: u64,
    binary_hash: &str,
) -> Result<BootDecision, SchemaBootError> {
    match stored {
        None => Ok(BootDecision::ApplyFresh),
        Some((version, hash)) => {
            if version > binary_version {
                return Err(SchemaBootError::NewerThanBinary {
                    stored: version,
                    binary: binary_version,
                });
            }
            if version == binary_version && hash != binary_hash {
                return Err(SchemaBootError::HashMismatch {
                    version,
                    stored_hash: hash,
                    binary_hash: binary_hash.to_owned(),
                });
            }
            if version == binary_version {
                Ok(BootDecision::AcceptCurrent)
            } else {
                Ok(BootDecision::UpgradeFrom { stored: version })
            }
        }
    }
}

/// What boot did, for logs and evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaBootReport {
    /// The decision taken.
    pub decision: BootDecision,
    /// The schema version this binary carries.
    pub binary_version: u64,
    /// The schema version stored in the server before boot, when any.
    pub stored_version: Option<u64>,
    /// Whether the schema was altered by this boot.
    pub applied: bool,
    /// How many required predicates were verified against the live
    /// server after the decision.
    pub verified_predicates: usize,
}

/// Reads the schema metadata stored in the server, if any.
///
/// More than one metadata node is corruption: the versioning bookkeeping
/// must be unique, so a duplicate refuses the boot instead of letting
/// boot pick one silently.
pub async fn read_stored_schema(
    client: &GraphClient,
) -> Result<Option<(u64, String)>, SchemaBootError> {
    let response = client
        .execute_dql(
            "{ q(func: type(HodeiSchemaMeta)) { uid hodei.schema.version hodei.schema.hash } }",
        )
        .await?;
    let nodes = response
        .json
        .get("q")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    match nodes.len() {
        0 => Ok(None),
        1 => {
            let node = &nodes[0];
            let version = node
                .get("hodei.schema.version")
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| SchemaBootError::CorruptMeta {
                    detail: "the metadata node has no readable version".to_owned(),
                })?;
            let hash = node
                .get("hodei.schema.hash")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| SchemaBootError::CorruptMeta {
                    detail: "the metadata node has no readable hash".to_owned(),
                })?
                .to_owned();
            Ok(Some((version, hash)))
        }
        n => Err(SchemaBootError::CorruptMeta {
            detail: format!("{n} schema metadata nodes exist; exactly one may"),
        }),
    }
}

/// Writes (or updates) the metadata node for the given version and hash.
///
/// The metadata node is looked up **by type**, never by its content: the
/// graph holds exactly one `HodeiSchemaMeta` node, and a metadata write
/// with a different hash must UPDATE that node (that is how a foreign
/// hash is installed and rolled back), not create a duplicate. Two
/// concurrent creators are still mutually exclusive through the
/// `@upsert` index on `hodei.schema.hash`: the loser revalidates and,
/// finding the winner's node, updates it.
pub async fn write_schema_meta(
    client: &GraphClient,
    version: u64,
    hash: &str,
    applied_at_rfc3339: &str,
) -> Result<(), SchemaBootError> {
    let mut last_error: Option<GraphClientError> = None;
    for attempt in 1..=MAX_META_ATTEMPTS {
        let hash_literal = dql_string(hash);
        let transaction = GraphUpsertTransaction {
            query: "{ m as var(func: type(HodeiSchemaMeta)) }".to_owned(),
            blocks: vec![
                crate::port::GraphMutationBlock::set_if(
                    "eq(len(m), 0)",
                    format!(
                        "_:m <dgraph.type> \"HodeiSchemaMeta\" .\n\
                         _:m <hodei.schema.version> \"{version}\" .\n\
                         _:m <hodei.schema.hash> {hash_literal} .\n\
                         _:m <hodei.schema.applied_at> \"{applied_at_rfc3339}\" .\n",
                    ),
                ),
                crate::port::GraphMutationBlock::set_if(
                    "eq(len(m), 1)",
                    format!(
                        "uid(m) <hodei.schema.version> \"{version}\" .\n\
                         uid(m) <hodei.schema.hash> {hash_literal} .\n\
                         uid(m) <hodei.schema.applied_at> \"{applied_at_rfc3339}\" .\n",
                    ),
                ),
            ],
            commit_now: true,
        };
        match client.upsert(transaction).await {
            Ok(_) => return Ok(()),
            Err(GraphClientError::UpsertConflict { detail }) => {
                debug!(attempt, %detail, "schema metadata write conflicted; revalidating");
                last_error = Some(GraphClientError::UpsertConflict { detail });
            }
            Err(other) => return Err(other.into()),
        }
        tokio::time::sleep(META_RETRY_BASE * (1u32 << (attempt - 1))).await;
    }
    Err(SchemaBootError::CorruptMeta {
        detail: format!(
            "the schema metadata write did not commit after {MAX_META_ATTEMPTS} attempts: {}",
            last_error
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "no detail".to_owned()),
        ),
    })
}

/// Verifies that every required predicate exists in the live server with
/// the type, index and flags the schema declares. This is the readiness
/// half of the boot: even when the stored metadata says "current", the
/// live schema must prove itself, because the metadata node can survive
/// a manual alteration of the server schema.
pub async fn verify_predicates(
    client: &GraphClient,
    schema: &HodeiSchema,
) -> Result<usize, SchemaBootError> {
    let response = client
        .execute_dql(&schema.verification_query())
        .await
        .map_err(SchemaBootError::from)?;
    let live = response
        .json
        .get("schema")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for spec in &schema.predicates {
        let found = live
            .iter()
            .find(|entry| {
                entry.get("predicate").and_then(serde_json::Value::as_str) == Some(spec.name)
            })
            .ok_or(SchemaBootError::PredicateMissing {
                predicate: spec.name,
            })?;
        let found_type = found
            .get("type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if found_type != spec.expected_type {
            return Err(SchemaBootError::PredicateTypeMismatch {
                predicate: spec.name,
                expected: spec.expected_type,
                found: found_type,
            });
        }
        if let Some(expected_index) = spec.expected_index {
            // The live schema reports the tokenizers in their own array
            // (with `index: true` as the marker that indexing exists).
            let tokenizers = found
                .get("tokenizer")
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default();
            let has_index = tokenizers
                .iter()
                .filter_map(serde_json::Value::as_str)
                .any(|tokenizer| tokenizer == expected_index);
            if !has_index {
                return Err(SchemaBootError::PredicateIndexMissing {
                    predicate: spec.name,
                    expected: expected_index,
                });
            }
        }
        let found_list = found
            .get("list")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let found_reverse = found
            .get("reverse")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let found_upsert = found
            .get("upsert")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if found_list != spec.expected_list
            || found_reverse != spec.expected_reverse
            || found_upsert != spec.expected_upsert
        {
            return Err(SchemaBootError::PredicateFlagsMismatch {
                predicate: spec.name,
                detail: format!(
                    "list = {found_list}, reverse = {found_reverse}, upsert = {found_upsert}"
                ),
                expected_list: spec.expected_list,
                expected_reverse: spec.expected_reverse,
                expected_upsert: spec.expected_upsert,
            });
        }
    }
    Ok(schema.predicates.len())
}

/// Boots the schema: the fail-closed gate every write path must pass.
///
/// Reads the stored metadata, decides ([`decide_boot`]), applies when the
/// decision says so, writes the metadata node, and always verifies the
/// live predicates before reporting success. Any incompatibility is a
/// typed `SchemaBootError` and the caller must not open the store.
pub async fn ensure_ready(client: &GraphClient) -> Result<SchemaBootReport, SchemaBootError> {
    let schema = hodei_schema();
    let stored = read_stored_schema(client).await?;
    let decision = decide_boot(stored.clone(), schema.version(), &schema.hash())?;
    let mut applied = false;
    match &decision {
        BootDecision::ApplyFresh => {
            info!(
                version = schema.version(),
                "applying the schema to a fresh graph"
            );
            client.alter_schema(&schema.canonical_dql()).await?;
            applied = true;
        }
        BootDecision::AcceptCurrent => {
            debug!(
                version = schema.version(),
                "schema is current; verifying only"
            );
        }
        BootDecision::UpgradeFrom { stored } => {
            info!(
                binary = schema.version(),
                stored, "upgrading the stored schema to the binary version (idempotent alter)"
            );
            client.alter_schema(&schema.canonical_dql()).await?;
            applied = true;
        }
    }
    let verified_predicates = verify_predicates(client, &schema).await?;
    if applied {
        write_schema_meta(client, schema.version(), &schema.hash(), &rfc3339_now()).await?;
    }
    Ok(SchemaBootReport {
        decision,
        binary_version: schema.version(),
        stored_version: stored.map(|(version, _)| version),
        applied,
        verified_predicates,
    })
}

/// The UTC instant now, RFC 3339 (what a Dgraph `datetime` accepts).
fn rfc3339_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Renders a Rust string as a DQL string literal: JSON-style escaping of
/// quotes, backslashes and control characters. Keys, HRNs and digests
/// are built from safe charsets, but the escaping is uniform so no
/// caller-side string can inject DQL.
///
/// Public because the IAM adapters (and any future graph consumer)
/// compose DQL literals from caller text (policy content, tags, names)
/// and must not re-implement the escaping.
pub fn dql_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The canonical text is stable across calls: the hash the binary
    /// stores must equal the hash it recomputes on the next boot.
    #[test]
    fn canonical_text_and_hash_are_stable() {
        let first = hodei_schema().canonical_dql();
        let second = hodei_schema().canonical_dql();
        assert_eq!(first, second, "the canonical schema text must be stable");
        assert_eq!(
            hodei_schema().hash(),
            hodei_schema().hash(),
            "the schema hash must be stable"
        );
        assert!(first.contains("hodei.identity.key: string @index(exact) @upsert ."));
        assert!(first.contains("type HodeiIdentity {"));
    }

    /// The unique-key predicates carry the indexes the upsert contract
    /// needs; a schema that forgot one would silently break uniqueness.
    #[test]
    fn uniqueness_predicates_are_indexed_and_upsert_guarded() {
        let schema = hodei_schema();
        for name in [
            "hodei.identity.key",
            "hodei.identity.digest",
            "hodei.schema.hash",
            "hodei.identity.revision",
            "hodei.schema.version",
            "hodei.guard.key",
            "hodei.guard.revision",
            "hodei.guard.token",
            "hodei.operation.id",
        ] {
            let spec = schema
                .predicates()
                .iter()
                .find(|p| p.name == name)
                .expect("declared predicate");
            assert!(spec.expected_upsert, "{name} must be @upsert");
            assert!(spec.expected_index.is_some(), "{name} must be indexed");
        }
    }

    /// Fresh graph: no metadata, the boot must apply everything.
    #[test]
    fn a_fresh_graph_is_applied_fully() {
        let schema = hodei_schema();
        let decision = decide_boot(None, schema.version(), &schema.hash())
            .expect("a fresh graph is compatible");
        assert_eq!(decision, BootDecision::ApplyFresh);
    }

    /// Same version and hash: nothing to apply, just verify.
    #[test]
    fn a_current_graph_is_accepted_without_writes() {
        let schema = hodei_schema();
        let decision = decide_boot(
            Some((schema.version(), schema.hash())),
            schema.version(),
            &schema.hash(),
        )
        .expect("an equal version and hash is compatible");
        assert_eq!(decision, BootDecision::AcceptCurrent);
    }

    /// Older stored version: the boot upgrades (apply + metadata update).
    #[test]
    fn an_older_stored_version_upgrades() {
        let schema = hodei_schema();
        let decision = decide_boot(
            Some((schema.version() - 1, "old-hash".to_owned())),
            schema.version(),
            &schema.hash(),
        )
        .expect("an older version is compatible by upgrade");
        assert_eq!(
            decision,
            BootDecision::UpgradeFrom {
                stored: schema.version() - 1
            }
        );
    }

    /// A stored version newer than the binary is refused: a downgrade
    /// must be an explicit operator decision, never a silent open.
    #[test]
    fn a_newer_stored_version_is_refused() {
        let schema = hodei_schema();
        let error = decide_boot(
            Some((schema.version() + 1, schema.hash())),
            schema.version(),
            &schema.hash(),
        )
        .expect_err("a newer stored version must be refused");
        assert!(
            matches!(error, SchemaBootError::NewerThanBinary { .. }),
            "unexpected: {error}"
        );
    }

    /// Same version, different hash: a foreign schema is refused.
    #[test]
    fn a_hash_mismatch_at_the_same_version_is_refused() {
        let schema = hodei_schema();
        let error = decide_boot(
            Some((schema.version(), "deadbeef".to_owned())),
            schema.version(),
            &schema.hash(),
        )
        .expect_err("a foreign hash must be refused");
        assert!(
            matches!(error, SchemaBootError::HashMismatch { .. }),
            "unexpected: {error}"
        );
    }

    /// The DQL string escaping never lets a caller-side string break out
    /// of the literal.
    #[test]
    fn dql_strings_are_escaped() {
        assert_eq!(dql_string("plain"), "\"plain\"");
        assert_eq!(dql_string("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(dql_string("back\\slash"), "\"back\\\\slash\"");
        assert_eq!(dql_string("\u{1}"), "\"\\u0001\"");
    }
}
