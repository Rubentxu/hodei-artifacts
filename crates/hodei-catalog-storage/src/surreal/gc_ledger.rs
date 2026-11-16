//! El libro de la recoleccion sobre la base del catalogo.
//!
//! Las raices se leen de las mismas tablas que sirven el plano de datos
//! (assets, referencias, versiones): no hay una copia aparte que pueda
//! desincronizarse. Lo que si es propio de la recoleccion son tres
//! tablas: las retenciones, las pasadas y el rastro de los borrados.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_audit::{AuditContext, AuditEvent, AuditResult};
use hodei_catalog::{
    CatalogError, ContentHold, ContentLedger, ContentRoot, DeletionState, GcDeletion, GcRun,
    HoldScope, RootKind, RunCandidate, RunState,
};
use hodei_evidence::ContentDigest;
use serde::{Deserialize, Serialize};
use surrealdb::sql::{Datetime, Thing};

use super::{SurrealCatalogStore, composite_id, digest_of, size_of, the_subject_of};

fn unavailable(error: impl ToString) -> CatalogError {
    CatalogError::unavailable(error.to_string())
}

fn hold_thing(id: &str) -> Thing {
    Thing::from(("gc_hold", composite_id(&[id]).as_str()))
}

fn run_thing(id: &str) -> Thing {
    Thing::from(("gc_run", composite_id(&[id]).as_str()))
}

fn tombstone_thing(digest: &ContentDigest) -> Thing {
    Thing::from((
        "gc_tombstone",
        composite_id(&[&digest.algorithm.to_string(), &digest.value]).as_str(),
    ))
}

/// La fila de una retencion.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct HoldRow {
    hold_id: String,
    scope_kind: String,
    account: Option<String>,
    repository: Option<String>,
    digest_algorithm: Option<String>,
    digest: Option<String>,
    reason: String,
    actor: String,
    created_at: Datetime,
    expires_at: Option<Datetime>,
}

impl HoldRow {
    fn of(hold: &ContentHold) -> Self {
        let (scope_kind, account, repository, digest) = match &hold.scope {
            HoldScope::Digest { digest } => ("digest", None, None, Some(digest.clone())),
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
        Self {
            hold_id: hold.id.clone(),
            scope_kind: scope_kind.to_string(),
            account,
            repository,
            digest_algorithm: digest.as_ref().map(|d| d.algorithm.to_string()),
            digest: digest.map(|d| d.value),
            reason: hold.reason.clone(),
            actor: hold.actor.clone(),
            created_at: Datetime::from(hold.created_at),
            expires_at: hold.expires_at.map(Datetime::from),
        }
    }

    fn the_hold(self) -> Result<ContentHold, CatalogError> {
        let corrupt =
            |detail: &str| CatalogError::corrupt("hold", self.hold_id.clone(), detail.to_string());
        let scope = match self.scope_kind.as_str() {
            "digest" => {
                let (Some(algorithm), Some(value)) =
                    (self.digest_algorithm.as_deref(), self.digest.as_deref())
                else {
                    return Err(corrupt("the digest hold has no digest"));
                };
                HoldScope::Digest {
                    digest: digest_of(algorithm, value)?,
                }
            }
            "repository" => match (self.account.clone(), self.repository.clone()) {
                (Some(account), Some(repository)) => HoldScope::Repository {
                    account,
                    repository,
                },
                _ => return Err(corrupt("the repository hold has no repository")),
            },
            "account" => match self.account.clone() {
                Some(account) => HoldScope::Account { account },
                None => return Err(corrupt("the account hold has no account")),
            },
            _ => return Err(corrupt("the hold scope is not known")),
        };
        Ok(ContentHold {
            id: self.hold_id,
            scope,
            reason: self.reason,
            actor: self.actor,
            created_at: self.created_at.0,
            expires_at: self.expires_at.map(|at| at.0),
        })
    }
}

/// La fila de una pasada.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RunRow {
    run_id: String,
    state: String,
    grace_secs: i64,
    actor: String,
    /// `<algoritmo>:<valor>:<bytes>` por candidato.
    candidates: Vec<String>,
    marked_at: Datetime,
    deleted: i64,
    bytes_freed: i64,
    swept_at: Option<Datetime>,
}

