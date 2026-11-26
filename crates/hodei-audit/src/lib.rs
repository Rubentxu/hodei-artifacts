//! The audit of Hodei: the event, the sink, the outbox and the dispatcher.
//!
//! This crate is **a contract**, not a service: it stores nothing and does not
//! write to any database. What it defines is how a critical
//! mutation is stated, where the event waits until it is delivered,
//! and what it means for the delivery to have happened.
//!
//! # The three pieces and why there are three
//!
//! - **The event** ([`AuditEvent`]) is the statement: who acted, in which
//!   account, what they did, on which resource, with what content, how it ended and
//!   with what correlation. It never carries a credential.
//! - **The sink** ([`AuditSink`]) is where the event goes when it is
//!   delivered. The default sink writes structured fields to
//!   `tracing`; a durable sink writes to the audit store.
//! - **The outbox** ([`AuditOutbox`]) is where the event **waits** between
//!   being stated and being delivered, with states that can be read.
//!
//! The separation matters because the three fail in different ways: a
//! malformed event is an event that cannot be stated, a sink that is down
//! is a delivery that did not happen, and an outbox that nobody watches is a
//! delivery that nobody knows is pending.
//!
//! # The outbox and the single boundary
//!
//! The outbox exists so that the event and the mutation that causes it
//! share **a single commit boundary**: if writing the event
//! fails, the mutation did not happen either, and there is no fact left without its
//! trail. The publisher writes the outbox row **inside** the
//! transaction that publishes; the deliverer takes it afterwards, and only
//! marks it delivered when the sink says so.
//!
//! # Redelivery and the logical effect
//!
//! An event carries a stable `event_id`, and that identifier is the idempotency
//! key: **redelivering an event is the same event**, not another one.
//! For that reason the sink contract requires it to treat the `event_id` as the idempotency
//! key, and for that reason a crash between delivery and the mark does not
//! duplicate the logical effect: it duplicates the send, which is not the same thing.
//!
//! The chain, stated plainly: the outbox delivers **at least once**,
//! redelivery carries **the same** `event_id`, and the **audit
//! store** stores by that identifier. The logical effect belongs to the
//! store; the outbox sets the condition so that there can be only one.

#![forbid(unsafe_code)]

pub mod dispatcher;
pub mod event;
pub mod outbox;
pub mod sink;

pub use dispatcher::{AuditDispatcher, DispatchOutcome, DispatchReport};
pub use event::{AuditContext, AuditEvent, AuditResult, sanitize_text};
pub use outbox::{AuditEntry, AuditOutbox, EntryState, InMemoryAuditOutbox};
pub use sink::{AuditError, AuditSink, FailingAuditSink, InMemoryAuditSink, TracingAuditSink};
