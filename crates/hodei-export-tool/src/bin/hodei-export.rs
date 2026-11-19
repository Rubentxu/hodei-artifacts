//! Operator command for the one-shot portable semantic export.
//!
//! Usage:
//!   hodei-export --db PATH --out DIR [--namespace NAME] [--database NAME] \
//!                [--backend NAME] [--batch-rows N] [--limit-batches N] [--resume] [--json]
//!
//! Global options: `--db PATH` (the RocksDB directory of the source),
//! `--out DIR` (where the export lands), `--namespace NAME` and
//! `--database NAME` (defaulting to `hodei` and `artifacts`, the names
//! the service reads), `--backend NAME` (default `rocksdb`), and `--json`
//! to print the finished manifest.
//!
//! # Quiesce
//!
//! The command needs the service **stopped**: the embedded engine locks
//! the database directory while the service lives, so a run that opens at
//! all is a run where no writer can commit. The lock is the quiesce
//! guarantee; the frontier captured before the export is re-captured
//! after it, and any movement fails the export instead of producing a
//! torn snapshot. No RocksDB file is ever copied: rows travel as JSON
//! batches, blobs stay in the CAS and are referenced by digest.
//!
//! # Resume
//!
//! A run that dies leaves a progress marker in the output directory; run
//! the same command again with `--resume` (and the same output) to
//! continue from the recorded position. Every batch already written is
//! re-verified against its recorded hash first: a truncated file is
//! named, not carried over.
//!
//! Exit codes: 0 export finished, 1 damage found (corruption, mutation,
//! truncated batch), 2 the command failed.

use std::path::PathBuf;
use std::process::ExitCode;

use hodei_export_tool::{ExportConfig, ExportError};
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

const USAGE: &str = "usage: hodei-export --db PATH --out DIR [--namespace NAME] [--database NAME] \
[--backend NAME] [--batch-rows N] [--limit-batches N] [--resume] [--json]";

struct Args {
    db: PathBuf,
    out: PathBuf,
    namespace: String,
    database: String,
    backend: String,
    batch_rows: usize,
    limit_batches: Option<u64>,
    resume: bool,
    json: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        db: PathBuf::new(),
        out: PathBuf::new(),
        namespace: "hodei".to_string(),
        database: "artifacts".to_string(),
        backend: "rocksdb".to_string(),
        batch_rows: hodei_export_tool::DEFAULT_BATCH_ROWS,
        limit_batches: None,
        resume: false,
        json: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = |name: &str| -> Result<String, String> {
            it.next().ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "--db" => args.db = PathBuf::from(value("--db")?),
            "--out" => args.out = PathBuf::from(value("--out")?),
            "--namespace" => args.namespace = value("--namespace")?,
            "--database" => args.database = value("--database")?,
            "--backend" => args.backend = value("--backend")?,
            "--batch-rows" => {
                args.batch_rows = value("--batch-rows")?
                    .parse()
                    .map_err(|error| format!("--batch-rows must be a number: {error}"))?;
            }
            "--limit-batches" => {
                args.limit_batches = Some(
                    value("--limit-batches")?
                        .parse()
                        .map_err(|error| format!("--limit-batches must be a number: {error}"))?,
                );
            }
            "--resume" => args.resume = true,
            "--json" => args.json = true,
            "--help" | "-h" => return Err(USAGE.to_string()),
            other => return Err(format!("unknown argument {other}; {USAGE}")),
        }
    }
    if args.db.as_os_str().is_empty() {
        return Err(format!("--db is required; {USAGE}"));
    }
    if args.out.as_os_str().is_empty() {
        return Err(format!("--out is required; {USAGE}"));
    }
    Ok(args)
}

/// Opens the source the same way the recovery tools do: the engine lock
/// is the quiesce guarantee, and a failure to open names the cause.
async fn open_source(path: &str, namespace: &str, database: &str) -> Result<Surreal<Db>, String> {
    let db = Surreal::new::<RocksDb>(path).await.map_err(|error| {
        format!(
            "cannot open the database {path}: {error}; the service must be stopped, \
             it holds the database while it runs"
        )
    })?;
    db.use_ns(namespace)
        .use_db(database)
        .await
        .map_err(|error| format!("cannot select {namespace}/{database}: {error}"))?;
    Ok(db)
}

fn exit_of(error: &ExportError) -> ExitCode {
    match error {
        ExportError::RowDecode { .. }
        | ExportError::DuplicateRow { .. }
        | ExportError::SourceMutated(_)
        | ExportError::TruncatedBatch { .. }
        | ExportError::ResumeMismatch(_) => ExitCode::from(1),
        _ => ExitCode::from(2),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };
    let db_path = args.db.display().to_string();
    let config = ExportConfig {
        namespace: args.namespace.clone(),
        database: args.database.clone(),
        backend: args.backend.clone(),
        output: args.out.clone(),
        batch_rows: args.batch_rows,
        resume: args.resume,
        limit_batches: args.limit_batches,
    };
    match run(&db_path, config).await {
        Ok(manifest) => {
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&manifest).expect("the manifest serializes")
                );
            } else {
                println!(
                    "export finished: {} tables, {} batches, {} rows, content sha256 {}",
                    manifest.tables.len(),
                    manifest.all_batches().len(),
                    manifest.frontier.tables.values().sum::<u64>(),
                    manifest.content_sha256
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            exit_of(&error)
        }
    }
}

async fn run(
    db_path: &str,
    config: ExportConfig,
) -> Result<hodei_export_tool::ExportManifest, ExportError> {
    let db = open_source(db_path, &config.namespace, &config.database)
        .await
        .map_err(ExportError::Invalid)?;
    hodei_export_tool::export(&db, &config).await
}
