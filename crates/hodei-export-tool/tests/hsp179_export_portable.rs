//! The portable semantic export is deterministic and refuses damage.
//!
//! The export is the front half of the migration contract (doc-20): it
//! reads the one authority in bounded batches, writes canonical JSON
//! batches and closes with a manifest whose SHA-256 digests pin every
//! batch and the whole content. The oracles here:
//!
//! - **Determinism**: two sources seeded with identical content produce
//!   byte-identical exports, batch files included. Nothing wall-clock
//!   may enter the manifest.
//! - **Blobs never travel**: the manifest says so, the digest fields
//!   travel with the rows, and no blob bytes appear in the export.
//! - **Damage is named**: a row missing an identity field (truncated
//!   source row), a repeated canonical identity (broken uniqueness), a
//!   source that moved under the export (frontier mismatch) and a batch
//!   file that no longer hashes to what was recorded (truncated batch)
//!   all fail with typed errors carrying the culprit.
//! - **Resume**: a paged run continues from the progress marker and
//!   produces the same manifest a single run would have.

use std::path::{Path, PathBuf};

use hodei_catalog_storage::surreal::SCHEMA;
use hodei_export_tool::{
    ExportConfig, ExportError, FORMAT, MANIFEST_FILE, PROGRESS_FILE, capture_frontier, export,
    export_with_frontier,
};
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, Mem};

/// Rows per batch small enough that the fixture paginates for real.
const BATCH_ROWS: usize = 2;

/// The fixture: accounts, repositories, assets, versions, references,
/// tags, grants, provenance receipts, the audit outbox and the evidence
/// ledger, every row with fixed ids, fixed digests and fixed instants.
const FIXTURE: &str = r#"
CREATE catalog_repository:eu_lab_npm SET
    account = 'eu', repo_key = 'lab_npm',
    hrn = 'hodei:artifacts:eu:repository/lab_npm',
    repo_format = 'npm', repo_mode = 'hosted', config_version = 1,
    digest_scope = 'global', config_settings = {},
    created_at = d'2026-01-01T00:00:00Z', updated_at = d'2026-01-01T00:00:00Z';
CREATE catalog_repository:us_lab_cargo SET
    account = 'us', repo_key = 'lab_cargo',
    hrn = 'hodei:artifacts:us:repository/lab_cargo',
    repo_format = 'cargo', repo_mode = 'hosted', config_version = 1,
    digest_scope = 'scoped', config_settings = {},
    created_at = d'2026-01-01T00:00:00Z', updated_at = d'2026-01-01T00:00:00Z';
CREATE catalog_version:eu_one_100 SET
    account = 'eu', repository = 'lab_npm', package_name = 'one',
    package_version = '1.0.0', digest_algorithm = 'sha256',
    digest = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
    size_bytes = 11, published_at = d'2026-01-02T03:04:05Z';
CREATE catalog_version:eu_one_101 SET
    account = 'eu', repository = 'lab_npm', package_name = 'one',
    package_version = '1.0.1', digest_algorithm = 'sha256',
    digest = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
    size_bytes = 12, published_at = d'2026-01-03T03:04:05Z';
CREATE catalog_version:us_two_010 SET
    account = 'us', repository = 'lab_cargo', package_name = 'two',
    package_version = '0.1.0', digest_algorithm = 'sha256',
    digest = 'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
    size_bytes = 13, published_at = d'2026-01-04T03:04:05Z';
CREATE catalog_asset:eu_a100_tgz SET
    account = 'eu', repository = 'lab_npm', asset_path = 'one/one-1.0.0.tgz',
    package_name = 'one', package_version = '1.0.0', digest_algorithm = 'sha256',
    digest = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
    size_bytes = 11, subject = NONE, updated_at = d'2026-01-02T03:04:05Z';
CREATE catalog_asset:eu_a101_tgz SET
    account = 'eu', repository = 'lab_npm', asset_path = 'one/one-1.0.1.tgz',
    package_name = 'one', package_version = '1.0.1', digest_algorithm = 'sha256',
    digest = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
    size_bytes = 12, subject = NONE, updated_at = d'2026-01-03T03:04:05Z';
