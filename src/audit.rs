//! Minimal structured audit events for security-relevant operations.
//!
//! The event contains identifiers and the decision, never credentials. The
//! default sink writes structured `tracing` fields; it is best-effort, not a
//! durable audit store. `AuditSink` is the seam a future Audit Trail adapter
//! implements without changing authorization or create rules.
//!
//! The contract itself lives in the `hodei-audit` crate, because it is not
//! only this application that speaks it: the catalog writes the same event
//! into the same outbox, and a contract with two definitions is a contract
//! that can have two shapes. This module is the door that the application
//! already had, and it keeps pointing at the same types.

pub use hodei_audit::{
    AuditContext, AuditError, AuditEvent, AuditResult, AuditSink, FailingAuditSink,
    InMemoryAuditSink, TracingAuditSink,
};
