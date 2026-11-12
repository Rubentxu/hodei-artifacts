//! Operator command that guarantees the indexes of the evidence graph.
//!
//! Usage:
//!   ensure-graph-index [--db PATH] [--namespace NAME] [--database NAME]
//!
//! With no arguments it reads the same configuration the service reads --
//! the configuration files and the `HODEI_*` environment overrides -- so it
//! declares the indexes in the database the service actually uses. The flags
//! override that configuration for the rare case where they differ.
//!
//! # Why the graph needs indexes
//!
//! The measure of the scale model (SPIKE-012) said the answer: the graph
//! answers its questions by reading the whole table and picking the rows
//! that match. On the reference load that turned a query that touches ten
//! relations into a read of every relation there is, and the query of three
//! hops into three such reads. The same measure, with the indexes in place,
//! took the same query from 435 ms to 4.8 ms.
//!
//! # Why the indexes are declared here and not in the service
//!
//! Defining an index on the local engine keeps the database directory locked
//! by the process that defined it: the service can serve with the index,
//! but the same process cannot open the database again, which would break
//! booting twice against the same database.
//!
//! Running this command from its own process therefore both creates the
//! indexes and releases the lock when it exits. The statements are
//! idempotent, so the command can be run on every deployment.
//!
//! # What the command declares
//!
//! The indexes are declared on the tables of the graph projection, and on
//! the tables of the rebuild projection, because the rebuild answers the
//! same questions on the same tables. A table that does not exist yet is
//! created by the declaration itself, so the command may run before the
//! service has ever written a graph.

use std::process::ExitCode;

use hodei_artifacts_api::config::AppConfig;
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
                    "usage: ensure-graph-index [--db PATH] [--namespace NAME] [--database NAME]"
                );
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(options)
}

/// The tables the graph holds, and the indexes each one needs.
///
/// The names and the fields are the ones the storage crate declares, so
/// there is one place that says what the graph indexes on.
use hodei_evidence_storage::surreal::graph::{GRAPH_INDEXES, declare_index_statement};

#[tokio::main]
async fn main() -> ExitCode {
    let options = match parse_options() {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}");
            eprintln!("usage: ensure-graph-index [--db PATH] [--namespace NAME] [--database NAME]");
            return ExitCode::FAILURE;
        }
    };

    // The service reads its configuration from the files and the environment.
    // The command reads the same one, so the indexes land in the database the
    // service uses: a command that declared them somewhere else would report
    // success and leave the service scanning the table.
    let config = match AppConfig::new() {
        Ok(config) => config,
        Err(error) => {
            if options.database_path.is_none() {
                eprintln!("cannot read the configuration: {error}");
                eprintln!("pass --db, --namespace and --database to declare the indexes anyway");
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
    // declares the indexes in a database the service never opens: the
    // service would keep scanning the table and would say so on every boot.
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

    for (table, fields) in GRAPH_INDEXES {
        let statement = declare_index_statement(table, fields);
        match db.query(&statement).await {
            Ok(response) => match response.check() {
                Ok(_) => println!("the index on {table} ({fields}) is in place"),
                Err(error) => {
                    eprintln!("cannot declare the index on {table} ({fields}): {error}");
                    return ExitCode::FAILURE;
                }
            },
            Err(error) => {
                eprintln!("cannot declare the index on {table} ({fields}): {error}");
                return ExitCode::FAILURE;
            }
        }
    }

    println!("the indexes of the graph are in place in {namespace}/{database}");
    ExitCode::SUCCESS
}
