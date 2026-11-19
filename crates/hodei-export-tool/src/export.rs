//! The paginated export: read the source in bounded batches, hash and
//! write each one, and close the run with a deterministic manifest.
//!
//! # The shape of the work
//!
//! Every table is read by keyset pagination -- `WHERE id > $after ORDER
//! BY id ASC LIMIT $n` -- so the memory of the run is one batch, never a
//! table (the HSP-174 lesson: a full catalog materialized in memory is a
//! timeout with extra steps). The batch bytes are canonical JSON (sorted
//! keys, compact), hashed with SHA-256 as they are written, and recorded
//! in the progress marker after every batch, which is what makes a
//! crashed run resumable: the next run re-verifies every recorded batch
//! against its hash before continuing.
//!
//! # Quiesce and the frontier
//!
//! The tool opens the embedded engine directly: SurrealDB holds an
//! exclusive lock on the database directory, so a run that opens at all
//! is a run where the service cannot have written. That lock is the
//! quiesce guarantee; there is no hot export and none is promised. The
//! frontier (row counts per table, ledger head) is captured before the
//! export and re-captured after it -- and before a resume -- and any
//! difference fails the run with [`ExportError::SourceMutated`]: an
//! export of a moved source would be a torn snapshot, and a torn snapshot
//! must not look finished.
//!
//! # What travels
//!
//! Rows, as canonical JSON with their record ids and their digests.
//! Blob bytes never travel: the CAS holds them by digest, and the import
//! reconciles every referenced digest against it.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::Value;
use surrealdb::{Connection, Surreal};

use crate::error::ExportError;
use crate::manifest::{
    BATCHES_DIR, BatchEntry, BlobPolicy, EXPORTED_TABLES, ExportManifest, FORMAT, Frontier,
    MANIFEST_FILE, PROGRESS_FILE, ProgressEntry, ProgressState, SourceInfo, TableEntry, sha256_hex,
};

/// The default rows per batch.
///
/// Bounded on purpose: the batch lives in memory once (read, serialize,
/// hash), and the HSP-174 lesson is that unbounded reads are how a
/// catalog-sized job dies. An operator may lower it; raising it raises
/// the memory of the run with it.
pub const DEFAULT_BATCH_ROWS: usize = 500;

/// The contract floor of the export, as the catalog adapter states it:
/// the ordered keyset pagination this tool leans on was measured on 2.7.
pub const SOURCE_ENGINE: &str = "surrealdb";
pub const SOURCE_ENGINE_VERSION: &str = "2.7.0";

/// How an export is run.
#[derive(Debug, Clone)]
pub struct ExportConfig {
    /// The SurrealDB namespace of the source.
    pub namespace: String,
    /// The SurrealDB database of the source.
    pub database: String,
    /// The storage backend the source runs on, as the operator declared
    /// it (`rocksdb` in production).
    pub backend: String,
    /// Where the export lands. A directory that already has a manifest is
    /// refused: an export is never overwritten.
    pub output: PathBuf,
    /// Rows per batch.
    pub batch_rows: usize,
    /// Resume an unfinished export recorded in the output directory.
    pub resume: bool,
    /// How many batches this run writes at most, when the export is
    /// paged across several runs. `None` writes everything.
    pub limit_batches: Option<u64>,
}

impl ExportConfig {
    /// A configuration for a fresh, single-run export.
    pub fn new(namespace: &str, database: &str, backend: &str, output: PathBuf) -> Self {
        Self {
            namespace: namespace.to_string(),
            database: database.to_string(),
            backend: backend.to_string(),
            output,
            batch_rows: DEFAULT_BATCH_ROWS,
            resume: false,
            limit_batches: None,
        }
    }
}

/// What one run of the export left behind.
#[derive(Debug, Clone)]
pub struct ExportRun {
    /// The finished manifest, when the run completed the export. A run
    /// that spent its batch budget leaves `None`: the export is unfinished
    /// by design, and the next run resumes it.
    pub manifest: Option<ExportManifest>,
    /// How many batches this run wrote.
    pub batches_written: u64,
}