CREATE catalog_asset:us_a010_crate SET
    account = 'us', repository = 'lab_cargo', asset_path = 'two/two-0.1.0.crate',
    package_name = 'two', package_version = '0.1.0', digest_algorithm = 'sha256',
    digest = 'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
    size_bytes = 13, subject = NONE, updated_at = d'2026-01-04T03:04:05Z';
CREATE catalog_asset:eu_a100_readme SET
    account = 'eu', repository = 'lab_npm', asset_path = 'one/readme.md',
    package_name = 'one', package_version = '1.0.0', digest_algorithm = 'sha256',
    digest = 'dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',
    size_bytes = 4, subject = NONE, updated_at = d'2026-01-02T03:04:05Z';
CREATE catalog_reference:eu_ref_100 SET
    account = 'eu', repository = 'lab_npm', asset_path = 'one/one-1.0.0.tgz',
    digest_algorithm = 'sha256',
    digest = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
    digest_scope = 'global', trust_state = 'trusted', size_bytes = 11,
    updated_at = d'2026-01-02T03:04:05Z';
CREATE catalog_reference:eu_ref_101 SET
    account = 'eu', repository = 'lab_npm', asset_path = 'one/one-1.0.1.tgz',
    digest_algorithm = 'sha256',
    digest = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
    digest_scope = 'global', trust_state = 'pending', size_bytes = 12,
    updated_at = d'2026-01-03T03:04:05Z';
CREATE catalog_tag:eu_stable SET
    account = 'eu', repository = 'lab_npm', tag_name = 'stable',
    package_name = 'one', package_version = '1.0.0',
    updated_at = d'2026-01-02T03:04:05Z';
CREATE catalog_tag:eu_latest SET
    account = 'eu', repository = 'lab_npm', tag_name = 'latest',
    package_name = 'one', package_version = '1.0.1',
    updated_at = d'2026-01-03T03:04:05Z';
CREATE trust_grant:grant_live SET
    account = 'eu', repository = 'lab_npm', digest_algorithm = 'sha256',
    digest = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
    decision_id = 'decision-live', actor = 'operator',
    granted_at = d'2026-01-02T03:04:05Z', revoked_at = NONE, revoked_by = NONE,
    revoke_reason = NONE, subject = NONE, policy_version = NONE,
    capsule_hash = NONE, risk_version = NONE, valid_until = NONE;
CREATE trust_grant:grant_revoked SET
    account = 'eu', repository = 'lab_npm', digest_algorithm = 'sha256',
    digest = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',
    decision_id = 'decision-revoked', actor = 'operator',
    granted_at = d'2026-01-03T03:04:05Z', revoked_at = d'2026-01-05T03:04:05Z',
    revoked_by = 'auditor', revoke_reason = 'incident', subject = NONE,
    policy_version = NONE, capsule_hash = NONE, risk_version = NONE,
    valid_until = NONE;
CREATE provenance_receipt:receipt_one SET
    receipt_id = 'receipt-one', account = 'eu', repository = 'lab_npm',
    asset_path = 'one/one-1.0.0.tgz', upstream_id = 'upstream-a',
    source_url = 'https://upstream.example/one-1.0.0.tgz', config_version = 1,
    digest_algorithm = 'sha256',
    digest = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',
    size_bytes = 11, checksum_kind = 'sha256', checksum_header = NONE,
    status = 200, redirects = 0, etag = NONE, last_modified = NONE,
    outcome = NONE, actor = 'proxy', correlation_id = 'corr-one',
    fetched_at = d'2026-01-02T03:04:05Z';
CREATE provenance_receipt:receipt_two SET
    receipt_id = 'receipt-two', account = 'us', repository = 'lab_cargo',
    asset_path = 'two/two-0.1.0.crate', upstream_id = 'upstream-b',
    source_url = 'https://upstream.example/two-0.1.0.crate', config_version = 1,
    digest_algorithm = 'sha256',
    digest = 'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',
    size_bytes = 13, checksum_kind = 'sha256', checksum_header = NONE,
    status = 200, redirects = 0, etag = NONE, last_modified = NONE,
    outcome = NONE, actor = 'proxy', correlation_id = 'corr-two',
    fetched_at = d'2026-01-04T03:04:05Z';
