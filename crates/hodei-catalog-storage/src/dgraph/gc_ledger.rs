//! The ledger of coordinated collection over the native graph (HSP-192).
//!
//! The GC claim **lives in the authority**, not in the worker's
//! memory. Two replicas on the same scope exclude each other through the
//! token of the current generation; a zombie holding an expired generation
//! cannot mark, confirm, close, or renew.
//!
//! The rules this module carries:
//!
//! 1. **The claim is a durable CAS.** The scope's lease row carries
//!    `generation`, `token`, `owner` and `lease_until`. The guard goes in
//!    the upsert's `@filter` on the (token, lease) that was just
//!    read: the concurrent writer that read the same thing sees its block empty. An
//!    expired lease is taken over with a NEW generation and token. The
//!    claim verdict comes from the **post-commit re-read**
//!    (HSP-205), not from the fact that the commit went through.
//! 2. **The mark is transactional and guarded** (frontier pattern,
//!    HSP-199): the guard of the current claim goes in the `@filter` of the
//!    upsert query and the mark is written conditioned on that guard,
//!    in the SAME transaction. The deletion intent ALSO carries the
//!    reference guard in that same transaction: three variables
//!    per data-plane family count the rows that name the
//!    digest and, if any shows up between revalidation and commit,
//!    the intent is not written. What gets swept is what the
//!    authority returned when read after the commit.
//! 3. **The fence is the current token, not the stamp.** Every pass
//!    operation (mark, reconfirm, confirm, quarantine, renew,
//!    close) checks the claim before writing: if it is not its own,
//!    `PreconditionFailed` and the pass stops deleting (AC1). A
//!    take-over of a dead lease CAN resume the durable mark
//!    of the previous generation — that is durability — but only
//!    by presenting the live token.
//! 4. **The census is paged** (AC3): `root_page` walks the three
//!    data-plane families of the owner with `first(limit+1)`
//!    ordered by digest and a strict cursor; ties at the cut are
//!    completed so that successive pages are disjoint and
//!    convergent. The full graph is never materialized, and there is no
//!    global census: the legacy membership census is declared
//!    unavailable and forces per-scope paging.
//! 5. **Live references are counted fresh** (post-census
//!    revalidation): `live_reference_count` asks each family without a
//!    global scan; a candidate with a live name is neither marked nor swept.

use async_trait::async_trait;
use chrono::{DateTime, SecondsFormat, Utc};
use hodei_catalog::{
    CatalogError, ContentHold, ContentLedger, ContentRoot, DeletionState, GcDeletion, GcLease,
    GcRun, HoldScope, Quarantined, RootKind, RootPage, RunCandidate, RunState,
};
use hodei_evidence::{ContentDigest, DigestAlgorithm};
use hodei_graph_client::{
    GraphClientError, GraphMutationBlock, GraphMutationPort, GraphUpsertTransaction, dql_string,
};
use kernel::Hrn;

use super::{
    DgraphCatalogStore, key_part, node_digest, node_option_text, node_text, node_u64, single_row,
};

/// The graph type of the lease of a coordinated pass.
pub const GC_LEASE_TYPE: &str = "GcLease";
/// The graph type of the durable mark of a pass.
pub const GC_RUN_TYPE: &str = "GcRun";
/// The graph type of the trace of a deletion.
pub const GC_TOMBSTONE_TYPE: &str = "GcTombstone";
/// The graph type of a versioned quarantine.
pub const GC_QUARANTINE_TYPE: &str = "GcQuarantine";
/// The graph type of a retention.
pub const GC_HOLD_TYPE: &str = "GcHold";

const LEASE_FIELDS: &str = "hodei.gc.lease.scope hodei.gc.lease.generation \
     hodei.gc.lease.token hodei.gc.lease.owner hodei.gc.lease.lease_until";

/// The additive DDL of the collection ledger over Dgraph. Each key
/// is an exact index and `@upsert`: the server —not a pre-read—
/// arbitrates two writers of the same key inside and outside the
/// transaction.
pub fn gc_schema_dql() -> String {
    let mut ddl = String::new();
    for (predicate, kind, index) in [
        // The scope's lease: the fencing bound.
        ("hodei.gc.lease.key", "string", "@index(exact) @upsert"),
        ("hodei.gc.lease.scope", "string", "@index(exact)"),
        ("hodei.gc.lease.generation", "int", ""),
        ("hodei.gc.lease.token", "string", ""),
        ("hodei.gc.lease.owner", "string", ""),
        ("hodei.gc.lease.lease_until", "datetime", ""),
        ("hodei.gc.lease.renewed_at", "datetime", ""),
        // The durable mark of the pass.
        ("hodei.gc.run.key", "string", "@index(exact) @upsert"),
        ("hodei.gc.run.run_id", "string", "@index(exact)"),
        ("hodei.gc.run.scope", "string", "@index(exact)"),
        ("hodei.gc.run.generation", "int", ""),
        ("hodei.gc.run.state", "string", "@index(exact)"),
        ("hodei.gc.run.actor", "string", ""),
        ("hodei.gc.run.grace_secs", "int", ""),
        ("hodei.gc.run.deleted", "int", ""),
        ("hodei.gc.run.bytes_freed", "int", ""),
        ("hodei.gc.run.marked_at", "datetime", ""),
        ("hodei.gc.run.swept_at", "datetime", ""),
        // The mark can have N candidates: the predicate is a LIST
        // of uid. Declaring it scalar would make each new edge overwrite
        // the previous one and the post-commit re-read would not see the trace.
        ("hodei.gc.run.candidate", "[uid]", ""),
        // The mark's candidates. The composite key
        // `digest_key|run_id` keeps two passes from overwriting a
        // candidate and makes the re-read order them stably.
        ("hodei.gc.candidate.key", "string", "@index(exact) @upsert"),
        ("hodei.gc.candidate.digest_of", "string", "@index(exact)"),
        ("hodei.gc.candidate.run_id", "string", "@index(exact)"),
        ("hodei.gc.candidate.length", "int", ""),
        // The deletion trace, by digests.
        ("hodei.gc.tombstone.key", "string", "@index(exact) @upsert"),
        ("hodei.gc.tombstone.digest_of", "string", "@index(exact)"),
        ("hodei.gc.tombstone.algorithm", "string", ""),
        ("hodei.gc.tombstone.digest", "string", ""),
        ("hodei.gc.tombstone.run_id", "string", "@index(exact)"),
        ("hodei.gc.tombstone.length", "int", ""),
        ("hodei.gc.tombstone.actor", "string", ""),
        ("hodei.gc.tombstone.account", "string", ""),
        ("hodei.gc.tombstone.state", "string", ""),
        ("hodei.gc.tombstone.marker", "string", ""),
        ("hodei.gc.tombstone.at", "datetime", ""),
        // The versioned quarantine, per pass (AC2).
        ("hodei.gc.quarantine.key", "string", "@index(exact) @upsert"),
        ("hodei.gc.quarantine.run_id", "string", "@index(exact)"),
        ("hodei.gc.quarantine.digest_of", "string", "@index(exact)"),
        ("hodei.gc.quarantine.length", "int", ""),
        ("hodei.gc.quarantine.quarantine_key", "string", ""),
        ("hodei.gc.quarantine.marker", "string", ""),
        ("hodei.gc.quarantine.at", "datetime", ""),
        // The census retentions.
        ("hodei.gc.hold.key", "string", "@index(exact) @upsert"),
        ("hodei.gc.hold.hold_id", "string", "@index(exact)"),
        ("hodei.gc.hold.scope_kind", "string", "@index(exact)"),
        ("hodei.gc.hold.account", "string", "@index(exact)"),
        ("hodei.gc.hold.repository", "string", ""),
        ("hodei.gc.hold.digest_of", "string", "@index(exact)"),
        ("hodei.gc.hold.reason", "string", ""),
        ("hodei.gc.hold.actor", "string", ""),
        ("hodei.gc.hold.created_at", "datetime", ""),
        ("hodei.gc.hold.expires_at", "datetime", ""),
    ] {
        ddl.push_str(predicate);
        ddl.push_str(": ");
        ddl.push_str(kind);
        if !index.is_empty() {
            ddl.push(' ');
            ddl.push_str(index);
        }
        ddl.push_str(" .\n");
    }
    for (name, family, fields) in [
        (
            GC_LEASE_TYPE,
            "lease",
            "key scope generation token owner lease_until renewed_at",
        ),
        (
            GC_RUN_TYPE,
            "run",
            "key run_id scope generation state actor grace_secs deleted bytes_freed marked_at \
             swept_at candidate",
        ),
        (
            GC_TOMBSTONE_TYPE,
            "tombstone",
            "key digest_of algorithm digest run_id length actor account state marker at",
        ),
        (
            GC_QUARANTINE_TYPE,
            "quarantine",
            "key run_id digest_of length quarantine_key marker at",
        ),
        (
            GC_HOLD_TYPE,
            "hold",
            "key hold_id scope_kind account repository digest_of reason actor created_at \
             expires_at",
        ),
    ] {
        ddl.push_str(&format!("type {name} {{\n"));
        for field in fields.split_whitespace() {
            ddl.push_str(&format!("hodei.gc.{family}.{field}\n"));
        }
        ddl.push_str("}\n");
    }
    ddl
}

/// The key of a scope's lease: unique per scope.
pub fn gc_lease_key(scope: &str) -> String {
    format!("lease|{}", key_part(scope))
}

/// The run key.
fn gc_run_key(run_id: &str) -> String {
    format!("run|{}", key_part(run_id))
}