/// Exports the semantic content of the source into `config.output`.
///
/// It captures the frontier itself; see [`export_with_frontier`] when the
/// frontier was captured earlier by an operator probe.
pub async fn export<C: Connection>(
    db: &Surreal<C>,
    config: &ExportConfig,
) -> Result<ExportManifest, ExportError> {
    let run = export_with_frontier(db, config, None).await?;
    // Sin presupuesto de lotes, la corrida siempre termina: el invariante
    // que hace que la comodidad no pida al operador desenrollar un Option.
    Ok(run
        .manifest
        .expect("a run without a batch budget always finishes"))
}

/// Exports the source with a frontier captured before this call.
///
/// The frontier is the mutation tripwire: the export refuses to finish if
/// the source it reads no longer matches it. Capturing it separately is
/// the two-phase operator flow -- probe the frontier, check quiesce, then
/// export against the probed frontier -- and it is what lets a test
/// rehearse a writer committing between the probe and the first batch.
pub async fn export_with_frontier<C: Connection>(
    db: &Surreal<C>,
    config: &ExportConfig,
    frontier: Option<Frontier>,
) -> Result<ExportRun, ExportError> {
    if config.batch_rows == 0 {
        return Err(ExportError::Invalid("the batch size cannot be zero".into()));
    }
    prepare_output(config)?;

    let (frontier, mut done, mut position) = if config.resume {
        let state = load_progress(config)?;
        // La frontera de una reanudacion es la que el export capturo la
        // primera vez; se verifica contra la fuente ahora, despues de
        // comprobar los ficheros ya escritos, porque una fuente que
        // movio durante el hueco entre corridas invalida lo hecho.
        verify_done_batches(config, &state.done)?;
        let now = capture_frontier(db).await?;
        if now != state.frontier {
            return Err(ExportError::SourceMutated(mutation_message(
                &state.frontier,
                &now,
            )));
        }
        (
            state.frontier,
            state.done,
            (state.next_table, state.next_batch_index),
        )
    } else {
        let frontier = match frontier {
            Some(captured) => captured,
            None => capture_frontier(db).await?,
        };
        (frontier, Vec::new(), (None, 1))
    };

    let (written, exhausted) = run_batches(db, config, &frontier, &mut done, &mut position).await?;
    if exhausted {
        // El presupuesto de la corrida se gasto: el export sigue
        // abierto, con su marcador de progreso al dia. No hay manifiesto,
        // porque un export a medias no se hace pasar por terminado.
        return Ok(ExportRun {
            manifest: None,
            batches_written: written,
        });
    }

    // La frontera despues del trabajo: si no es la de antes, lo escrito
    // es una foto rota y el export no se hace pasar por terminado.
    let now = capture_frontier(db).await?;
    if now != frontier {
        return Err(ExportError::SourceMutated(mutation_message(
            &frontier, &now,
        )));
    }

    let manifest = build_manifest(config, &frontier, &done);
    write_atomic(
        &config.output.join(MANIFEST_FILE),
        &serialize_manifest(&manifest),
    )?;
    if config.resume || written > 0 {
        remove_progress(config)?;
    }
    Ok(ExportRun {
        manifest: Some(manifest),
        batches_written: written,
    })
}

/// The table this position continues with, and the 1-based batch index it
/// continues at.
type Position = (Option<String>, usize);

