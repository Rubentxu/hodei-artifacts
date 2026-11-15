//! El outbox de auditoría sobre la base del catálogo.
//!
//! El outbox vive en la **misma base** que el catálogo, y a propósito: la
//! fila del evento se escribe dentro de la misma transacción que publica,
//! y una transacción que toca dos bases no es una transacción.
//!
//! Lo que hay aquí es el lado de la **entrega**: reclamar un evento,
//! marcarlo entregado y devolverlo al mercado cuando la entrega no
//! ocurrió. El lado de la escritura lo tiene el catálogo, porque el
//! catálogo es quien tiene la transacción.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_audit::{
    AuditContext, AuditEntry, AuditError, AuditEvent, AuditOutbox, AuditResult, EntryState,
};
use serde::{Deserialize, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::sql::{Datetime, Thing};

/// La tabla donde esperan los eventos de auditoría.
pub const AUDIT_OUTBOX_TABLE: &str = "catalog_audit_outbox";

/// Escapa una parte de un identificador de registro.
fn escape_id(value: &str) -> String {
    value.replace('~', "~0").replace(':', "~1")
}

/// El identificador de la fila de un evento.
pub fn audit_outbox_id(event_id: &str) -> Thing {
    Thing::from((AUDIT_OUTBOX_TABLE.to_string(), escape_id(event_id)))
}

/// La fila de un evento, como está en la base.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRow {
    /// El identificador del evento.
    pub event_id: String,
    /// La cuenta verificada en la que ocurrió.
    pub tenant: String,
    /// El actor verificado.
    pub actor: String,
    /// La correlación de la operación.
    pub correlation_id: String,
    /// Qué se hizo.
    pub action: String,
    /// Sobre qué se hizo.
    pub resource: String,
    /// El digest del contenido, cuando la operación trata de contenido.
    #[serde(default)]
    pub digest: Option<String>,
    /// Cómo acabó.
    pub result: String,
    /// Cuándo ocurrió.
    pub occurred_at: Datetime,
    /// En qué estado está.
    pub state: String,
    /// Cuántos intentos de entrega lleva.
    pub attempts: i64,
    /// El worker que lo tiene, mientras el lease dure.
    #[serde(default)]
    pub lease_owner: Option<String>,
    /// Cuándo expira el lease.
    #[serde(default)]
    pub lease_until: Option<Datetime>,
    /// Por qué no se entregó, la última vez.
    #[serde(default)]
    pub last_error: Option<String>,
    /// Cuándo se entregó.
    #[serde(default)]
    pub delivered_at: Option<Datetime>,
}

impl AuditRow {
    /// La fila de un evento que se acaba de decir.
    pub fn of(event: &AuditEvent, state: EntryState) -> Self {
        Self {
            event_id: event.event_id.clone(),
            tenant: event.tenant().to_string(),
            actor: event.actor().to_string(),
            correlation_id: event.correlation_id().to_string(),
            action: event.action.clone(),
            resource: event.resource.clone(),
            digest: event.digest.clone(),
            result: event.result.to_string(),
            occurred_at: Datetime::from(event.timestamp),
            state: state.as_str().to_string(),
            attempts: 0,
            lease_owner: None,
            lease_until: None,
            last_error: None,
            delivered_at: None,
        }
    }

    /// El evento de la fila, reconstruido.
    pub fn the_event(&self) -> AuditEvent {
        AuditEvent::new(
            self.event_id.clone(),
            AuditContext::new(
                self.actor.clone(),
                self.tenant.clone(),
                self.correlation_id.clone(),
            ),
            self.occurred_at.clone().into(),
            self.action.clone(),
            self.resource.clone(),
            self.digest.clone(),
            the_result(&self.result),
        )
    }

    /// La entrada de la fila, con su estado.
    pub fn the_entry(&self) -> AuditEntry {
        AuditEntry {
            event: self.the_event(),
            state: the_state(&self.state),
            attempts: u32::try_from(self.attempts).unwrap_or(u32::MAX),
            lease_owner: self.lease_owner.clone(),
            lease_until: self.lease_until.clone().map(|at| at.into()),
            last_error: self.last_error.clone(),
            delivered_at: self.delivered_at.clone().map(|at| at.into()),
        }
    }
}

