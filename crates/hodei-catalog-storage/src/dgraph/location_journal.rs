//! The location journal of a blob over the native graph (HSP-325).
//!
//! One row per digest says where its reads go (see `hodei_catalog::location`).
//! The rules this module carries, copied from the GC lease because they are the
//! same problem:
//!
//! 1. **The claim is a durable CAS.** The row carries `fence`, `token`, `owner`
//!    and `lease_until`. The guard goes in the `@filter` of the upsert, on the
//!    (token, lease) that was just read, so the concurrent writer that read the
//!    same thing finds its block empty.
//! 2. **The fence is monotonic.** An expired lease is taken over with a NEW
//!    token and `fence + 1`; the fence never goes down, so a writer holding an
//!    old one can never match the guard again.
//! 3. **Every advance is guarded in the same transaction** by token, fence,
//!    live lease and the phase it expects to leave. The verdict comes from the
//!    post-commit re-read, never from the commit having gone through.

use async_trait::async_trait;
use chrono::{DateTime, SecondsFormat, Utc};
use hodei_catalog::{
    CatalogError, LocationEntry, LocationJournal, LocationLease, Phase,
};
use hodei_evidence::ContentDigest;
use hodei_graph_client::{
    GraphClientError, GraphMutationBlock, GraphMutationPort, GraphUpsertTransaction, dql_string,
};

use super::{DgraphCatalogStore, key_part, node_text, node_u64, single_row};

/// The graph type of a location row.
pub const LOCATION_TYPE: &str = "LocationJournal";

const LOCATION_FIELDS: &str = "hodei.loc.digest_of hodei.loc.phase hodei.loc.fence \
     hodei.loc.token hodei.loc.owner hodei.loc.lease_until";

/// The additive DDL of the journal: own predicates, idempotent.
pub fn location_schema_dql() -> String {
    let mut ddl = String::new();
    for (predicate, kind, index) in [
        ("hodei.loc.key", "string", "@index(exact) @upsert"),
        ("hodei.loc.digest_of", "string", "@index(exact)"),
        ("hodei.loc.phase", "string", ""),
        ("hodei.loc.fence", "int", ""),
        ("hodei.loc.token", "string", ""),
        ("hodei.loc.owner", "string", ""),
        ("hodei.loc.lease_until", "datetime", ""),
        ("hodei.loc.updated_at", "datetime", ""),
    ] {
        ddl.push_str(&format!("{predicate}: {kind}"));
        if !index.is_empty() {
            ddl.push(' ');
            ddl.push_str(index);
        }
        ddl.push_str(" .\n");
    }
    ddl.push_str(&format!(
        "type {LOCATION_TYPE} {{\nhodei.loc.key\nhodei.loc.digest_of\nhodei.loc.phase\n\
         hodei.loc.fence\nhodei.loc.token\nhodei.loc.owner\nhodei.loc.lease_until\n\
         hodei.loc.updated_at\n}}\n"
    ));
    ddl
}

fn location_key(digest: &ContentDigest) -> String {
    format!(
        "loc|{}|{}",
        key_part(&digest.algorithm.to_string()),
        key_part(&digest.value)
    )
}