/// Writes batches from the current position until the tables run out or
/// the run's batch budget is spent.
///
/// Returns how many batches were written and whether the budget was
/// spent (the export is unfinished and the next run resumes it).
async fn run_batches<C: Connection>(
    db: &Surreal<C>,
    config: &ExportConfig,
    frontier: &Frontier,
    done: &mut Vec<ProgressEntry>,
    position: &mut Position,
) -> Result<(u64, bool), ExportError> {
    let mut written = 0u64;
    let mut remaining: Option<u64> = config.limit_batches;
    // Sin tabla pendiente y con lotes ya hechos, el trabajo esta
    // terminado: solo quedan por escribir la frontera y el manifiesto.
    if position.0.is_none() && !done.is_empty() {
        return Ok((0, false));
    }
    // La tabla donde la corrida anterior quedo: se copia antes del bucle
    // porque la posicion se reescribe dentro de el.
    let resume_from = position.0.clone();
    let tables = table_iter(resume_from.as_deref());
    for (table, identity_fields) in tables {
        let mut batch_index = if resume_from.as_deref() == Some(table) {
            position.1
        } else {
            1
        };
        let mut after: Option<String> = None;
        if resume_from.as_deref() == Some(table) {
            // Reanudar a mitad de tabla: el lote anterior ya esta
            // verificado, asi que su ultimo id es la frontera de la
            // lectura.
            after = done
                .iter()
                .rev()
                .find(|entry| entry.table == table)
                .map(|entry| entry.batch.last_id.clone());
        }
        loop {
            let rows = read_batch(db, table, after.as_deref(), config.batch_rows).await?;
            if rows.is_empty() {
                break;
            }
            let batch = write_batch(config, table, batch_index, &rows, identity_fields)?;
            after = Some(batch.last_id.clone());
            done.push(ProgressEntry {
                table: table.to_string(),
                batch,
            });
            batch_index += 1;
            written += 1;
            if let Some(left) = remaining.as_mut() {
                *left -= 1;
                if *left == 0 {
                    save_progress(config, frontier, done, table, batch_index)?;
                    *position = (Some(table.to_string()), batch_index);
                    return Ok((written, true));
                }
            }
        }
        if resume_from.as_deref() == Some(table) {
            // La tabla termino de reanudarse: las siguientes empiezan de
            // cero.
            *position = (None, 1);
        }
    }
    *position = (None, 1);
    Ok((written, false))
}