/// El resultado de un evento, dicho como texto.
///
/// Un resultado que no se reconoce se lee como `failed`: un evento cuyo
/// resultado no se puede leer no es un evento que salió bien, y ante la
/// duda se dice lo conservador.
fn the_result(value: &str) -> AuditResult {
    match value {
        "allowed" => AuditResult::Allowed,
        "denied" => AuditResult::Denied,
        "conflict" => AuditResult::Conflict,
        _ => AuditResult::Failed,
    }
}

/// El estado de una entrada, dicho como texto.
fn the_state(value: &str) -> EntryState {
    match value {
        "claimed" => EntryState::Claimed,
        "delivered" => EntryState::Delivered,
        _ => EntryState::Pending,
    }
}

/// El outbox de auditoría sobre la base del catálogo.
#[derive(Clone)]
pub struct SurrealAuditOutbox {
    db: Surreal<Db>,
}

impl std::fmt::Debug for SurrealAuditOutbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SurrealAuditOutbox").finish_non_exhaustive()
    }
}

impl SurrealAuditOutbox {
    /// El outbox sobre una base.
    pub fn new(db: Surreal<Db>) -> Self {
        Self { db }
    }

    /// La fila de un evento, si está.
    async fn the_row(&self, event_id: &str) -> Result<Option<AuditRow>, AuditError> {
        let mut response = self
            .db
            .query("SELECT * FROM $id")
            .bind(("id", audit_outbox_id(event_id)))
            .await
            .map_err(|error| AuditError::StoreUnavailable {
                detail: error.to_string(),
            })?;
        let rows: Vec<AuditRow> =
            response
                .take(0)
                .map_err(|error| AuditError::StoreUnavailable {
                    detail: error.to_string(),
                })?;
        Ok(rows.into_iter().next())
    }
}

#[async_trait]
impl AuditOutbox for SurrealAuditOutbox {
    async fn put_pending(&self, event: &AuditEvent) -> Result<(), AuditError> {
        let row = AuditRow::of(event, EntryState::Pending);
        let written = self
            .db
            .query("CREATE $id CONTENT $row")
            .bind(("id", audit_outbox_id(&event.event_id)))
            .bind(("row", row))
            .await;

        let failed = match written {
            Ok(response) => response.check().err(),
            Err(error) => Some(error),
        };
        let Some(error) = failed else {
            return Ok(());
        };

        // Decir un evento dos veces es decirlo una. Antes de culpar al
        // almacenamiento se mira lo que hay: puede que el evento ya
        // estuviera dicho, y entonces la pregunta no es "ha fallado" sino
        // "ya estaba".
        if let Some(existing) = self.the_row(&event.event_id).await? {
            if existing.the_event().same_fact(event) {
                return Ok(());
            }
            // El identificador ya está usado por OTRO hecho. No es el mismo
            // evento dicho dos veces: es una contradicción, y callarla
            // dejaría el outbox diciendo algo que no ocurrió.
            return Err(AuditError::StoreUnavailable {
                detail: format!(
                    "the event {} is already said with another content",
                    event.event_id
                ),
            });
        }
        Err(AuditError::StoreUnavailable {
            detail: error.to_string(),
        })
    }

