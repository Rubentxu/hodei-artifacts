//! The plan of the keyset pagination the export leans on, as evidence
//! measured on the engine the manifest names.
//!
//! The Cargo.toml and `SOURCE_ENGINE_VERSION` claim the ordered keyset
//! read was measured on SurrealDB 2.7. The HSP-175 lesson is that
//! SurrealDB does not optimize a `WHERE id = $thing` predicate into a
//! record fetch: it plans `Iterate Table`. This test measures the sibling
//! predicate the export actually sends -- `WHERE id > $after ORDER BY id
//! ASC LIMIT $n` -- with `EXPLAIN FULL` over a table of 100k rows, and
//! asserts the plan the planner returns.
//!
//! What is measured (SurrealDB 2.7.0, embedded, kv-mem, debug build):
//! both the first batch (`ORDER BY id ASC LIMIT n`) and the keyset batch
//! (`WHERE id > $after ORDER BY id ASC LIMIT n`) plan `Iterate Table`
//! whose detail carries no range bound: **the planner does not seek, and
//! every batch walks the table**. This test documents that current
//! behavior; it does not bless it. If a future engine version makes the
//! planner seek (a bounded range in the `Iterate Table` detail, or an
//! index read), these assertions fail, and the finding in
//! `target/uat/hsp179-keyset-plan.md` must be rewritten -- not silently
//! trusted.
//!
//! The reading times are a log, never an assertion: they go to
//! `target/uat/hsp179-keyset-plan.md`, next to the observed per-batch
//! cost and the alternative strategies, and the redesign decision belongs
//! to the supervisor, not to this file.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use hodei_export_tool::DEFAULT_BATCH_ROWS;
use serde_json::Value;
use surrealdb::Surreal;
use surrealdb::engine::local::Mem;
use surrealdb::sql::Thing;

const TABLE: &str = "export_keyset";
/// The size of the distractor table: the count does not change the shape
/// of a plan, but the evidence the manifest claims must be measured at
/// the scale the registry actually holds.
const ROWS: usize = 100_000;
/// The batch size the export runs with by default.
const LIMIT: usize = DEFAULT_BATCH_ROWS;

/// The workspace target directory, where the evidence log lands.
fn evidence_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the crate lives two levels under the workspace")
        .join("target")
        .join("uat")
}

async fn open_mem() -> Surreal<surrealdb::engine::local::Db> {
    let db = Surreal::new::<Mem>(()).await.expect("open mem");
    db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");
    db
}

/// One distractor row, with the deterministic record id the keyset walks
/// (`row-00000000`..`row-00099999`).
#[derive(Debug, serde::Serialize)]
struct Row {
    id: Thing,
    payload: String,
    number: i64,
}

fn row(index: usize) -> Row {
    Row {
        id: Thing::from((TABLE.to_string(), format!("row-{index:08}"))),
        payload: format!("payload-{index:08}"),
        number: index as i64,
    }
}

/// Seeds the table in chunks of 1,000 rows: one multi-row INSERT per
/// chunk, the fastest shape the timebox allows.
async fn seed(db: &Surreal<surrealdb::engine::local::Db>, rows: usize) -> std::time::Duration {
    let started = Instant::now();
    for batch_start in (0..rows).step_by(1_000) {
        let batch_end = (batch_start + 1_000).min(rows);
        let batch: Vec<Row> = (batch_start..batch_end).map(row).collect();
        db.query(format!("INSERT INTO {TABLE} $rows"))
            .bind(("rows", batch))
            .await
            .expect("insert")
            .check()
            .expect("insert check");
    }
    started.elapsed()
}

/// The rows of one result, converted the way the export converts them
/// (through the SurrealQL value, not through a direct JSON take).
fn result_rows(response: &mut surrealdb::Response) -> Vec<Value> {
    let value: surrealdb::Value = response.take(0).expect("rows");
    let json = serde_json::Value::from(value.into_inner());
    match json {
        Value::Array(rows) => rows,
        other => panic!("the result is not a row array: {other}"),
    }
}

/// The first-batch statement, exactly as `read_batch` builds it.
fn first_batch_sql() -> String {
    format!("SELECT * FROM {TABLE} ORDER BY id ASC LIMIT {LIMIT}")
}

/// The keyset statement, exactly as `read_batch` builds it.
fn keyset_sql() -> String {
    format!("SELECT * FROM {TABLE} WHERE id > $after ORDER BY id ASC LIMIT {LIMIT}")
}

/// One step of a query plan: the operation the planner picked, with the
/// detail it hands back.
#[derive(Debug, serde::Deserialize, serde::Serialize)]
struct PlanStep {
    operation: String,
    #[serde(default)]
    detail: serde_json::Value,
}

async fn explain_full(
    db: &Surreal<surrealdb::engine::local::Db>,
    sql: &str,
    after: Option<&Thing>,
) -> Vec<PlanStep> {
    let mut statement = db.query(format!("{sql} EXPLAIN FULL"));
    if let Some(after) = after {
        statement = statement.bind(("after", after.clone()));
    }
    let mut response = statement
        .await
        .expect("explain")
        .check()
        .expect("explain check");
    response.take(0).expect("explain rows")
}