impl RunRow {
    fn of(run: &GcRun) -> Self {
        Self {
            run_id: run.id.clone(),
            state: run.state.as_str().to_string(),
            grace_secs: i64::try_from(run.grace_secs).unwrap_or(i64::MAX),
            actor: run.actor.clone(),
            candidates: run
                .candidates
                .iter()
                .map(|c| format!("{}:{}:{}", c.digest.algorithm, c.digest.value, c.length))
                .collect(),
            marked_at: Datetime::from(run.marked_at),
            deleted: i64::try_from(run.deleted).unwrap_or(i64::MAX),
            bytes_freed: i64::try_from(run.bytes_freed).unwrap_or(i64::MAX),
            swept_at: run.swept_at.map(Datetime::from),
        }
    }

    fn the_run(self) -> Result<GcRun, CatalogError> {
        let corrupt = |detail: String| CatalogError::corrupt("gc run", self.run_id.clone(), detail);
        let state = match self.state.as_str() {
            "marked" => RunState::Marked,
            "swept" => RunState::Swept,
            other => return Err(corrupt(format!("the run state is not known: {other}"))),
        };
        let mut candidates = Vec::with_capacity(self.candidates.len());
        for text in &self.candidates {
            let mut parts = text.splitn(3, ':');
            let (Some(algorithm), Some(value), Some(length)) =
                (parts.next(), parts.next(), parts.next())
            else {
                return Err(corrupt(format!("the candidate is malformed: {text}")));
            };
            let length: u64 = length
                .parse()
                .map_err(|_| corrupt(format!("the candidate length is malformed: {text}")))?;
            candidates.push(RunCandidate {
                digest: digest_of(algorithm, value)?,
                length,
            });
        }
        Ok(GcRun {
            id: self.run_id.clone(),
            state,
            grace_secs: u64::try_from(self.grace_secs).unwrap_or(0),
            actor: self.actor.clone(),
            candidates,
            marked_at: self.marked_at.0,
            deleted: u64::try_from(self.deleted).unwrap_or(0),
            bytes_freed: u64::try_from(self.bytes_freed).unwrap_or(0),
            swept_at: self.swept_at.map(|at| at.0),
        })
    }
}

/// La fila del rastro de un borrado.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TombstoneRow {
    digest_algorithm: String,
    digest: String,
    length: i64,
    run_id: String,
    actor: String,
    account: String,
    state: String,
    at: Datetime,
}

impl TombstoneRow {
    fn the_deletion(self) -> Result<GcDeletion, CatalogError> {
        let state = match self.state.as_str() {
            "intended" => DeletionState::Intended,
            "confirmed" => DeletionState::Confirmed,
            other => {
                return Err(CatalogError::corrupt(
                    "gc deletion",
                    self.digest.clone(),
                    format!("the deletion state is not known: {other}"),
                ));
            }
        };
        Ok(GcDeletion {
            digest: digest_of(&self.digest_algorithm, &self.digest)?,
            length: size_of(self.length)?,
            run_id: self.run_id,
            actor: self.actor,
            account: self.account,
            state,
            at: self.at.0,
        })
    }
}

