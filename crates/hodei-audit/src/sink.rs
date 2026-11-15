//! El sink: a dónde va el evento cuando se entrega.
//!
//! El sink es la **frontera de entrega**. Lo que hay al otro lado puede
//! ser un `tracing` (que se pierde con el proceso) o un almacén durable
//! (que no). El contrato no promete ninguna de las dos cosas: promete que
//! el sink dice si la entrega ocurrió, y por eso un sink caído se puede
//! distinguir de una entrega que sí ocurrió.
//!
//! # La idempotencia es del contrato
//!
//! Un evento puede llegar dos veces: una caída entre la entrega y la
//! marca deja el evento pendiente y vuelve a entregarlo. Por eso el
//! `event_id` es la clave de idempotencia, y el sink **debe** tratarlo
//! como tal: entregar dos veces el mismo evento es un efecto lógico, no
//! dos. Un sink que no deduplica no cumple el contrato, y el dispatcher
//! no puede saber si lo cumple.

use std::sync::{Arc, Mutex};

use thiserror::Error;

use crate::event::AuditEvent;

/// Lo que puede fallar en la frontera de auditoría.
///
/// Los dos fallos son distintos y se distinguen: el sink caído es una
/// entrega que **no** ocurrió, y el almacén que no responde es un hecho
/// que **no se pudo comprobar**. Ninguno de los dos es una entrega.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AuditError {
    /// El sink no pudo registrar el evento.
    #[error("audit sink unavailable")]
    Unavailable,
    /// El almacén del outbox no pudo responder.
    #[error("the audit outbox is unavailable: {detail}")]
    StoreUnavailable {
        /// Lo que dijo el almacén.
        detail: String,
    },
}

/// A dónde va un evento de auditoría.
///
/// Las implementaciones **no reciben credenciales**: el evento ya no las
/// lleva, y una implementación que las pidiera las pediría por otro
/// camino.
///
/// El contrato exige dos cosas:
///
/// - **Decir si la entrega ocurrió.** Un `Err` significa que no ocurrió, y
///   nunca que "quizá": quien recibe el error tiene que poder dejar el
///   evento pendiente.
/// - **Tratar el `event_id` como clave de idempotencia.** Entregar dos
///   veces el mismo evento es un efecto lógico.
pub trait AuditSink: Send + Sync {
    /// Registra el evento.
    fn record(&self, event: &AuditEvent) -> Result<(), AuditError>;
}

/// El sink por defecto: campos estructurados en `tracing`.
///
/// Es **best effort**: se pierde con el proceso. Quien necesite durabilidad
/// inyecta otro sink; la frontera es la misma.
pub struct TracingAuditSink;

impl AuditSink for TracingAuditSink {
    fn record(&self, event: &AuditEvent) -> Result<(), AuditError> {
        tracing::info!(
            audit.event_id = %event.event_id,
            audit.correlation_id = %event.correlation_id(),
            audit.timestamp = %event.timestamp,
            audit.actor = %event.actor(),
            audit.tenant = %event.tenant(),
            audit.action = %event.action,
            audit.resource = %event.resource,
            audit.digest = ?event.digest,
            audit.result = %event.result,
            "security audit event"
        );
        Ok(())
    }
}

/// El sink en memoria, para pruebas y verificación local.
///
/// Deduplica por `event_id`: entregar dos veces el mismo evento es un
/// evento. Es lo que hace que "una reentrega produce exactamente un efecto
/// lógico" sea una **medida** y no una promesa.
#[derive(Debug, Clone, Default)]
pub struct InMemoryAuditSink {
    events: Arc<Mutex<Vec<AuditEvent>>>,
}

impl InMemoryAuditSink {
    /// Un sink sin nada en él.
    pub fn new() -> Self {
        Self::default()
    }

    /// Los eventos registrados, en el orden en que se registraron.
    pub fn events(&self) -> Vec<AuditEvent> {
        self.events.lock().expect("audit lock").clone()
    }

    /// Cuántos eventos hay.
    pub fn len(&self) -> usize {
        self.events.lock().expect("audit lock").len()
    }

    /// Si no hay ningún evento.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl AuditSink for InMemoryAuditSink {
    fn record(&self, event: &AuditEvent) -> Result<(), AuditError> {
        let mut events = self.events.lock().expect("audit lock");
        // El mismo evento dos veces es el mismo evento.
        if events.iter().any(|seen| seen.event_id == event.event_id) {
            return Ok(());
        }
        events.push(event.clone());
        Ok(())
    }
}

/// El sink que falla; sirve para probar que un fallo de auditoría no
/// corrompe la decisión y que el evento **no** se da por entregado.
#[derive(Debug, Clone, Default)]
pub struct FailingAuditSink {
    attempts: Arc<std::sync::atomic::AtomicUsize>,
}

impl FailingAuditSink {
    /// Un sink que falla siempre.
    pub fn new() -> Self {
        Self::default()
    }

    /// Cuántos intentos ha recibido.
    pub fn attempts(&self) -> usize {
        self.attempts.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl AuditSink for FailingAuditSink {
    fn record(&self, _event: &AuditEvent) -> Result<(), AuditError> {
        self.attempts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(AuditError::Unavailable)
    }
}