fn stamp(instant: DateTime<Utc>) -> String {
    instant.to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn parse_stamp(field: &str, text: &str) -> Result<DateTime<Utc>, CatalogError> {
    DateTime::parse_from_rfc3339(text)
        .map(|instant| instant.with_timezone(&Utc))
        .map_err(|_| CatalogError::corrupt("location", field, format!("'{text}' is not a time")))
}

fn graph_error(error: GraphClientError) -> CatalogError {
    match error {
        GraphClientError::UpsertConflict { detail } => {
            CatalogError::conflict("location", "key", detail)
        }
        other => CatalogError::unavailable(other.to_string()),
    }
}

struct Row {
    phase: Phase,
    fence: u64,
    token: String,
    owner: String,
    lease_until: DateTime<Utc>,
}

fn row_of(node: &serde_json::Value) -> Result<Row, CatalogError> {
    let phase_text = node_text(node, "hodei.loc.phase")?;
    let phase = Phase::parse(&phase_text).ok_or_else(|| {
        CatalogError::corrupt("location", phase_text.clone(), "not a location phase")
    })?;
    Ok(Row {
        phase,
        fence: node_u64(node, "hodei.loc.fence")?,
        token: node_text(node, "hodei.loc.token")?,
        owner: node_text(node, "hodei.loc.owner")?,
        lease_until: parse_stamp(
            "hodei.loc.lease_until",
            &node_text(node, "hodei.loc.lease_until")?,
        )?,
    })
}

impl DgraphCatalogStore {
    async fn location_row(
        &self,
        digest: &ContentDigest,
    ) -> Result<Option<serde_json::Value>, CatalogError> {
        let key = location_key(digest);
        let query = format!(
            "{{ q(func: eq(hodei.loc.key, {})) {{ {LOCATION_FIELDS} }} }}",
            dql_string(&key)
        );
        let json = self
            .client
            .execute_dql(&query)
            .await
            .map(|response| response.json)
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        single_row(json.get("q"), "location", key)
    }

    /// The row of a digest, read.
    pub async fn location_of_here(
        &self,
        digest: &ContentDigest,
    ) -> Result<Option<LocationEntry>, CatalogError> {
        let Some(node) = self.location_row(digest).await? else {
            return Ok(None);
        };
        let row = row_of(&node)?;
        Ok(Some(LocationEntry {
            digest: digest.clone(),
            phase: row.phase,
            fence: row.fence,
            lease_until: row.lease_until,
        }))
    }
}

#[async_trait]
impl LocationJournal for DgraphCatalogStore {
    async fn location_of(
        &self,
        digest: &ContentDigest,
    ) -> Result<Option<LocationEntry>, CatalogError> {
        self.location_of_here(digest).await
    }

    async fn acquire_location(
        &self,
        digest: &ContentDigest,
        owner: &str,
        lease_for: std::time::Duration,
        now: DateTime<Utc>,
    ) -> Result<LocationLease, CatalogError> {
        if owner.is_empty() {
            return Err(CatalogError::invalid("the claim needs an owner"));
        }
        let lease_for = chrono::Duration::from_std(lease_for)
            .map_err(|error| CatalogError::invalid(format!("the lease is not valid: {error}")))?;
        let until = now + lease_for;
        let key = location_key(digest);
        let seen = self.location_row(digest).await?;
        let (query, set, cond, expected_token, expected_fence);
        match &seen {
            None => {
                let token = format!("{}-{}", self.identity.new_id(), stamp(now));
                expected_token = token.clone();
                expected_fence = 1;
                query = format!(
                    "{{ nc as var(func: eq(hodei.loc.key, {})) }}",
                    dql_string(&key)
                );
                cond = "eq(len(nc), 0)".to_owned();
                set = format!(
                    "_:lr <dgraph.type> \"{LOCATION_TYPE}\" .\n\
                     _:lr <hodei.loc.key> {key_q} .\n\
                     _:lr <hodei.loc.digest_of> {digest_q} .\n\
                     _:lr <hodei.loc.phase> \"origin\" .\n\
                     _:lr <hodei.loc.fence> \"1\" .\n\
                     _:lr <hodei.loc.token> {token_q} .\n\
                     _:lr <hodei.loc.owner> {owner_q} .\n\
                     _:lr <hodei.loc.lease_until> \"{until}\" .\n\
                     _:lr <hodei.loc.updated_at> \"{at}\" .\n",
                    key_q = dql_string(&key),
                    digest_q = dql_string(&digest.to_string()),
                    token_q = dql_string(&token),
                    owner_q = dql_string(owner),
                    until = stamp(until),
                    at = stamp(now),
                );
            }
            Some(node) => {
                let row = row_of(node)?;
                if row.lease_until > now && row.owner != owner {
                    return Err(CatalogError::conflict(
                        "location lease",
                        digest.to_string(),
                        format!(
                            "the digest is held by '{}' until {}",
                            row.owner,
                            stamp(row.lease_until)
                        ),
                    ));
                }
                if row.lease_until > now {
                    // The holder renews: same token and fence, longer lease.
                    query = format!(
                        "{{ lr as var(func: eq(hodei.loc.key, {})) \
                         @filter(eq(hodei.loc.token, {}) AND \
                         gt(hodei.loc.lease_until, \"{}\")) }}",
                        dql_string(&key),
                        dql_string(&row.token),
                        stamp(now)
                    );
                    expected_token = row.token.clone();
                    expected_fence = row.fence;
                    cond = "eq(len(lr), 1)".to_owned();
                    set = format!(
                        "uid(lr) <hodei.loc.lease_until> \"{until}\" .\n",
                        until = stamp(until)
                    );
                } else {
                    // Take-over: NEW token and a larger fence.
                    query = format!(
                        "{{ lr as var(func: eq(hodei.loc.key, {})) \
                         @filter(eq(hodei.loc.token, {}) AND \
                         le(hodei.loc.lease_until, \"{}\")) }}",
                        dql_string(&key),
                        dql_string(&row.token),
                        stamp(now)
                    );
                    let token = format!("{}-{}", self.identity.new_id(), stamp(now));
                    expected_token = token.clone();
                    expected_fence = row.fence + 1;
                    cond = "eq(len(lr), 1)".to_owned();
                    set = format!(
                        "uid(lr) <hodei.loc.fence> \"{next}\" .\n\
                         uid(lr) <hodei.loc.token> {token_q} .\n\
                         uid(lr) <hodei.loc.owner> {owner_q} .\n\
                         uid(lr) <hodei.loc.lease_until> \"{until}\" .\n\
                         uid(lr) <hodei.loc.updated_at> \"{at}\" .\n",
                        next = row.fence + 1,
                        token_q = dql_string(&token),
                        owner_q = dql_string(owner),
                        until = stamp(until),
                        at = stamp(now),
                    );
                }
            }
        }
        self.client
            .upsert(GraphUpsertTransaction {
                query,
                blocks: vec![GraphMutationBlock::set_if(cond, set)],
                commit_now: true,
            })
            .await
            .map_err(graph_error)?;
        // The verdict is the re-read: if the row is not what this claim meant
        // to leave, someone else won the race.
        let back = self
            .location_row(digest)
            .await?
            .ok_or_else(|| CatalogError::unavailable("the location row vanished after commit"))?;
        let back = row_of(&back)?;
        if back.token != expected_token || back.fence != expected_fence {
            return Err(CatalogError::conflict(
                "location lease",
                digest.to_string(),
                "another claim won the race for this digest",
            ));
        }
        Ok(LocationLease {
            digest: digest.clone(),
            fence: back.fence,
            token: back.token,
            lease_until: back.lease_until,
        })
    }

    async fn advance_location(
        &self,
        lease: &LocationLease,
        from: Phase,
        to: Phase,
    ) -> Result<LocationEntry, CatalogError> {
        if !from.may_become(to) {
            return Err(CatalogError::invalid(format!(
                "a location cannot go from {} to {}",
                from.as_str(),
                to.as_str()
            )));
        }
        let now = Utc::now();
        let key = location_key(&lease.digest);
        self.client
            .upsert(GraphUpsertTransaction {
                query: format!(
                    "{{ lr as var(func: eq(hodei.loc.key, {})) \
                     @filter(eq(hodei.loc.token, {}) AND eq(hodei.loc.fence, {}) AND \
                     eq(hodei.loc.phase, \"{}\") AND \
                     gt(hodei.loc.lease_until, \"{}\")) }}",
                    dql_string(&key),
                    dql_string(&lease.token),
                    lease.fence,
                    from.as_str(),
                    stamp(now),
                ),
                blocks: vec![GraphMutationBlock::set_if(
                    "eq(len(lr), 1)",
                    format!(
                        "uid(lr) <hodei.loc.phase> \"{}\" .\n\
                         uid(lr) <hodei.loc.updated_at> \"{}\" .\n",
                        to.as_str(),
                        stamp(now)
                    ),
                )],
                commit_now: true,
            })
            .await
            .map_err(graph_error)?;
        let node = self.location_row(&lease.digest).await?.ok_or_else(|| {
            CatalogError::precondition_failed(
                "location",
                lease.digest.to_string(),
                "the location row does not exist",
            )
        })?;
        let row = row_of(&node)?;
        if row.token != lease.token || row.fence != lease.fence || row.phase != to {
            return Err(CatalogError::precondition_failed(
                "location",
                lease.digest.to_string(),
                format!(
                    "the advance did not land: the presented fence {} is not the live claim of \
                     a row in {}",
                    lease.fence,
                    from.as_str()
                ),
            ));
        }
        Ok(LocationEntry {
            digest: lease.digest.clone(),
            phase: row.phase,
            fence: row.fence,
            lease_until: row.lease_until,
        })
    }

    async fn release_location(&self, lease: &LocationLease) -> Result<(), CatalogError> {
        let now = Utc::now();
        self.client
            .upsert(GraphUpsertTransaction {
                query: format!(
                    "{{ lr as var(func: eq(hodei.loc.key, {})) \
                     @filter(eq(hodei.loc.token, {})) }}",
                    dql_string(&location_key(&lease.digest)),
                    dql_string(&lease.token)
                ),
                blocks: vec![GraphMutationBlock::set_if(
                    "eq(len(lr), 1)",
                    format!("uid(lr) <hodei.loc.lease_until> \"{}\" .\n", stamp(now)),
                )],
                commit_now: true,
            })
            .await
            .map(|_| ())
            .map_err(graph_error)
    }

    async fn digest_owners(
        &self,
        digest: &ContentDigest,
    ) -> Result<Vec<(String, String)>, CatalogError> {
        self.digest_owners_here(digest).await
    }
}