/// The digests part of a composite key: `algorithm|value`.
fn digest_key(digest: &ContentDigest) -> String {
    format!(
        "{}|{}",
        key_part(&digest.algorithm.to_string()),
        key_part(&digest.value)
    )
}

/// The candidate key of a pass.
fn gc_candidate_key(digest: &ContentDigest, run_id: &str) -> String {
    format!("{}|{}", digest_key(digest), key_part(run_id))
}

/// Unpacks a digests key: `algorithm|value`.
fn split_digest_key(digest_of: &str) -> Option<ContentDigest> {
    let (algorithm, value) = digest_of.split_once('|')?;
    let algorithm = match algorithm {
        "sha256" => DigestAlgorithm::Sha256,
        "sha512" => DigestAlgorithm::Sha512,
        _ => return None,
    };
    Some(ContentDigest {
        algorithm,
        value: value.to_owned(),
    })
}

fn stamp(instant: DateTime<Utc>) -> String {
    instant.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn parse_stamp(field: &str, text: &str) -> Result<DateTime<Utc>, CatalogError> {
    DateTime::parse_from_rfc3339(text)
        .map(|instant| instant.with_timezone(&Utc))
        .map_err(|_| CatalogError::corrupt("gc ledger", field, format!("'{text}' is not a time")))
}

/// The graph error expressed as a ledger error: losing the
/// `@upsert` race is a conflict; anything else, unavailability.
fn graph_error(error: GraphClientError) -> CatalogError {
    match error {
        GraphClientError::UpsertConflict { detail } => {
            CatalogError::conflict("gc ledger", "key", detail)
        }
        other => CatalogError::unavailable(other.to_string()),
    }
}

/// The current claim as the graph carries it.
fn lease_from(node: &serde_json::Value) -> Result<GcLease, CatalogError> {
    Ok(GcLease {
        generation: node_u64(node, "hodei.gc.lease.generation")?,
        token: node_text(node, "hodei.gc.lease.token")?,
        lease_until: parse_stamp(
            "hodei.gc.lease.lease_until",
            &node_text(node, "hodei.gc.lease.lease_until")?,
        )?,
    })
}

/// The stable blank node of a candidate.
fn blank_of(key: &str) -> String {
    format!(
        "_:c_{}",
        key.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect::<String>()
    )
}

/// The paging cursor is the digests fingerprint: `algorithm:value`.
fn cursor_of_digest(cursor: &str) -> Result<ContentDigest, CatalogError> {
    let (algorithm, value) = cursor
        .split_once(':')
        .ok_or_else(|| CatalogError::invalid("the census cursor is not a digest"))?;
    let algorithm = match algorithm {
        "sha256" => DigestAlgorithm::Sha256,
        "sha512" => DigestAlgorithm::Sha512,
        other => {
            return Err(CatalogError::invalid(format!(
                "'{other}' is not a digest algorithm"
            )));
        }
    };
    Ok(ContentDigest {
        algorithm,
        value: value.to_owned(),
    })
}

/// The census owner: if the scope is an HRN, its account; otherwise the
/// scope itself. The account is what partitions the data-plane
/// rows — the census is NEVER global.
fn scope_owner(scope: &str) -> String {
    Hrn::from_string(scope)
        .map(|hrn| hrn.account_id().to_owned())
        .unwrap_or_else(|| scope.to_owned())
}

/// The data-plane families that keep a digest alive.
#[derive(Clone, Copy)]
enum Family {
    Asset,
    Version,
    Reference,
    AptSnapshot,
}

impl Family {
    fn prefix(self) -> &'static str {
        match self {
            Self::Asset => "hodei.catalog.asset",
            Self::Version => "hodei.catalog.version",
            Self::Reference => "hodei.catalog.reference",
            Self::AptSnapshot => "hodei.catalog.apt_snapshot_root",
        }
    }

    fn detail(self) -> &'static str {
        match self {
            Self::Asset => "hodei.catalog.asset.asset_path",
            Self::Version => {
                "hodei.catalog.version.package_name hodei.catalog.version.package_version"
            }
            Self::Reference => "hodei.catalog.reference.asset_path",
            Self::AptSnapshot => "hodei.catalog.apt_snapshot_root.subject",
        }
    }

    fn kind(self) -> RootKind {
        match self {
            Self::Asset | Self::AptSnapshot => RootKind::Asset,
            Self::Version => RootKind::Version,
            Self::Reference => RootKind::Reference,
        }
    }
}

/// A census row read from the graph, sortable by (digest, key).
struct CensusRow {
    row_key: String,
    root: ContentRoot,
    digest: ContentDigest,
}

impl CensusRow {
    fn from_node(family: Family, node: &serde_json::Value) -> Result<Self, CatalogError> {
        let prefix = family.prefix();
        let digest = node_digest(
            node,
            &format!("{prefix}.digest_algorithm"),
            &format!("{prefix}.digest"),
        )?;
        let account = node_text(node, &format!("{prefix}.account"))?;
        let repository = node_text(node, &format!("{prefix}.repository"))?;
        let subject = match family {
            Family::Asset => node_text(node, "hodei.catalog.asset.asset_path")?,
            Family::Version => format!(
                "{}@{}",
                node_text(node, "hodei.catalog.version.package_name")?,
                node_text(node, "hodei.catalog.version.package_version")?
            ),
            Family::Reference => node_text(node, "hodei.catalog.reference.asset_path")?,
            Family::AptSnapshot => node_text(node, "hodei.catalog.apt_snapshot_root.subject")?,
        };
        let row_key = node_text(node, &format!("{prefix}.key"))?;
        Ok(Self {
            digest: digest.clone(),
            row_key,
            root: ContentRoot {
                kind: family.kind(),
                account,
                repository,
                subject,
                digest,
            },
        })
    }
}

impl DgraphCatalogStore {
    // -----------------------------------------------------------------------
    // Fenced claim (AC1).
    // -----------------------------------------------------------------------

    /// Takes or renews the scope's durable lease.
    ///
    /// Reads the current claim and the upsert carries the CAS guard in the
    /// `@filter` on what I just saw: a live foreign claim is
    /// answered as busy without writing; the holder itself renews with the
    /// SAME token; an expired lease (or no row) is taken over with a
    /// new generation and a NEW token. The verdict comes from the
    /// re-read: if the current token is not the one this upsert left,
    /// someone else won the race and I am not the holder.
    pub async fn claim_gc_pass_here(
        &self,
        scope: &str,
        owner: &str,
        lease_for: std::time::Duration,
        now: DateTime<Utc>,
    ) -> Result<GcLease, CatalogError> {
        if scope.is_empty() || owner.is_empty() {
            return Err(CatalogError::invalid(
                "the claim needs a scope and an owner",
            ));
        }
        let lease_for = chrono::Duration::from_std(lease_for)
            .map_err(|error| CatalogError::invalid(format!("the lease is not valid: {error}")))?;
        let until = now + lease_for;
        let key = gc_lease_key(scope);
        let seen = self.lease_row(&key).await?;
        // The claim verdict comes from the post-commit re-read
        // (HSP-205); the write is guarded by the row this
        // claim read: the query binds the (key, token) seen and, on
        // take-over, only if the lease had already expired at `now`. If someone else won
        // the race between the read and the commit, the guard binds
        // nothing and the block stays inert.
        let (query, set, cond);
        let expected_token;
        let expected_generation;
        match &seen {
            None => {
                // No row: the first generation, with its own token.
                // Creation is arbitrated by the absence of a row and the
                // key's `@upsert`: two writers of the first
                // generation win or lose by the index, not by
                // read order.
                let token = format!("{}-{}", self.identity.new_id(), stamp(now));
                expected_token = token.clone();
                expected_generation = 1;
                query = format!(
                    "{{ nc as var(func: eq(hodei.gc.lease.key, {})) }}",
                    dql_string(&key),
                );
                cond = "eq(len(nc), 0)".to_owned();
                set = format!(
                    "_:ls <dgraph.type> \"{GC_LEASE_TYPE}\" .\n\
                     _:ls <hodei.gc.lease.key> {key_q} .\n\
                     _:ls <hodei.gc.lease.scope> {scope_q} .\n\
                     _:ls <hodei.gc.lease.generation> \"1\" .\n\
                     _:ls <hodei.gc.lease.token> {token_q} .\n\
                     _:ls <hodei.gc.lease.owner> {owner_q} .\n\
                     _:ls <hodei.gc.lease.lease_until> \"{until}\" .\n\
                     _:ls <hodei.gc.lease.renewed_at> \"{at}\" .\n",
                    key_q = dql_string(&key),
                    scope_q = dql_string(scope),
                    token_q = dql_string(&token),
                    owner_q = dql_string(owner),
                    until = stamp(until),
                    at = stamp(now),
                );
            }
            Some(row) => {
                let generation = node_u64(row, "hodei.gc.lease.generation")?;
                let seen_token = node_text(row, "hodei.gc.lease.token")?;
                let seen_owner = node_text(row, "hodei.gc.lease.owner")?;
                let seen_until = parse_stamp(
                    "hodei.gc.lease.lease_until",
                    &node_text(row, "hodei.gc.lease.lease_until")?,
                )?;
                if seen_until > now && seen_owner != owner {
                    return Err(CatalogError::conflict(
                        "gc lease",
                        scope,
                        format!(
                            "the scope is held by owner '{}' until {}",
                            seen_owner,
                            stamp(seen_until)
                        ),
                    ));
                }
                // The guard of the seen claim is bound in the upsert
                // query with the token AND the lease state this
                // claim read: a renewal that extends a live
                // lease keeps the token (the fence would not detect it),
                // and a take-over that changes the token clears it. With
                // both, the race is arbitrated by the authority
                // in the transaction snapshot; the post-commit
                // re-read says who won.
                if seen_until > now {
                    // The holder itself renews: same token, longer
                    // lease.
                    query = format!(
                        "{{ gl as var(func: eq(hodei.gc.lease.key, {})) \
                         @filter(eq(hodei.gc.lease.token, {}) AND \
                         gt(hodei.gc.lease.lease_until, \"{}\")) }}",
                        dql_string(&key),
                        dql_string(&seen_token),
                        stamp(now),
                    );
                    expected_token = seen_token.clone();
                    expected_generation = generation;
                    cond = "eq(len(gl), 1)".to_owned();
                    set = format!(
                        "uid(gl) <hodei.gc.lease.lease_until> \"{until}\" .\n\
                         uid(gl) <hodei.gc.lease.renewed_at> \"{at}\" .\n",
                        until = stamp(until),
                        at = stamp(now),
                    );
                } else {
                    // Take-over: new generation and NEW token.
                    query = format!(
                        "{{ gl as var(func: eq(hodei.gc.lease.key, {})) \
                         @filter(eq(hodei.gc.lease.token, {}) AND \
                         le(hodei.gc.lease.lease_until, \"{}\")) }}",
                        dql_string(&key),
                        dql_string(&seen_token),
                        stamp(now),
                    );
                    let token = format!("{}-{}", self.identity.new_id(), stamp(now));
                    expected_token = token.clone();
                    expected_generation = generation + 1;
                    cond = "eq(len(gl), 1)".to_owned();
                    set = format!(
                        "uid(gl) <hodei.gc.lease.generation> \"{next}\" .\n\
                         uid(gl) <hodei.gc.lease.token> {token_q} .\n\
                         uid(gl) <hodei.gc.lease.owner> {owner_q} .\n\
                         uid(gl) <hodei.gc.lease.lease_until> \"{until}\" .\n\
                         uid(gl) <hodei.gc.lease.renewed_at> \"{at}\" .\n",
                        next = generation + 1,
                        token_q = dql_string(&token),
                        owner_q = dql_string(owner),
                        until = stamp(until),
                        at = stamp(now),
                    );
                }
            }
        }
        let transaction = GraphUpsertTransaction {
            query,
            blocks: vec![GraphMutationBlock::set_if(cond, set)],
            commit_now: true,
        };
        self.client.upsert(transaction).await.map_err(graph_error)?;
        // Post-commit re-read: the claim verdict is stated by the
        // authority after the commit, not by the fact that the commit
        // went through (HSP-205). If the current claim is not the one that
        // was meant to be left, someone else won the race: busy.
        let back = self
            .current_gc_lease_here(scope, Utc::now())
            .await?
            .ok_or_else(|| CatalogError::unavailable("the claim vanished after commit"))?;
        if back.token != expected_token || back.generation != expected_generation {
            return Err(CatalogError::conflict(
                "gc lease",
                scope,
                "another claim won the race for this scope",
            ));
        }
        Ok(back)
    }

