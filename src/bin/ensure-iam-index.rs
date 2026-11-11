//! Operator command that guarantees the account index of the IAM policies.
//!
//! Usage:
//!   cargo run --bin ensure-iam-index -- --db data/hodei.rocksdb
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

use hodei_iam::infrastructure::surreal::policy_adapter::SurrealPolicyAdapter;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

#[tokio::main]
async fn main() -> ExitCode {
    let mut db_path = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => db_path = args.next(),
            "--help" | "-h" => {
                println!("usage: ensure-iam-index --db PATH");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown argument: {other}");
                return ExitCode::FAILURE;
            }
        }
    }
    let Some(db_path) = db_path else {
        eprintln!("usage: ensure-iam-index --db PATH");
        return ExitCode::FAILURE;
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

    let adapter = SurrealPolicyAdapter::new(Arc::new(db));
    match adapter.initialize_scope().await {
        Ok(()) => {
            // The statement is idempotent, so a second run is harmless. It is
            // still checked, so the command reports the state it found rather
            // than the state it asked for.
            match adapter.scope_initialized().await {
                Ok(true) => {
                    println!("the policy account index is in place");
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
        Err(error) => {
            eprintln!("cannot create the policy account index: {error}");
            ExitCode::FAILURE
        }
    }
}