CREATE catalog_audit_outbox:outbox_one SET
    event_id = 'event-one', tenant = 'eu', actor = 'publisher',
    correlation_id = 'corr-one', action = 'publish', resource = 'one@1.0.0',
    digest = NONE, result = 'accepted', occurred_at = d'2026-01-02T03:04:05Z',
    state = 'pending', attempts = 0, lease_owner = NONE, lease_until = NONE,
    last_error = NONE, delivered_at = NONE;
CREATE catalog_audit_outbox:outbox_two SET
    event_id = 'event-two', tenant = 'eu', actor = 'publisher',
    correlation_id = 'corr-two', action = 'publish', resource = 'one@1.0.1',
    digest = NONE, result = 'accepted', occurred_at = d'2026-01-03T03:04:05Z',
    state = 'delivered', attempts = 1, lease_owner = NONE, lease_until = NONE,
    last_error = NONE, delivered_at = d'2026-01-03T03:05:05Z';
CREATE event_ledger:⟨stream_a,0⟩ SET
    stream_id = 'stream_a', stream_position = 0, event_type = 'published',
    payload = { package: 'one', version: '1.0.0' },
    occurred_at = d'2026-01-02T03:04:05Z';
CREATE event_ledger:⟨stream_a,1⟩ SET
    stream_id = 'stream_a', stream_position = 1, event_type = 'granted',
    payload = { decision_id: 'decision-live' },
    occurred_at = d'2026-01-02T03:04:06Z';
CREATE event_ledger:⟨stream_b,0⟩ SET
    stream_id = 'stream_b', stream_position = 0, event_type = 'published',
    payload = { package: 'two', version: '0.1.0' },
    occurred_at = d'2026-01-04T03:04:05Z';
CREATE event_ledger_head:⟨stream_a⟩ SET
    stream_id = 'stream_a', stream_position = 1,
    updated_at = d'2026-01-02T03:04:06Z';
CREATE event_ledger_head:⟨stream_b⟩ SET
    stream_id = 'stream_b', stream_position = 0,
    updated_at = d'2026-01-04T03:04:05Z';
"#;

async fn open_seeded() -> Surreal<Db> {
    let db = Surreal::new::<Mem>(()).await.expect("open mem");
    db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");
    run(&db, SCHEMA).await;
    run(&db, FIXTURE).await;
    db
}

async fn open_empty() -> Surreal<Db> {
    let db = Surreal::new::<Mem>(()).await.expect("open mem");
    db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");
    run(&db, SCHEMA).await;
    db
}

/// Runs a script and fails the test if any statement failed.
async fn run(db: &Surreal<Db>, script: &str) {
    let response = db.query(script).await.expect("run script");
    response.check().expect("script statements");
}

fn config(output: PathBuf) -> ExportConfig {
    let mut config = ExportConfig::new("hodei", "artifacts", "mem", output);
    config.batch_rows = BATCH_ROWS;
    config
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("hsp179-export-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// The relative paths of every file in the export, sorted.
fn export_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_files(dir, dir, &mut files);
    files.sort();
    files
}

fn collect_files(root: &Path, dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let entry = entry.expect("entry");
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, files);
        } else {
            files.push(path.strip_prefix(root).expect("relative").to_path_buf());
        }
    }
}

/// Reads the manifest of a finished export.
#[allow(dead_code)]
fn read_manifest(dir: &Path) -> hodei_export_tool::ExportManifest {
    let bytes = std::fs::read(dir.join(MANIFEST_FILE)).expect("manifest bytes");
    serde_json::from_slice(&bytes).expect("manifest json")
}

/// The batches of a table, from the manifest of an export.
fn batches_of<'a>(
    manifest: &'a hodei_export_tool::ExportManifest,
    table: &str,
) -> &'a hodei_export_tool::TableEntry {
    manifest
        .tables
        .iter()
        .find(|entry| entry.table == table)
        .expect("table in manifest")
}