    /// The scope's current claim, read. `None` if there is none or it expired.
    pub async fn current_gc_lease_here(
        &self,
        scope: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<GcLease>, CatalogError> {
        let row = self.lease_row(&gc_lease_key(scope)).await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let lease = lease_from(&row)?;
        Ok((lease.lease_until > now).then_some(lease))
    }

    async fn lease_row(&self, key: &str) -> Result<Option<serde_json::Value>, CatalogError> {
        let query = format!(
            "{{ q(func: eq(hodei.gc.lease.key, {})) {{ {LEASE_FIELDS} }} }}",
            dql_string(key),
        );
        let json = self.execute_dql_row(&query).await?;
        single_row(json.get("q"), "gc lease", key.to_owned())
    }

    /// The authority's fence: the presented token is the scope's
    /// current claim. Otherwise, `PreconditionFailed` (AC1).
    async fn the_fence(
        &self,
        scope: &str,
        lease: &GcLease,
        now: DateTime<Utc>,
    ) -> Result<(), CatalogError> {
        let row = self.lease_row(&gc_lease_key(scope)).await?;
        match &row {
            Some(node) => {
                let live = lease_from(node)?;
                if live.lease_until <= now {
                    return Err(CatalogError::precondition_failed(
                        "gc lease",
                        scope,
                        "the lease expired; its generation no longer rules",
                    ));
                }
                if live.token != lease.token || live.generation != lease.generation {
                    return Err(CatalogError::precondition_failed(
                        "gc lease",
                        scope,
                        format!(
                            "token presented for generation {} but the scope is held by generation {}",
                            lease.generation, live.generation
                        ),
                    ));
                }
                Ok(())
            }
            None => Err(CatalogError::precondition_failed(
                "gc lease",
                scope,
                "the scope has no live lease",
            )),
        }
    }

    async fn execute_dql_row(&self, query: &str) -> Result<serde_json::Value, CatalogError> {
        self.client
            .execute_dql(query)
            .await
            .map(|response| response.json)
            .map_err(|error| CatalogError::unavailable(error.to_string()))
    }

    // -----------------------------------------------------------------------
    // Fenced mark (HSP-199) and resumption.
    // -----------------------------------------------------------------------

    /// Writes the pass's mark in ONE guarded transaction.
    ///
    /// The claim guard goes in the upsert query's `@filter`: if
    /// the presented token is not the current one, `gl` stays empty and no
    /// n-quad is applied. After the commit the mark is re-read: what gets
    /// swept is what the authority confirmed, and a mark that is not from
    /// this generation is `PreconditionFailed`.
    pub async fn mark_run_fenced_here(
        &self,
        scope: &str,
        lease: &GcLease,
        run: &GcRun,
    ) -> Result<Vec<RunCandidate>, CatalogError> {
        let now = Utc::now();
        let (set, _) = run_mark_nquads(scope, lease, run);
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{ gl as var(func: eq(hodei.gc.lease.key, {})) \
                 @filter(eq(hodei.gc.lease.token, {}) AND eq(hodei.gc.lease.generation, {}) \
                 AND gt(hodei.gc.lease.lease_until, \"{}\")) }}",
                dql_string(&gc_lease_key(scope)),
                dql_string(&lease.token),
                lease.generation,
                stamp(now),
            ),
            blocks: vec![GraphMutationBlock::set_if("eq(len(gl), 1)", set)],
            commit_now: true,
        };
        self.client.upsert(transaction).await.map_err(graph_error)?;
        let marked = self.the_run_row(&run.id).await?;
        match marked {
            Some(row)
                if row.scope.as_deref() == Some(scope)
                    && row.generation == Some(lease.generation) =>
            {
                Ok(row.candidates)
            }
            Some(_) => Err(CatalogError::precondition_failed(
                "gc mark",
                &run.id,
                "the mark committed under another scope or generation",
            )),
            None => Err(CatalogError::precondition_failed(
                "gc mark",
                &run.id,
                "the mark did not land: the presented token is not the live claim",
            )),
        }
    }

    /// The mark of `run_id` reconfirmed under the current claim.
    ///
    /// The fence is by current token, not by the mark's stamp: a
    /// take-over of a dead lease can resume the durable mark of
    /// the previous generation (that is durability), but only
    /// by presenting the live token. A zombie gets
    /// `PreconditionFailed` here and cannot sweep.
    pub async fn mark_of_run_here(
        &self,
        scope: &str,
        lease: &GcLease,
        run_id: &str,
    ) -> Result<Option<GcRun>, CatalogError> {
        self.the_fence(scope, lease, Utc::now()).await?;
        let run = self.the_run_row(run_id).await?;
        Ok(run.filter(|run| run.scope.as_deref() == Some(scope)))
    }

    /// The marked, not-completed pass of the scope, read from the graph.
    pub async fn pending_scoped_run_here(
        &self,
        scope: &str,
        _now: DateTime<Utc>,
    ) -> Result<Option<GcRun>, CatalogError> {
        let nodes = self
            .all_nodes(
                &format!("eq(hodei.gc.run.scope, {})", dql_string(scope)),
                "@filter(eq(hodei.gc.run.state, \"marked\"))",
                "hodei.gc.run.run_id",
                HOLDS_PAGE,
                MAX_GC_ROWS_READ,
            )
            .await?;
        let run_id = nodes
            .iter()
            .filter_map(|node| node.get("hodei.gc.run.run_id").and_then(|v| v.as_str()))
            .min()
            .map(str::to_owned);
        match run_id {
            Some(run_id) => self.the_run_row(&run_id).await,
            None => Ok(None),
        }
    }

    /// Renews the lease of the current generation for its own owner.
    ///
    /// The fence first; the upsert is guarded by the token. After the
    /// commit it is re-read: if the scope moved to another generation between the
    /// fence and the re-read, this is `PreconditionFailed` and the pass
    /// stops sweeping — a zombie does not continue with the new token.
    pub async fn renew_gc_lease_here(
        &self,
        scope: &str,
        lease: &GcLease,
        owner: &str,
        lease_for: std::time::Duration,
        now: DateTime<Utc>,
    ) -> Result<GcLease, CatalogError> {
        self.the_fence(scope, lease, now).await?;
        let lease_for = chrono::Duration::from_std(lease_for)
            .map_err(|error| CatalogError::invalid(format!("the lease is not valid: {error}")))?;
        let until = now + lease_for;
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{ gl as var(func: eq(hodei.gc.lease.key, {})) \
                 @filter(eq(hodei.gc.lease.token, {}) AND eq(hodei.gc.lease.owner, {})) }}",
                dql_string(&gc_lease_key(scope)),
                dql_string(&lease.token),
                dql_string(owner),
            ),
            blocks: vec![GraphMutationBlock::set_if(
                "eq(len(gl), 1)",
                format!(
                    "uid(gl) <hodei.gc.lease.lease_until> \"{until}\" .\n\
                     uid(gl) <hodei.gc.lease.renewed_at> \"{at}\" .\n",
                    until = stamp(until),
                    at = stamp(now),
                ),
            )],
            commit_now: true,
        };
        self.client.upsert(transaction).await.map_err(graph_error)?;
        let back = self
            .current_gc_lease_here(scope, Utc::now())
            .await?
            .ok_or_else(|| CatalogError::unavailable("the lease vanished after renew"))?;
        if back.token != lease.token || back.generation != lease.generation {
            return Err(CatalogError::precondition_failed(
                "gc lease",
                scope,
                "the scope moved to a new generation mid-sweep",
            ));
        }
        Ok(back)
    }

    /// Gives the claim back when the pass is over: the lease expires now.
    ///
    /// Guarded by the live token, so a zombie cannot release the claim of
    /// the new generation. Without it, the process that replaces the one
    /// that finished (a restart, another replica) would find the scope
    /// held until the lease ran out on its own.
    pub async fn release_gc_lease_here(
        &self,
        scope: &str,
        lease: &GcLease,
    ) -> Result<(), CatalogError> {
        let now = Utc::now();
        self.the_fence(scope, lease, now).await?;
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{ gl as var(func: eq(hodei.gc.lease.key, {})) \
                 @filter(eq(hodei.gc.lease.token, {})) }}",
                dql_string(&gc_lease_key(scope)),
                dql_string(&lease.token),
            ),
            blocks: vec![GraphMutationBlock::set_if(
                "eq(len(gl), 1)",
                format!(
                    "uid(gl) <hodei.gc.lease.lease_until> \"{until}\" .\n",
                    until = stamp(now),
                ),
            )],
            commit_now: true,
        };
        self.client
            .upsert(transaction)
            .await
            .map(|_| ())
            .map_err(graph_error)
    }

    /// The accounts whose rows keep a digest alive: the union of the distinct
    /// accounts of the four root families (asset, version, reference, apt
    /// snapshot) and of the repositories.
    ///
    /// The census pages each family by its OWN account column, so the
    /// accounts must come from those same columns: an account with roots
    /// and no repository row (a repository removed without cascade, a
    /// direct seed, a migration) must still be censused, or its digests
    /// would look unrooted and a sweep would delete live blobs.
    /// `@groupby` answers one row per account, so the response is bounded
    /// by the number of accounts, never by the number of roots.
    pub async fn census_accounts_here(&self) -> Result<Vec<String>, CatalogError> {
        let mut accounts: Vec<String> = Vec::new();
        for predicate in [
            "hodei.catalog.repository.account".to_owned(),
            format!("{}.account", Family::Asset.prefix()),
            format!("{}.account", Family::Version.prefix()),
            format!("{}.account", Family::Reference.prefix()),
            format!("{}.account", Family::AptSnapshot.prefix()),
        ] {
            let query =
                format!("{{ q(func: has({predicate})) @groupby({predicate}) {{ count(uid) }} }}");
            let json = self.execute_dql_row(&query).await?;
            accounts.extend(
                json.get("q")
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|group| group.get("@groupby").and_then(serde_json::Value::as_array))
                    .flatten()
                    .filter_map(|row| {
                        row.get(&predicate)
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned)
                    }),
            );
        }
        accounts.sort();
        accounts.dedup();
        Ok(accounts)
    }

    /// Closes the pass under the current claim.
    pub async fn complete_scoped_run_here(
        &self,
        scope: &str,
        lease: &GcLease,
        run_id: &str,
        deleted: u64,
        bytes_freed: u64,
        at: DateTime<Utc>,
    ) -> Result<(), CatalogError> {
        self.the_fence(scope, lease, at).await?;
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{ ru as var(func: eq(hodei.gc.run.run_id, {})) \
                 @filter(eq(hodei.gc.run.scope, {})) }}",
                dql_string(run_id),
                dql_string(scope),
            ),
            blocks: vec![GraphMutationBlock::set_if(
                "eq(len(ru), 1)",
                format!(
                    "uid(ru) <hodei.gc.run.state> \"swept\" .\n\
                     uid(ru) <hodei.gc.run.deleted> \"{deleted}\" .\n\
                     uid(ru) <hodei.gc.run.bytes_freed> \"{bytes}\" .\n\
                     uid(ru) <hodei.gc.run.swept_at> \"{at}\" .\n",
                    bytes = bytes_freed,
                    at = stamp(at),
                ),
            )],
            commit_now: true,
        };
        self.client.upsert(transaction).await.map_err(graph_error)?;
        let back = self.the_run_row(run_id).await?;
        match back {
            Some(row) if row.state == RunState::Swept => Ok(()),
            _ => Err(CatalogError::precondition_failed(
                "gc run",
                run_id,
                "the close did not land under the presented token",
            )),
        }
    }

    // -----------------------------------------------------------------------
    // Fenced trace and quarantine.
    // -----------------------------------------------------------------------

    /// The fenced trace of a deletion. The fence first; the write
    /// is guarded by the token and by the reference guard in the
    /// SAME upsert (frontier pattern, HSP-199): three variables bound
    /// by digest equality over the three data-plane
    /// families, counted in the transaction snapshot. A concurrent
    /// publisher that named the blob between the caller's
    /// revalidation and this commit leaves the intent unwritten — the
    /// verdict comes from the post-commit re-read (HSP-205). A
    /// zombie with an expired lease cannot even note that it deleted.
    pub async fn record_deletion_fenced_here(
        &self,
        scope: &str,
        lease: &GcLease,
        deletion: &GcDeletion,
    ) -> Result<(), CatalogError> {
        self.the_fence(scope, lease, Utc::now()).await?;
        let tkey = format!("tomb|{}", digest_key(&deletion.digest));
        let create = tombstone_nquads("_:td", &tkey, deletion, true);
        let update = tombstone_nquads("uid(td)", &tkey, deletion, false);
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{ gl as var(func: eq(hodei.gc.lease.key, {})) \
                 @filter(eq(hodei.gc.lease.token, {})) \n\
                 td as var(func: eq(hodei.gc.tombstone.key, {})) \n\
                 {} }}",
                dql_string(&gc_lease_key(scope)),
                dql_string(&lease.token),
                dql_string(&tkey),
                reference_guard_lines(&deletion.digest),
            ),
            blocks: vec![
                GraphMutationBlock::set_if(
                    format!(
                        "eq(len(gl), 1) AND {} AND eq(len(td), 0)",
                        no_live_reference()
                    ),
                    create,
                ),
                GraphMutationBlock::set_if(
                    format!(
                        "eq(len(gl), 1) AND {} AND eq(len(td), 1)",
                        no_live_reference()
                    ),
                    update,
                ),
            ],
            commit_now: true,
        };
        self.client.upsert(transaction).await.map_err(graph_error)?;
        self.verdict_tombstone(deletion).await
    }

    /// The trace of a digests, read.
    pub async fn deletion_of_here(
        &self,
        digest: &ContentDigest,
    ) -> Result<Option<GcDeletion>, CatalogError> {
        let tkey = format!("tomb|{}", digest_key(digest));
        let query = format!(
            "{{ q(func: eq(hodei.gc.tombstone.key, {})) {{ \
             hodei.gc.tombstone.algorithm hodei.gc.tombstone.digest \
             hodei.gc.tombstone.run_id hodei.gc.tombstone.length \
             hodei.gc.tombstone.actor hodei.gc.tombstone.account \
             hodei.gc.tombstone.state hodei.gc.tombstone.at \
             hodei.gc.tombstone.marker }} }}",
            dql_string(&tkey),
        );
        let json = self.execute_dql_row(&query).await?;
        let node = single_row(json.get("q"), "gc tombstone", tkey)?;
        let Some(node) = node else {
            return Ok(None);
        };
        let state = match node_text(&node, "hodei.gc.tombstone.state")?.as_str() {
            "intended" => DeletionState::Intended,
            "confirmed" => DeletionState::Confirmed,
            other => {
                return Err(CatalogError::corrupt(
                    "gc tombstone",
                    other.to_owned(),
                    "not a deletion state",
                ));
            }
        };
        Ok(Some(GcDeletion {
            digest: node_digest(
                &node,
                "hodei.gc.tombstone.algorithm",
                "hodei.gc.tombstone.digest",
            )?,
            length: node_u64(&node, "hodei.gc.tombstone.length")?,
            run_id: node_text(&node, "hodei.gc.tombstone.run_id")?,
            actor: node_text(&node, "hodei.gc.tombstone.actor")?,
            account: node_text(&node, "hodei.gc.tombstone.account")?,
            state,
            at: parse_stamp(
                "hodei.gc.tombstone.at",
                &node_text(&node, "hodei.gc.tombstone.at")?,
            )?,
            marker: node_option_text(&node, "hodei.gc.tombstone.marker"),
        }))
    }

    async fn verdict_tombstone(&self, deletion: &GcDeletion) -> Result<(), CatalogError> {
        let back = self.deletion_of_here(&deletion.digest).await?;
        match back {
            Some(row) if row.state == deletion.state && row.run_id == deletion.run_id => Ok(()),
            Some(_) => Err(CatalogError::precondition_failed(
                "gc tombstone",
                deletion.digest.to_string(),
                "the trace did not land under the presented token",
            )),
            None => Err(CatalogError::unavailable(
                "the trace is not readable after commit",
            )),
        }
    }

    /// The fenced quarantine (AC2): the marked bytes were copied to a
    /// NEW name and the original key stays reserved in the ledger
    /// until reconciled — the reservation is the quarantine trace.
    pub async fn record_quarantine_here(
        &self,
        scope: &str,
        lease: &GcLease,
        quarantine: &Quarantined,
    ) -> Result<(), CatalogError> {
        self.the_fence(scope, lease, Utc::now()).await?;
        let qkey = format!(
            "quar|{}|{}",
            key_part(&quarantine.run_id),
            digest_key(&quarantine.digest)
        );
        let create = quarantine_nquads("_:qz", &qkey, quarantine, true);
        let update = quarantine_nquads("uid(qz)", &qkey, quarantine, false);
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{ gl as var(func: eq(hodei.gc.lease.key, {})) \
                 @filter(eq(hodei.gc.lease.token, {})) \
                 qz as var(func: eq(hodei.gc.quarantine.key, {})) }}",
                dql_string(&gc_lease_key(scope)),
                dql_string(&lease.token),
                dql_string(&qkey),
            ),
            blocks: vec![
                GraphMutationBlock::set_if("eq(len(gl), 1) AND eq(len(qz), 0)", create),
                GraphMutationBlock::set_if("eq(len(gl), 1) AND eq(len(qz), 1)", update),
            ],
            commit_now: true,
        };
        self.client.upsert(transaction).await.map_err(graph_error)?;
        let back = self
            .quarantines_of_run_here(&quarantine.run_id)
            .await?
            .into_iter()
            .any(|item| item.digest == quarantine.digest);
        if back {
            Ok(())
        } else {
            Err(CatalogError::precondition_failed(
                "gc quarantine",
                quarantine.digest.to_string(),
                "the quarantine did not land under the presented token",
            ))
        }
    }

    /// The quarantines of a pass (key reservation, reconciliation).
    pub async fn quarantines_of_run_here(
        &self,
        run_id: &str,
    ) -> Result<Vec<Quarantined>, CatalogError> {
        let nodes = self
            .all_nodes(
                &format!("eq(hodei.gc.quarantine.run_id, {})", dql_string(run_id)),
                "",
                "hodei.gc.quarantine.digest_of hodei.gc.quarantine.length \
                 hodei.gc.quarantine.quarantine_key hodei.gc.quarantine.marker \
                 hodei.gc.quarantine.at",
                HOLDS_PAGE,
                MAX_GC_ROWS_READ,
            )
            .await?;
        let mut items = Vec::new();
        for node in &nodes {
            let digest_of = node_text(node, "hodei.gc.quarantine.digest_of")?;
            let digest = split_digest_key(&digest_of).ok_or_else(|| {
                CatalogError::corrupt("gc quarantine", digest_of, "the key is not a digest")
            })?;
            items.push(Quarantined {
                digest,
                length: node_u64(node, "hodei.gc.quarantine.length")?,
                run_id: run_id.to_owned(),
                quarantine_key: node_text(node, "hodei.gc.quarantine.quarantine_key")?,
                marker: node_text(node, "hodei.gc.quarantine.marker")?,
                at: parse_stamp(
                    "hodei.gc.quarantine.at",
                    &node_text(node, "hodei.gc.quarantine.at")?,
                )?,
            });
        }
        Ok(items)
    }

    // -----------------------------------------------------------------------
    // Paged census (AC3).
    // -----------------------------------------------------------------------

    /// A bounded page of the scope's roots, ordered by digests.
    ///
    /// Each data-plane family of the owner is read with
    /// `first(limit+1)`, `order: digest` and a strict `gt` cursor; the
    /// pages are disjoint and convergent. The cut may tie the
    /// last digests of the page with rows of another family: those
    /// rows are completed in the SAME page and the cursor leaves them
    /// behind, or `has_more` would keep promising an infinite page.
    /// The output cursor is the fingerprint of the page's last digests;
    /// `has_more` is only true if something remains after the
    /// limit.
    pub async fn root_page_here(
        &self,
        scope: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<RootPage, CatalogError> {
        let limit = limit.clamp(1, ROOT_PAGE_MAX);
        let account = scope_owner(scope);
        let cursor = after.map(cursor_of_digest).transpose()?;
        let mut fetched: Vec<CensusRow> = Vec::new();
        for family in [
            Family::Asset,
            Family::Version,
            Family::Reference,
            Family::AptSnapshot,
        ] {
            fetched.extend(
                self.census_window(&account, family, cursor.as_ref(), limit + 1)
                    .await?,
            );
        }
        fetched.sort_by(|a, b| {
            a.digest
                .value
                .cmp(&b.digest.value)
                .then_with(|| a.row_key.cmp(&b.row_key))
        });
        if fetched.len() <= limit {
            // The whole window fits: these are all the roots there are.
            return Ok(RootPage {
                roots: fetched.into_iter().map(|row| row.root).collect(),
                has_more: false,
            });
        }
        fetched.truncate(limit);
        let boundary = fetched[limit - 1].digest.value.clone();
        // Limit ties: rows of another family with the same
        // digests as the cut are completed in the SAME page — the
        // output cursor leaves them behind and if they were not completed,
        // `has_more` would promise a page that always returns the
        // same digests. They are read from the authority by exact equality,
        // not from the truncated window.
        for family in [
            Family::Asset,
            Family::Version,
            Family::Reference,
            Family::AptSnapshot,
        ] {
            for row in self.census_equal(&account, family, &boundary).await? {
                if !fetched.iter().any(|seen| seen.row_key == row.row_key) {
                    fetched.push(row);
                }
            }
        }
        fetched.sort_by(|a, b| {
            a.digest
                .value
                .cmp(&b.digest.value)
                .then_with(|| a.row_key.cmp(&b.row_key))
        });
        // There is more if anything, in any family, remains strictly
        // after the limit digests.
        let mut has_more = false;
        for family in [
            Family::Asset,
            Family::Version,
            Family::Reference,
            Family::AptSnapshot,
        ] {
            if self.census_any_gt(&account, family, &boundary).await? {
                has_more = true;
                break;
            }
        }
        Ok(RootPage {
            roots: fetched.into_iter().map(|row| row.root).collect(),
            has_more,
        })
    }

    /// The window of a family strictly after the cursor:
    /// `first: {window}` + `orderasc: digest` + `@filter(gt(digest,
    /// cursor))` — the repo's paging idiom, never an unbounded
    /// scan. `after:` does not work here: in DQL it cuts by UID, not by the
    /// order value. The `has` and the `gt` share ONE single
    /// `@filter` joined by AND: two filters at the root are "Repeated
    /// filter at root".
    async fn census_window(
        &self,
        account: &str,
        family: Family,
        cursor: Option<&ContentDigest>,
        window: usize,
    ) -> Result<Vec<CensusRow>, CatalogError> {
        let filter = match cursor {
            Some(digest) => format!(
                "has({}.digest) AND gt({}.digest, {})",
                family.prefix(),
                family.prefix(),
                dql_string(&digest.value)
            ),
            None => format!("has({}.digest)", family.prefix()),
        };
        let query = format!(
            "{{ q(func: eq({}.account, {}), orderasc: {}.digest, first: {}) \
             @filter({}) {{ \
             {}.key {}.account {}.repository {}.digest_algorithm {}.digest {} }} }}",
            family.prefix(),
            dql_string(account),
            family.prefix(),
            window,
            filter,
            family.prefix(),
            family.prefix(),
            family.prefix(),
            family.prefix(),
            family.prefix(),
            family.detail(),
        );
        let json = self.execute_dql_row(&query).await?;
        json.get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|node| CensusRow::from_node(family, node))
            .collect()
    }

    /// The rows of a family with the exact cut digests.
    async fn census_equal(
        &self,
        account: &str,
        family: Family,
        value: &str,
    ) -> Result<Vec<CensusRow>, CatalogError> {
        let p = family.prefix();
        let fields = format!(
            "{p}.key {p}.account {p}.repository {p}.digest_algorithm {p}.digest {}",
            family.detail()
        );
        self.all_nodes(
            &format!("eq({p}.account, {})", dql_string(account)),
            &format!("@filter(eq({p}.digest, {}))", dql_string(value)),
            &fields,
            HOLDS_PAGE,
            MAX_GC_ROWS_READ,
        )
        .await?
        .iter()
        .map(|node| CensusRow::from_node(family, node))
        .collect::<Result<Vec<_>, _>>()
    }

    /// Something remains strictly after this digests in the family.
    async fn census_any_gt(
        &self,
        account: &str,
        family: Family,
        value: &str,
    ) -> Result<bool, CatalogError> {
        let query = format!(
            "{{ q(func: eq({}.account, {}), first: 1) @filter(gt({}.digest, {})) {{ uid }} }}",
            family.prefix(),
            dql_string(account),
            family.prefix(),
            dql_string(value),
        );
        let json = self.execute_dql_row(&query).await?;
        Ok(json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|array| !array.is_empty()))
    }

    /// How many data-plane rows name this digests, read
    /// fresh by membership — never by global scan (post-census
    /// revalidation, reference guard). The `exact` index of
    /// `{family}.digest` resolves the query by key; nothing is
    /// materialized: only the response is counted.
    pub async fn live_reference_count_here(
        &self,
        digest: &ContentDigest,
    ) -> Result<u64, CatalogError> {
        let mut total = 0u64;
        for family in [
            Family::Asset,
            Family::Version,
            Family::Reference,
            Family::AptSnapshot,
        ] {
            total += self
                .all_nodes(
                    &format!(
                        "eq({}.digest, {})",
                        family.prefix(),
                        dql_string(&digest.value)
                    ),
                    "",
                    "",
                    HOLDS_PAGE,
                    MAX_GC_ROWS_READ,
                )
                .await?
                .len() as u64;
        }
        Ok(total)
    }

    // -----------------------------------------------------------------------
    // Shared internal reads.
    // -----------------------------------------------------------------------

    async fn the_run_row(&self, run_id: &str) -> Result<Option<GcRun>, CatalogError> {
        let query = format!(
            "{{ q(func: eq(hodei.gc.run.run_id, {})) {{ \
             hodei.gc.run.scope hodei.gc.run.generation hodei.gc.run.state \
             hodei.gc.run.actor hodei.gc.run.grace_secs hodei.gc.run.deleted \
             hodei.gc.run.bytes_freed hodei.gc.run.marked_at hodei.gc.run.swept_at \
             hodei.gc.run.candidate @filter(has(hodei.gc.candidate.key)) {{ \
             hodei.gc.candidate.digest_of hodei.gc.candidate.length }} }} }}",
            dql_string(run_id),
        );
        let json = self.execute_dql_row(&query).await?;
        let node = single_row(json.get("q"), "gc run", run_id.to_owned())?;
        let Some(node) = node else {
            return Ok(None);
        };
        let state = match node_text(&node, "hodei.gc.run.state")?.as_str() {
            "marked" => RunState::Marked,
            "swept" => RunState::Swept,
            other => {
                return Err(CatalogError::corrupt(
                    "gc run",
                    other.to_owned(),
                    "not a run state",
                ));
            }
        };
        let mut candidates = Vec::new();
        for item in node
            .get("hodei.gc.run.candidate")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default()
        {
            let digest_of = node_text(&item, "hodei.gc.candidate.digest_of")?;
            let digest = split_digest_key(&digest_of).ok_or_else(|| {
                CatalogError::corrupt("gc candidate", digest_of, "the digest key is corrupt")
            })?;
            candidates.push(RunCandidate {
                digest,
                length: node_u64(&item, "hodei.gc.candidate.length")?,
            });
        }
        Ok(Some(GcRun {
            id: run_id.to_owned(),
            state,
            grace_secs: node_u64(&node, "hodei.gc.run.grace_secs")?,
            actor: node_text(&node, "hodei.gc.run.actor")?,
            candidates,
            marked_at: parse_stamp(
                "hodei.gc.run.marked_at",
                &node_text(&node, "hodei.gc.run.marked_at")?,
            )?,
            deleted: node_u64(&node, "hodei.gc.run.deleted")?,
            bytes_freed: node_u64(&node, "hodei.gc.run.bytes_freed")?,
            swept_at: node_option_text(&node, "hodei.gc.run.swept_at")
                .map(|text| parse_stamp("hodei.gc.run.swept_at", &text))
                .transpose()?,
            scope: node_option_text(&node, "hodei.gc.run.scope"),
            generation: Some(node_u64(&node, "hodei.gc.run.generation")?),
        }))
    }
}

