//! La auditoría de Hodei: el evento, el sink, el outbox y el dispatcher.
//!
//! Este crate es **un contrato**, no un servicio: no guarda nada y no
//! escribe en ninguna base de datos. Lo que define es de qué manera se
//! dice una mutación crítica, dónde espera el evento hasta que se entrega,
//! y qué significa que la entrega ocurriera.
//!
//! # Las tres piezas y por qué son tres
//!
//! - **El evento** ([`AuditEvent`]) es la afirmación: quién actuó, en qué
//!   cuenta, qué hizo, sobre qué recurso, con qué contenido, cómo acabó y
//!   con qué correlación. No lleva nunca una credencial.
//! - **El sink** ([`AuditSink`]) es a dónde va el evento cuando se
//!   entrega. El sink por defecto escribe campos estructurados en
//!   `tracing`; un sink durable escribe en el almacén de auditoría.
//! - **El outbox** ([`AuditOutbox`]) es dónde **espera** el evento entre
//!   que se dice y que se entrega, con estados que se pueden leer.
//!
//! La separación importa porque las tres fallan de manera distinta: un
//! evento mal formado es un evento que no se puede decir, un sink caído
//! es una entrega que no ocurrió, y un outbox que nadie mira es una
//! entrega que nadie sabe que está pendiente.
//!
//! # El outbox y la frontera única
//!
//! El outbox existe para que el evento y la mutación que lo provoca
//! compartan **una sola frontera de commit**: si la escritura del evento
//! falla, la mutación tampoco ocurrió, y no queda un hecho sin su
//! rastro. Quien publica escribe la fila del outbox **dentro** de la
//! transacción que publica; quien entrega la toma después, y solo la
//! marca entregada cuando el sink lo dice.
//!
//! # La reentrega y el efecto lógico
//!
//! Un evento lleva un `event_id` estable, y ese identificador es la clave
//! de idempotencia: **reentregar un evento es el mismo evento**, no otro.
//! Por eso el contrato del sink exige que trate el `event_id` como clave
//! de idempotencia, y por eso una caída entre la entrega y la marca no
//! duplica el efecto lógico: duplica el envío, que no es lo mismo.

#![forbid(unsafe_code)]

pub mod dispatcher;
pub mod event;
pub mod outbox;
pub mod sink;

pub use dispatcher::{AuditDispatcher, DispatchOutcome, DispatchReport};
pub use event::{AuditContext, AuditEvent, AuditResult};
pub use outbox::{AuditEntry, AuditOutbox, EntryState, InMemoryAuditOutbox};
pub use sink::{AuditError, AuditSink, FailingAuditSink, InMemoryAuditSink, TracingAuditSink};
