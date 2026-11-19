//! Persistent identity by tenant and HRN, on the real Dgraph.
//!
//! The identity of a resource is **deterministic**: it is derived from
//! the pair `(tenant, canonical HRN)` and from nothing else — not from
//! creation order, not from a counter, not from the server's internal
//! UIDs. Two derivations of the same pair produce the same identity,
//! and the Dgraph UID that the server assigns physically is never part
//! of the public surface: it stays inside this module and inside the
//! graph, because a physical pointer is not an identity and leaking it
//! would let callers mistake storage topology for meaning (doc-20:
//! `tenant + canonical HRN` as the unequivocal logical key, physical
//! UID never visible).
//!
//! Uniqueness is enforced by the server, not by hope:
//!
//! - `hodei.identity.key` (the composite key) and
//!   `hodei.identity.digest` (its SHA-256) are `exact`-indexed and
//!   `@upsert`-guarded, so two transactions writing the same key
//!   cannot both commit: the server aborts the loser with
//!   `ErrIndexingConflict`, which surfaces here as a retryable
//!   [`IdentityError::Graph`] wrapping [`GraphClientError::UpsertConflict`];
//! - `ensure_identity` revalidates on every retry (the query is part of
//!   each attempt's transaction), so the loser of the write race finds
//!   the winner's identity and returns it;
//! - any update of an existing identity goes through the CAS revision
//!   guard (`hodei.identity.revision` is `@upsert` too), with revalidation
//!   on retry, bounded attempts and backoff.
//!
//! The composite key format is `{tenant}|{canonical HRN}`. The `|` is
//! unambiguous by construction: tenants are restricted to the same
//! charset the HRN grammar uses for its account field (ASCII
//! alphanumeric plus `-` and `_`, non-empty), and canonical HRNs are
//! ASCII and never contain `|`, so the separator cannot appear in either
//! half and no concatenation can produce two readings. The digest is the
//! public, stable handle derived from the key.
//!
//! The canonical HRN itself is trusted to be canonical: the grammar lives
//! in the `hodei-hrn` crate of `hodei-security-contracts` (a sibling
//! repository that this crate must not depend on), so this module
//! validates the safety-relevant subset (ASCII, no separator, no
//! whitespace, non-empty, `hrn:` prefix) and leaves the grammar to its
//! authority.

use std::sync::Arc;
use std::time::Duration;

use sha2::{Digest, Sha256};
use tokio::time::sleep;
use tracing::{debug, info};

use crate::client::GraphClient;
use crate::error::GraphClientError;
use crate::port::{GraphMutationPort, GraphUpsertTransaction};
use crate::schema::{SchemaBootReport, ensure_ready};

/// The separator of the composite key. Both halves are validated to
/// exclude it, so `tenant|hrn` has exactly one parsing.
pub const KEY_SEPARATOR: char = '|';

/// The type tag of identity nodes in the graph.
pub const IDENTITY_TYPE: &str = "HodeiIdentity";

/// How many attempts a uniqueness upsert may lose before giving up.
const MAX_ATTEMPTS: usize = 5;

/// Base delay of the retry backoff; it doubles per attempt.
const RETRY_BASE: Duration = Duration::from_millis(10);