/// The query lines of a digest's reference guard: one
/// variable per data-plane family, bound by exact digest equality.
/// They count in the transaction snapshot; the block names them
/// in its condition so DQL does not discard them as dead.
fn reference_guard_lines(digest: &ContentDigest) -> String {
    [
        Family::Asset,
        Family::Version,
        Family::Reference,
        Family::AptSnapshot,
    ]
    .iter()
    .enumerate()
    .map(|(index, family)| {
        format!(
            "rv{} as var(func: eq({}.digest, {}))",
            index,
            family.prefix(),
            dql_string(&digest.value)
        )
    })
    .collect::<Vec<_>>()
    .join(" \n")
}

/// The guard condition: no row names the digest.
fn no_live_reference() -> String {
    "eq(len(rv0), 0) AND eq(len(rv1), 0) AND eq(len(rv2), 0) AND eq(len(rv3), 0)".to_owned()
}

/// The n-quads of a pass's mark: the run, its candidates and the
/// run→candidate edges. Returns the set and the run key.
fn run_mark_nquads(scope: &str, lease: &GcLease, run: &GcRun) -> (String, String) {
    let key = gc_run_key(&run.id);
    let mut set = format!(
        "_:ru <dgraph.type> \"{GC_RUN_TYPE}\" .\n\
         _:ru <hodei.gc.run.key> {key_q} .\n\
         _:ru <hodei.gc.run.run_id> {id_q} .\n\
         _:ru <hodei.gc.run.scope> {scope_q} .\n\
         _:ru <hodei.gc.run.generation> \"{generation}\" .\n\
         _:ru <hodei.gc.run.state> \"marked\" .\n\
         _:ru <hodei.gc.run.actor> {actor_q} .\n\
         _:ru <hodei.gc.run.grace_secs> \"{grace}\" .\n\
         _:ru <hodei.gc.run.deleted> \"0\" .\n\
         _:ru <hodei.gc.run.bytes_freed> \"0\" .\n\
         _:ru <hodei.gc.run.marked_at> \"{marked}\" .\n",
        key_q = dql_string(&key),
        id_q = dql_string(&run.id),
        scope_q = dql_string(scope),
        generation = lease.generation,
        actor_q = dql_string(&run.actor),
        grace = run.grace_secs,
        marked = stamp(run.marked_at),
    );
    for candidate in &run.candidates {
        let dkey = gc_candidate_key(&candidate.digest, &run.id);
        let blank = blank_of(&dkey);
        set.push_str(&format!(
            "{blank} <hodei.gc.candidate.key> {dkey_q} .\n\
             {blank} <hodei.gc.candidate.digest_of> {digest_q} .\n\
             {blank} <hodei.gc.candidate.run_id> {id_q} .\n\
             {blank} <hodei.gc.candidate.length> \"{length}\" .\n\
             _:ru <hodei.gc.run.candidate> {blank} .\n",
            dkey_q = dql_string(&dkey),
            digest_q = dql_string(&digest_key(&candidate.digest)),
            id_q = dql_string(&run.id),
            length = candidate.length,
        ));
    }
    (set, key)
}

