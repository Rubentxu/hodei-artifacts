//! One-shot migration of legacy IAM policy rows to the scoped key format.
//!
//! The old adapter stored rows as `policy:<policy_id>`. The account-scoped
//! adapter stores them as `policy:<account>~<policy_id>`. This module builds a
//! dry-run plan, copies rows with the original content/attachments and removes
//! the legacy row only after the copy succeeds.
//!
//! Three properties make the migration safe to run against live data:
//!
//! * The copy and the revocation of each legacy row happen inside one
//!   SurrealDB transaction, so a failure between them can never leave both
//!   rows alive.
//! * Ownership that cannot be established is reported rather than guessed, and
//!   an attached principal that is not a valid HRN makes the row ambiguous
//!   instead of being silently dropped.
//! * A scoped row that already holds a different policy is reported as a
//!   conflict instead of being overwritten.

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::json;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::sql::Thing;

/// Rows migrated per transaction.
///
/// One batch is one SurrealDB transaction, so every row it carries is applied
/// or none is. The bound keeps the statement count of a single transaction
/// reasonable when the policy table is large.
const ROWS_PER_TRANSACTION: usize = 500;

#[derive(Debug, Clone, Deserialize)]
struct PolicyRow {
    id: Thing,
    content: String,
    #[serde(default)]
    attached_principals: Vec<String>,
    #[serde(default)]
    account: Option<String>,
}

/// Why a row cannot be migrated automatically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackfillStatus {
    /// The row can be copied to `target_id`.
    Ready,
    /// The scoped row already holds the same policy; only the legacy row is
    /// left to revoke.
    AlreadyMigrated,
    /// Ownership cannot be established, so the row is left untouched.
    Ambiguous(String),
    /// The scoped row holds a different policy, so the row is left untouched.
    Conflict(String),
}

#[derive(Debug, Clone)]
pub struct BackfillEntry {
    pub legacy_id: String,
    pub account: Option<String>,
    pub target_id: Option<String>,
    pub status: BackfillStatus,
}

#[derive(Debug, Clone, Default)]
pub struct BackfillReport {
    pub entries: Vec<BackfillEntry>,
    /// Rows copied to the scoped key.
    pub migrated: usize,
    /// Rows that needed no copy because the scoped row already held the policy.
    pub skipped: usize,
    /// Legacy rows revoked because the scoped copy already existed.
    pub revoked: usize,
    /// Rows whose ownership could not be established.
    pub ambiguous: usize,
    /// Rows whose scoped counterpart holds a different policy.
    pub conflicts: usize,
}

/// What the apply phase has to write for one planned row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// Nothing to write: the row already carries a scoped id.
    None,
    /// Copy the legacy row to the scoped key and revoke the legacy one.
    Copy,
    /// The scoped row already holds the same policy; revoke the legacy one.
    Revoke,
}

/// One row of the plan together with the write it implies.
struct PlannedRow {
    entry: BackfillEntry,
    action: Action,
    source: PolicyRow,
}

/// The account a legacy row belongs to, or why it cannot be determined.
enum Ownership {
    Account(String),
    Unresolved(String),
}

fn short_id(id: &Thing) -> String {
    match &id.id {
        surrealdb::sql::Id::String(value) => value.clone(),
        surrealdb::sql::Id::Number(value) => value.to_string(),
        other => other.to_string(),
    }
}

/// The account shared by every attached principal.
///
/// A principal that is not a valid HRN makes the row ambiguous: dropping it
/// would let a row that also carries a foreign principal be migrated under
/// the account of the principals that happen to parse.
fn account_from_attachments(row: &PolicyRow) -> Ownership {
    if row.attached_principals.is_empty() {
        return Ownership::Unresolved("no attached principals and no explicit mapping".to_string());
    }
    let mut accounts: Vec<String> = Vec::with_capacity(row.attached_principals.len());
    for principal in &row.attached_principals {
        match kernel::Hrn::from_string(principal) {
            Some(hrn) => accounts.push(hrn.account_id().to_string()),
            None => {
                return Ownership::Unresolved(format!(
                    "attached principal `{principal}` is not a valid HRN"
                ));
            }
        }
    }
    let first = accounts[0].clone();
    if accounts.iter().all(|account| *account == first) {
        Ownership::Account(first)
    } else {
        Ownership::Unresolved(format!(
            "attached principals span multiple accounts: {}",
            accounts.join(", ")
        ))
    }
}