#[derive(Debug, Deserialize)]
struct AssetRootRow {
    account: String,
    repository: String,
    asset_path: String,
    digest_algorithm: String,
    digest: String,
    subject: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReferenceRootRow {
    account: String,
    repository: String,
    asset_path: String,
    digest_algorithm: String,
    digest: String,
}

#[derive(Debug, Deserialize)]
struct VersionRootRow {
    account: String,
    repository: String,
    package_name: String,
    package_version: String,
    digest_algorithm: String,
    digest: String,
}

impl SurrealCatalogStore {
    async fn read_rows<T: for<'de> Deserialize<'de>>(
        &self,
        query: &str,
    ) -> Result<Vec<T>, CatalogError> {
        let mut response = self.db.query(query).await.map_err(unavailable)?;
        response.take(0).map_err(unavailable)
    }
}

#[async_trait]
impl ContentLedger for SurrealCatalogStore {
    async fn content_roots(&self) -> Result<Vec<ContentRoot>, CatalogError> {
        let mut roots = Vec::new();

        let assets: Vec<AssetRootRow> = self
            .read_rows(
                "SELECT account, repository, asset_path, digest_algorithm, digest, subject \
                 FROM catalog_asset",
            )
            .await?;
        for row in assets {
            roots.push(ContentRoot {
                kind: RootKind::Asset,
                account: row.account.clone(),
                repository: row.repository.clone(),
                subject: row.asset_path.clone(),
                digest: digest_of(&row.digest_algorithm, &row.digest)?,
            });
            if let Some(subject) = the_subject_of(row.subject.as_deref())? {
                roots.push(ContentRoot {
                    kind: RootKind::Subject,
                    account: row.account,
                    repository: row.repository,
                    subject: row.asset_path,
                    digest: subject,
                });
            }
        }

        let references: Vec<ReferenceRootRow> = self
            .read_rows(
                "SELECT account, repository, asset_path, digest_algorithm, digest \
                 FROM catalog_reference",
            )
            .await?;
        for row in references {
            roots.push(ContentRoot {
                kind: RootKind::Reference,
                account: row.account,
                repository: row.repository,
                subject: row.asset_path,
                digest: digest_of(&row.digest_algorithm, &row.digest)?,
            });
        }

        let versions: Vec<VersionRootRow> = self
            .read_rows(
                "SELECT account, repository, package_name, package_version, \
                 digest_algorithm, digest FROM catalog_version",
            )
            .await?;
        for row in versions {
            roots.push(ContentRoot {
                kind: RootKind::Version,
                account: row.account,
                repository: row.repository,
                subject: format!("{}@{}", row.package_name, row.package_version),
                digest: digest_of(&row.digest_algorithm, &row.digest)?,
            });
        }
        Ok(roots)
    }

    async fn holds(&self) -> Result<Vec<ContentHold>, CatalogError> {
        let rows: Vec<HoldRow> = self
            .read_rows(
                "SELECT hold_id, scope_kind, account, repository, digest_algorithm, digest, \
                 reason, actor, created_at, expires_at FROM gc_hold",
            )
            .await?;
        let mut holds = rows
            .into_iter()
            .map(HoldRow::the_hold)
            .collect::<Result<Vec<_>, _>>()?;
        holds.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(holds)
    }

    async fn place_hold(&self, hold: &ContentHold) -> Result<(), CatalogError> {
        self.db
            .query("UPSERT $id CONTENT $row")
            .bind(("id", hold_thing(&hold.id)))
            .bind(("row", HoldRow::of(hold)))
            .await
            .map_err(unavailable)?
            .check()
            .map_err(unavailable)?;
        Ok(())
    }

    async fn release_hold(&self, id: &str) -> Result<bool, CatalogError> {
        let mut response = self
            .db
            .query("DELETE $id RETURN BEFORE")
            .bind(("id", hold_thing(id)))
            .await
            .map_err(unavailable)?;
        #[derive(Deserialize)]
        struct Removed {
            #[allow(dead_code)]
            hold_id: String,
        }
        let removed: Vec<Removed> = response.take(0).map_err(unavailable)?;
        Ok(!removed.is_empty())
    }

    async fn pending_run(&self) -> Result<Option<GcRun>, CatalogError> {
        let rows: Vec<RunRow> = self
            .read_rows(
                "SELECT run_id, state, grace_secs, actor, candidates, marked_at, deleted, \
                 bytes_freed, swept_at FROM gc_run WHERE state = 'marked' \
                 ORDER BY marked_at ASC LIMIT 1",
            )
            .await?;
        rows.into_iter().next().map(RunRow::the_run).transpose()
    }