#[tokio::test]
async fn the_same_source_yields_identical_export_bytes() {
    let db_a = open_seeded().await;
    let db_b = open_seeded().await;
    let dir_a = scratch("determinism-a");
    let dir_b = scratch("determinism-b");

    let manifest_a = export(&db_a, &config(dir_a.clone()))
        .await
        .expect("export a");
    let manifest_b = export(&db_b, &config(dir_b.clone()))
        .await
        .expect("export b");
    assert_eq!(manifest_a, manifest_b);

    let files_a = export_files(&dir_a);
    let files_b = export_files(&dir_b);
    assert_eq!(files_a, files_b);
    assert!(files_a.contains(&PathBuf::from(MANIFEST_FILE)));
    for file in &files_a {
        let bytes_a = std::fs::read(dir_a.join(file)).expect("bytes a");
        let bytes_b = std::fs::read(dir_b.join(file)).expect("bytes b");
        assert_eq!(bytes_a, bytes_b, "{file:?} must be identical");
    }

    // The content digest is the manifest's own claim, and the manifest
    // verifies against the files on disk.
    manifest_a.verify(&dir_a).expect("verified export");
    // The fixture paginates for real: several batches across tables.
    assert!(
        files_a
            .iter()
            .filter(|file| file.starts_with("batches/"))
            .count()
            > 5,
        "the fixture must produce several batches"
    );
}

#[tokio::test]
async fn blobs_are_referenced_by_digest_and_never_copied() {
    let db = open_seeded().await;
    let dir = scratch("blobs");
    let manifest = export(&db, &config(dir.clone())).await.expect("export");

    assert!(!manifest.blobs.copied);
    assert_eq!(manifest.blobs.policy, "referenced-by-digest");

    // The asset rows carry their digests: the digest is the reference.
    let assets = batches_of(&manifest, "catalog_asset");
    let bytes = std::fs::read(dir.join(&assets.batches[0].file)).expect("asset batch");
    let rows: Vec<serde_json::Value> = serde_json::from_slice(&bytes).expect("rows");
    assert!(rows.iter().all(|row| row["digest"].is_string()));
    assert!(rows.iter().all(|row| row["digest_algorithm"] == "sha256"));

    // The export carries only manifest and batches: no blob bytes.
    for file in export_files(&dir) {
        assert!(
            file.starts_with("batches/") || file == Path::new(MANIFEST_FILE),
            "{file:?}"
        );
    }
}

#[tokio::test]
async fn a_row_without_its_identity_field_is_rejected() {
    let db = open_seeded().await;
    // Una fila truncada del origen: sin el campo que su identidad
    // declara. El esquema del origen la rechaza hoy, asi que la deriva
    // que la deja entrar es que el campo deja de estar declarado: la
    // fila se escribe incompleta y el export la nombra.
    run(&db, "REMOVE FIELD IF EXISTS asset_path ON catalog_asset;").await;
    run(
        &db,
        "CREATE catalog_asset:broken_row SET
            account = 'eu', repository = 'lab_npm',
            package_name = 'one', package_version = '9.9.9',
            digest_algorithm = 'sha256',
            digest = 'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee',
            size_bytes = 1, subject = NONE, updated_at = d'2026-01-06T03:04:05Z';",
    )
    .await;
    let dir = scratch("truncated-row");
    let error = export(&db, &config(dir)).await.expect_err("rejected");
    match error {
        ExportError::RowDecode { table, id, detail } => {
            assert_eq!(table, "catalog_asset");
            assert_eq!(id, "catalog_asset:broken_row");
            assert!(detail.contains("asset_path"), "{detail}");
        }
        other => panic!("expected RowDecode, got {other:?}"),
    }
}

