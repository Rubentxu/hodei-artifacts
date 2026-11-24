//! The plan of the keyset pagination the export leans on, as evidence
//! measured on the engine the manifest names (SurrealDB 2.7).
//!
//! The HSP-175 lesson is that SurrealDB does not optimize a record-id
//! predicate into a bounded read: `WHERE id > $after ORDER BY id ASC
//! LIMIT n` plans `Iterate Table` with no bound, so every batch walked
//! the whole table and a full export was quadratic. The export now reads
//! a record-id range instead (`SELECT * FROM $range LIMIT n`, with
//! `$range` = `table:'after'>..`), which the planner answers with
//! `Iterate Range`: the bound is in the plan and the limit is pushed into
//! the scan.
//!
//! The oracle is the plan, never the clock (style of
//! `hsp176_capsule_subject_bounded.rs`): the plan over an empty table and
//! over a table with a distractor of 2,000+ rows has the same shape, the
//! range bound is in the detail, and the rows the scan fetches per batch
//! are at most the limit. This holds for every table the export reads.
//! The banned predicate is kept as a control: it must still plan a table
//! walk, which is the shape the export must never carry again.

use hodei_export_tool::manifest::EXPORTED_TABLES;
use hodei_export_tool::{batch_range, batch_sql};
use serde_json::Value;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, Mem};
use surrealdb::sql::{Id, Thing};

const DISTRACTOR_ROWS: usize = 2_500;
const LIMIT: usize = 100;

async fn open_mem() -> Surreal<Db> {
    let db = Surreal::new::<Mem>(()).await.expect("open mem");
    db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");
    db
}

/// The plan of a statement, as the JSON steps the engine returns.
async fn plan(db: &Surreal<Db>, sql: &str, binds: Vec<(&'static str, Thing)>) -> Vec<Value> {
    let mut statement = db.query(format!("{sql} EXPLAIN FULL"));
    for bind in binds {
        statement = statement.bind(bind);
    }
    let mut response = statement
        .await
        .expect("explain")
        .check()
        .expect("explain check");
    let value: surrealdb::Value = response.take(0).expect("explain rows");
    match serde_json::Value::from(value.into_inner()) {
        Value::Array(steps) => steps,
        other => panic!("the plan is not an array: {other}"),
    }
}

fn operations(plan: &[Value]) -> Vec<String> {
    plan.iter()
        .map(|step| step["operation"].as_str().unwrap_or("?").to_string())
        .collect()
}

fn without_fetch(plan: &[Value]) -> Vec<&Value> {
    plan.iter()
        .filter(|step| step["operation"] != "Fetch")
        .collect()
}

fn step<'a>(plan: &'a [Value], operation: &str) -> &'a Value {
    plan.iter()
        .find(|step| step["operation"] == operation)
        .unwrap_or_else(|| panic!("the plan has no {operation} step: {plan:?}"))
}

async fn seed(db: &Surreal<Db>, table: &str, rows: usize) {
    db.query(format!(
        "FOR $i IN 0..{rows} {{ CREATE type::thing('{table}', string::concat('row-', <string>$i)) SET n = $i; }}"
    ))
    .await
    .expect("seed")
    .check()
    .expect("seed check");
}

fn after_key(index: usize) -> Id {
    Id::String(format!("row-{index}"))
}

async fn ids_of(db: &Surreal<Db>, table: &str, after: Option<Id>) -> Vec<String> {
    let mut response = db
        .query(batch_sql(LIMIT))
        .bind(("range", batch_range(table, after).expect("range")))
        .await
        .expect("read")
        .check()
        .expect("check");
    let value: surrealdb::Value = response.take(0).expect("rows");
    match serde_json::Value::from(value.into_inner()) {
        Value::Array(rows) => rows
            .iter()
            .map(|row| row["id"].as_str().expect("id").to_string())
            .collect(),
        other => panic!("not rows: {other}"),
    }
}

