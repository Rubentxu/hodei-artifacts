//! Operator command that guarantees the account index of the IAM policies.
//!
//! Usage:
//!   ensure-iam-index [--db PATH] [--namespace NAME] [--database NAME]
//!
//! With no arguments it reads the same configuration the service reads --
//! the configuration files and the `HODEI_*` environment overrides -- so it
//! declares the index in the database the service actually uses. The flags
//! override that configuration for the rare case where they differ.
//!
//! The listing of IAM policies filters, counts and paginates by account
//! inside SurrealDB, and that query leans on the `policy_account` index. The
//! index is declared here rather than in the bootstrap because defining an
//! index on the local engine keeps the database directory locked by the
//! process that declared it: the service can serve with the index, but the
//! same process cannot open the database again, which would break booting
//! twice against the same database.
//!
//! Running this command from its own process therefore both creates the index
//! and releases the lock when it exits. The statement is idempotent, so the
//! command can be run on every deployment.

use std::process::ExitCode;
use std::sync::Arc;

use hodei_artifacts_api::config::AppConfig;
use hodei_iam::infrastructure::surreal::policy_adapter::SurrealPolicyAdapter;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

/// What the command was asked to do.
struct Options {
    database_path: Option<String>,
    namespace: Option<String>,
    database: Option<String>,
}

fn parse_options() -> Result<Options, String> {
    let mut options = Options {
        database_path: None,
        namespace: None,
        database: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => options.database_path = Some(args.next().ok_or("--db needs a path")?),
            "--namespace" => {
                options.namespace = Some(args.next().ok_or("--namespace needs a name")?)
            }
            "--database" => options.database = Some(args.next().ok_or("--database needs a name")?),
            "--help" | "-h" => {
                println!(
                    "usage: ensure-iam-index [--db PATH] [--namespace NAME] [--database NAME]"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(options)
}

#[tokio::main]
async fn main() -> ExitCode {
    let options = match parse_options() {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}");
            eprintln!("usage: ensure-iam-index [--db PATH] [--namespace NAME] [--database NAME]");
            return ExitCode::FAILURE;
        }
    };

    // The service reads its configuration from the files and the environment.
    // The command reads the same one, so the index lands in the database the
    // service uses: a command that declared the index somewhere else would
    // report success and leave the service scanning the table.
    let config = match AppConfig::new() {
        Ok(config) => config,
        Err(error) => {
            if options.database_path.is_none() {
                eprintln!("cannot read the configuration: {error}");
                eprintln!("pass --db, --namespace and --database to declare the index anyway");
                return ExitCode::FAILURE;
            }
            eprintln!("cannot read the configuration ({error}); using the given arguments");
            AppConfig::default()
        }
    };

    let config_path = config.rocksdb.path.clone();
    let database_path = options
        .database_path
        .clone()
        .unwrap_or_else(|| config_path.clone());
    let namespace = options
        .namespace
        .clone()
        .or_else(|| config.database.namespace.clone())
        .unwrap_or_else(|| "hodei".to_string());
    let database = options
        .database
        .clone()
        .or_else(|| config.database.database.clone())
        .unwrap_or_else(|| "artifacts".to_string());

    // The path the command uses is named out loud, because a wrong path
    // declares the index in a database the service never opens: the service
    // would keep scanning the table and would say so on every boot.
    if options.database_path.is_some() && options.database_path.as_deref() != Some(&config_path) {
        eprintln!(
            "warning: using the database {database_path}; the configuration names {config_path}"
        );
    }
    if !std::path::Path::new(&database_path).exists() {
        println!("the database {database_path} did not exist and was created");
    }

    let db: Surreal<Db> = match Surreal::new::<RocksDb>(&database_path).await {
        Ok(db) => db,
        Err(error) => {
            eprintln!("cannot open database {database_path}: {error}");
            eprintln!("the service must be stopped: it holds the database while it runs");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = db.use_ns(&namespace).use_db(&database).await {
        eprintln!("cannot select {namespace}/{database}: {error}");
        return ExitCode::FAILURE;
    }

    let adapter = SurrealPolicyAdapter::new(Arc::new(db));
    if let Err(error) = adapter.initialize_scope().await {
        eprintln!("cannot create the policy account index: {error}");
        return ExitCode::FAILURE;
    }

    // The statement is idempotent, so a second run is harmless. It is still
    // checked, so the command reports the state it found rather than the
    // state it asked for.
    match adapter.scope_initialized().await {
        Ok(true) => {
            println!("the policy account index is in place in {namespace}/{database}");
            ExitCode::SUCCESS
        }
        Ok(false) => {
            eprintln!("the policy account index is still missing after the statement");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("cannot read the policy account index: {error}");
            ExitCode::FAILURE
        }
    }
}
