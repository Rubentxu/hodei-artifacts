//! The idempotent operation registry: an operation is applied once, and
//! only once, no matter how many times it is retried.
//!
//! Why: the network does not deliver ACKs. A transaction that actually
//! committed can be reported to the caller as a timeout; a partial
//! failure can be re-driven by an orchestrator; the same logical write
//! can arrive twice through different paths. Without a ledger, every
//! retry risks a duplicate effect. With one:
//!
//! - every guarded write carries a caller-chosen [`OperationId`];
//! - the executor consults the registry BEFORE applying: a committed id
//!   returns its recorded result (a replay), never a second application;
//! - the marker is written in the SAME transaction as the effect, so
//!   "effect committed" and "marker committed" are one atomic fact —
//!   there is no window where the effect exists without its marker;
//! - the `@upsert` index on `hodei.operation.id` forces the server-side
//!   conflict if two transactions race to commit the same id: snapshot
//!   isolation is not serialization, so once-only is decided by the
//!   server, not by the pre-read.
//!
//! The registry never guesses: an unknown id is simply not found, and a
//! stored record that contradicts the contract is corruption (typed
//! error, fail closed). The id is the caller's choice — a UUID or any
//! printable-ASCII token without the composite-key separator — and the
//! registry treats it as globally unique per graph.

use serde_json::Value;

use crate::identity::KEY_SEPARATOR;

/// The type tag of operation nodes in the graph.
pub const OPERATION_TYPE: &str = "HodeiOperation";

/// The maximum length of an operation id.
const MAX_OPERATION_ID_LEN: usize = 128;

/// How the operation registry fails.
#[derive(Debug, thiserror::Error)]
pub enum OperationError {
    /// The proposed id is not a safe registry key: it must be non-empty
    /// printable ASCII (no whitespace, no control characters) without
    /// the composite-key separator, at most [`MAX_OPERATION_ID_LEN`]
    /// bytes.
    #[error("the operation id '{id}' is not valid: {reason}")]
    InvalidId {
        /// The id as received.
        id: String,
        /// Why it was rejected.
        reason: &'static str,
    },
    /// A stored record contradicts the registry contract (duplicated id,
    /// stored id mismatch, ...): a real invariant break, never repaired
    /// silently.
    #[error("the operation record of '{operation_id}' contradicts the registry contract: {detail}")]
    CorruptRecord {
        /// The operation id involved.
        operation_id: String,
        /// What contradicts the contract.
        detail: String,
    },
}

/// A caller-chosen identifier that makes an operation once-only. Two
/// calls that carry the same id are the SAME operation: the second one
/// replays the first one's committed result instead of applying the
/// effect again.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OperationId(String);

impl OperationId {
    /// Validates and wraps an operation id. Any non-empty printable
    /// ASCII token without the composite-key separator is accepted (a
    /// UUID is the recommended shape); what matters is that the caller
    /// never reuses an id for a different operation.
    pub fn new(id: &str) -> Result<Self, OperationError> {
        if id.is_empty() {
            return Err(OperationError::InvalidId {
                id: id.to_owned(),
                reason: "it is empty",
            });
        }
        if id.len() > MAX_OPERATION_ID_LEN {
            return Err(OperationError::InvalidId {
                id: id.to_owned(),
                reason: "it exceeds the maximum id length",
            });
        }
        if !id
            .chars()
            .all(|c| c.is_ascii_graphic() && c != KEY_SEPARATOR)
        {
            return Err(OperationError::InvalidId {
                id: id.to_owned(),
                reason: "it must be printable ASCII without the key separator",
            });
        }
        Ok(Self(id.to_owned()))
    }

    /// The validated operation id text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The committed record of an operation, read back from the registry.
/// This is what a replay returns: the result that is already committed,
/// not a recomputation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationRecord {
    /// The operation id the record is keyed by.
    pub operation_id: String,
    /// The scope key the operation ran under.
    pub scope: String,
    /// The result the operation recorded when it committed.
    pub result: String,
    /// When the operation committed (RFC 3339).
    pub committed_at: String,
}

/// The replay decision: the recorded result of an already-committed
/// operation, or `None` when the operation has never committed and its
/// effect must be applied by the caller. Pure and deterministic: the
/// same committed record replays the SAME result every time.
pub fn replayed_result(record: Option<&OperationRecord>) -> Option<String> {
    record.map(|record| record.result.clone())
}

