//! Operator command for backup, restore, scrub and repair of the registry.
//!
//! Usage:
//!   hodei-recovery [GLOBAL OPTIONS] backup --to DIR
//!   hodei-recovery [GLOBAL OPTIONS] verify DIR
//!   hodei-recovery [GLOBAL OPTIONS] restore --from DIR
//!   hodei-recovery [GLOBAL OPTIONS] scrub
//!   hodei-recovery [GLOBAL OPTIONS] repair --from DIR
//!   hodei-recovery [GLOBAL OPTIONS] bundle export --to DIR --account ACC --repository KEY
//!   hodei-recovery [GLOBAL OPTIONS] bundle import --from DIR --account ACC --repository KEY \
//!     [--allow-mapping] [--overwrite]
//!
//! Global options: `--db PATH`, `--cas PATH`, `--namespace NAME`,
//! `--database NAME`, `--grace-secs N`, `--json`.
//!
//! A resolution bundle is not a copy of the registry: it is what a proxy
//! repository resolved, with the provenance that backed each path. The
//! import verifies the whole package before making anything visible, and
//! signs the receipts under the destination configuration: the upstream
//! of the origin is provenance, not authority.
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

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use hodei_artifacts_api::bundles::{self, Mapping};
use hodei_artifacts_api::config::AppConfig;
use hodei_artifacts_api::recovery::{self, RecoveryError, ScrubReport};
use serde::Serialize;

const USAGE: &str = "usage: hodei-recovery [--db PATH] [--cas PATH] [--namespace NAME] \
[--database NAME] [--grace-secs N] [--json] <backup --to DIR | verify DIR | restore --from DIR | \
scrub | repair --from DIR | bundle export --to DIR --account ACC --repository KEY [--signing-key KEY.ASC] | \
bundle import --from DIR --account ACC --repository KEY [--allow-mapping] [--overwrite] | \
capsule verify --from DIR --key PUBLIC.ASC>";

enum Command {
    Backup {
        to: PathBuf,
    },
    Verify {
        backup: PathBuf,
    },
    Restore {
        from: PathBuf,
    },
    Scrub,
    Repair {
        from: PathBuf,
    },
    BundleExport {
        to: PathBuf,
        account: String,
        repository: String,
    },
    /// Lee la capsula OCI de un paquete exportado y dice si la firma del
    /// manifiesto de imagen corresponde a la clave dada y a esta capsula.
    CapsuleVerify {
        from: PathBuf,
        key: PathBuf,
    },
    BundleImport {
        from: PathBuf,
        account: String,
        repository: String,
        allow_mapping: bool,
        overwrite: bool,
    },
}

struct Options {
    db: Option<String>,
    cas: Option<String>,
    namespace: Option<String>,
    database: Option<String>,
    grace_secs: Option<u64>,
    json: bool,
    signing_key: Option<PathBuf>,
    command: Command,
}

