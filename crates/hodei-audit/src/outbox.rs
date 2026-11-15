//! El outbox: dónde espera el evento entre que se dice y que se entrega.
//!
//! El outbox es la pieza que hace que el evento y la mutación que lo
//! provoca compartan **una sola frontera de commit**. Quien publica escribe
//! la fila del outbox dentro de la misma transacción que publica; quien
//! entrega la toma después.
//!
//! # Los estados son explícitos, porque un estado adivinado es un estado
//! que miente
//!
//! - `pending`: dicho, esperando a que alguien lo lleve.
//! - `claimed`: un worker lo tiene, con un **lease**. Cuando el lease
//!   expira, el evento vuelve a ser reclamable: es lo que hace que la
//!   muerte de un worker no pierda el evento.
//! - `delivered`: el sink dijo que la entrega ocurrió.
//!
//! No hay un estado "entregado" para lo que no se entregó. Un evento cuyo
//! sink falló vuelve a `pending` con su contador de intentos y su último
//! error: **observable**, que es lo que permite reconciliarlo.
//!
//! # El claim es un compare-and-set
//!
//! Dos workers piden el mismo evento y **uno** gana. El que llega
//! segundo ve que ya no hay nada que reclamar, y no entrega: una
//! reentrega concurrente no son dos entregas.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::event::AuditEvent;
use crate::sink::AuditError;

/// En qué estado está un evento en el outbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryState {
    /// Dicho, esperando.
    Pending,
    /// Reclamado por un worker, con un lease.
    Claimed,
    /// Entregado: el sink lo dijo.
    Delivered,
}

impl EntryState {
    /// El nombre del estado, para el texto y para la base.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Claimed => "claimed",
            Self::Delivered => "delivered",
        }
    }
}

/// Un evento en el outbox, con su estado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    /// El evento.
    pub event: AuditEvent,
    /// En qué estado está.
    pub state: EntryState,
    /// Cuántos intentos de entrega lleva.
    pub attempts: u32,
    /// El worker que lo tiene, mientras el lease dure.
    pub lease_owner: Option<String>,
    /// Cuándo expira el lease.
    pub lease_until: Option<DateTime<Utc>>,
    /// Por qué no se entregó, la última vez.
    pub last_error: Option<String>,
    /// Cuándo se entregó.
    pub delivered_at: Option<DateTime<Utc>>,
}

impl AuditEntry {
    /// Una entrada pendiente, recién dicha.
    pub fn pending(event: AuditEvent) -> Self {
        Self {
            event,
            state: EntryState::Pending,
            attempts: 0,
            lease_owner: None,
            lease_until: None,
            last_error: None,
            delivered_at: None,
        }
    }
}

/// El outbox de auditoría.
///
/// El puerto no dice "guarda el evento": dice **qué le pasa**. Quien lo
/// implementa decide dónde se guarda; lo que no puede cambiar es lo que
/// el puerto promete:
///
/// - `put_pending` dice un evento, y decirlo dos veces es decirlo una.
/// - `claim` es un compare-and-set: dos workers, uno gana.
/// - `mark_delivered` marca lo que el sink dijo que entregó.
/// - `mark_undelivered` devuelve el evento al mercado con su error y su
///   intento contado.
/// - `pending` deja **ver** lo que está esperando.
#[async_trait]
pub trait AuditOutbox: Send + Sync {
    /// Dice el evento: entra como `pending`.
    ///
    /// Decirlo dos veces es decirlo una: una entrada que ya está entregada
    /// no vuelve a pendiente, porque el hecho de que la entrega ocurriera
    /// no se deshace por volver a decir el evento.
    async fn put_pending(&self, event: &AuditEvent) -> Result<(), AuditError>;