#[tokio::test]
async fn a_repeated_identity_is_rejected() {
    let db = open_seeded().await;
    // La corrupcion que la exportacion si puede ver: la unicidad que el
    // esquema declara ya no sostiene la tabla (indice perdido, escritura
    // directa), y dos filas comparten la identidad canonica. La lectura
    // de identidad es por lote acotado: la prueba deja el par en el
    // mismo lote, que es el caso que el export detiene; un par separado
    // en dos lotes lo detiene el import al aplicar los indices unicos.
    run(
        &db,
        "REMOVE INDEX IF EXISTS catalog_tag_unique ON catalog_tag;",
    )
    .await;
    run(
        &db,
        "CREATE catalog_tag:eu_stable_shadow SET
            account = 'eu', repository = 'lab_npm', tag_name = 'stable',
            package_name = 'one', package_version = '1.0.0',
            updated_at = d'2026-01-02T03:04:05Z';",
    )
    .await;
    let dir = scratch("duplicate");
    let mut config = config(dir);
    config.batch_rows = 10;
    let error = export(&db, &config).await.expect_err("rejected");
    match error {
        ExportError::DuplicateRow { table, key } => {
            assert_eq!(table, "catalog_tag");
            assert_eq!(key, "eu|lab_npm|stable");
        }
        other => panic!("expected DuplicateRow, got {other:?}"),
    }
}

#[tokio::test]
async fn a_source_that_moves_between_frontier_and_export_is_refused() {
    let db = open_seeded().await;
    let frontier = capture_frontier(&db).await.expect("frontier");
    // Un escritor se cuela entre la captura de la frontera y el primer
    // lote: el export lee una fuente que ya no es la que midio.
    run(
        &db,
        "CREATE catalog_tag:eu_hot SET
            account = 'eu', repository = 'lab_npm', tag_name = 'hot',
            package_name = 'one', package_version = '1.0.0',
            updated_at = d'2026-01-07T03:04:05Z';",
    )
    .await;
    let dir = scratch("mutated");
    let error = export_with_frontier(&db, &config(dir), Some(frontier))
        .await
        .expect_err("refused");
    match error {
        ExportError::SourceMutated(message) => {
            assert!(message.contains("catalog_tag"), "{message}");
            assert!(message.contains("2"), "{message}");
        }
        other => panic!("expected SourceMutated, got {other:?}"),
    }
}

#[tokio::test]
async fn a_quiescent_source_passes_the_frontier_twice() {
    let db = open_seeded().await;
    let frontier = capture_frontier(&db).await.expect("frontier");
    let dir = scratch("frontier-ok");
    let run = export_with_frontier(&db, &config(dir), Some(frontier))
        .await
        .expect("export");
    assert!(run.manifest.is_some());
    // La frontera del manifiesto es la de la fuente: 24 filas y la
    // cabeza del ledger en la posicion 1 del stream a.
    let manifest = run.manifest.expect("manifest");
    assert_eq!(manifest.frontier.tables["catalog_asset"], 4);
    assert_eq!(manifest.frontier.tables["catalog_tag"], 2);
    assert_eq!(manifest.frontier.ledger_rows, 3);
    assert_eq!(manifest.frontier.ledger_max_position, Some(1));
    assert_eq!(manifest.frontier.head_rows, 2);
}

#[tokio::test]
async fn the_export_resumes_to_the_same_manifest() {
    let db = open_seeded().await;
    let reference_dir = scratch("resume-reference");
    let reference = export(&db, &config(reference_dir.clone()))
        .await
        .expect("reference export");

    let dir = scratch("resume-paged");
    let mut paged = config(dir.clone());
    paged.limit_batches = Some(5);
    let run = export_with_frontier(&db, &paged, None)
        .await
        .expect("paged run");
    assert!(run.manifest.is_none(), "a paged run does not finish");
    assert_eq!(run.batches_written, 5);
    assert!(
        !dir.join(MANIFEST_FILE).exists(),
        "no manifest while unfinished"
    );
    assert!(dir.join(PROGRESS_FILE).exists(), "the marker is written");

    let mut resumed_config = config(dir.clone());
    resumed_config.resume = true;
    let run = export_with_frontier(&db, &resumed_config, None)
        .await
        .expect("resume run");
    assert!(run.manifest.is_some(), "the resume finishes");
    assert!(!dir.join(PROGRESS_FILE).exists(), "the marker is replaced");

    let resumed = run.manifest.expect("manifest");
    assert_eq!(resumed, reference);
    resumed.verify(&dir).expect("verified resumed export");
    // La reanudacion no repitio los lotes ya escritos: mismos ficheros,
    // mismos hashes.
    let files = export_files(&dir);
    assert_eq!(files, export_files(&reference_dir));
}