/// The tables to export, from the one the position names (or the first)
/// to the last.
fn table_iter(
    start: Option<&str>,
) -> impl Iterator<Item = (&'static str, &'static [&'static str])> {
    let skip = start
        .and_then(|name| EXPORTED_TABLES.iter().position(|(table, _)| *table == name))
        .unwrap_or(0);
    EXPORTED_TABLES.iter().skip(skip).copied()
}

/// Reads one batch of a table, by keyset pagination.
///
/// The first batch of a table has no `$after`; the rest continue strictly
/// after the last id read. The query is `ORDER BY id ASC LIMIT $n`: the
/// work per batch is what the limit says, not what the table has.
async fn read_batch<C: Connection>(
    db: &Surreal<C>,
    table: &str,
    after: Option<&str>,
    limit: usize,
) -> Result<Vec<Value>, ExportError> {
    let sql = match after {
        None => format!("SELECT * FROM {table} ORDER BY id ASC LIMIT {limit}"),
        Some(_) => format!("SELECT * FROM {table} WHERE id > $after ORDER BY id ASC LIMIT {limit}"),
    };
    let mut query = db.query(sql);
    if let Some(last_id) = after {
        // The parse error of the engine's id grammar is opaque (`()`),
        // so the message names the id itself and what it is for.
        let thing: surrealdb::sql::Thing = last_id.parse().map_err(|_| ExportError::RowDecode {
            table: table.to_string(),
            id: last_id.to_string(),
            detail: "the last id of the previous batch is not a record id this engine can read"
                .to_string(),
        })?;
        query = query.bind(("after", thing));
    }
    let mut response = query.await.map_err(db_error)?;
    result_rows(&mut response)
}

/// The rows of the first statement's result, as canonical JSON.
///
/// The rows travel as SurrealQL values, and the JSON they become is the
/// contract of the export: record ids as `table:key` strings, datetimes
/// as RFC-3339 strings, absent options omitted, keys sorted by the JSON
/// map. That mapping is deterministic, which is what makes the manifest
/// deterministic.
fn result_rows(response: &mut surrealdb::Response) -> Result<Vec<Value>, ExportError> {
    // El Value de la API envuelve el valor de SurrealQL; su interior es
    // el que sabe convertirse a JSON sin perder forma.
    let value: surrealdb::Value = response.take(0).map_err(db_error)?;
    let json = serde_json::Value::from(value.into_inner());
    match json {
        Value::Array(rows) => Ok(rows),
        other => Err(db_error(format!(
            "the statement result is not a row array: {other}"
        ))),
    }
}

/// Serializes one batch to canonical JSON, hashes it, writes it, and
/// rejects a batch whose rows break the identity the schema declares.
fn write_batch(
    config: &ExportConfig,
    table: &str,
    index: usize,
    rows: &[Value],
    identity_fields: &[&str],
) -> Result<BatchEntry, ExportError> {
    let (first_id, last_id) = match (rows.first(), rows.last()) {
        (Some(first), Some(last)) => (record_id(table, first)?, record_id(table, last)?),
        _ => {
            return Err(ExportError::Invalid(format!(
                "the batch of {table} is empty"
            )));
        }
    };
    let mut seen_ids = HashSet::with_capacity(rows.len());
    let mut seen_identities = HashSet::with_capacity(rows.len());
    for row in rows {
        let id = record_id(table, row)?;
        if !seen_ids.insert(id.clone()) {
            return Err(ExportError::DuplicateRow {
                table: table.to_string(),
                key: id,
            });
        }
        if !identity_fields.is_empty() {
            let identity = canonical_identity(table, row, &id, identity_fields)?;
            if !seen_identities.insert(identity.clone()) {
                return Err(ExportError::DuplicateRow {
                    table: table.to_string(),
                    key: identity,
                });
            }
        }
    }
    let bytes = serde_json::to_vec(rows)?;
    let file = format!("{BATCHES_DIR}/{table}-{index:06}.json");
    let path = config.output.join(&file);
    std::fs::create_dir_all(config.output.join(BATCHES_DIR))?;
    write_atomic(&path, &bytes)?;
    Ok(BatchEntry {
        file,
        sha256: sha256_hex(&bytes),
        rows: rows.len() as u64,
        first_id,
        last_id,
    })
}

/// The record id of a row, as the source wrote it (`table:key`).
fn record_id(table: &str, row: &Value) -> Result<String, ExportError> {
    let id = row.get("id").ok_or_else(|| ExportError::RowDecode {
        table: table.to_string(),
        id: String::new(),
        detail: "the row has no id".to_string(),
    })?;
    match id {
        Value::String(text) => Ok(text.clone()),
        Value::Object(object) => {
            // Alguna forma del cliente serializa el id como objeto; la
            // forma canonica es `table:key`.
            let tb =
                object
                    .get("tb")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ExportError::RowDecode {
                        table: table.to_string(),
                        id: String::new(),
                        detail: "the id object has no table".to_string(),
                    })?;
            let key = object
                .get("id")
                .map(|key| match key {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                })
                .ok_or_else(|| ExportError::RowDecode {
                    table: table.to_string(),
                    id: String::new(),
                    detail: "the id object has no key".to_string(),
                })?;
            Ok(format!("{tb}:{key}"))
        }
        _ => Err(ExportError::RowDecode {
            table: table.to_string(),
            id: String::new(),
            detail: "the id is neither a string nor an object".to_string(),
        }),
    }
}

/// The canonical identity of a row: the fields the schema declares
/// unique, joined with `|`. A row missing one of them is truncated.
fn canonical_identity(
    table: &str,
    row: &Value,
    id: &str,
    fields: &[&str],
) -> Result<String, ExportError> {
    let mut parts = Vec::with_capacity(fields.len());
    for field in fields {
        let value = row.get(*field).ok_or_else(|| ExportError::RowDecode {
            table: table.to_string(),
            id: id.to_string(),
            detail: format!("the row has no {field}, which its identity needs"),
        })?;
        let text = match value {
            Value::String(text) => text.clone(),
            Value::Number(number) => number.to_string(),
            Value::Null => "null".to_string(),
            other => other.to_string(),
        };
        parts.push(text);
    }
    Ok(parts.join("|"))
}