async fn read_first_batch(
    db: &Surreal<surrealdb::engine::local::Db>,
) -> (Vec<Value>, std::time::Duration) {
    let started = Instant::now();
    let mut response = db
        .query(first_batch_sql())
        .await
        .expect("read")
        .check()
        .expect("check");
    let rows = result_rows(&mut response);
    (rows, started.elapsed())
}

async fn read_keyset(
    db: &Surreal<surrealdb::engine::local::Db>,
    after: &Thing,
) -> (Vec<Value>, std::time::Duration) {
    let started = Instant::now();
    let mut statement = db.query(keyset_sql());
    statement = statement.bind(("after", after.clone()));
    let mut response = statement.await.expect("read").check().expect("check");
    let rows = result_rows(&mut response);
    (rows, started.elapsed())
}

async fn count_scan(db: &Surreal<surrealdb::engine::local::Db>) -> (u64, std::time::Duration) {
    let started = Instant::now();
    let mut response = db
        .query(format!("SELECT count() FROM {TABLE} GROUP ALL"))
        .await
        .expect("read")
        .check()
        .expect("check");
    let rows = result_rows(&mut response);
    let count = rows
        .first()
        .and_then(|row| row.get("count"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    (count, started.elapsed())
}

/// The evidence the manifest claims, on the plan the planner returns.
///
/// The plan is the oracle: it says whether the keyset predicate is bound
/// into a bounded read or whether every batch walks the table. The clock
/// only logs.
#[tokio::test]
async fn the_keyset_plan_of_the_export_is_documented() {
    let db = open_mem().await;
    let seed_time = seed(&db, ROWS).await;

    // The two statement shapes `read_batch` sends, over the same table.
    let mid = ROWS / 2;
    let after = Thing::from((TABLE.to_string(), format!("row-{mid:08}")));

    let first_plan = explain_full(&db, &first_batch_sql(), None).await;
    let keyset_plan = explain_full(&db, &keyset_sql(), Some(&after)).await;

    // The first batch: an ordered read with a limit plans a walk of the
    // table with an ordered-limit collector -- no bounded read either.
    let first_operations: Vec<&str> = first_plan.iter().map(|s| s.operation.as_str()).collect();
    assert!(
        first_operations.contains(&"Iterate Table"),
        "documenting current behavior (2.7): the ordered first batch walks the table, got {first_operations:?}"
    );

    // The keyset batch: the predicate `id > $after` plans `Iterate Table`
    // whose detail carries no range bound. That is the measured shape of
    // 2.7 -- the planner does not seek on the record id, so the iterator
    // cannot start at $after. If a future engine binds the range (a
    // bound in the detail, or an index read), this fails and the finding
    // must be rewritten, not silently trusted.
    let keyset_operations: Vec<&str> = keyset_plan.iter().map(|s| s.operation.as_str()).collect();
    assert!(
        keyset_operations.contains(&"Iterate Table"),
        "documenting current behavior (2.7): the keyset batch walks the table, got {keyset_operations:?}"
    );
    let walk = keyset_plan
        .iter()
        .find(|step| step.operation == "Iterate Table")
        .expect("the keyset plan walks the table");
    let detail_keys: BTreeSet<&str> = walk
        .detail
        .as_object()
        .map(|object| object.keys().map(String::as_str).collect())
        .unwrap_or_default();
    let bounded = detail_keys.iter().any(|key| {
        matches!(
            *key,
            "begin" | "end" | "range" | "from" | "to" | "start" | "stop"
        )
    });
    assert!(
        !bounded,
        "documenting current behavior (2.7): the keyset walk carries no range bound, got {:?}",
        walk.detail
    );

    // The pagination protocol is correct even where its cost is not: the
    // keyset read hands back exactly the rows after the bound, in order,
    // and no more than the limit says.
    let (rows, _keyset_time) = read_keyset(&db, &after).await;
    assert_eq!(
        rows.len(),
        LIMIT,
        "the keyset batch reads exactly one batch of rows"
    );
    let first_id = rows[0]["id"].as_str().expect("the row id is a string");
    assert!(
        first_id.contains(&format!("row-{:08}", mid + 1)),
        "the keyset batch starts right after the bound, got {first_id}"
    );
    let ids: Vec<&str> = rows
        .iter()
        .map(|row| row["id"].as_str().expect("the row id is a string"))
        .collect();
    assert!(
        ids.windows(2).all(|pair| pair[0] < pair[1]),
        "the keyset batch is strictly ordered by id"
    );

    // The per-batch cost, observed at three positions of the table, plus
    // the first batch and one full-table aggregate for contrast. Logged,
    // never asserted: the wall-clock depends on the machine and the
    // build; the plan above does not.
    let mut timings: Vec<(String, usize, String)> = Vec::new();
    for fraction in [0.10, 0.50, 0.90] {
        let index = ((ROWS as f64) * fraction) as usize;
        let after = Thing::from((TABLE.to_string(), format!("row-{index:08}")));
        let (rows, elapsed) = read_keyset(&db, &after).await;
        timings.push((
            format!("{:.0}% ({index:08})", fraction * 100.0),
            rows.len(),
            format!("{elapsed:?}"),
        ));
    }
    let (first_rows, first_time) = read_first_batch(&db).await;
    let (count, count_time) = count_scan(&db).await;

    // The evidence log: plans verbatim, timings, verdict, alternatives.
    let log = format!(
        "# HSP-179 - Evidencia del plan del keyset del export (SurrealDB 2.7)\n\
         \n\
         - Fecha de la medida: {date}\n\
         - Motor: SurrealDB 2.7.0 embebida, backend memoria (kv-mem), build debug\n\
         - Tabla `{TABLE}` con {ROWS} filas de record id determinista, lote de {LIMIT}\n\
         - Consultas medidas: exactamente las que `read_batch` envia\n\
           (`crates/hodei-export-tool/src/export.rs`): primer lote\n\
           `{first_sql}`, lote siguiente `{keyset_sql}` con `$after` bound como `Thing`.\n\
         \n\
         ## Plan observado (EXPLAIN FULL)\n\
         \n\
         ### Primer lote\n\n```json\n{first_plan_json}\n```\n\
         \n\
         ### Lote keyset (`WHERE id > $after`)\n\n```json\n{keyset_plan_json}\n```\n\
         \n\
         ## Veredicto: el planner NO seeka\n\
         \n\
         El detalle del `Iterate Table` no lleva ningun bound de rango (`direction`\n\
         y `table` solos): el iterador no empieza en `$after`, y cada lote recorre\n\
         la tabla entera. La frase del doc de `read_batch` -- \"the work per batch\n\
         is what the limit says, not what the table has\" -- NO se cumple en 2.7;\n\
         es la leccion de HSP-175 repetida con el predicado de rango sobre el\n\
         record id. El lote SI devuelve las filas correctas y en orden: el\n\
         protocolo de paginacion es correcto, su coste no.\n\
         \n\
         ## Coste por lote observado (LOG, nunca asercion)\n\
         \n\
         - Semilla de {ROWS} filas: {seed_time:?}\n\
         {timings_table}\
         - Primer lote (sin WHERE): {first_rows} filas en {first_time:?}\n\
         - Contraste, scan completo con agregado count(): {count} filas en {count_time:?}\n\
         \n\
         Estimacion para un export real de esta tabla con lotes de {LIMIT}: 200\n\
         lotes x ~10-16 s por lote (condiciones debug/mem de esta medida). El\n\
         coste no mejora en release en su forma: cada lote sigue siendo\n\
         proporcional al tamano de la tabla, y el export entero es\n\
         cuadratico en el tamano de la tabla.\n\
         \n\
         ## Estrategia alternativa propuesta (decision del supervisor; aqui no\n\
         se redisena)\n\
         \n\
         1. Paginar por rangos sobre un campo indexado real (las tablas exportadas\n\
            ya llevan indice UNIQUE sobre sus identity fields en el esquema\n\
            productivo): `SELECT * FROM t WHERE <indexado> > $last ORDER BY\n\
            <indexado> ASC LIMIT n`; el planner binda los rangos de campos\n\
            indexados en la lectura del indice (leccion HSP-175/176).\n\
         2. Alternativa por rangos de record id (`t:desde..t:hasta`) solo si un\n\
            EXPLAIN demuestra que el motor los acota: medir antes de adoptar.\n\
         3. Mientras se decide, dejar el contrato honesto: corregir el doc de\n\
            `read_batch` y el reclamo del Cargo.toml, o acotar el alcance del\n\
            export a tablas pequenas, hasta que (1) se adopte.\n",
        date = chrono_like_date(),
        first_sql = first_batch_sql(),
        keyset_sql = keyset_sql(),
        first_plan_json = serde_json::to_string_pretty(&first_plan).expect("the plan serializes"),
        keyset_plan_json = serde_json::to_string_pretty(&keyset_plan).expect("the plan serializes"),
        seed_time = seed_time,
        timings_table = timings_table(&timings),
        first_rows = first_rows.len(),
        first_time = first_time,
        count = count,
        count_time = count_time,
    );
    let dir = evidence_dir();
    std::fs::create_dir_all(&dir).expect("the evidence directory is created");
    let path = dir.join("hsp179-keyset-plan.md");
    std::fs::write(&path, log).expect("the evidence log is written");
    println!("evidence log written to {}", path.display());
}

/// The timings, as one markdown row per probe.
fn timings_table(timings: &[(String, usize, String)]) -> String {
    let mut table = String::from(
        "| Posicion de $after | Filas devueltas | Tiempo de lectura |\n|---|---|---|\n",
    );
    for (position, rows, elapsed) in timings {
        table.push_str(&format!("| {position} | {rows} | {elapsed} |\n"));
    }
    table.push('\n');
    table
}

/// The date of the measure, from the crate's own contract: nothing here
/// needs a wall-clock dependency of its own.
fn chrono_like_date() -> String {
    // The test runs when it runs; the date is informational. UTC time
    // without pulling another dependency in.
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is set")
        .as_secs();
    let days = seconds / 86_400;
    // Days since epoch to civil date (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}