#[tokio::test]
async fn the_resume_names_a_truncated_batch() {
    let db = open_seeded().await;
    let dir = scratch("resume-truncated");
    let manifest = export(&db, &config(dir.clone())).await.expect("export");

    // El estado de una corrida que murio tras escribir el ultimo lote y
    // antes del manifiesto: todo hecho, marcador al dia, sin manifest.
    let mut done = Vec::new();
    for (table, batch) in manifest.all_batches() {
        done.push(serde_json::json!({
            "table": table,
            "batch": batch,
        }));
    }
    let progress = serde_json::json!({
        "format": FORMAT,
        "frontier": serde_json::to_value(&manifest.frontier).expect("frontier"),
        "done": done,
        "next_table": null,
        "next_batch_index": 1,
    });
    std::fs::write(
        dir.join(PROGRESS_FILE),
        serde_json::to_vec(&progress).expect("marker"),
    )
    .expect("write marker");
    std::fs::remove_file(dir.join(MANIFEST_FILE)).expect("remove manifest");

    // Y el lote que la corrida trunco: los bytes de un fichero escrito a
    // medias ya no cuadran con el hash que se le grabo.
    let mut config = config(dir.clone());
    config.resume = true;
    let first_batch = batches_of(&manifest, "catalog_repository").batches[0]
        .file
        .clone();
    std::fs::write(dir.join(&first_batch), b"[").expect("truncate batch");

    let error = export_with_frontier(&db, &config, None)
        .await
        .expect_err("refused");
    match error {
        ExportError::TruncatedBatch {
            file,
            expected,
            actual,
        } => {
            assert_eq!(file, first_batch);
            assert_eq!(expected, manifest.all_batches()[0].1.sha256);
            assert_ne!(expected, actual);
        }
        other => panic!("expected TruncatedBatch, got {other:?}"),
    }
}

#[tokio::test]
async fn an_empty_source_exports_a_deterministic_empty_manifest() {
    let db = open_empty().await;
    let dir = scratch("empty");
    let manifest = export(&db, &config(dir.clone())).await.expect("export");
    assert!(manifest.tables.is_empty());
    for count in manifest.frontier.tables.values() {
        assert_eq!(*count, 0, "an empty table counts zero");
    }
    assert_eq!(manifest.frontier.ledger_rows, 0);
    assert_eq!(manifest.frontier.ledger_max_position, None);
    assert_eq!(manifest.content_sha256, manifest.content_digest());

    let dir_again = scratch("empty-2");
    let again = export(&db, &config(dir_again.clone()))
        .await
        .expect("export");
    let bytes = std::fs::read(dir.join(MANIFEST_FILE)).expect("manifest bytes");
    let bytes_again = std::fs::read(dir_again.join(MANIFEST_FILE)).expect("manifest bytes");
    assert_eq!(bytes, bytes_again);
    assert_eq!(manifest, again);
}

#[tokio::test]
async fn an_export_never_overwrites_its_output() {
    let db = open_seeded().await;
    let dir = scratch("no-overwrite");
    export(&db, &config(dir.clone()))
        .await
        .expect("first export");
    let error = export(&db, &config(dir.clone()))
        .await
        .expect_err("refused");
    assert!(matches!(error, ExportError::Invalid(_)));
    let error = export_with_frontier(
        &db,
        &{
            let mut config = config(dir.clone());
            config.resume = true;
            config
        },
        None,
    )
    .await
    .expect_err("a finished export cannot resume");
    assert!(matches!(error, ExportError::ResumeMismatch(_)));
}

#[tokio::test]
async fn a_frontier_captured_by_a_probe_names_the_tables() {
    let db = open_seeded().await;
    let frontier = capture_frontier(&db).await.expect("frontier");
    // Every exported table is in the frontier, with the rows the fixture
    // carries: the probe is what an operator uses to state the point in
    // time the export represents.
    assert_eq!(frontier.tables.len(), 10);
    assert_eq!(frontier.tables["catalog_repository"], 2);
    assert_eq!(frontier.tables["event_ledger"], 3);
    assert!(frontier.tables.contains_key("catalog_audit_outbox"));
    assert!(frontier.tables.contains_key("trust_grant"));
    assert!(frontier.tables.contains_key("provenance_receipt"));
}
