//! El dispatcher: quien lleva el evento del outbox al sink.
//!
//! El dispatcher es el único que toca el sink, y hace tres cosas en un
//! orden que no se puede cambiar:
//!
//! 1. **Reclama** el evento (compare-and-set: si otro worker se lo llevó,
//!    no se entrega).
//! 2. **Entrega** al sink.
//! 3. **Marca** lo entregado, **solo si el sink lo dijo**.
//!
//! # Lo que el dispatcher nunca dice
//!
//! Nunca dice "entregado" por una entrega que no ocurrió. El sink puede
//! fallar, y cuando falla el evento vuelve al mercado con su error y su
//! intento contado. Y hay un tercer caso, el menos cómodo: el sink dijo
//! que sí, pero la marca no se pudo escribir. Ahí la entrega **ocurrió** y
//! el outbox **no lo sabe**: eso no es "entregado", es "incierto", y se
//! dice como tal. La reconciliación lo resuelve: el `event_id` hace que
//! la reentrega sea el mismo evento.

use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::outbox::AuditOutbox;
use crate::sink::{AuditError, AuditSink};

/// Lo que pasó con un evento en una ronda de entrega.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchOutcome {
    /// El sink lo dijo y la marca se escribió.
    Delivered,
    /// El sink no lo dijo: el evento vuelve a estar pendiente.
    NotDelivered {
        /// Por qué.
        detail: String,
    },
    /// El sink lo dijo, pero la marca no se pudo escribir: nadie sabe si
    /// el outbox lo sabe.
    Uncertain {
        /// Lo que falló al marcar.
        detail: String,
    },
}

/// Lo que pasó en una ronda de entrega.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DispatchReport {
    /// Los eventos que quedaron marcados como entregados.
    pub delivered: Vec<String>,
    /// Los eventos que no se pudieron entregar, con el motivo.
    pub not_delivered: Vec<(String, String)>,
    /// Los eventos cuya entrega ocurrió y cuya marca no se pudo escribir.
    pub uncertain: Vec<(String, String)>,
}

impl DispatchReport {
    /// Cuántos eventos se entregaron.
    pub fn delivered(&self) -> usize {
        self.delivered.len()
    }

    /// Si no pasó nada en la ronda.
    pub fn is_empty(&self) -> bool {
        self.delivered.is_empty() && self.not_delivered.is_empty() && self.uncertain.is_empty()
    }
}

/// Quien lleva los eventos del outbox al sink.
pub struct AuditDispatcher {
    outbox: Arc<dyn AuditOutbox>,
    sink: Arc<dyn AuditSink>,
    lease: Duration,
}

impl AuditDispatcher {
    /// Un dispatcher sobre un outbox y un sink.
    pub fn new(outbox: Arc<dyn AuditOutbox>, sink: Arc<dyn AuditSink>) -> Self {
        Self {
            outbox,
            sink,
            lease: Duration::from_secs(30),
        }
    }

    /// El dispatcher con otro lease.
    ///
    /// El lease es cuánto tiempo un worker puede tener un evento sin
    /// entregarlo: cuando expira, el evento vuelve al mercado y otro
    /// worker puede tomarlo. Un lease corto hace que un worker muerto no
    /// retenga el evento mucho tiempo; uno largo hace que un evento no se
    /// entregue dos veces por el camino.
    pub fn with_lease(mut self, lease: Duration) -> Self {
        self.lease = lease;
        self
    }

    /// El lease que usa el dispatcher.
    pub fn lease(&self) -> Duration {
        self.lease
    }