    async fn claim(
        &self,
        event_id: &str,
        worker: &str,
        lease_until: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<Option<AuditEntry>, AuditError> {
        // El compare-and-set es UNA sentencia, y el servidor responde con
        // las filas que movió: dos workers piden lo mismo y el que llega
        // segundo ve que ya no hay nada que reclamar.
        let moved: Vec<AuditRow> = self
            .db
            .query(
                "UPDATE $id SET \
                 state = 'claimed', lease_owner = $owner, lease_until = $until \
                 WHERE state = 'pending' OR (state = 'claimed' AND lease_until <= $now)",
            )
            .bind(("id", audit_outbox_id(event_id)))
            .bind(("owner", worker.to_string()))
            .bind(("until", Datetime::from(lease_until)))
            .bind(("now", Datetime::from(now)))
            .await
            .map_err(|error| AuditError::StoreUnavailable {
                detail: error.to_string(),
            })?
            .take(0)
            .map_err(|error| AuditError::StoreUnavailable {
                detail: error.to_string(),
            })?;

        Ok(moved.into_iter().next().map(|row| row.the_entry()))
    }

    async fn mark_delivered(
        &self,
        event_id: &str,
        worker: &str,
        at: DateTime<Utc>,
    ) -> Result<(), AuditError> {
        // Lo que ya es verdad no se cambia: una entrega que ocurrió no se
        // deshace marcándola otra vez.
        if let Some(row) = self.the_row(event_id).await? {
            if row.state == EntryState::Delivered.as_str() {
                return Ok(());
            }
            if row.state != EntryState::Claimed.as_str() {
                return Err(AuditError::StoreUnavailable {
                    detail: format!("the event {event_id} is not claimed, so nobody delivered it"),
                });
            }
        }

        // El compare-and-set es UNA sentencia, y el servidor responde con
        // las filas que movió. El worker va en la condición: un lease que
        // expiró ya no es un permiso para marcar nada, y sin esto un
        // worker lento podría marcar la entrega de otro.
        let moved: Vec<AuditRow> = self
            .db
            .query(
                "UPDATE $id SET \
                 state = 'delivered', lease_owner = NONE, lease_until = NONE, delivered_at = $at \
                 WHERE state = 'claimed' AND lease_owner = $owner",
            )
            .bind(("id", audit_outbox_id(event_id)))
            .bind(("owner", worker.to_string()))
            .bind(("at", Datetime::from(at)))
            .await
            .map_err(|error| AuditError::StoreUnavailable {
                detail: error.to_string(),
            })?
            .take(0)
            .map_err(|error| AuditError::StoreUnavailable {
                detail: error.to_string(),
            })?;

        if moved.is_empty() {
            return Err(AuditError::StoreUnavailable {
                detail: format!(
                    "the event {event_id} is not claimed by {worker}, so nobody delivered it"
                ),
            });
        }
        Ok(())
    }

    async fn mark_undelivered(
        &self,
        event_id: &str,
        worker: &str,
        detail: &str,
    ) -> Result<(), AuditError> {
        // Una entrega que ya ocurrió no se puede desmentir: marcarla como
        // no entregada sería mentir sobre un hecho.
        if let Some(row) = self.the_row(event_id).await?
            && row.state == EntryState::Delivered.as_str()
        {
            return Ok(());
        }

        // El worker va en la condición por el mismo motivo que en la
        // marca de entregado: si el lease expiró y otro worker lo reclamó,
        // el evento ya no es de este, y devolverlo al mercado le quitaría
        // de debajo al que lo tiene.
        let moved: Vec<AuditRow> = self
            .db
            .query(
                "UPDATE $id SET \
                 state = 'pending', lease_owner = NONE, lease_until = NONE, \
                 attempts = attempts + 1, last_error = $detail \
                 WHERE state = 'claimed' AND lease_owner = $owner",
            )
            .bind(("id", audit_outbox_id(event_id)))
            .bind(("owner", worker.to_string()))
            // El detalle lo escribe el sink, y va a una fila durable: se
            // acota y se limpian los caracteres de control, que en una fila
            // que alguien va a leer son ruido y en un log son inyección.
            .bind(("detail", hodei_audit::sanitize_text(detail)))
            .await
            .map_err(|error| AuditError::StoreUnavailable {
                detail: error.to_string(),
            })?
            .take(0)
            .map_err(|error| AuditError::StoreUnavailable {
                detail: error.to_string(),
            })?;

        if moved.is_empty() {
            return Err(AuditError::StoreUnavailable {
                detail: format!(
                    "the event {event_id} is not claimed by {worker}, so nobody can say it failed"
                ),
            });
        }
        Ok(())
    }

    async fn pending(&self, now: DateTime<Utc>) -> Result<Vec<AuditEntry>, AuditError> {
        let mut response = self
            .db
            .query(
                "SELECT * FROM catalog_audit_outbox WHERE state = 'pending' \
                 OR (state = 'claimed' AND lease_until <= $now)",
            )
            .bind(("now", Datetime::from(now)))
            .await
            .map_err(|error| AuditError::StoreUnavailable {
                detail: error.to_string(),
            })?;
        let rows: Vec<AuditRow> =
            response
                .take(0)
                .map_err(|error| AuditError::StoreUnavailable {
                    detail: error.to_string(),
                })?;
        let mut entries: Vec<AuditEntry> = rows.into_iter().map(|row| row.the_entry()).collect();
        entries.sort_by(|left, right| left.event.event_id.cmp(&right.event.event_id));
        Ok(entries)
    }

    async fn entry(&self, event_id: &str) -> Result<Option<AuditEntry>, AuditError> {
        Ok(self.the_row(event_id).await?.map(|row| row.the_entry()))
    }
}