/// The n-quads of a deletion trace.
fn tombstone_nquads(subject: &str, tkey: &str, deletion: &GcDeletion, create: bool) -> String {
    let type_line = if create {
        format!("{subject} <dgraph.type> \"{GC_TOMBSTONE_TYPE}\" .\n")
    } else {
        String::new()
    };
    let key_line = if create {
        format!(
            "{subject} <hodei.gc.tombstone.key> {} .\n",
            dql_string(tkey)
        )
    } else {
        String::new()
    };
    let marker = deletion
        .marker
        .as_ref()
        .map(|value| {
            format!(
                "{subject} <hodei.gc.tombstone.marker> {} .\n",
                dql_string(value)
            )
        })
        .unwrap_or_default();
    format!(
        "{type_line}{key_line}\
         {subject} <hodei.gc.tombstone.digest_of> {digest_q} .\n\
         {subject} <hodei.gc.tombstone.algorithm> {alg_q} .\n\
         {subject} <hodei.gc.tombstone.digest> {value_q} .\n\
         {subject} <hodei.gc.tombstone.run_id> {run_q} .\n\
         {subject} <hodei.gc.tombstone.length> \"{length}\" .\n\
         {subject} <hodei.gc.tombstone.actor> {actor_q} .\n\
         {subject} <hodei.gc.tombstone.account> {account_q} .\n\
         {subject} <hodei.gc.tombstone.state> {state_q} .\n\
         {subject} <hodei.gc.tombstone.at> \"{at}\" .\n\
         {marker}",
        digest_q = dql_string(&digest_key(&deletion.digest)),
        alg_q = dql_string(&deletion.digest.algorithm.to_string()),
        value_q = dql_string(&deletion.digest.value),
        run_q = dql_string(&deletion.run_id),
        length = deletion.length,
        actor_q = dql_string(&deletion.actor),
        account_q = dql_string(&deletion.account),
        state_q = dql_string(deletion.state.as_str()),
        at = stamp(deletion.at),
    )
}