fn parse_options() -> Result<Options, String> {
    let mut db = None;
    let mut cas = None;
    let mut namespace = None;
    let mut database = None;
    let mut grace_secs = None;
    let mut json = false;
    let mut account = None;
    let mut repository = None;
    let mut allow_mapping = false;
    let mut overwrite = false;
    let mut name: Option<String> = None;
    let mut to = None;
    let mut from = None;
    let mut signing_key: Option<PathBuf> = None;
    let mut verify_key: Option<PathBuf> = None;
    let mut positional = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => db = Some(args.next().ok_or("--db needs a path")?),
            "--cas" => cas = Some(args.next().ok_or("--cas needs a path")?),
            "--namespace" => namespace = Some(args.next().ok_or("--namespace needs a name")?),
            "--database" => database = Some(args.next().ok_or("--database needs a name")?),
            "--account" => account = Some(args.next().ok_or("--account needs a name")?),
            "--repository" => {
                repository = Some(args.next().ok_or("--repository needs a name")?);
            }
            "--allow-mapping" => allow_mapping = true,
            "--overwrite" => overwrite = true,
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
            "--signing-key" => {
                signing_key = Some(PathBuf::from(
                    args.next().ok_or("--signing-key needs a path")?,
                ));
            }
            "--key" => {
                verify_key = Some(PathBuf::from(args.next().ok_or("--key needs a path")?));
            }
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
        Some("capsule") => {
            if args.next().as_deref() != Some("verify") {
                return Err("capsule needs verify".to_string());
            }
            Command::CapsuleVerify {
                from: from.ok_or("capsule verify needs --from DIR")?,
                key: verify_key.ok_or("capsule verify needs --key PUBLIC.ASC")?,
            }
        }
        Some("bundle") => {
            let account = account
                .clone()
                .ok_or("a bundle command needs --account ACC")?;
            let repository = repository
                .clone()
                .ok_or("a bundle command needs --repository KEY")?;
            match positional
                .and_then(|sub| sub.into_os_string().into_string().ok())
                .as_deref()
            {
                Some("export") => Command::BundleExport {
                    to: to.ok_or("bundle export needs --to DIR")?,
                    account,
                    repository,
                },
                Some("import") => Command::BundleImport {
                    from: from.ok_or("bundle import needs --from DIR")?,
                    account,
                    repository,
                    allow_mapping,
                    overwrite,
                },
                Some(other) => return Err(format!("unknown bundle command: {other}")),
                None => return Err("bundle needs export or import".to_string()),
            }
        }
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
        signing_key,
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
        Command::BundleExport {
            to,
            account,
            repository,
        } => {
            let (store, blobs) = bundle_targets(&target).await?;
            let signer = options
                .signing_key
                .as_deref()
                .map(load_or_create_signer)
                .transpose()?;
            let report = bundles::export(
                &store,
                &store,
                &store,
                &blobs,
                account,
                repository,
                to,
                signer.as_ref(),
            )
            .await?;
            if options.json {
                print_json(&report);
            } else {
                let capsule_note = match &report.capsule {
                    Some(capsule) => format!(
                        ", capsule at {} signed by {}",
                        capsule.layout.display(),
                        capsule.signer
                    ),
                    None => ", no capsule (pass --signing-key)".to_string(),
                };
                println!(
                    "bundle written to {}: {} paths, {} bytes of blobs{capsule_note}",
                    report.path.display(),
                    report.exported,
                    report.blob_bytes
                );
                for withheld in &report.withheld {
                    println!("  withheld {}: {}", withheld.asset_path, withheld.reason);
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::CapsuleVerify { from, key } => {
            let armored = std::fs::read_to_string(key)
                .map_err(|error| RecoveryError::Invalid(format!("the key: {error}")))?;
            let signer = hodei_artifacts_api::openpgp::MetadataSigner::from_armor(&armored)
                .map_err(|error| RecoveryError::Invalid(error.to_string()))?;
            let report = bundles::verify_capsule(&from.join("oci"), &signer)
                .map_err(|error| RecoveryError::Bundle(error.to_string()))?;
            if options.json {
                print_json(&serde_json::json!({
                    "image": report.image.annotations.get("hodei.capsule"),
                    "entries": report.capsule.entries.len(),
                    "withheld": report.capsule.withheld.len(),
                    "signer": report.signer,
                }));
            } else {
                println!(
                    "capsule of {} entries ({} withheld), signed by {}",
                    report.capsule.entries.len(),
                    report.capsule.withheld.len(),
                    report.signer
                );
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::BundleImport {
            from,
            account,
            repository,
            allow_mapping,
            overwrite,
        } => {
            let (store, blobs) = bundle_targets(&target).await?;
            let mapping = Mapping {
                allow: *allow_mapping,
                overwrite: *overwrite,
            };
            let correlation_id = format!("bundle-import-{}", Utc::now().to_rfc3339());
            let report = bundles::import(
                &store,
                &store,
                &store,
                &blobs,
                account,
                repository,
                from,
                mapping,
                "hodei-recovery",
                &correlation_id,
            )
            .await?;
            if options.json {
                print_json(&report);
            } else {
                println!(
                    "imported {} paths ({} already present) and {} bytes of blobs into {}/{}",
                    report.imported, report.already_present, report.blob_bytes, account, repository
                );
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// The signing key a capsule export works on: an armored secret key in
/// `path`, created and stored there on first use. A key is not a
/// throwaway: the capsule is verified later against its public half.
fn load_or_create_signer(
    path: &Path,
) -> Result<hodei_artifacts_api::openpgp::MetadataSigner, RecoveryError> {
    if let Ok(armored) = std::fs::read_to_string(path) {
        return hodei_artifacts_api::openpgp::MetadataSigner::from_armor(&armored)
            .map_err(|error| RecoveryError::Invalid(error.to_string()));
    }
    let signer = hodei_artifacts_api::openpgp::MetadataSigner::generate(env!("CARGO_PKG_NAME"))
        .map_err(|error| RecoveryError::Invalid(error.to_string()))?;
    let armored = signer
        .to_armor()
        .map_err(|error| RecoveryError::Invalid(error.to_string()))?;
    std::fs::write(path, armored)
        .map_err(|error| RecoveryError::Store(format!("the key: {error}")))?;
    Ok(signer)
}

/// The catalog and the content store a bundle command works on.
///
/// The command operates as the operator, outside the API boundary: the
/// authorizer it injects allows everything it reads and writes, because
/// the permission of the operator is the invocation of the command.
async fn bundle_targets(
    target: &Target,
) -> Result<
    (
        hodei_catalog_storage::SurrealCatalogStore,
        hodei_hub::blob::FileBlobStore,
    ),
    RecoveryError,
> {
    let db = recovery::open_database(&target.db, &target.namespace, &target.database).await?;
    let store = hodei_catalog_storage::SurrealCatalogStore::with_authorizer(
        db,
        Arc::new(hodei_catalog::AllowAllAuthorizer),
    );
    let blobs = hodei_hub::blob::FileBlobStore::new(&target.cas)
        .map_err(|error| RecoveryError::Store(error.to_string()))?;
    Ok((store, blobs))
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