/// The frontier of the source: the row count of every exported table and
/// the head of the evidence ledger.
///
/// Every count is a bounded aggregate or a `LIMIT 1` read; nothing here
/// scans a table into memory.
pub async fn capture_frontier<C: Connection>(db: &Surreal<C>) -> Result<Frontier, ExportError> {
    let mut tables = BTreeMap::new();
    for (table, _) in EXPORTED_TABLES {
        tables.insert(table.to_string(), table_count(db, table).await?);
    }
    let ledger_rows = tables.get("event_ledger").copied().unwrap_or(0);
    let head_rows = tables.get("event_ledger_head").copied().unwrap_or(0);
    let ledger_max_position = ledger_head(db).await?;
    Ok(Frontier {
        tables,
        ledger_rows,
        ledger_max_position,
        head_rows,
    })
}

/// The row count of a table, by aggregate: `SELECT count() FROM t GROUP
/// ALL` answers with one row, whatever the table holds.
async fn table_count<C: Connection>(db: &Surreal<C>, table: &str) -> Result<u64, ExportError> {
    let mut response = db
        .query(format!("SELECT count() FROM {table} GROUP ALL"))
        .await
        .map_err(db_error)?;
    let rows = result_rows(&mut response)?;
    let count = rows
        .iter()
        .filter_map(|row| row.get("count").and_then(Value::as_u64))
        .sum();
    Ok(count)
}

/// The highest stream position of the evidence ledger, when it is not
/// empty: a `LIMIT 1` read of the head, not a scan.
async fn ledger_head<C: Connection>(db: &Surreal<C>) -> Result<Option<u64>, ExportError> {
    let mut response = db
        .query(
            "SELECT VALUE stream_position FROM event_ledger ORDER BY stream_position DESC LIMIT 1",
        )
        .await
        .map_err(db_error)?;
    let rows = result_rows(&mut response)?;
    let head = rows
        .first()
        .and_then(Value::as_i64)
        .and_then(|position| u64::try_from(position).ok());
    Ok(head)
}

/// Builds the manifest from what was written.
///
/// Every field is derived from the source content and the tool version;
/// nothing wall-clock enters, so the same source yields the same bytes.
fn build_manifest(
    config: &ExportConfig,
    frontier: &Frontier,
    done: &[ProgressEntry],
) -> ExportManifest {
    let mut tables: Vec<TableEntry> = Vec::new();
    for (table, _) in EXPORTED_TABLES {
        let batches: Vec<BatchEntry> = done
            .iter()
            .filter(|entry| entry.table == *table)
            .map(|entry| entry.batch.clone())
            .collect();
        let rows = batches.iter().map(|batch| batch.rows).sum();
        if !batches.is_empty() {
            tables.push(TableEntry {
                table: table.to_string(),
                rows,
                batches,
            });
        }
    }
    let manifest = ExportManifest {
        format: FORMAT,
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        source: SourceInfo {
            engine: SOURCE_ENGINE.to_string(),
            engine_version: SOURCE_ENGINE_VERSION.to_string(),
            backend: config.backend.clone(),
            namespace: config.namespace.clone(),
            database: config.database.clone(),
        },
        frontier: frontier.clone(),
        tables,
        blobs: BlobPolicy {
            policy: "referenced-by-digest".to_string(),
            copied: false,
        },
        content_sha256: String::new(),
    };
    let mut manifest = manifest;
    manifest.content_sha256 = manifest.content_digest();
    manifest
}

fn serialize_manifest(manifest: &ExportManifest) -> Vec<u8> {
    serde_json::to_vec_pretty(manifest).expect("the manifest serializes")
}