/// Why an identity operation failed.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    /// The schema boot refused the server: the store never opens on an
    /// incompatible graph, so no write can land on it.
    #[error("the store cannot open on this server: {0}")]
    Schema(#[from] crate::schema::SchemaBootError),
    /// A transport, auth or protocol failure, including the
    /// `UpsertConflict` that retryable loops handle internally.
    #[error(transparent)]
    Graph(#[from] GraphClientError),
    /// The tenant is not a safe composite-key half.
    #[error(
        "the tenant '{tenant}' is not a valid key half: tenants must be non-empty ASCII \
         alphanumeric with '-' and '_', at most {max} characters, without the '{separator}' \
         separator"
    )]
    InvalidTenant {
        /// The tenant as received.
        tenant: String,
        /// The maximum length accepted.
        max: usize,
        /// The separator the tenant must not contain.
        separator: char,
    },
    /// The HRN string is not a safe canonical HRN for the key.
    #[error("the HRN '{hrn}' is not a canonical HRN usable as a key half: {reason}")]
    InvalidHrn {
        /// The HRN as received.
        hrn: String,
        /// Why it was rejected.
        reason: &'static str,
    },
    /// The CAS revision guard rejected the update: someone else advanced
    /// the aggregate first. The caller reads the current revision from
    /// the error and decides (retry with the new revision, or stop).
    #[error(
        "the update of '{key}' lost the CAS check: expected revision {expected_revision} but \
         the aggregate is at revision {current_revision}"
    )]
    CasRejected {
        /// The composite key of the identity.
        key: String,
        /// The revision the caller expected.
        expected_revision: u64,
        /// The revision the aggregate actually has.
        current_revision: u64,
    },
    /// The graph contradicts its own uniqueness contract (duplicate key,
    /// key/HRN mismatch inside one node, ...): a real invariant break,
    /// never repaired silently.
    #[error("the identity state of '{key}' contradicts the uniqueness contract: {detail}")]
    CorruptState {
        /// The composite key involved.
        key: String,
        /// What contradicts the contract.
        detail: String,
    },
    /// The identity does not exist.
    #[error("the identity '{key}' does not exist")]
    NotFound {
        /// The composite key that was not found.
        key: String,
    },
    /// The uniqueness upsert lost every attempt. Bounded, explicit, and
    /// always reported: success is never inferred from exhaustion.
    #[error("the upsert of '{key}' did not commit after {attempts} attempts; last failure: {last}")]
    RetryExhausted {
        /// The composite key being written.
        key: String,
        /// How many attempts were made.
        attempts: usize,
        /// The last underlying failure.
        last: String,
    },
}

/// A validated tenant: the key half of every identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tenant(String);

/// The maximum tenant length (the HRN grammar's account limit).
const MAX_TENANT_LEN: usize = 128;

impl Tenant {
    /// Validates and wraps a tenant id.
    ///
    /// The charset is deliberately the same the HRN grammar accepts for
    /// its account field: ASCII alphanumeric, `-`, `_`, non-empty, and
    /// never the [`KEY_SEPARATOR`].
    pub fn new(tenant: &str) -> Result<Self, IdentityError> {
        let valid = !tenant.is_empty()
            && tenant.len() <= MAX_TENANT_LEN
            && tenant
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            && tenant.starts_with(|c: char| c.is_ascii_alphanumeric())
            && !tenant.contains(KEY_SEPARATOR);
        if valid {
            Ok(Self(tenant.to_owned()))
        } else {
            Err(IdentityError::InvalidTenant {
                tenant: tenant.to_owned(),
                max: MAX_TENANT_LEN,
                separator: KEY_SEPARATOR,
            })
        }
    }

    /// The validated tenant text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A validated canonical HRN string.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CanonicalHrn(String);

/// The maximum HRN length (the grammar's own limit).
const MAX_HRN_LEN: usize = 1481;

impl CanonicalHrn {
    /// Validates and wraps a canonical HRN string.
    ///
    /// The authoritative grammar lives in `hodei-hrn` (a sibling repo,
    /// intentionally not a dependency); this validates the subset that
    /// makes the string safe as a key half and as a DQL literal.
    pub fn new(hrn: &str) -> Result<Self, IdentityError> {
        if !hrn.starts_with("hrn:") {
            return Err(IdentityError::InvalidHrn {
                hrn: hrn.to_owned(),
                reason: "it must start with the 'hrn:' prefix",
            });
        }
        if hrn.len() > MAX_HRN_LEN {
            return Err(IdentityError::InvalidHrn {
                hrn: hrn.to_owned(),
                reason: "it exceeds the maximum HRN length",
            });
        }
        if !hrn.is_ascii() {
            return Err(IdentityError::InvalidHrn {
                hrn: hrn.to_owned(),
                reason: "it must be ASCII",
            });
        }
        if hrn.contains(KEY_SEPARATOR) {
            return Err(IdentityError::InvalidHrn {
                hrn: hrn.to_owned(),
                reason: "it must not contain the key separator",
            });
        }
        if hrn.chars().any(|c| c.is_whitespace() || (c as u32) < 0x21) {
            return Err(IdentityError::InvalidHrn {
                hrn: hrn.to_owned(),
                reason: "it must not contain whitespace or control characters",
            });
        }
        Ok(Self(hrn.to_owned()))
    }