/// The n-quads of a versioned quarantine.
fn quarantine_nquads(subject: &str, qkey: &str, quarantine: &Quarantined, create: bool) -> String {
    let type_line = if create {
        format!("{subject} <dgraph.type> \"{GC_QUARANTINE_TYPE}\" .\n")
    } else {
        String::new()
    };
    let key_line = if create {
        format!(
            "{subject} <hodei.gc.quarantine.key> {} .\n",
            dql_string(qkey)
        )
    } else {
        String::new()
    };
    format!(
        "{type_line}{key_line}\
         {subject} <hodei.gc.quarantine.run_id> {run_q} .\n\
         {subject} <hodei.gc.quarantine.digest_of> {digest_q} .\n\
         {subject} <hodei.gc.quarantine.length> \"{length}\" .\n\
         {subject} <hodei.gc.quarantine.quarantine_key> {qk_q} .\n\
         {subject} <hodei.gc.quarantine.marker> {marker_q} .\n\
         {subject} <hodei.gc.quarantine.at> \"{at}\" .\n",
        run_q = dql_string(&quarantine.run_id),
        digest_q = dql_string(&digest_key(&quarantine.digest)),
        length = quarantine.length,
        qk_q = dql_string(&quarantine.quarantine_key),
        marker_q = dql_string(&quarantine.marker),
        at = stamp(quarantine.at),
    )
}

// ---------------------------------------------------------------------------
// The ledger's contract over the graph.
// ---------------------------------------------------------------------------

/// The predicates a hold row is read with.
const HOLD_FIELDS: &str = "hodei.gc.hold.hold_id hodei.gc.hold.scope_kind hodei.gc.hold.account \
     hodei.gc.hold.repository hodei.gc.hold.digest_of hodei.gc.hold.reason \
     hodei.gc.hold.actor hodei.gc.hold.created_at hodei.gc.hold.expires_at";

