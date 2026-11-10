//! One-shot migration of legacy IAM policy rows to the scoped key format.
//!
//! The old adapter stored rows as `policy:<policy_id>`. The account-scoped
//! adapter stores them as `policy:<account>~<policy_id>`. This module builds a
//! dry-run plan, copies rows with the original content/attachments and removes
//! the legacy row only after the copy succeeds.

use std::collections::HashMap;

use serde::Deserialize;
use surrealdb::Surreal;
use surrealdb::engine::local::Db;

#[derive(Debug, Clone, Deserialize)]
struct PolicyRow {
    id: surrealdb::sql::Thing,
    content: String,
    #[serde(default)]
    attached_principals: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackfillStatus {
    Ready,
    AlreadyMigrated,
    Ambiguous(String),
}

#[derive(Debug, Clone)]
pub struct BackfillEntry {
    pub legacy_id: String,
    pub account: Option<String>,
    pub target_id: Option<String>,
    pub status: BackfillStatus,
}

#[derive(Debug, Default)]
pub struct BackfillReport {
    pub entries: Vec<BackfillEntry>,
    pub migrated: usize,
    pub skipped: usize,
    pub ambiguous: usize,
    pub conflicts: usize,
}

fn short_id(id: &surrealdb::sql::Thing) -> String {
    match &id.id {
        surrealdb::sql::Id::String(value) => value.clone(),
        surrealdb::sql::Id::Number(value) => value.to_string(),
        other => other.to_string(),
    }
}

fn account_from_attachments(row: &PolicyRow) -> Option<String> {
    let mut accounts = row.attached_principals.iter().filter_map(|principal| {
        kernel::Hrn::from_string(principal).map(|hrn| hrn.account_id().to_string())
    });
    let first = accounts.next()?;
    if accounts.all(|account| account == first) {
        Some(first)
    } else {
        None
    }
}

/// Build the deterministic migration plan. `mapping` is an explicit
/// `legacy_policy_id -> account` override; when absent, the attached principals
/// must all belong to one account.
fn plan(rows: &[PolicyRow], mapping: &HashMap<String, String>) -> Vec<BackfillEntry> {
    rows.iter()
        .map(|row| {
            let legacy_id = short_id(&row.id);
            if legacy_id.contains('~') {
                return BackfillEntry {
                    legacy_id,
                    account: None,
                    target_id: None,
                    status: BackfillStatus::AlreadyMigrated,
                };
            }
            let account = mapping
                .get(&legacy_id)
                .cloned()
                .or_else(|| account_from_attachments(row));
            match account {
                Some(account) => BackfillEntry {
                    target_id: Some(format!("{account}~{legacy_id}")),
                    legacy_id,
                    account: Some(account),
                    status: BackfillStatus::Ready,
                },
                None => BackfillEntry {
                    legacy_id,
                    account: None,
                    target_id: None,
                    status: BackfillStatus::Ambiguous(
                        "no explicit mapping and attached principals do not share one account"
                            .to_string(),
                    ),
                },
            }
        })
        .collect()
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
    let entries = plan(&rows, mapping);
    let mut report = BackfillReport::default();
    for entry in entries {
        match &entry.status {
            BackfillStatus::Ambiguous(_) => report.ambiguous += 1,
            BackfillStatus::AlreadyMigrated => report.skipped += 1,
            BackfillStatus::Ready => {
                let target = entry.target_id.as_deref().expect("ready target");
                let existing: Option<PolicyRow> = db
                    .select(("policy", target))
                    .await
                    .map_err(|error| error.to_string())?;
                if existing.is_some() {
                    report.skipped += 1;
                    report.entries.push(BackfillEntry {
                        status: BackfillStatus::AlreadyMigrated,
                        ..entry
                    });
                    continue;
                }
                let source = rows
                    .iter()
                    .find(|row| short_id(&row.id) == entry.legacy_id)
                    .ok_or_else(|| "legacy row disappeared".to_string())?;
                if apply {
                    let created: Option<PolicyRow> = db
                        .create(("policy", target))
                        .content(serde_json::json!({
                            "content": source.content,
                            "attached_principals": source.attached_principals,
                            "account": entry.account,
                        }))
                        .await
                        .map_err(|error| error.to_string())?;
                    if created.is_none() {
                        report.conflicts += 1;
                        continue;
                    }
                    let _: Option<PolicyRow> = db
                        .delete(("policy", entry.legacy_id.as_str()))
                        .await
                        .map_err(|error| error.to_string())?;
                    report.migrated += 1;
                }
                report.entries.push(entry);
            }
        }
    }
    Ok(report)
}