    /// The validated canonical HRN text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The composite logical key: `tenant|canonical-HRN`. Unequivocal by
/// construction of both halves.
pub fn identity_key(tenant: &Tenant, hrn: &CanonicalHrn) -> String {
    format!("{}{}{}", tenant.as_str(), KEY_SEPARATOR, hrn.as_str())
}

/// The deterministic public digest of an identity: SHA-256 of the
/// composite key, hex encoded. Same pair, same digest, forever.
pub fn identity_digest(tenant: &Tenant, hrn: &CanonicalHrn) -> String {
    let digest = Sha256::digest(identity_key(tenant, hrn).as_bytes());
    hex::encode(digest)
}

/// A persistent identity. The Dgraph UID is deliberately **not** a field:
/// the public identity is the tenant, the HRN, the composite key and the
/// deterministic digest — the physical node pointer never crosses this
/// boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityRecord {
    /// The tenant of the identity.
    pub tenant: String,
    /// The canonical HRN of the identity.
    pub hrn: String,
    /// The composite logical key (`tenant|hrn`).
    pub key: String,
    /// The deterministic digest (SHA-256 of the key, hex).
    pub digest: String,
    /// The display name (freely sharable across tenants).
    pub name: String,
    /// The CAS revision of the aggregate (0 on creation).
    pub revision: u64,
}

impl IdentityRecord {
    /// Builds the record for a fresh identity (revision 0) from the
    /// derived values, without touching the server.
    fn fresh(tenant: &Tenant, hrn: &CanonicalHrn, name: &str) -> Self {
        Self {
            key: identity_key(tenant, hrn),
            digest: identity_digest(tenant, hrn),
            tenant: tenant.as_str().to_owned(),
            hrn: hrn.as_str().to_owned(),
            name: name.to_owned(),
            revision: 0,
        }
    }
}

/// The persistent identity store. Open through [`IdentityStore::open`],
/// which runs the schema boot first: an incompatible server is refused
/// before any write path exists. Its `Debug` names the boot decision and
/// nothing else: no endpoint details, no tokens, no keys.
#[derive(Debug)]
pub struct IdentityStore {
    client: Arc<GraphClient>,
    boot: SchemaBootReport,
}

impl IdentityStore {
    /// Opens the store on a connected client, gating on the versioned
    /// schema. This is the readiness boundary: no store exists on an
    /// incompatible server, hence no write either.
    pub async fn open(client: Arc<GraphClient>) -> Result<Self, IdentityError> {
        let boot = ensure_ready(&client).await?;
        info!(
            decision = ?boot.decision,
            binary_version = boot.binary_version,
            verified_predicates = boot.verified_predicates,
            "the identity store opened on a validated schema"
        );
        Ok(Self { client, boot })
    }

    /// The schema boot report this store validated at open.
    pub fn boot_report(&self) -> &SchemaBootReport {
        &self.boot
    }