/// Reads one hold row.
fn hold_of_node(node: &serde_json::Value) -> Result<ContentHold, CatalogError> {
    let id = node_text(node, "hodei.gc.hold.hold_id")?;
    let scope = match node_text(node, "hodei.gc.hold.scope_kind")?.as_str() {
        "digest" => {
            let digest_of = node_text(node, "hodei.gc.hold.digest_of")?;
            let digest = split_digest_key(&digest_of).ok_or_else(|| {
                CatalogError::corrupt("gc hold", digest_of, "the digest key is corrupt")
            })?;
            HoldScope::Digest { digest }
        }
        "repository" => HoldScope::Repository {
            account: node_text(node, "hodei.gc.hold.account")?,
            repository: node_text(node, "hodei.gc.hold.repository")?,
        },
        "account" => HoldScope::Account {
            account: node_text(node, "hodei.gc.hold.account")?,
        },
        other => {
            return Err(CatalogError::corrupt(
                "gc hold",
                id,
                format!("'{other}' is not a hold scope"),
            ));
        }
    };
    Ok(ContentHold {
        id,
        scope,
        reason: node_text(node, "hodei.gc.hold.reason")?,
        actor: node_text(node, "hodei.gc.hold.actor")?,
        created_at: parse_stamp(
            "hodei.gc.hold.created_at",
            &node_text(node, "hodei.gc.hold.created_at")?,
        )?,
        expires_at: node_option_text(node, "hodei.gc.hold.expires_at")
            .map(|text| parse_stamp("hodei.gc.hold.expires_at", &text))
            .transpose()?,
    })
}

/// Rows requested per page of the hold read.
const HOLDS_PAGE: usize = 500;

/// Defensive ceiling of the hold read. Past it the read fails closed: a
/// truncated list would hide holds from the GC plan and from the per-account
/// limit, which is silent data loss.
const MAX_HOLDS_READ: usize = 200_000;

/// Largest census window of one `root_page` read; a bigger requested limit is
/// served as several pages (the caller sees `has_more`), never as one query
/// whose rows the server could cap silently.
const ROOT_PAGE_MAX: usize = 500;

/// Defensive ceiling of the rows of one unbounded-by-nature GC read.
const MAX_GC_ROWS_READ: usize = 200_000;

impl DgraphCatalogStore {
    /// Reads ALL the nodes of a root function in `page_size` pages keyed by uid
    /// (`first` + `after`), `fields` being the selection (a `uid` is added).
    ///
    /// The walk ends only on an EMPTY page, never on a short one, so a server
    /// side row cap lower than `page_size` cannot end it early. More than
    /// `max_rows` rows is an `Unavailable` error, never a partial result.
    async fn all_nodes(
        &self,
        func: &str,
        filter: &str,
        fields: &str,
        page_size: usize,
        max_rows: usize,
    ) -> Result<Vec<serde_json::Value>, CatalogError> {
        let page_size = page_size.max(1);
        let mut rows: Vec<serde_json::Value> = Vec::new();
        let mut after = "0x0".to_owned();
        loop {
            let query = format!(
                "{{ q(func: {func}, first: {page_size}, after: {after}) {filter} \
                 {{ uid {fields} }} }}"
            );
            let json = self.execute_dql_row(&query).await?;
            let nodes = json
                .get("q")
                .and_then(serde_json::Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let Some(last) = nodes.last() else {
                return Ok(rows);
            };
            after = node_text(last, "uid")?;
            if rows.len().saturating_add(nodes.len()) > max_rows {
                return Err(CatalogError::unavailable(format!(
                    "more than {max_rows} rows for {func}: refusing to return a partial list"
                )));
            }
            rows.extend(nodes.iter().cloned());
        }
    }

    /// Reads ALL holds in `page_size` pages. More than `max_holds` is an
    /// `Unavailable` error, never a partial list.
    pub async fn holds_bounded(
        &self,
        page_size: usize,
        max_holds: usize,
    ) -> Result<Vec<ContentHold>, CatalogError> {
        let nodes = self
            .all_nodes(
                "has(hodei.gc.hold.hold_id)",
                "",
                HOLD_FIELDS,
                page_size,
                max_holds,
            )
            .await?;
        let mut holds = nodes
            .iter()
            .map(hold_of_node)
            .collect::<Result<Vec<_>, _>>()?;
        holds.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(holds)
    }
}

#[async_trait]
impl ContentLedger for DgraphCatalogStore {
    fn supports_fenced_pass(&self) -> bool {
        true
    }

    /// The materialized global census does NOT exist in this authority:
    /// it would be the scan forbidden by HSP-220. The coordinated pass uses
    /// `root_page`, which walks bounded pages per scope.
    async fn content_roots(&self) -> Result<Vec<ContentRoot>, CatalogError> {
        Err(CatalogError::unavailable(
            "content_roots materializes the whole census: use the paged root_page by scope instead (HSP-220)",
        ))
    }

    /// Every hold, read page by page until the authority has no more.
    ///
    /// A hold is a deletion guard and a per-account limit input, so this read
    /// must never be partial: it is bounded defensively by
    /// `MAX_HOLDS_READ` and fails closed past that bound.
    async fn holds(&self) -> Result<Vec<ContentHold>, CatalogError> {
        self.holds_bounded(HOLDS_PAGE, MAX_HOLDS_READ).await
    }

    /// A point read by the exact key: it never walks the other holds.
    async fn hold(&self, id: &str) -> Result<Option<ContentHold>, CatalogError> {
        let key = format!("hold|{}", key_part(id));
        let query = format!(
            "{{ q(func: eq(hodei.gc.hold.key, {})) {{ {HOLD_FIELDS} }} }}",
            dql_string(&key)
        );
        let json = self.execute_dql_row(&query).await?;
        json.get("q")
            .and_then(serde_json::Value::as_array)
            .and_then(|nodes| nodes.first())
            .map(hold_of_node)
            .transpose()
    }