    /// Entrega lo que está esperando.
    ///
    /// Da una vuelta completa: reclama cada pendiente, lo entrega y lo
    /// marca. Un evento que otro worker tiene en este momento no se
    /// entrega, y eso no es un fallo: es el compare-and-set haciendo su
    /// trabajo.
    pub async fn dispatch_pending(
        &self,
        worker: &str,
        now: DateTime<Utc>,
    ) -> Result<DispatchReport, AuditError> {
        let mut report = DispatchReport::default();
        let waiting = self.outbox.pending(now).await?;
        let lease_until = now
            + chrono::Duration::from_std(self.lease)
                .unwrap_or_else(|_| chrono::Duration::seconds(30));

        for entry in waiting {
            let Some(claimed) = self
                .outbox
                .claim(&entry.event.event_id, worker, lease_until, now)
                .await?
            else {
                // Otro worker se lo llevó: no se entrega dos veces.
                continue;
            };

            match self.sink.record(&claimed.event) {
                Ok(()) => match self
                    .outbox
                    .mark_delivered(&claimed.event.event_id, worker, now)
                    .await
                {
                    Ok(()) => report.delivered.push(claimed.event.event_id.clone()),
                    Err(error) => report
                        .uncertain
                        .push((claimed.event.event_id.clone(), error.to_string())),
                },
                Err(error) => {
                    let mut detail = error.to_string();
                    // La entrega NO ocurrió: el evento vuelve al mercado con
                    // su error y su intento contado. Marcarlo como entregado
                    // sería mentir sobre un hecho.
                    if let Err(mark_error) = self
                        .outbox
                        .mark_undelivered(&claimed.event.event_id, worker, &detail)
                        .await
                    {
                        // Ni siquiera se pudo devolver al mercado: el evento
                        // se queda reclamado hasta que expire su lease. Se
                        // dice, porque un evento atascado es un hecho que
                        // hay que poder mirar.
                        detail = format!(
                            "{detail} (and it could not be returned to the market: {mark_error})"
                        );
                    }
                    report
                        .not_delivered
                        .push((claimed.event.event_id.clone(), detail));
                }
            }
        }

        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{AuditContext, AuditResult};
    use crate::outbox::InMemoryAuditOutbox;
    use crate::sink::{FailingAuditSink, InMemoryAuditSink};

    fn the_event(event_id: &str) -> crate::event::AuditEvent {
        crate::event::AuditEvent::new(
            event_id.to_string(),
            AuditContext::new("hrn:hodei:iam:acme:User/alice", "acme", "corr-1"),
            chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("the moment"),
            "publish",
            "raw/app/one.bin",
            Some("sha256:00".to_string()),
            AuditResult::Allowed,
        )
    }

    fn the_moment() -> chrono::DateTime<Utc> {
        chrono::DateTime::from_timestamp(1_700_000_000, 0).expect("the moment")
    }

    #[tokio::test]
    async fn the_dispatcher_delivers_the_pending_and_marks_it_delivered() {
        let outbox = Arc::new(InMemoryAuditOutbox::empty());
        outbox.put_pending(&the_event("e-1")).await.expect("put");
        let sink = Arc::new(InMemoryAuditSink::new());
        let dispatcher = AuditDispatcher::new(outbox.clone(), sink.clone());

        let report = dispatcher
            .dispatch_pending("worker-1", the_moment())
            .await
            .expect("dispatch");

        assert_eq!(report.delivered(), 1);
        assert!(report.not_delivered.is_empty());
        assert_eq!(sink.len(), 1);
        let entry = outbox.entry("e-1").await.expect("entry").expect("entry");
        assert_eq!(entry.state, crate::outbox::EntryState::Delivered);
    }

    #[tokio::test]
    async fn a_sink_that_fails_leaves_the_event_pending_and_visible() {
        let outbox = Arc::new(InMemoryAuditOutbox::empty());
        outbox.put_pending(&the_event("e-1")).await.expect("put");
        let sink = Arc::new(FailingAuditSink::new());
        let dispatcher = AuditDispatcher::new(outbox.clone(), sink.clone());

        let report = dispatcher
            .dispatch_pending("worker-1", the_moment())
            .await
            .expect("dispatch");

        assert_eq!(report.delivered(), 0);
        assert_eq!(report.not_delivered.len(), 1);
        let entry = outbox.entry("e-1").await.expect("entry").expect("entry");
        assert_eq!(entry.state, crate::outbox::EntryState::Pending);
        assert_eq!(entry.attempts, 1);
        assert!(entry.last_error.is_some());
    }

    #[tokio::test]
    async fn a_redelivery_is_one_logical_effect() {
        let outbox = Arc::new(InMemoryAuditOutbox::empty());
        outbox.put_pending(&the_event("e-1")).await.expect("put");
        let sink = Arc::new(InMemoryAuditSink::new());
        let dispatcher = AuditDispatcher::new(outbox.clone(), sink.clone());

        // La caída: el sink recibió el evento, pero la marca no se escribió.
        let claimed = outbox
            .claim(
                "e-1",
                "worker-1",
                the_moment() + chrono::Duration::minutes(1),
                the_moment(),
            )
            .await
            .expect("claim")
            .expect("the claim");
        sink.record(&claimed.event).expect("the sink received it");

        // El lease expira y el evento vuelve al mercado.
        let later = the_moment() + chrono::Duration::minutes(10);
        let report = dispatcher
            .dispatch_pending("worker-2", later)
            .await
            .expect("dispatch");

        assert_eq!(report.delivered(), 1);
        assert_eq!(
            sink.len(),
            1,
            "the same event delivered twice is one logical effect"
        );
    }
}