/// Whether two rows hold the same policy, regardless of principal order.
fn same_policy(left: &PolicyRow, right: &PolicyRow) -> bool {
    if left.content != right.content {
        return false;
    }
    let mut left_principals = left.attached_principals.clone();
    let mut right_principals = right.attached_principals.clone();
    left_principals.sort();
    right_principals.sort();
    left_principals == right_principals
}

/// Build the deterministic migration plan. `mapping` is an explicit
/// `legacy_policy_id -> account` override; when absent, the attached principals
/// must all belong to one account.
async fn plan(
    db: &Surreal<Db>,
    rows: &[PolicyRow],
    mapping: &HashMap<String, String>,
) -> Vec<PlannedRow> {
    let mut planned = Vec::with_capacity(rows.len());
    for row in rows {
        let legacy_id = short_id(&row.id);
        if legacy_id.contains('~') {
            // Already scoped: nothing to copy and nothing to revoke.
            let account = legacy_id.split('~').next().unwrap_or_default().to_string();
            planned.push(PlannedRow {
                entry: BackfillEntry {
                    account: Some(account),
                    target_id: Some(legacy_id.clone()),
                    legacy_id,
                    status: BackfillStatus::AlreadyMigrated,
                },
                action: Action::None,
                source: row.clone(),
            });
            continue;
        }
        let account = match mapping.get(&legacy_id) {
            Some(account) => account.clone(),
            None => match account_from_attachments(row) {
                Ownership::Account(account) => account,
                Ownership::Unresolved(reason) => {
                    planned.push(PlannedRow {
                        entry: BackfillEntry {
                            legacy_id,
                            account: None,
                            target_id: None,
                            status: BackfillStatus::Ambiguous(reason),
                        },
                        action: Action::None,
                        source: row.clone(),
                    });
                    continue;
                }
            },
        };
        let target_id = format!("{account}~{legacy_id}");
        let existing: Option<PolicyRow> = db
            .select(("policy", target_id.as_str()))
            .await
            .ok()
            .flatten();
        let (status, action) = match existing {
            None => (BackfillStatus::Ready, Action::Copy),
            Some(existing) => {
                let account_matches = existing
                    .account
                    .as_ref()
                    .is_none_or(|stored| *stored == account);
                if same_policy(&existing, row) && account_matches {
                    (BackfillStatus::AlreadyMigrated, Action::Revoke)
                } else {
                    let reason = if !account_matches {
                        format!(
                            "scoped row `{target_id}` belongs to account `{}`",
                            existing.account.as_deref().unwrap_or("<none>")
                        )
                    } else {
                        format!("scoped row `{target_id}` holds a different policy")
                    };
                    (BackfillStatus::Conflict(reason), Action::None)
                }
            }
        };
        planned.push(PlannedRow {
            entry: BackfillEntry {
                legacy_id,
                account: Some(account),
                target_id: Some(target_id),
                status,
            },
            action,
            source: row.clone(),
        });
    }
    planned
}

