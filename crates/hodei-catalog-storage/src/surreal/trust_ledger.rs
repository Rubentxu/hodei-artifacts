//! El libro de las concesiones de confianza sobre la base del catalogo.
//!
//! Una fila por (cuenta, repositorio, digest, decision). La consulta de
//! "esta concedido" mira filas **activas** y nada mas: no hay cache, de
//! modo que una revocacion se ve en la lectura siguiente.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_catalog::{CatalogError, TrustGrant, TrustLedger};
use hodei_evidence::ContentDigest;
use serde::{Deserialize, Serialize};
use surrealdb::sql::{Datetime, Thing};

use super::{SurrealCatalogStore, composite_id, digest_of};

fn unavailable(error: impl ToString) -> CatalogError {
    CatalogError::unavailable(error.to_string())
}

fn grant_thing(
    account: &str,
    repository: &str,
    digest: &ContentDigest,
    decision_id: &str,
) -> Thing {
    Thing::from((
        "trust_grant",
        composite_id(&[
            account,
            repository,
            &digest.algorithm.to_string(),
            &digest.value,
            decision_id,
        ])
        .as_str(),
    ))
}

/// La fila de una concesion, tal y como se escribe.
#[derive(Debug, Clone, Serialize)]
struct NewRow {
    id: Thing,
    account: String,
    repository: String,
    digest_algorithm: String,
    digest: String,
    decision_id: String,
    actor: String,
    granted_at: Datetime,
}

impl NewRow {
    fn of(grant: &TrustGrant) -> Self {
        Self {
            id: grant_thing(
                &grant.account,
                &grant.repository,
                &grant.digest,
                &grant.decision_id,
            ),
            account: grant.account.clone(),
            repository: grant.repository.clone(),
            digest_algorithm: grant.digest.algorithm.to_string(),
            digest: grant.digest.value.clone(),
            decision_id: grant.decision_id.clone(),
            actor: grant.actor.clone(),
            granted_at: Datetime::from(grant.granted_at),
        }
    }
}

/// La fila de una concesion, tal y como se lee.
#[derive(Debug, Clone, Deserialize)]
struct Row {
    account: String,
    repository: String,
    digest_algorithm: String,
    digest: String,
    decision_id: String,
    actor: String,
    granted_at: Datetime,
    revoked_at: Option<Datetime>,
    revoked_by: Option<String>,
    revoke_reason: Option<String>,
}

impl Row {
    fn the_grant(self) -> Result<TrustGrant, CatalogError> {
        Ok(TrustGrant {
            digest: digest_of(&self.digest_algorithm, &self.digest)?,
            account: self.account,
            repository: self.repository,
            decision_id: self.decision_id,
            actor: self.actor,
            granted_at: self.granted_at.0,
            revoked_at: self.revoked_at.map(|at| at.0),
            revoked_by: self.revoked_by,
            revoke_reason: self.revoke_reason,
        })
    }
}

const COLUMNS: &str = "account, repository, digest_algorithm, digest, decision_id, actor, \
                       granted_at, revoked_at, revoked_by, revoke_reason";

#[async_trait]
impl TrustLedger for SurrealCatalogStore {
    async fn grant(&self, grants: &[TrustGrant]) -> Result<(), CatalogError> {
        if grants.is_empty() {
            return Ok(());
        }
        let rows: Vec<NewRow> = grants.iter().map(NewRow::of).collect();
        // `IGNORE` hace la escritura idempotente y, sobre todo, impide
        // que repetirla reactive una concesion revocada.
        self.db
            .query("INSERT IGNORE INTO trust_grant $rows")
            .bind(("rows", rows))
            .await
            .map_err(unavailable)?
            .check()
            .map_err(unavailable)?;
        Ok(())
    }

    async fn revoke_decision(
        &self,
        account: &str,
        repository: &str,
        decision_id: &str,
        actor: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<u64, CatalogError> {
        let mut response = self
            .db
            .query(
                "UPDATE trust_grant SET revoked_at = $at, revoked_by = $actor, \
                 revoke_reason = $reason \
                 WHERE account = $account AND repository = $repository \
                 AND decision_id = $decision AND revoked_at IS NONE RETURN AFTER",
            )
            .bind(("at", Datetime::from(at)))
            .bind(("actor", actor.to_string()))
            .bind(("reason", reason.to_string()))
            .bind(("account", account.to_string()))
            .bind(("repository", repository.to_string()))
            .bind(("decision", decision_id.to_string()))
            .await
            .map_err(unavailable)?;
        let revoked: Vec<Row> = response.take(0).map_err(unavailable)?;
        Ok(revoked.len() as u64)
    }

    async fn revoke_digest(
        &self,
        account: &str,
        repository: &str,
        digest: &ContentDigest,
        actor: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<u64, CatalogError> {
        let mut response = self
            .db
            .query(
                "UPDATE trust_grant SET revoked_at = $at, revoked_by = $actor, \
                 revoke_reason = $reason \
                 WHERE account = $account AND repository = $repository \
                 AND digest_algorithm = $algorithm AND digest = $digest \
                 AND revoked_at IS NONE RETURN AFTER",
            )
            .bind(("at", Datetime::from(at)))
            .bind(("actor", actor.to_string()))
            .bind(("reason", reason.to_string()))
            .bind(("account", account.to_string()))
            .bind(("repository", repository.to_string()))
            .bind(("algorithm", digest.algorithm.to_string()))
            .bind(("digest", digest.value.clone()))
            .await
            .map_err(unavailable)?;
        let revoked: Vec<Row> = response.take(0).map_err(unavailable)?;
        Ok(revoked.len() as u64)
    }

    async fn active_grant(
        &self,
        account: &str,
        repository: &str,
        digest: &ContentDigest,
    ) -> Result<Option<TrustGrant>, CatalogError> {
        let mut response = self
            .db
            .query(format!(
                "SELECT {COLUMNS} FROM trust_grant \
                 WHERE account = $account AND repository = $repository \
                 AND digest_algorithm = $algorithm AND digest = $digest \
                 AND revoked_at IS NONE LIMIT 1"
            ))
            .bind(("account", account.to_string()))
            .bind(("repository", repository.to_string()))
            .bind(("algorithm", digest.algorithm.to_string()))
            .bind(("digest", digest.value.clone()))
            .await
            .map_err(unavailable)?;
        let rows: Vec<Row> = response.take(0).map_err(unavailable)?;
        rows.into_iter().next().map(Row::the_grant).transpose()
    }

    async fn grants_of(
        &self,
        account: &str,
        repository: &str,
    ) -> Result<Vec<TrustGrant>, CatalogError> {
        let mut response = self
            .db
            .query(format!(
                "SELECT {COLUMNS} FROM trust_grant \
                 WHERE account = $account AND repository = $repository \
                 ORDER BY granted_at ASC, decision_id ASC, digest ASC"
            ))
            .bind(("account", account.to_string()))
            .bind(("repository", repository.to_string()))
            .await
            .map_err(unavailable)?;
        let rows: Vec<Row> = response.take(0).map_err(unavailable)?;
        rows.into_iter().map(Row::the_grant).collect()
    }
}