/// Refuses to touch an output directory that already holds an export, and
/// refuses a resume without a marker to resume from.
fn prepare_output(config: &ExportConfig) -> Result<(), ExportError> {
    let manifest_path = config.output.join(MANIFEST_FILE);
    let progress_path = config.output.join(PROGRESS_FILE);
    if config.resume {
        if !progress_path.exists() {
            return Err(ExportError::ResumeMismatch(format!(
                "there is no progress marker at {} to resume",
                progress_path.display()
            )));
        }
        if manifest_path.exists() {
            return Err(ExportError::ResumeMismatch(format!(
                "the export at {} is already finished",
                config.output.display()
            )));
        }
        return Ok(());
    }
    if manifest_path.exists() || progress_path.exists() {
        return Err(ExportError::Invalid(format!(
            "the output {} already holds an export: an export is never overwritten",
            config.output.display()
        )));
    }
    std::fs::create_dir_all(config.output.join(BATCHES_DIR))?;
    Ok(())
}

/// Reads the progress marker.
fn load_progress(config: &ExportConfig) -> Result<ProgressState, ExportError> {
    let bytes = std::fs::read(config.output.join(PROGRESS_FILE))?;
    let state: ProgressState = serde_json::from_slice(&bytes).map_err(|error| {
        ExportError::ResumeMismatch(format!("the progress marker is not readable: {error}"))
    })?;
    if state.format != FORMAT {
        return Err(ExportError::ResumeMismatch(format!(
            "the progress marker has format {} and this build understands format {FORMAT}",
            state.format
        )));
    }
    Ok(state)
}

/// Verifies every recorded batch file against the hash recorded for it.
///
/// This is the truncation detector of a resume: a file that was written
/// halfway hashes to something else, and the run names the file.
fn verify_done_batches(config: &ExportConfig, done: &[ProgressEntry]) -> Result<(), ExportError> {
    for entry in done {
        crate::manifest::verify_batch_file(&config.output, &entry.batch)?;
    }
    Ok(())
}

/// Writes the progress marker atomically: a run that dies while writing
/// it leaves the previous marker, not a torn one.
fn save_progress(
    config: &ExportConfig,
    frontier: &Frontier,
    done: &[ProgressEntry],
    next_table: &str,
    next_batch_index: usize,
) -> Result<(), ExportError> {
    let state = ProgressState {
        format: FORMAT,
        frontier: frontier.clone(),
        done: done.to_vec(),
        next_table: Some(next_table.to_string()),
        next_batch_index,
    };
    let bytes = serde_json::to_vec_pretty(&state)?;
    write_atomic(&config.output.join(PROGRESS_FILE), &bytes)
}

/// Removes the progress marker once the manifest replaces it.
fn remove_progress(config: &ExportConfig) -> Result<(), ExportError> {
    let path = config.output.join(PROGRESS_FILE);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Writes bytes through a temporary file and a rename: readers never see
/// a half-written file.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ExportError> {
    let parent = path
        .parent()
        .ok_or_else(|| ExportError::Io(format!("the path {} has no parent", path.display())))?;
    std::fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".{}.partial",
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "file".to_string())
    ));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// What moved between two frontiers, for the error that says it.
fn mutation_message(before: &Frontier, after: &Frontier) -> String {
    for (table, rows_before) in &before.tables {
        let rows_after = after.tables.get(table).copied().unwrap_or(0);
        if rows_after != *rows_before {
            return format!("the table {table} had {rows_before} rows before and {rows_after} now");
        }
    }
    for (table, rows_after) in &after.tables {
        if !before.tables.contains_key(table) {
            return format!("the table {table} appeared with {rows_after} rows");
        }
    }
    if before.ledger_rows != after.ledger_rows {
        return format!(
            "the ledger had {} rows before and {} now",
            before.ledger_rows, after.ledger_rows
        );
    }
    if before.ledger_max_position != after.ledger_max_position {
        return format!(
            "the ledger head moved from {:?} to {:?}",
            before.ledger_max_position, after.ledger_max_position
        );
    }
    if before.head_rows != after.head_rows {
        return format!(
            "the ledger head table had {} rows before and {} now",
            before.head_rows, after.head_rows
        );
    }
    "the frontier changed in a way this build cannot name".to_string()
}

fn db_error(error: impl std::fmt::Display) -> ExportError {
    ExportError::Db(error.to_string())
}
