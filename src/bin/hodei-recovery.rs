//! Operator command for backup, restore, scrub and repair of the registry.
//!
//! Usage:
//!   hodei-recovery [GLOBAL OPTIONS] backup --to DIR
//!   hodei-recovery [GLOBAL OPTIONS] verify DIR
//!   hodei-recovery [GLOBAL OPTIONS] restore --from DIR
//!   hodei-recovery [GLOBAL OPTIONS] scrub
//!   hodei-recovery [GLOBAL OPTIONS] repair --from DIR
//!
//! Global options: `--db PATH`, `--cas PATH`, `--namespace NAME`,
//! `--database NAME`, `--grace-secs N`, `--json`.
//!
//! Paths, namespace and database default to the configuration the service
//! reads, so the command works on the data the service actually uses.
//!
//! The command needs the service **stopped**: the embedded database is locked
//! by the process that holds it. There is no hot backup and no high
//! availability; the procedure and its measured RPO/RTO are in the UAT of
//! operations.
//!
//! Exit codes: 0 sound, 1 the check found damage, 2 the command failed.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use hodei_artifacts_api::config::AppConfig;
use hodei_artifacts_api::recovery::{self, RecoveryError, ScrubReport};
use serde::Serialize;

const USAGE: &str = "usage: hodei-recovery [--db PATH] [--cas PATH] [--namespace NAME] \
[--database NAME] [--grace-secs N] [--json] <backup --to DIR | verify DIR | restore --from DIR | scrub | repair --from DIR>";

enum Command {
    Backup { to: PathBuf },
    Verify { backup: PathBuf },
    Restore { from: PathBuf },
    Scrub,
    Repair { from: PathBuf },
}

struct Options {
    db: Option<String>,
    cas: Option<String>,
    namespace: Option<String>,
    database: Option<String>,
    grace_secs: Option<u64>,
    json: bool,
    command: Command,
}

fn parse_options() -> Result<Options, String> {
    let mut db = None;
    let mut cas = None;
    let mut namespace = None;
    let mut database = None;
    let mut grace_secs = None;
    let mut json = false;
    let mut name: Option<String> = None;
    let mut to = None;
    let mut from = None;
    let mut positional = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => db = Some(args.next().ok_or("--db needs a path")?),
            "--cas" => cas = Some(args.next().ok_or("--cas needs a path")?),
            "--namespace" => namespace = Some(args.next().ok_or("--namespace needs a name")?),
            "--database" => database = Some(args.next().ok_or("--database needs a name")?),
            "--grace-secs" => {
                let value = args.next().ok_or("--grace-secs needs a number")?;
                grace_secs = Some(
                    value
                        .parse()
                        .map_err(|_| format!("--grace-secs needs a number, got {value}"))?,
                );
            }
            "--json" => json = true,
            "--to" => to = Some(PathBuf::from(args.next().ok_or("--to needs a path")?)),
            "--from" => from = Some(PathBuf::from(args.next().ok_or("--from needs a path")?)),
            "--help" | "-h" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other if other.starts_with("--") => return Err(format!("unknown argument: {other}")),
            other if name.is_none() => name = Some(other.to_string()),
            other if positional.is_none() => positional = Some(PathBuf::from(other)),
            other => return Err(format!("unexpected argument: {other}")),
        }
    }
    let command = match name.as_deref() {
        Some("backup") => Command::Backup {
            to: to.ok_or("backup needs --to DIR")?,
        },
        Some("verify") => Command::Verify {
            backup: positional.or(from).ok_or("verify needs the backup DIR")?,
        },
        Some("restore") => Command::Restore {
            from: from.ok_or("restore needs --from DIR")?,
        },
        Some("scrub") => Command::Scrub,
        Some("repair") => Command::Repair {
            from: from.ok_or("repair needs --from DIR")?,
        },
        Some(other) => return Err(format!("unknown command: {other}")),
        None => return Err("a command is needed".to_string()),
    };
    Ok(Options {
        db,
        cas,
        namespace,
        database,
        grace_secs,
        json,
        command,
    })
}

/// What the command works on, from the flags and the service configuration.
struct Target {
    db: String,
    cas: PathBuf,
    namespace: String,
    database: String,
    grace: Duration,
}

fn target(options: &Options) -> Result<Target, String> {
    let config = match AppConfig::new() {
        Ok(config) => config,
        Err(error) => {
            if options.db.is_none() || options.cas.is_none() {
                return Err(format!(
                    "cannot read the configuration ({error}); pass --db and --cas"
                ));
            }
            AppConfig::default()
        }
    };
    Ok(Target {
        db: options
            .db
            .clone()
            .unwrap_or_else(|| config.rocksdb.path.clone()),
        cas: PathBuf::from(
            options
                .cas
                .clone()
                .unwrap_or_else(|| config.content.cas_path.clone()),
        ),
        namespace: options
            .namespace
            .clone()
            .or_else(|| config.database.namespace.clone())
            .unwrap_or_else(|| "hodei".to_string()),
        database: options
            .database
            .clone()
            .or_else(|| config.database.database.clone())
            .unwrap_or_else(|| "artifacts".to_string()),
        grace: Duration::from_secs(options.grace_secs.unwrap_or(config.content.gc_grace_secs)),
    })
}