/// Parses one operation record out of an upsert/query answer. Refuses
/// duplicated ids and id mismatches: the once-only contract is checked
/// on the way out, not only on the way in.
pub fn parse_operation(
    json: &Value,
    root: &str,
    operation_id: &str,
) -> Result<Option<OperationRecord>, OperationError> {
    let nodes = json
        .get(root)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    match nodes.len() {
        0 => Ok(None),
        1 => {
            let node = &nodes[0];
            let stored_id = node
                .get("hodei.operation.id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if !stored_id.is_empty() && stored_id != operation_id {
                return Err(OperationError::CorruptRecord {
                    operation_id: operation_id.to_owned(),
                    detail: format!("the node stores the id '{stored_id}'"),
                });
            }
            let scope = node
                .get("hodei.operation.scope")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            if scope.is_empty() {
                return Err(OperationError::CorruptRecord {
                    operation_id: operation_id.to_owned(),
                    detail: "the node has no scope".to_owned(),
                });
            }
            let result = node
                .get("hodei.operation.result")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let committed_at = node
                .get("hodei.operation.committed_at")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            Ok(Some(OperationRecord {
                operation_id: if stored_id.is_empty() {
                    operation_id.to_owned()
                } else {
                    stored_id
                },
                scope,
                result,
                committed_at,
            }))
        }
        n => Err(OperationError::CorruptRecord {
            operation_id: operation_id.to_owned(),
            detail: format!("{n} nodes carry the same operation id"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record(id: &str, result: &str) -> OperationRecord {
        OperationRecord {
            operation_id: id.to_owned(),
            scope: "tag|tenant-a|subject".to_owned(),
            result: result.to_owned(),
            committed_at: "2026-11-19T00:00:00Z".to_owned(),
        }
    }

    /// An id already applied replays the SAME recorded result, every
    /// time: idempotency by id means the retry gets the committed
    /// answer, not a fresh application.
    #[test]
    fn an_applied_id_replays_the_same_result() {
        let applied = record("op-1", "tag-committed-at-revision-4");
        let first = replayed_result(Some(&applied));
        let second = replayed_result(Some(&applied));
        assert_eq!(first, Some("tag-committed-at-revision-4".to_owned()));
        assert_eq!(first, second, "the same record replays the same result");
    }

    /// An id never applied has nothing to replay: the caller applies
    /// the effect and records a result.
    #[test]
    fn an_unapplied_id_has_nothing_to_replay() {
        assert_eq!(replayed_result(None), None);
    }

    /// The operation id accepts UUID-like and hex-like tokens, and
    /// rejects anything that could break the registry contract.
    #[test]
    fn operation_ids_are_validated() {
        assert!(OperationId::new("018f2e1a-7c9b-6d2e-9f4a-3b5c7d9e1f2a").is_ok());
        assert!(OperationId::new("op_2026.11.19-a").is_ok());
        assert!(OperationId::new("").is_err());
        assert!(OperationId::new("op with spaces").is_err());
        assert!(OperationId::new("op|injected").is_err());
        assert!(OperationId::new("op\nnewline").is_err());
        let long = "a".repeat(MAX_OPERATION_ID_LEN + 1);
        assert!(OperationId::new(&long).is_err());
    }

    /// Parsing a well-formed node produces the record.
    #[test]
    fn parsing_produces_the_record() {
        let json = json!({
            "q": [
                {
                    "hodei.operation.id": "op-1",
                    "hodei.operation.scope": "tag|tenant-a|subject",
                    "hodei.operation.result": "ok",
                    "hodei.operation.committed_at": "2026-11-19T00:00:00Z",
                }
            ]
        });
        let parsed = parse_operation(&json, "q", "op-1")
            .expect("a well-formed node parses")
            .expect("one node is a record");
        assert_eq!(parsed.operation_id, "op-1");
        assert_eq!(parsed.scope, "tag|tenant-a|subject");
        assert_eq!(parsed.result, "ok");
    }

    /// Parsing refuses two nodes with the same id: the registry must
    /// never carry duplicates, and picking one silently is not an
    /// option.
    #[test]
    fn parsing_refuses_duplicated_ids() {
        let json = json!({
            "q": [
                { "hodei.operation.id": "op-1", "hodei.operation.scope": "s", "hodei.operation.result": "a" },
                { "hodei.operation.id": "op-1", "hodei.operation.scope": "s", "hodei.operation.result": "b" },
            ]
        });
        let error = parse_operation(&json, "q", "op-1").expect_err("duplicates must be refused");
        assert!(
            matches!(error, OperationError::CorruptRecord { .. }),
            "unexpected: {error}"
        );
    }

    /// Parsing refuses a node whose stored id does not match the
    /// requested id.
    #[test]
    fn parsing_refuses_id_mismatches() {
        let json = json!({
            "q": [
                { "hodei.operation.id": "op-other", "hodei.operation.scope": "s", "hodei.operation.result": "a" },
            ]
        });
        let error = parse_operation(&json, "q", "op-1").expect_err("a mismatch must be refused");
        assert!(
            matches!(error, OperationError::CorruptRecord { .. }),
            "unexpected: {error}"
        );
    }

    /// Parsing refuses a record with no scope: without the scope the
    /// record cannot be audited against its guard.
    #[test]
    fn parsing_refuses_scopeless_records() {
        let json = json!({ "q": [ { "hodei.operation.id": "op-1" } ] });
        let error = parse_operation(&json, "q", "op-1").expect_err("no scope must be refused");
        assert!(
            matches!(error, OperationError::CorruptRecord { .. }),
            "unexpected: {error}"
        );
    }
}