    /// Reclama el evento: de `pending`, o de un lease que expiró.
    ///
    /// Devuelve `None` cuando otro worker se lo llevó antes, o cuando el
    /// evento no está en el outbox.
    async fn claim(
        &self,
        event_id: &str,
        worker: &str,
        lease_until: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<Option<AuditEntry>, AuditError>;

    /// Marca el evento como entregado.
    ///
    /// Solo desde `claimed` **y solo por el worker que lo tiene**: una
    /// marca de una entrada que nadie reclamó es una marca que nadie
    /// puede explicar, y una marca de un worker cuyo lease ya expiró (y
    /// que otro reclamó mientras tanto) es una marca de una entrega que
    /// puede no ser la que ocurrió.
    async fn mark_delivered(
        &self,
        event_id: &str,
        worker: &str,
        at: DateTime<Utc>,
    ) -> Result<(), AuditError>;

    /// Devuelve el evento al mercado: la entrega no ocurrió.
    ///
    /// El intento queda contado y el error queda escrito: un evento que no
    /// se entrega tiene que **poder mirarse**. Solo el worker que lo tiene
    /// puede devolverlo: si su lease expiró y otro worker lo reclamó, el
    /// evento ya no es suyo.
    async fn mark_undelivered(
        &self,
        event_id: &str,
        worker: &str,
        detail: &str,
    ) -> Result<(), AuditError>;

    /// Los eventos que están esperando: los `pending` y los `claimed` cuyo
    /// lease expiró.
    async fn pending(&self, now: DateTime<Utc>) -> Result<Vec<AuditEntry>, AuditError>;

    /// La entrada de un evento, para el que pregunta qué fue de él.
    async fn entry(&self, event_id: &str) -> Result<Option<AuditEntry>, AuditError>;
}

/// El outbox en memoria: el material de referencia del puerto.
///
/// Lo que el almacén durable guarda en SurrealDB, esto lo guarda en el
/// proceso, con las **mismas** reglas: el mismo compare-and-set, el mismo
/// lease, el mismo "decirlo dos veces es decirlo una".
#[derive(Debug, Clone, Default)]
pub struct InMemoryAuditOutbox {
    inner: Arc<Mutex<HashMap<String, AuditEntry>>>,
}

impl InMemoryAuditOutbox {
    /// Un outbox sin nada en él.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Los estados de todas las entradas, ordenados por identificador.
    pub fn states(&self) -> Vec<(String, String)> {
        let mut states: Vec<(String, String)> = self
            .inner
            .lock()
            .expect("the audit outbox is not poisoned")
            .iter()
            .map(|(event_id, entry)| (event_id.clone(), entry.state.as_str().to_string()))
            .collect();
        states.sort();
        states
    }
}

#[async_trait]
impl AuditOutbox for InMemoryAuditOutbox {
    async fn put_pending(&self, event: &AuditEvent) -> Result<(), AuditError> {
        let mut inner = self.inner.lock().expect("the audit outbox is not poisoned");
        // Decirlo dos veces es decirlo una: si ya está, no se toca.
        inner
            .entry(event.event_id.clone())
            .or_insert_with(|| AuditEntry::pending(event.clone()));
        Ok(())
    }