    /// Creates the identity for `(tenant, hrn)` or returns the existing
    /// one. Concurrent creators converge on exactly one node: the server
    /// enforces the unique index, losers revalidate and read the winner.
    pub async fn ensure_identity(
        &self,
        tenant: &Tenant,
        hrn: &CanonicalHrn,
        name: &str,
    ) -> Result<IdentityRecord, IdentityError> {
        let key = identity_key(tenant, hrn);
        let digest = identity_digest(tenant, hrn);
        let mut last_error: Option<GraphClientError> = None;
        for attempt in 1..=MAX_ATTEMPTS {
            // Every attempt re-runs the query: the transaction itself is
            // the revalidation, so a retry never trusts stale state.
            let transaction = self.creation_upsert(tenant, hrn, &key, &digest, name);
            let response = match self.client.upsert(transaction).await {
                Ok(response) => response,
                Err(GraphClientError::UpsertConflict { detail }) => {
                    debug!(attempt, %key, %detail, "creation lost the write race; revalidating");
                    last_error = Some(GraphClientError::UpsertConflict { detail });
                    sleep(RETRY_BASE * (1u32 << (attempt - 1))).await;
                    continue;
                }
                Err(other) => return Err(other.into()),
            };
            // The upsert answer carries the query results: if the node
            // already existed (we lost the race but revalidated, or a
            // previous call created it) the record is in the answer.
            let existing = parse_identity(&response.json, "q", &key)?;
            if let Some(record) = existing {
                return Ok(record);
            }
            // Nothing was found and the creation block applied (it was
            // this attempt's transaction): revision 0 by construction.
            return Ok(IdentityRecord::fresh(tenant, hrn, name));
        }
        Err(IdentityError::RetryExhausted {
            key,
            attempts: MAX_ATTEMPTS,
            last: last_error
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "no detail".to_owned()),
        })
    }

    /// Looks up the identity for `(tenant, hrn)`. `None` when it does not
    /// exist. The UID is never part of the answer.
    pub async fn find_identity(
        &self,
        tenant: &Tenant,
        hrn: &CanonicalHrn,
    ) -> Result<Option<IdentityRecord>, IdentityError> {
        let key = identity_key(tenant, hrn);
        let query = format!(
            "{{ q(func: eq(hodei.identity.key, {})) {{ uid hodei.identity.tenant \
             hodei.identity.hrn hodei.identity.key hodei.identity.digest hodei.identity.name \
             hodei.identity.revision }} }}",
            crate::schema::dql_string(&key),
        );
        let response = self.client.execute_dql(&query).await?;
        parse_identity(&response.json, "q", &key)
    }

    /// Renames the identity under the CAS guard of the aggregate.
    ///
    /// The guard is the revision predicate (declared `@upsert`, so
    /// concurrent writers of the aggregate conflict server-side instead
    /// of silently overwriting each other). The loop is the doc-20
    /// prescription: every attempt **revalidates** (a fresh read decides
    /// whether the aggregate is still at `expected_revision`), the write
    /// is the guarded update, conflicts retry with backoff, and the
    /// outcome is decided by the state after the commit — success is
    /// never inferred from the transaction merely having committed.
    pub async fn rename_identity(
        &self,
        tenant: &Tenant,
        hrn: &CanonicalHrn,
        expected_revision: u64,
        new_name: &str,
    ) -> Result<IdentityRecord, IdentityError> {
        let key = identity_key(tenant, hrn);
        let mut last_conflict: Option<String> = None;
        for attempt in 1..=MAX_ATTEMPTS {
            // Revalidation: a fresh read of the aggregate decides.
            let Some(current) = self.find_identity(tenant, hrn).await? else {
                return Err(IdentityError::NotFound { key });
            };
            if current.revision != expected_revision {
                return Err(IdentityError::CasRejected {
                    key,
                    expected_revision,
                    current_revision: current.revision,
                });
            }
            let transaction = self.rename_upsert(&key, expected_revision, new_name);
            match self.client.upsert(transaction).await {
                Ok(_) => {}
                Err(GraphClientError::UpsertConflict { detail }) => {
                    debug!(attempt, %key, %detail, "the CAS update conflicted; revalidating");
                    last_conflict = Some(detail);
                    sleep(RETRY_BASE * (1u32 << (attempt - 1))).await;
                    continue;
                }
                Err(other) => return Err(other.into()),
            }
            // The state after the commit decides the answer.
            let Some(record) = self.find_identity(tenant, hrn).await? else {
                return Err(IdentityError::NotFound { key });
            };
            if record.revision == expected_revision + 1 {
                return Ok(record);
            }
            return Err(IdentityError::CasRejected {
                key,
                expected_revision,
                current_revision: record.revision,
            });
        }
        Err(IdentityError::RetryExhausted {
            key,
            attempts: MAX_ATTEMPTS,
            last: last_conflict.unwrap_or_else(|| "no detail".to_owned()),
        })
    }