/// For each of the ten exported tables, the batch plan seeks: an
/// `Iterate Range` whose detail carries the bound, with the limit pushed
/// into the scan, and the same shape whether the table is empty or holds
/// a distractor.
#[tokio::test]
async fn the_batch_plan_of_every_exported_table_seeks_and_does_not_grow() {
    let db = open_mem().await;
    assert_eq!(EXPORTED_TABLES.len(), 10, "the export covers ten tables");
    let mut empty_plans = Vec::new();
    for (table, _) in EXPORTED_TABLES {
        let first = plan(
            &db,
            &batch_sql(LIMIT),
            vec![("range", batch_range(table, None).unwrap())],
        )
        .await;
        let next = plan(
            &db,
            &batch_sql(LIMIT),
            vec![("range", batch_range(table, Some(after_key(1_000))).unwrap())],
        )
        .await;
        empty_plans.push((first, next));
    }

    for (table, _) in EXPORTED_TABLES {
        seed(&db, table, DISTRACTOR_ROWS).await;
    }

    for ((table, _), (empty_first, empty_next)) in EXPORTED_TABLES.iter().zip(&empty_plans) {
        let first = plan(
            &db,
            &batch_sql(LIMIT),
            vec![("range", batch_range(table, None).unwrap())],
        )
        .await;
        let next = plan(
            &db,
            &batch_sql(LIMIT),
            vec![("range", batch_range(table, Some(after_key(1_000))).unwrap())],
        )
        .await;

        // Same shape empty and seeded, first batch and keyset batch.
        assert_eq!(
            operations(empty_first),
            operations(&first),
            "{table}: first batch shape moved"
        );
        assert_eq!(
            operations(empty_next),
            operations(&next),
            "{table}: keyset batch shape moved"
        );
        assert_eq!(
            without_fetch(empty_next),
            without_fetch(&next),
            "{table}: the keyset plan must not depend on the table size"
        );

        for batch in [&first, &next] {
            let ops = operations(batch);
            assert!(
                ops.contains(&"Iterate Range".to_string()),
                "{table}: must seek, got {ops:?}"
            );
            assert!(
                !ops.contains(&"Iterate Table".to_string()),
                "{table}: must not walk the table, got {ops:?}"
            );
            // The limit is pushed into the scan: the rows fetched per
            // batch are at most the limit, whatever the table holds.
            assert_eq!(
                step(batch, "StartLimitStrategy")["detail"]["CancelOnLimit"],
                LIMIT,
                "{table}"
            );
            assert!(
                step(batch, "Fetch")["detail"]["count"].as_u64().unwrap() as usize <= LIMIT,
                "{table}"
            );
        }
        // The keyset batch carries its bound in the plan.
        let range = step(&next, "Iterate Range")["detail"]["range"].to_string();
        assert!(
            range.contains("Excluded") && range.contains("row-1000"),
            "{table}: bound missing: {range}"
        );
    }
}

/// The protocol is correct: each batch starts strictly after the bound,
/// in the key order `ORDER BY id ASC` gave, and walking the table batch
/// by batch visits every row exactly once.
#[tokio::test]
async fn the_range_walk_visits_every_row_once_in_id_order() {
    let db = open_mem().await;
    let table = "catalog_tag";
    seed(&db, table, DISTRACTOR_ROWS).await;

    let mut all: Vec<String> = Vec::new();
    let mut after: Option<Id> = None;
    loop {
        let ids = ids_of(&db, table, after.clone()).await;
        if ids.is_empty() {
            break;
        }
        assert!(ids.len() <= LIMIT);
        let last: Thing = ids.last().unwrap().parse().expect("id parses");
        after = Some(last.id);
        all.extend(ids);
    }
    assert_eq!(all.len(), DISTRACTOR_ROWS);
    // Same order the previous `ORDER BY id ASC` produced.
    let mut response = db
        .query(format!("SELECT id FROM {table} ORDER BY id ASC"))
        .await
        .expect("order")
        .check()
        .expect("check");
    let value: surrealdb::Value = response.take(0).expect("rows");
    let ordered: Vec<String> = match serde_json::Value::from(value.into_inner()) {
        Value::Array(rows) => rows
            .iter()
            .map(|row| row["id"].as_str().expect("id").to_string())
            .collect(),
        other => panic!("not rows: {other}"),
    };
    assert_eq!(all, ordered, "the range walk equals the ORDER BY id order");
}

/// Control: the predicate the export used to send still plans a table
/// walk with no bound. If the engine ever seeks on it, this fails and the
/// batch statement can be reconsidered; until then the range is the
/// strategy that the planner proves.
#[tokio::test]
async fn the_banned_where_id_predicate_still_walks_the_table() {
    let db = open_mem().await;
    seed(&db, "export_keyset", DISTRACTOR_ROWS).await;
    let after = Thing::from(("export_keyset".to_string(), after_key(1_000)));
    let banned = plan(
        &db,
        &format!("SELECT * FROM export_keyset WHERE id > $after ORDER BY id ASC LIMIT {LIMIT}"),
        vec![("after", after)],
    )
    .await;
    let ops = operations(&banned);
    assert!(ops.contains(&"Iterate Table".to_string()), "got {ops:?}");
    assert!(
        step(&banned, "Iterate Table")["detail"]
            .get("range")
            .is_none(),
        "the WHERE id predicate carries no range bound: {banned:?}"
    );
}
