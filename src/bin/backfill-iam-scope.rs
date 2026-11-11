//! One-shot operator command for the legacy IAM policy scope migration.
//!
//! Usage:
//!   cargo run --bin backfill-iam-scope -- --db data/hodei.rocksdb \
//!     --mapping backfill.json [--apply]
//!
//! `backfill.json` is a JSON object `{ "legacy-policy-id": "account" }`.
//! Without `--apply` it only prints the plan.

use std::collections::HashMap;
use std::process::ExitCode;

use hodei_artifacts_api::iam_backfill::run;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

#[tokio::main]
async fn main() -> ExitCode {
    let mut db_path = None;
    let mut mapping_path = None;
    let mut apply = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => db_path = args.next(),
            "--mapping" => mapping_path = args.next(),
            "--apply" => apply = true,
            "--help" | "-h" => {
                println!("usage: backfill-iam-scope --db PATH --mapping JSON [--apply]");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown argument: {other}");
                return ExitCode::FAILURE;
            }
        }
    }
    let (Some(db_path), Some(mapping_path)) = (db_path, mapping_path) else {
        eprintln!("usage: backfill-iam-scope --db PATH --mapping JSON [--apply]");
        return ExitCode::FAILURE;
    };
    let mapping_text = match std::fs::read_to_string(&mapping_path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("cannot read mapping: {error}");
            return ExitCode::FAILURE;
        }
    };
    let mapping: HashMap<String, String> = match serde_json::from_str(&mapping_text) {
        Ok(mapping) => mapping,
        Err(error) => {
            eprintln!("invalid mapping JSON: {error}");
            return ExitCode::FAILURE;
        }
    };

    let db: Surreal<Db> = match Surreal::new::<RocksDb>(&db_path).await {
        Ok(db) => db,
        Err(error) => {
            eprintln!("cannot open database: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = db.use_ns("hodei").use_db("artifacts").await {
        eprintln!("cannot select namespace/database: {error}");
        return ExitCode::FAILURE;
    }

    match run(&db, &mapping, apply).await {
        Ok(report) => {
            for entry in &report.entries {
                println!(
                    "{} -> {:?} [{}]",
                    entry.legacy_id,
                    entry.target_id,
                    match &entry.status {
                        hodei_artifacts_api::iam_backfill::BackfillStatus::Ready => "ready",
                        hodei_artifacts_api::iam_backfill::BackfillStatus::AlreadyMigrated =>
                            "already-migrated",
                        hodei_artifacts_api::iam_backfill::BackfillStatus::Ambiguous(reason) => {
                            println!("  ambiguous: {reason}");
                            "ambiguous"
                        }
                        hodei_artifacts_api::iam_backfill::BackfillStatus::Conflict(reason) => {
                            println!("  conflict: {reason}");
                            "conflict"
                        }
                    }
                );
            }
            println!(
                "migrated={} revoked={} skipped={} ambiguous={} conflicts={} apply={apply}",
                report.migrated, report.revoked, report.skipped, report.ambiguous, report.conflicts
            );
            if report.ambiguous > 0 || report.conflicts > 0 {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(error) => {
            eprintln!("backfill failed: {error}");
            ExitCode::FAILURE
        }
    }
}