    /// The creation upsert: find the node by key; create it only if it
    /// does not exist. The `@upsert` index makes concurrent creators
    /// conflict server-side; this transaction is rebuilt per attempt,
    /// which is the revalidation.
    fn creation_upsert(
        &self,
        tenant: &Tenant,
        hrn: &CanonicalHrn,
        key: &str,
        digest: &str,
        name: &str,
    ) -> GraphUpsertTransaction {
        let key_literal = crate::schema::dql_string(key);
        GraphUpsertTransaction {
            query: format!(
                "{{ q(func: eq(hodei.identity.key, {key_literal})) {{ u as uid \
                 hodei.identity.hrn hodei.identity.tenant hodei.identity.digest \
                 hodei.identity.name hodei.identity.revision }} }}"
            ),
            blocks: vec![crate::port::GraphMutationBlock::set_if(
                "eq(len(u), 0)",
                format!(
                    "_:i <dgraph.type> \"{IDENTITY_TYPE}\" .\n\
                     _:i <hodei.identity.key> {} .\n\
                     _:i <hodei.identity.hrn> {} .\n\
                     _:i <hodei.identity.tenant> {} .\n\
                     _:i <hodei.identity.digest> {} .\n\
                     _:i <hodei.identity.name> {} .\n\
                     _:i <hodei.identity.revision> \"0\" .\n\
                     _:i <hodei.identity.created_at> \"{}\" .\n",
                    crate::schema::dql_string(key),
                    crate::schema::dql_string(hrn.as_str()),
                    crate::schema::dql_string(tenant.as_str()),
                    crate::schema::dql_string(digest),
                    crate::schema::dql_string(name),
                    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                ),
            )],
            commit_now: true,
        }
    }

    /// The guarded rename write: it applies only when the aggregate
    /// exists, and writes the name plus the revision the caller already
    /// revalidated. Two writers that revalidated the same revision
    /// conflict on the `@upsert` index of `hodei.identity.revision`:
    /// the server itself decides who advances the aggregate.
    fn rename_upsert(
        &self,
        key: &str,
        expected_revision: u64,
        new_name: &str,
    ) -> GraphUpsertTransaction {
        let key_literal = crate::schema::dql_string(key);
        GraphUpsertTransaction {
            query: format!("{{ q(func: eq(hodei.identity.key, {key_literal})) {{ u as uid }} }}"),
            blocks: vec![crate::port::GraphMutationBlock::set_if(
                "eq(len(u), 1)",
                format!(
                    "uid(u) <hodei.identity.name> {} .\n\
                     uid(u) <hodei.identity.revision> \"{}\" .\n",
                    crate::schema::dql_string(new_name),
                    expected_revision + 1,
                ),
            )],
            commit_now: true,
        }
    }
}