/// Execute (or dry-run) the plan against a SurrealDB connection.
pub async fn run(
    db: &Surreal<Db>,
    mapping: &HashMap<String, String>,
    apply: bool,
) -> Result<BackfillReport, String> {
    let rows: Vec<PolicyRow> = db
        .select("policy")
        .await
        .map_err(|error| error.to_string())?;
    let planned = plan(db, &rows, mapping).await;

    let mut report = BackfillReport::default();
    for row in &planned {
        match &row.entry.status {
            BackfillStatus::Ambiguous(_) => report.ambiguous += 1,
            BackfillStatus::Conflict(_) => report.conflicts += 1,
            BackfillStatus::AlreadyMigrated => report.skipped += 1,
            BackfillStatus::Ready => {}
        }
        report.entries.push(row.entry.clone());
    }

    if !apply {
        return Ok(report);
    }

    let writes: Vec<&PlannedRow> = planned
        .iter()
        .filter(|row| row.action != Action::None)
        .collect();
    let mut migrated = 0usize;
    let mut revoked = 0usize;
    for batch in writes.chunks(ROWS_PER_TRANSACTION) {
        let (copied, revoked_rows) = apply_batch(db, batch).await?;
        migrated += copied;
        revoked += revoked_rows;
    }
    report.migrated = migrated;
    report.revoked = revoked;
    Ok(report)
}

/// One binding of the transaction, kept in the type the wire needs.
///
/// A record id has to reach SurrealDB as a record id and a policy body as a
/// JSON object, and the two cannot share one Rust type inside a collection.
enum Binding {
    /// A bound record id, such as the scoped or the legacy key.
    Id(String, Thing),
    /// A bound policy body.
    Content(String, serde_json::Value),
}

/// Apply one batch inside a single SurrealDB transaction.
///
/// A SurrealDB transaction covers the statements of one query, so the copy and
/// the revocation of a row travel together. Any statement error aborts the
/// whole batch, which leaves every row of it exactly as it was.
async fn apply_batch(db: &Surreal<Db>, batch: &[&PlannedRow]) -> Result<(usize, usize), String> {
    if batch.is_empty() {
        return Ok((0, 0));
    }
    let mut statements = String::from("BEGIN TRANSACTION;");
    let mut bindings: Vec<Binding> = Vec::new();
    let mut copied = 0usize;
    let mut revoked = 0usize;
    for (index, row) in batch.iter().enumerate() {
        let legacy_id = row.entry.legacy_id.as_str();
        let target_id = row
            .entry
            .target_id
            .as_deref()
            .ok_or_else(|| format!("planned row `{legacy_id}` has no target id"))?;
        match row.action {
            Action::None => continue,
            Action::Copy => {
                statements.push_str(&format!(
                    " CREATE $target_{index} CONTENT $content_{index}; DELETE $legacy_{index};"
                ));
                bindings.push(Binding::Id(
                    format!("target_{index}"),
                    Thing::from(("policy", target_id)),
                ));
                bindings.push(Binding::Content(
                    format!("content_{index}"),
                    json!({
                        "content": row.source.content,
                        "attached_principals": row.source.attached_principals,
                        "account": row.entry.account,
                    }),
                ));
                bindings.push(Binding::Id(
                    format!("legacy_{index}"),
                    Thing::from(("policy", legacy_id)),
                ));
                copied += 1;
            }
            Action::Revoke => {
                statements.push_str(&format!(" DELETE $legacy_{index};"));
                bindings.push(Binding::Id(
                    format!("legacy_{index}"),
                    Thing::from(("policy", legacy_id)),
                ));
                revoked += 1;
            }
        }
    }
    statements.push_str(" COMMIT TRANSACTION;");
    let mut query = db.query(statements);
    for binding in bindings {
        query = match binding {
            Binding::Id(name, thing) => query.bind((name, thing)),
            Binding::Content(name, value) => query.bind((name, value)),
        };
    }
    let mut response = query.await.map_err(|error| error.to_string())?;
    let errors = response.take_errors();
    if !errors.is_empty() {
        let details: Vec<String> = errors
            .into_iter()
            .map(|(index, error)| format!("statement {index}: {error}"))
            .collect();
        // The transaction was aborted, so no row of this batch was written.
        return Err(format!(
            "backfill transaction rolled back ({} statement(s) failed): {}",
            details.len(),
            details.join("; ")
        ));
    }
    Ok((copied, revoked))
}