    async fn last_swept_run(&self) -> Result<Option<GcRun>, CatalogError> {
        let rows: Vec<RunRow> = self
            .read_rows(
                "SELECT run_id, state, grace_secs, actor, candidates, marked_at, deleted, \
                 bytes_freed, swept_at FROM gc_run WHERE state = 'swept' \
                 ORDER BY swept_at DESC LIMIT 1",
            )
            .await?;
        rows.into_iter().next().map(RunRow::the_run).transpose()
    }

    async fn open_run(&self, run: &GcRun) -> Result<(), CatalogError> {
        self.db
            .query("UPSERT $id CONTENT $row")
            .bind(("id", run_thing(&run.id)))
            .bind(("row", RunRow::of(run)))
            .await
            .map_err(unavailable)?
            .check()
            .map_err(unavailable)?;
        Ok(())
    }

    async fn complete_run(
        &self,
        run_id: &str,
        deleted: u64,
        bytes_freed: u64,
        at: DateTime<Utc>,
    ) -> Result<(), CatalogError> {
        self.db
            .query(
                "UPDATE $id SET state = 'swept', deleted = $deleted, \
                 bytes_freed = $bytes, swept_at = $at",
            )
            .bind(("id", run_thing(run_id)))
            .bind(("deleted", i64::try_from(deleted).unwrap_or(i64::MAX)))
            .bind(("bytes", i64::try_from(bytes_freed).unwrap_or(i64::MAX)))
            .bind(("at", Datetime::from(at)))
            .await
            .map_err(unavailable)?
            .check()
            .map_err(unavailable)?;
        Ok(())
    }

    async fn record_deletion(&self, deletion: &GcDeletion) -> Result<(), CatalogError> {
        let previous = self.deletion_of(&deletion.digest).await?;
        let row = TombstoneRow {
            digest_algorithm: deletion.digest.algorithm.to_string(),
            digest: deletion.digest.value.clone(),
            length: i64::try_from(deletion.length).unwrap_or(i64::MAX),
            run_id: deletion.run_id.clone(),
            actor: deletion.actor.clone(),
            account: deletion.account.clone(),
            state: deletion.state.as_str().to_string(),
            at: Datetime::from(deletion.at),
        };
        self.db
            .query("UPSERT $id CONTENT $row")
            .bind(("id", tombstone_thing(&deletion.digest)))
            .bind(("row", row))
            .await
            .map_err(unavailable)?
            .check()
            .map_err(unavailable)?;

        // El evento de auditoria se dice una vez por borrado confirmado:
        // confirmar otra vez la misma pasada no lo repite.
        let already_confirmed = previous
            .is_some_and(|p| p.state == DeletionState::Confirmed && p.run_id == deletion.run_id);
        if deletion.state == DeletionState::Confirmed && !already_confirmed {
            let event = AuditEvent::new(
                format!("gc-delete-{}-{}", deletion.run_id, deletion.digest.value),
                AuditContext::new(
                    deletion.actor.clone(),
                    deletion.account.clone(),
                    deletion.run_id.clone(),
                ),
                deletion.at,
                "gc.delete",
                format!("cas/{}", deletion.digest.value),
                Some(deletion.digest.to_string()),
                AuditResult::Allowed,
            );
            self.record_the_audit(&event).await;
        }
        Ok(())
    }

    async fn deletion_of(
        &self,
        digest: &ContentDigest,
    ) -> Result<Option<GcDeletion>, CatalogError> {
        let mut response = self
            .db
            .query(
                "SELECT digest_algorithm, digest, length, run_id, actor, account, state, at \
                 FROM $id",
            )
            .bind(("id", tombstone_thing(digest)))
            .await
            .map_err(unavailable)?;
        let rows: Vec<TombstoneRow> = response.take(0).map_err(unavailable)?;
        rows.into_iter()
            .next()
            .map(TombstoneRow::the_deletion)
            .transpose()
    }
}