/// Parses one identity out of an upsert/query answer. Refuses duplicated
/// keys and key/HRN mismatches: the uniqueness contract is checked on
/// the way out, not only on the way in. The UID is read only to detect
/// duplicates — it is never returned.
fn parse_identity(
    json: &serde_json::Value,
    root: &str,
    key: &str,
) -> Result<Option<IdentityRecord>, IdentityError> {
    let nodes = json
        .get(root)
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    match nodes.len() {
        0 => Ok(None),
        1 => {
            let node = &nodes[0];
            let tenant = node
                .get("hodei.identity.tenant")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| IdentityError::CorruptState {
                    key: key.to_owned(),
                    detail: "the node has no tenant".to_owned(),
                })?
                .to_owned();
            let hrn = node
                .get("hodei.identity.hrn")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| IdentityError::CorruptState {
                    key: key.to_owned(),
                    detail: "the node has no HRN".to_owned(),
                })?
                .to_owned();
            let stored_key = node
                .get("hodei.identity.key")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if !stored_key.is_empty() && stored_key != key {
                return Err(IdentityError::CorruptState {
                    key: key.to_owned(),
                    detail: format!("the node stores the key '{stored_key}'"),
                });
            }
            let digest = node
                .get("hodei.identity.digest")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| IdentityError::CorruptState {
                    key: key.to_owned(),
                    detail: "the node has no digest".to_owned(),
                })?
                .to_owned();
            let name = node
                .get("hodei.identity.name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let revision = node
                .get("hodei.identity.revision")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default();
            Ok(Some(IdentityRecord {
                tenant,
                hrn,
                key: if stored_key.is_empty() {
                    key.to_owned()
                } else {
                    stored_key
                },
                digest,
                name,
                revision,
            }))
        }
        n => Err(IdentityError::CorruptState {
            key: key.to_owned(),
            detail: format!("{n} nodes carry the same unique key"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant(text: &str) -> Tenant {
        Tenant::new(text).expect("valid tenant")
    }

    fn hrn(text: &str) -> CanonicalHrn {
        CanonicalHrn::new(text).expect("valid HRN")
    }

    /// The composite key is built as `tenant|hrn` and both halves keep
    /// their text unchanged.
    #[test]
    fn the_composite_key_is_deterministic() {
        let key = identity_key(
            &tenant("tenant-a"),
            &hrn("hrn:hodei:identity:local:tenant-a:account/alice"),
        );
        assert_eq!(
            key,
            "tenant-a|hrn:hodei:identity:local:tenant-a:account/alice"
        );
        assert_eq!(
            identity_key(
                &tenant("tenant-a"),
                &hrn("hrn:hodei:identity:local:tenant-a:account/alice")
            ),
            key
        );
    }

    /// The digest is a stable function of the key: same pair, same
    /// digest; a different tenant or HRN, a different digest.
    #[test]
    fn the_digest_is_deterministic_and_discriminating() {
        let a = (
            &tenant("tenant-a"),
            &hrn("hrn:hodei:identity:local:tenant-a:account/alice"),
        );
        let b = (
            &tenant("tenant-b"),
            &hrn("hrn:hodei:identity:local:tenant-b:account/alice"),
        );
        let digest_a = identity_digest(a.0, a.1);
        assert_eq!(
            digest_a,
            identity_digest(a.0, a.1),
            "same pair, same digest"
        );
        assert_ne!(
            digest_a,
            identity_digest(b.0, b.1),
            "different tenants, different digests"
        );
        assert_eq!(digest_a.len(), 64, "the digest is SHA-256 hex");
    }

    /// Two tenants naming their resources identically do not collide:
    /// the keys differ in the tenant half, which is the whole point of
    /// the composite key.
    #[test]
    fn the_same_name_in_two_tenants_produces_two_keys() {
        let key_a = identity_key(
            &tenant("tenant-a"),
            &hrn("hrn:hodei:identity:local:tenant-a:account/alice"),
        );
        let key_b = identity_key(
            &tenant("tenant-b"),
            &hrn("hrn:hodei:identity:local:tenant-b:account/alice"),
        );
        assert_ne!(key_a, key_b);
    }

    /// The tenant validation rejects the separator and the empty tenant,
    /// which is what keeps the composite key unambiguous.
    #[test]
    fn tenants_with_the_separator_are_rejected() {
        assert!(Tenant::new("tenant-a").is_ok());
        assert!(Tenant::new("tenant_A-1").is_ok());
        assert!(Tenant::new("").is_err());
        assert!(Tenant::new("tenant|a").is_err());
        assert!(Tenant::new("tenant a").is_err());
        assert!(Tenant::new("-tenant").is_err());
    }

    /// The HRN validation rejects anything that is not a safe canonical
    /// HRN string.
    #[test]
    fn hrns_that_are_not_canonical_are_rejected() {
        assert!(CanonicalHrn::new("hrn:hodei:identity:local:tenant-a:account/alice").is_ok());
        assert!(CanonicalHrn::new("odei:hodei:identity:local:tenant-a:account/alice").is_err());
        assert!(CanonicalHrn::new("hrn:hodei identity:local:tenant-a:account/alice").is_err());
        assert!(CanonicalHrn::new("hrn:hodei:identity:local:tenant|a:account/alice").is_err());
    }

    /// The public record never carries the physical UID: the struct has
    /// no field for it, and its Debug output contains no `0x` node id.
    #[test]
    fn the_public_record_has_no_physical_uid() {
        let record = IdentityRecord::fresh(
            &tenant("tenant-a"),
            &hrn("hrn:hodei:identity:local:tenant-a:account/alice"),
            "alice",
        );
        let rendered = format!("{record:?}");
        assert!(
            !rendered.contains("uid"),
            "the record leaked a uid field: {rendered}"
        );
        assert!(
            !rendered.contains("0x"),
            "the record leaked a node id: {rendered}"
        );
        assert_eq!(record.revision, 0, "a fresh identity starts at revision 0");
    }

    /// Parsing refuses a node whose stored key does not match the
    /// requested key: the uniqueness contract is checked on the way out.
    #[test]
    fn parsing_refuses_key_mismatches() {
        let json: serde_json::Value = serde_json::json!({
            "q": [
                {
                    "uid": "0x1234",
                    "hodei.identity.tenant": "tenant-a",
                    "hodei.identity.hrn": "hrn:hodei:identity:local:tenant-a:account/alice",
                    "hodei.identity.key": "tenant-a|hrn:hodei:identity:local:tenant-a:account/other",
                    "hodei.identity.digest": "aa",
                    "hodei.identity.name": "alice",
                    "hodei.identity.revision": 0,
                }
            ]
        });
        let error = parse_identity(
            &json,
            "q",
            "tenant-a|hrn:hodei:identity:local:tenant-a:account/alice",
        )
        .expect_err("a mismatched key must be refused");
        assert!(
            matches!(error, IdentityError::CorruptState { .. }),
            "unexpected: {error}"
        );
    }

    /// Parsing refuses duplicated keys, never picks one silently.
    #[test]
    fn parsing_refuses_duplicated_keys() {
        let json: serde_json::Value = serde_json::json!({
            "q": [
                { "hodei.identity.tenant": "tenant-a", "hodei.identity.hrn": "hrn:x",
                  "hodei.identity.digest": "a", "hodei.identity.name": "a" },
                { "hodei.identity.tenant": "tenant-a", "hodei.identity.hrn": "hrn:x",
                  "hodei.identity.digest": "b", "hodei.identity.name": "b" },
            ]
        });
        let error =
            parse_identity(&json, "q", "tenant-a|hrn:x").expect_err("duplicates must be refused");
        assert!(
            matches!(error, IdentityError::CorruptState { .. }),
            "unexpected: {error}"
        );
    }

    /// Parsing produces the record from a well-formed node, without the
    /// UID.
    #[test]
    fn parsing_produces_the_record() {
        let json: serde_json::Value = serde_json::json!({
            "q": [
                {
                    "uid": "0x4321",
                    "hodei.identity.tenant": "tenant-a",
                    "hodei.identity.hrn": "hrn:hodei:identity:local:tenant-a:account/alice",
                    "hodei.identity.key": "tenant-a|hrn:hodei:identity:local:tenant-a:account/alice",
                    "hodei.identity.digest": "aa",
                    "hodei.identity.name": "alice",
                    "hodei.identity.revision": 2,
                }
            ]
        });
        let record = parse_identity(
            &json,
            "q",
            "tenant-a|hrn:hodei:identity:local:tenant-a:account/alice",
        )
        .expect("a well-formed node parses")
        .expect("one node is a record");
        assert_eq!(record.tenant, "tenant-a");
        assert_eq!(record.name, "alice");
        assert_eq!(record.revision, 2);
        assert_eq!(
            record.key,
            "tenant-a|hrn:hodei:identity:local:tenant-a:account/alice"
        );
    }
}