    async fn claim(
        &self,
        event_id: &str,
        worker: &str,
        lease_until: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<Option<AuditEntry>, AuditError> {
        let mut inner = self.inner.lock().expect("the audit outbox is not poisoned");
        let Some(entry) = inner.get_mut(event_id) else {
            return Ok(None);
        };
        // El compare-and-set: pendiente, o un lease que murió.
        let claimable = entry.state == EntryState::Pending
            || (entry.state == EntryState::Claimed
                && entry.lease_until.is_some_and(|until| until <= now));
        if !claimable {
            return Ok(None);
        }
        entry.state = EntryState::Claimed;
        entry.lease_owner = Some(worker.to_string());
        entry.lease_until = Some(lease_until);
        Ok(Some(entry.clone()))
    }

    async fn mark_delivered(
        &self,
        event_id: &str,
        worker: &str,
        at: DateTime<Utc>,
    ) -> Result<(), AuditError> {
        let mut inner = self.inner.lock().expect("the audit outbox is not poisoned");
        let Some(entry) = inner.get_mut(event_id) else {
            return Err(AuditError::StoreUnavailable {
                detail: format!("the event {event_id} is not in the outbox"),
            });
        };
        // Lo que ya es verdad no se cambia: la entrega que ocurrió no se
        // deshace por marcarla otra vez.
        if entry.state == EntryState::Delivered {
            return Ok(());
        }
        if entry.state != EntryState::Claimed {
            return Err(AuditError::StoreUnavailable {
                detail: format!("the event {event_id} is not claimed, so nobody delivered it"),
            });
        }
        // Y solo el que lo tiene: un lease que expiró ya no es un permiso
        // para marcar nada.
        if entry.lease_owner.as_deref() != Some(worker) {
            return Err(AuditError::StoreUnavailable {
                detail: format!(
                    "the event {event_id} is claimed by another worker, so {worker} did not deliver it"
                ),
            });
        }
        entry.state = EntryState::Delivered;
        entry.lease_owner = None;
        entry.lease_until = None;
        entry.delivered_at = Some(at);
        Ok(())
    }

    async fn mark_undelivered(
        &self,
        event_id: &str,
        worker: &str,
        detail: &str,
    ) -> Result<(), AuditError> {
        let mut inner = self.inner.lock().expect("the audit outbox is not poisoned");
        let Some(entry) = inner.get_mut(event_id) else {
            return Err(AuditError::StoreUnavailable {
                detail: format!("the event {event_id} is not in the outbox"),
            });
        };
        // Una entrega que ya ocurrió no se puede desmentir: marcarla como
        // no entregada sería mentir sobre un hecho.
        if entry.state == EntryState::Delivered {
            return Ok(());
        }
        if entry.state != EntryState::Claimed {
            return Err(AuditError::StoreUnavailable {
                detail: format!(
                    "the event {event_id} was not claimed, so nobody can say it failed"
                ),
            });
        }
        // Y solo el que lo tiene: si su lease expiró y otro worker lo
        // reclamó, el evento ya no es suyo y devolverlo al mercado sería
        // sacar de debajo al worker que lo tiene.
        if entry.lease_owner.as_deref() != Some(worker) {
            return Err(AuditError::StoreUnavailable {
                detail: format!(
                    "the event {event_id} is claimed by another worker, so {worker} cannot return it"
                ),
            });
        }
        entry.state = EntryState::Pending;
        entry.lease_owner = None;
        entry.lease_until = None;
        entry.attempts = entry.attempts.saturating_add(1);
        entry.last_error = Some(detail.to_string());
        Ok(())
    }

    async fn pending(&self, now: DateTime<Utc>) -> Result<Vec<AuditEntry>, AuditError> {
        let inner = self.inner.lock().expect("the audit outbox is not poisoned");
        let mut entries: Vec<AuditEntry> = inner
            .values()
            .filter(|entry| {
                entry.state == EntryState::Pending
                    || (entry.state == EntryState::Claimed
                        && entry.lease_until.is_some_and(|until| until <= now))
            })
            .cloned()
            .collect();
        entries.sort_by(|left, right| left.event.event_id.cmp(&right.event.event_id));
        Ok(entries)
    }

    async fn entry(&self, event_id: &str) -> Result<Option<AuditEntry>, AuditError> {
        let inner = self.inner.lock().expect("the audit outbox is not poisoned");
        Ok(inner.get(event_id).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{AuditContext, AuditResult};

    fn the_event(event_id: &str) -> AuditEvent {
        AuditEvent::new(
            event_id.to_string(),
            AuditContext::new("hrn:hodei:iam:acme:User/alice", "acme", "corr-1"),
            DateTime::from_timestamp(1_700_000_000, 0).expect("the moment"),
            "publish",
            "raw/app/one.bin",
            Some("sha256:00".to_string()),
            AuditResult::Allowed,
        )
    }

    fn the_moment() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).expect("the moment")
    }

    #[tokio::test]
    async fn saying_an_event_twice_is_saying_it_once() {
        let outbox = InMemoryAuditOutbox::empty();
        outbox.put_pending(&the_event("e-1")).await.expect("put");
        outbox.put_pending(&the_event("e-1")).await.expect("put");
        assert_eq!(
            outbox.states(),
            vec![("e-1".to_string(), "pending".to_string())]
        );
    }

    #[tokio::test]
    async fn the_claim_is_a_compare_and_set() {
        let outbox = InMemoryAuditOutbox::empty();
        outbox.put_pending(&the_event("e-1")).await.expect("put");
        let until = the_moment() + chrono::Duration::minutes(1);

        let first = outbox
            .claim("e-1", "worker-1", until, the_moment())
            .await
            .expect("claim");
        let second = outbox
            .claim("e-1", "worker-2", until, the_moment())
            .await
            .expect("claim");

        assert!(first.is_some(), "the first worker takes the event");
        assert!(second.is_none(), "the second worker does not");
    }

    #[tokio::test]
    async fn an_expired_lease_can_be_claimed_again() {
        let outbox = InMemoryAuditOutbox::empty();
        outbox.put_pending(&the_event("e-1")).await.expect("put");
        let until = the_moment() + chrono::Duration::minutes(1);
        outbox
            .claim("e-1", "worker-1", until, the_moment())
            .await
            .expect("claim");

        let later = the_moment() + chrono::Duration::minutes(2);
        let reclaimed = outbox
            .claim(
                "e-1",
                "worker-2",
                later + chrono::Duration::minutes(1),
                later,
            )
            .await
            .expect("claim");

        assert!(
            reclaimed.is_some(),
            "a dead worker's event goes back to the market"
        );
    }

    #[tokio::test]
    async fn an_event_that_nobody_claimed_cannot_be_marked_as_delivered() {
        let outbox = InMemoryAuditOutbox::empty();
        outbox.put_pending(&the_event("e-1")).await.expect("put");
        assert!(
            outbox
                .mark_delivered("e-1", "worker-1", the_moment())
                .await
                .is_err(),
            "a delivery that nobody claimed cannot be marked"
        );
    }

    #[tokio::test]
    async fn a_worker_whose_lease_expired_cannot_mark_the_event() {
        let outbox = InMemoryAuditOutbox::empty();
        outbox.put_pending(&the_event("e-1")).await.expect("put");
        let until = the_moment() + chrono::Duration::minutes(1);
        outbox
            .claim("e-1", "worker-1", until, the_moment())
            .await
            .expect("claim");

        // El lease muere y otro worker lo toma: el primero ya no puede
        // decir nada de él.
        let later = the_moment() + chrono::Duration::minutes(2);
        outbox
            .claim(
                "e-1",
                "worker-2",
                later + chrono::Duration::minutes(1),
                later,
            )
            .await
            .expect("claim");

        assert!(
            outbox
                .mark_delivered("e-1", "worker-1", later)
                .await
                .is_err(),
            "the worker that lost the lease cannot mark a delivery"
        );
        assert!(
            outbox
                .mark_undelivered("e-1", "worker-1", "the sink is down")
                .await
                .is_err(),
            "the worker that lost the lease cannot return the event"
        );
    }

    #[tokio::test]
    async fn an_event_that_was_not_delivered_goes_back_to_pending_with_its_error() {
        let outbox = InMemoryAuditOutbox::empty();
        outbox.put_pending(&the_event("e-1")).await.expect("put");
        let until = the_moment() + chrono::Duration::minutes(1);
        outbox
            .claim("e-1", "worker-1", until, the_moment())
            .await
            .expect("claim");
        outbox
            .mark_undelivered("e-1", "worker-1", "the sink is down")
            .await
            .expect("mark");

        let entry = outbox.entry("e-1").await.expect("entry").expect("entry");
        assert_eq!(entry.state, EntryState::Pending);
        assert_eq!(entry.attempts, 1);
        assert_eq!(entry.last_error.as_deref(), Some("the sink is down"));
        assert!(outbox.pending(the_moment()).await.expect("pending").len() == 1);
    }

    #[tokio::test]
    async fn a_delivered_event_is_not_pending_anymore() {
        let outbox = InMemoryAuditOutbox::empty();
        outbox.put_pending(&the_event("e-1")).await.expect("put");
        let until = the_moment() + chrono::Duration::minutes(1);
        outbox
            .claim("e-1", "worker-1", until, the_moment())
            .await
            .expect("claim");
        outbox
            .mark_delivered("e-1", "worker-1", the_moment())
            .await
            .expect("mark");

        assert!(
            outbox
                .pending(the_moment())
                .await
                .expect("pending")
                .is_empty()
        );
        let entry = outbox.entry("e-1").await.expect("entry").expect("entry");
        assert_eq!(entry.state, EntryState::Delivered);
        assert!(entry.delivered_at.is_some());
    }
}