    async fn place_hold(&self, hold: &ContentHold) -> Result<(), CatalogError> {
        let (scope_kind, account, repository, digest_of) = match &hold.scope {
            HoldScope::Digest { digest } => ("digest", None, None, Some(digest_key(digest))),
            HoldScope::Repository {
                account,
                repository,
            } => (
                "repository",
                Some(account.clone()),
                Some(repository.clone()),
                None,
            ),
            HoldScope::Account { account } => ("account", Some(account.clone()), None, None),
        };
        let key = format!("hold|{}", key_part(&hold.id));
        // The body is bound to a blank node on creation and to the uid
        // bound on update: the same row written twice
        // by two writers leaves no stale fields from the previous scope.
        let body = |subject: &str| -> String {
            let mut set = format!(
                "{subject} <hodei.gc.hold.hold_id> {id_q} .\n\
                 {subject} <hodei.gc.hold.scope_kind> {scope_q} .\n\
                 {subject} <hodei.gc.hold.reason> {reason_q} .\n\
                 {subject} <hodei.gc.hold.actor> {actor_q} .\n\
                 {subject} <hodei.gc.hold.created_at> \"{created}\" .\n",
                id_q = dql_string(&hold.id),
                scope_q = dql_string(scope_kind),
                reason_q = dql_string(&hold.reason),
                actor_q = dql_string(&hold.actor),
                created = stamp(hold.created_at),
            );
            for (predicate, value) in [
                ("hodei.gc.hold.account", account.clone()),
                ("hodei.gc.hold.repository", repository.clone()),
                ("hodei.gc.hold.digest_of", digest_of.clone()),
            ] {
                if let Some(value) = value {
                    set.push_str(&format!(
                        "{subject} <{predicate}> {} .\n",
                        dql_string(&value)
                    ));
                }
            }
            if let Some(expires) = hold.expires_at {
                set.push_str(&format!(
                    "{subject} <hodei.gc.hold.expires_at> \"{}\" .\n",
                    stamp(expires)
                ));
            }
            set
        };
        let delete = "uid(hl) <hodei.gc.hold.account> * .\n\
             uid(hl) <hodei.gc.hold.repository> * .\n\
             uid(hl) <hodei.gc.hold.digest_of> * .\n\
             uid(hl) <hodei.gc.hold.expires_at> * .\n";
        let create = format!(
            "_:hl <dgraph.type> \"{GC_HOLD_TYPE}\" .\n\
             _:hl <hodei.gc.hold.key> {key_q} .\n{}",
            body("_:hl"),
            key_q = dql_string(&key),
        );
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{ hl as var(func: eq(hodei.gc.hold.key, {})) }}",
                dql_string(&key),
            ),
            blocks: vec![
                GraphMutationBlock::set_if("eq(len(hl), 0)", create),
                GraphMutationBlock {
                    cond: Some("eq(len(hl), 1)".to_owned()),
                    set_nquads: body("uid(hl)"),
                    delete_nquads: delete.to_owned(),
                },
            ],
            commit_now: true,
        };
        self.client.upsert(transaction).await.map_err(graph_error)?;
        Ok(())
    }

    async fn release_hold(&self, id: &str) -> Result<bool, CatalogError> {
        let key = format!("hold|{}", key_part(id));
        let exists = {
            let json = self
                .execute_dql_row(&format!(
                    "{{ q(func: eq(hodei.gc.hold.key, {})) {{ uid }} }}",
                    dql_string(&key),
                ))
                .await?;
            json.get("q")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|array| !array.is_empty())
        };
        if !exists {
            return Ok(false);
        }
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{ hl as var(func: eq(hodei.gc.hold.key, {})) }}",
                dql_string(&key),
            ),
            blocks: vec![GraphMutationBlock {
                cond: Some("eq(len(hl), 1)".to_owned()),
                set_nquads: String::new(),
                delete_nquads: "uid(hl) * * .\n".to_owned(),
            }],
            commit_now: true,
        };
        self.client.upsert(transaction).await.map_err(graph_error)?;
        Ok(true)
    }

    /// The global pending mark does NOT exist in this authority: the
    /// legacy scopeless pass has nothing to rely on here. The
    /// coordinated mode uses `pending_scoped_run`.
    async fn pending_run(&self) -> Result<Option<GcRun>, CatalogError> {
        Err(CatalogError::unavailable(
            "pending_run is a global scan: the Dgraph ledger only knows scoped pending runs (pending_scoped_run)",
        ))
    }

    async fn last_swept_run(&self) -> Result<Option<GcRun>, CatalogError> {
        let nodes = self
            .all_nodes(
                "eq(hodei.gc.run.state, \"swept\")",
                "",
                "hodei.gc.run.run_id",
                HOLDS_PAGE,
                MAX_GC_ROWS_READ,
            )
            .await?;
        let run_id = nodes
            .iter()
            .filter_map(|node| node.get("hodei.gc.run.run_id").and_then(|v| v.as_str()))
            .max()
            .map(str::to_owned);
        match run_id {
            Some(run_id) => self.the_run_row(&run_id).await,
            None => Ok(None),
        }
    }

    /// Opening a pass without a claim is only valid if a live claim
    /// already exists for its generation — otherwise legacy mode has nothing
    /// to rely on in this authority.
    async fn open_run(&self, run: &GcRun) -> Result<(), CatalogError> {
        let Some(scope) = run.scope.as_deref() else {
            return Err(CatalogError::unavailable(
                "open_run without a scope needs a coordinated claim (HSP-192)",
            ));
        };
        // The mark is only opened under the scope's LIVE claim: without a
        // live lease there is no generation to open, and an expired lease
        // is the sign that another replica is in charge now.
        let now = Utc::now();
        let row = self
            .lease_row(&gc_lease_key(scope))
            .await?
            .ok_or_else(|| CatalogError::precondition_failed("gc lease", scope, "no live lease"))?;
        let live = lease_from(&row)?;
        if live.lease_until <= now {
            return Err(CatalogError::precondition_failed(
                "gc lease",
                scope,
                "the lease expired; its generation no longer rules",
            ));
        }
        let generation = run.generation.unwrap_or(live.generation);
        if generation != live.generation {
            return Err(CatalogError::precondition_failed(
                "gc lease",
                scope,
                format!(
                    "the run names generation {generation} but the scope lives in generation {}",
                    live.generation
                ),
            ));
        }
        let (set, _) = run_mark_nquads(scope, &live, run);
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{ gl as var(func: eq(hodei.gc.lease.key, {})) \
                 @filter(eq(hodei.gc.lease.token, {}) AND gt(hodei.gc.lease.lease_until, \"{}\")) }}",
                dql_string(&gc_lease_key(scope)),
                dql_string(&live.token),
                stamp(now),
            ),
            blocks: vec![GraphMutationBlock::set_if("eq(len(gl), 1)", set)],
            commit_now: true,
        };
        self.client.upsert(transaction).await.map_err(graph_error)?;
        Ok(())
    }

    async fn complete_run(
        &self,
        run_id: &str,
        deleted: u64,
        bytes_freed: u64,
        at: DateTime<Utc>,
    ) -> Result<(), CatalogError> {
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{ ru as var(func: eq(hodei.gc.run.run_id, {})) }}",
                dql_string(run_id),
            ),
            blocks: vec![GraphMutationBlock::set_if(
                "eq(len(ru), 1)",
                format!(
                    "uid(ru) <hodei.gc.run.state> \"swept\" .\n\
                     uid(ru) <hodei.gc.run.deleted> \"{deleted}\" .\n\
                     uid(ru) <hodei.gc.run.bytes_freed> \"{bytes}\" .\n\
                     uid(ru) <hodei.gc.run.swept_at> \"{at}\" .\n",
                    bytes = bytes_freed,
                    at = stamp(at),
                ),
            )],
            commit_now: true,
        };
        self.client.upsert(transaction).await.map_err(graph_error)?;
        Ok(())
    }

    /// The legacy trace, without a claim: like the coordinated one but without
    /// the lease guard. Revalidation is the caller's job.
    async fn record_deletion(&self, deletion: &GcDeletion) -> Result<(), CatalogError> {
        let tkey = format!("tomb|{}", digest_key(&deletion.digest));
        let create = tombstone_nquads("_:td", &tkey, deletion, true);
        let update = tombstone_nquads("uid(td)", &tkey, deletion, false);
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{ td as var(func: eq(hodei.gc.tombstone.key, {})) }}",
                dql_string(&tkey),
            ),
            blocks: vec![
                GraphMutationBlock::set_if("eq(len(td), 0)", create),
                GraphMutationBlock::set_if("eq(len(td), 1)", update),
            ],
            commit_now: true,
        };
        self.client.upsert(transaction).await.map_err(graph_error)?;
        self.verdict_tombstone(deletion).await
    }

    async fn deletion_of(
        &self,
        digest: &ContentDigest,
    ) -> Result<Option<GcDeletion>, CatalogError> {
        self.deletion_of_here(digest).await
    }

    async fn claim_gc_pass(
        &self,
        scope: &str,
        owner: &str,
        lease: std::time::Duration,
        now: DateTime<Utc>,
    ) -> Result<GcLease, CatalogError> {
        self.claim_gc_pass_here(scope, owner, lease, now).await
    }

    async fn current_gc_lease(
        &self,
        scope: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<GcLease>, CatalogError> {
        self.current_gc_lease_here(scope, now).await
    }

    async fn mark_run_fenced(
        &self,
        scope: &str,
        lease: &GcLease,
        run: &GcRun,
        candidate_digests: &[ContentDigest],
    ) -> Result<Vec<RunCandidate>, CatalogError> {
        // The reference guard lives IN the mark: they are asked
        // before the commit for each digests the plan proposed.
        for digest in candidate_digests {
            let live = self.live_reference_count_here(digest).await?;
            if live > 0 {
                return Err(CatalogError::precondition_failed(
                    "gc mark",
                    digest.to_string(),
                    "a candidate gained a live reference while marking",
                ));
            }
        }
        self.mark_run_fenced_here(scope, lease, run).await
    }

    async fn mark_of_run(
        &self,
        scope: &str,
        lease: &GcLease,
        run_id: &str,
    ) -> Result<Option<GcRun>, CatalogError> {
        self.mark_of_run_here(scope, lease, run_id).await
    }

    async fn record_deletion_fenced(
        &self,
        scope: &str,
        lease: &GcLease,
        deletion: &GcDeletion,
    ) -> Result<(), CatalogError> {
        self.record_deletion_fenced_here(scope, lease, deletion)
            .await
    }

    async fn record_quarantine(
        &self,
        scope: &str,
        lease: &GcLease,
        quarantine: &Quarantined,
    ) -> Result<(), CatalogError> {
        self.record_quarantine_here(scope, lease, quarantine).await
    }

    async fn root_page(
        &self,
        scope: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<RootPage, CatalogError> {
        self.root_page_here(scope, after, limit).await
    }

    async fn quarantines_of_run(&self, run_id: &str) -> Result<Vec<Quarantined>, CatalogError> {
        self.quarantines_of_run_here(run_id).await
    }

    async fn pending_scoped_run(
        &self,
        scope: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<GcRun>, CatalogError> {
        self.pending_scoped_run_here(scope, now).await
    }

    async fn renew_gc_lease(
        &self,
        scope: &str,
        lease: &GcLease,
        owner: &str,
        lease_for: std::time::Duration,
        now: DateTime<Utc>,
    ) -> Result<GcLease, CatalogError> {
        self.renew_gc_lease_here(scope, lease, owner, lease_for, now)
            .await
    }

    async fn live_reference_count(&self, digest: &ContentDigest) -> Result<u64, CatalogError> {
        self.live_reference_count_here(digest).await
    }

    async fn complete_scoped_run(
        &self,
        scope: &str,
        lease: &GcLease,
        run_id: &str,
        deleted: u64,
        bytes_freed: u64,
        _quarantined: u64,
        at: DateTime<Utc>,
    ) -> Result<(), CatalogError> {
        self.complete_scoped_run_here(scope, lease, run_id, deleted, bytes_freed, at)
            .await
    }

    async fn release_gc_lease(&self, scope: &str, lease: &GcLease) -> Result<(), CatalogError> {
        self.release_gc_lease_here(scope, lease).await
    }

    async fn census_accounts(&self) -> Result<Vec<String>, CatalogError> {
        self.census_accounts_here().await
    }
}

#[cfg(test)]
mod reference_guard_tests {
    use super::{no_live_reference, reference_guard_lines};
    use hodei_evidence::{ContentDigest, DigestAlgorithm};

    #[test]
    fn deletion_guard_checks_every_live_root_family() {
        let digest = ContentDigest {
            algorithm: DigestAlgorithm::Sha256,
            value: "a".repeat(64),
        };
        let guards = reference_guard_lines(&digest);
        let condition = no_live_reference();

        for index in 0..4 {
            assert!(guards.contains(&format!("rv{index} as var(")));
            assert!(condition.contains(&format!("eq(len(rv{index}), 0)")));
        }
        assert!(guards.contains("hodei.catalog.apt_snapshot_root.digest"));
    }
}