fn print_json(value: &impl Serialize) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("a report serializes")
    );
}

fn print_scrub(report: &ScrubReport) {
    println!(
        "checked {} blobs ({} bytes) and {} references in {} ms",
        report.blobs_checked, report.bytes_checked, report.roots_checked, report.elapsed_ms
    );
    let summary = &report.summary;
    println!(
        "corrupt: {}, broken references: {}, orphans: {} ({} in grace)",
        summary.corrupt, summary.broken_references, summary.orphans, summary.orphans_in_grace
    );
    for finding in &report.findings {
        println!(
            "  {}",
            serde_json::to_string(finding).expect("a finding serializes")
        );
    }
}

/// The outcome of one command: what to print and the exit code.
async fn run(options: &Options) -> Result<ExitCode, RecoveryError> {
    if let Command::Verify { backup } = &options.command {
        let outcome = recovery::inspect(backup).await?;
        if options.json {
            print_json(&outcome);
        } else if outcome.is_sound() {
            println!(
                "the backup is sound: {} blobs, metadata {} bytes, checked in {} ms",
                outcome.manifest.blobs.len(),
                outcome.manifest.metadata.bytes,
                outcome.elapsed_ms
            );
        } else {
            println!("the backup is NOT sound:");
            for problem in &outcome.problems {
                println!("  {problem}");
            }
        }
        return Ok(if outcome.is_sound() {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        });
    }

    let target = target(options).map_err(RecoveryError::Invalid)?;
    match &options.command {
        Command::Verify { .. } => unreachable!("handled above"),
        Command::Backup { to } => {
            let db =
                recovery::open_database(&target.db, &target.namespace, &target.database).await?;
            let report =
                recovery::backup(&db, &target.cas, to, &target.namespace, &target.database).await?;
            if options.json {
                print_json(&report);
            } else {
                println!(
                    "backup written to {}: {} blobs, {} excluded, {} ms",
                    report.path.display(),
                    report.manifest.blobs.len(),
                    report.manifest.excluded.len(),
                    report.elapsed_ms
                );
                for excluded in &report.manifest.excluded {
                    println!("  excluded {}: {}", excluded.digest, excluded.reason);
                }
            }
            // A blob that was damaged is not in the copy: the copy is sound
            // but the registry it came from was not.
            Ok(if report.manifest.excluded.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
        Command::Restore { from } => {
            let db =
                recovery::open_database(&target.db, &target.namespace, &target.database).await?;
            let report = recovery::restore(&db, from, &target.cas, target.grace).await?;
            if options.json {
                print_json(&report);
            } else {
                println!(
                    "restored {} blobs ({} bytes) in {} ms",
                    report.blobs_restored, report.bytes_restored, report.elapsed_ms
                );
                print_scrub(&report.scrub);
            }
            Ok(if report.scrub.is_sound() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
        Command::Scrub => {
            let db =
                recovery::open_database(&target.db, &target.namespace, &target.database).await?;
            let scrubber = recovery::backup::scrubber_for(&db, &target.cas, target.grace).await?;
            let report = scrubber.run().await?;
            if options.json {
                print_json(&report);
            } else {
                print_scrub(&report);
            }
            Ok(if report.is_sound() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
        Command::Repair { from } => {
            let db =
                recovery::open_database(&target.db, &target.namespace, &target.database).await?;
            // The backup is checked whole before a single byte is taken
            // from it: a damaged backup must not repair anything.
            recovery::verify(from).await?;
            let scrubber = recovery::backup::scrubber_for(&db, &target.cas, target.grace).await?;
            let before = scrubber.run().await?;
            let repair = recovery::repair_from_backup(&target.cas, from, &before).await?;
            let after = scrubber.run().await?;
            if options.json {
                print_json(&serde_json::json!({
                    "before": before,
                    "repair": repair,
                    "after": after,
                }));
            } else {
                println!(
                    "repaired {} blobs, quarantined {}, not in the backup {}, damaged in the backup {}",
                    repair.restored.len(),
                    repair.quarantined.len(),
                    repair.not_in_backup.len(),
                    repair.backup_damaged.len()
                );
                print_scrub(&after);
            }
            Ok(if after.is_sound() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let options = match parse_options() {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}");
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(&options).await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(2)
        }
    }
}
