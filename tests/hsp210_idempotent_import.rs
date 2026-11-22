//! Import idempotente de autoridades persistentes (HSP-210 / HGS-28).
//!
//! El fixture `tests/fixtures/hsp210-authorities` es un paquete del formato
//! portable que produce la herramienta de export (HSP-179): 31 filas en 13
//! tablas semanticas (cuentas, policies, catalogo, tags, grants,
//! decisiones, evidencia, outbox y ledger con su cabezal), lotes con su
//! SHA-256 y un manifiesto cuya fecha es la frontera de la fuente.
//!
//! - AC1: el fixture se importa **dos veces** sobre el mismo destino, con
//!   un kill simulado a mitad de lote y su resume. El fingerprint de
//!   autoridades es el mismo que el de una corrida limpia, y el numero de
//!   filas no cambia en ninguna de las pasadas: ningun duplicado.
//! - AC2: el historial de decisiones, el ledger (identidades compuestas
//!   `stream,pos` y su cabezal) y los scopes sobreviven al import tal
//!   cual; una fila divergente, una identidad repetida, un lote alterado,
//!   un digest huerfano o un manifiesto que miente abortan con el culpable
//!   nombrado, y el destino queda intacto o resumible — nunca silencioso.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use hodei_artifacts_api::recovery::RecoveryError;
use hodei_artifacts_api::recovery::import::{
    BlobPolicy, DestinationState, DigestReconciler, ExportFrontier, ExportSource, IMPORT_FORMAT,
    IMPORT_MANIFEST_FILE, ImportBatch, ImportFailure, ImportManifest, ImportOptions, ImportTable,
    MemorySink, Row, RowSink, SetReconciler, canonical_digest, canonicalize_id, import_export,
    read_batch, read_import_manifest,
};
use serde_json::{Map, Value};

const PACKAGE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/hsp210-authorities"
);

/// Claves de las filas del fixture que llevan un digest de contenido.
const DIGEST_KEYS: [(&str, &str); 5] = [
    ("catalog_asset", "sha256"),
    ("catalog_reference", "sha256"),
    ("provenance_receipt", "digest"),
    ("trust_grant", "evidence"),
    ("catalog_audit_outbox", "payload_digest"),
];

fn package_dir() -> PathBuf {
    PathBuf::from(PACKAGE)
}

/// Copia el paquete de fixture a un directorio temporal para las variantes
/// que lo alteran: el fixture en git no se toca jamas.
fn copy_package(target: &Path) -> PathBuf {
    let source = package_dir();
    std::fs::create_dir_all(target.join("batches")).unwrap();
    std::fs::copy(
        source.join(IMPORT_MANIFEST_FILE),
        target.join(IMPORT_MANIFEST_FILE),
    )
    .unwrap();
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(source.join("batches")).unwrap() {
        entries.push(entry.unwrap().path());
    }
    entries.sort();
    for entry in entries {
        std::fs::copy(
            &entry,
            target.join("batches").join(entry.file_name().unwrap()),
        )
        .unwrap();
    }
    target.to_path_buf()
}

fn options(resume: bool, overwrite: bool, batch_rows: usize) -> ImportOptions {
    ImportOptions {
        batch_rows,
        resume,
        overwrite,
        allow_missing_blobs: false,
    }
}

/// El reconciliador que conoce exactamente los digests que el fixture
/// referencia, leidos del propio paquete.
fn fixture_reconciler() -> Arc<dyn DigestReconciler> {
    let manifest: ImportManifest =
        serde_json::from_slice(&std::fs::read(package_dir().join(IMPORT_MANIFEST_FILE)).unwrap())
            .unwrap();
    let mut digests: BTreeSet<String> = BTreeSet::new();
    for table in &manifest.tables {
        for batch in &table.batches {
            let rows: Vec<Row> = read_batch(&package_dir(), batch).unwrap();
            for row in rows {
                for (name, field) in DIGEST_KEYS {
                    if table.table == name
                        && let Some(Value::String(digest)) = row.get(field)
                    {
                        digests.insert(digest.clone());
                    }
                }
            }
        }
    }
    Arc::new(SetReconciler::new(digests))
}

async fn fingerprint_of(sink: &MemorySink) -> String {
    sink.state_summary().await.unwrap().fingerprint()
}

/// El oraculo del AC1: una corrida limpia del fixture de principio a fin.
async fn clean_reference() -> (Arc<MemorySink>, String) {
    let dir = tempfile::tempdir().unwrap();
    let sink = Arc::new(MemorySink::new());
    let report = import_export(
        &package_dir(),
        sink.clone(),
        Some(fixture_reconciler()),
        &options(false, false, 4),
        &dir.path().join(".import-progress.json"),
    )
    .await
    .expect("the fixture imports cleanly");
    assert_eq!(report.created, 31, "the fixture carries 31 rows");
    assert_eq!(report.skipped, 0);
    assert!(report.finished);
    let fingerprint = fingerprint_of(&sink).await;
    (sink, fingerprint)
}

#[tokio::test]
async fn importing_the_fixture_twice_changes_nothing() {
    // AC1, parte 1: segunda pasada completa sobre el mismo destino.
    let (sink, fingerprint) = clean_reference().await;
    let dir = tempfile::tempdir().unwrap();
    let second = import_export(
        &package_dir(),
        sink.clone(),
        Some(fixture_reconciler()),
        &options(false, false, 4),
        &dir.path().join(".import-progress.json"),
    )
    .await
    .expect("the second import runs");
    assert_eq!(second.created, 0, "nothing is created twice");
    assert_eq!(
        second.skipped, 31,
        "every row is recognized by its canonical identity"
    );
    assert_eq!(second.overwritten, 0);
    assert!(second.finished);
    assert_eq!(second.fingerprint_before, fingerprint);
    assert_eq!(
        second.fingerprint_after, fingerprint,
        "same authorities fingerprint"
    );
    assert_eq!(sink.len().await, 31, "no duplicate landed");
    assert!(
        !dir.path().join(".import-progress.json").exists(),
        "a finished run leaves no progress marker"
    );
}

/// El sink que "muere" tras N escrituras: simula el kill a mitad de lote.
/// El estado de las escrituras anteriores sobrevive, como sobreviviria en
/// el disco tras un corte real.
struct CrashSink {
    inner: Arc<MemorySink>,
    budget: AtomicUsize,
}

#[async_trait]
impl RowSink for CrashSink {
    async fn lookup(&self, identity: &str) -> Result<Option<Row>, RecoveryError> {
        self.inner.lookup(identity).await
    }

    async fn put(&self, identity: &str, row: &Row) -> Result<(), RecoveryError> {
        let left = self
            .budget
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                if n == 0 { None } else { Some(n - 1) }
            });
        if left.is_err() {
            return Err(RecoveryError::Store("simulated kill".to_string()));
        }
        self.inner.put(identity, row).await
    }

    fn fingerprint_row(&self, row: &Row) -> String {
        self.inner.fingerprint_row(row)
    }

    async fn state_summary(&self) -> Result<DestinationState, RecoveryError> {
        self.inner.state_summary().await
    }
}

#[tokio::test]
async fn a_kill_midway_resumes_to_the_same_fingerprint_without_duplicates() {
    // AC1, parte 2: kill a mitad de lote y resume. batch_rows=1 hace que
    // cada escritura sea inmediata: el corte cae dentro de un lote, no
    // entre lotes. Presupuesto 5: iam_account (2) + iam_policy (2) + la
    // primera fila del primer lote de catalog_repository. El marcador de
    // progreso solo registra lotes **terminados**: el corte deja un lote a
    // medias y su re-aplicacion por identidad es lo que se prueba.
    let dir = tempfile::tempdir().unwrap();
    let progress = dir.path().join(".import-progress.json");
    let sink = Arc::new(MemorySink::new());
    let crash = Arc::new(CrashSink {
        inner: sink.clone(),
        budget: AtomicUsize::new(5),
    });

    let first = import_export(
        &package_dir(),
        crash,
        Some(fixture_reconciler()),
        &options(false, false, 1),
        &progress,
    )
    .await;
    let error = first.expect_err("the run dies with the simulated kill");
    assert!(
        error.to_string().contains("simulated kill"),
        "the store error surfaces: {error}"
    );
    assert_eq!(
        sink.len().await,
        5,
        "only what was written before the kill survives"
    );
    assert!(progress.exists(), "the run left a resumable marker");

    // Reintentar sin `resume` aborta: hay una corrida a medias.
    let blind = import_export(
        &package_dir(),
        sink.clone(),
        Some(fixture_reconciler()),
        &options(false, false, 1),
        &progress,
    )
    .await
    .expect_err("an unfinished import must not be silently restarted");
    assert!(blind.to_string().contains("resume"), "{blind}");

    // Resume: los lotes del progreso se verifican contra su hash y contra
    // el destino; el lote a medias se re-aplica por identidad (la fila
    // presente se salta, la que faltaba se crea); el resto fluye.
    let second = import_export(
        &package_dir(),
        sink.clone(),
        Some(fixture_reconciler()),
        &options(true, false, 1),
        &progress,
    )
    .await
    .expect("the resume completes the import");
    assert!(second.finished);
    assert!(!progress.exists(), "the finished resume deletes its marker");

    let (_, reference) = clean_reference().await;
    let resumed = fingerprint_of(&sink).await;
    assert_eq!(
        resumed, reference,
        "resume after a kill reaches the same authorities fingerprint"
    );
    assert_eq!(sink.len().await, 31, "the kill added no duplicate");

    // Y una tercera pasada completa: misma identidad, mismo fingerprint.
    let third = import_export(
        &package_dir(),
        sink.clone(),
        Some(fixture_reconciler()),
        &options(false, false, 1),
        &dir.path().join(".third.json"),
    )
    .await
    .expect("the third import runs");
    assert_eq!(third.created, 0);
    assert_eq!(third.fingerprint_after, reference);
}

#[tokio::test]
async fn decisions_ledger_and_scopes_survive_the_import() {
    // AC2, mitad conservacion: el historial de decisiones, el ledger con
    // sus identidades compuestas `stream,pos`, su cabezal y los scopes
    // viajan byte a byte, sin reescribir claves.
    let (sink, _) = clean_reference().await;
    let rows = sink.snapshot().await;

    for id in [
        "trust_decision:d-0001",
        "trust_decision:d-0002",
        "trust_decision:d-0003",
    ] {
        let decision = rows.get(id).unwrap_or_else(|| panic!("{id} lost"));
        assert!(
            decision.contains_key("decided_at"),
            "{id} lost its timestamp"
        );
        assert!(decision.contains_key("policy"), "{id} lost its policy");
        assert!(
            decision
                .get("scope")
                .and_then(Value::as_str)
                .is_some_and(|s| s.starts_with("tenant/")),
            "{id} lost its scope"
        );
    }
    for id in [
        "event_ledger:stream_a,0",
        "event_ledger:stream_a,1",
        "event_ledger:stream_a,2",
        "event_ledger:stream_b,0",
    ] {
        let entry = rows
            .get(id)
            .unwrap_or_else(|| panic!("ledger position {id} lost"));
        assert_eq!(
            entry.get("id").and_then(Value::as_str),
            Some(id),
            "the composite ledger key was rewritten"
        );
    }
    for stream in ["stream_a", "stream_b"] {
        let id = format!("event_ledger_head:{stream}");
        assert!(rows.contains_key(&id), "head of {stream} lost");
    }
    // Las cuentas conservan su scope de tenant.
    assert_eq!(
        rows.get("iam_account:acme")
            .and_then(|r| r.get("scope"))
            .and_then(Value::as_str),
        Some("tenant/acme")
    );
    // La clave canonica de una identidad compuesta con separadores sigue
    // siendo la que viajaba en el export.
    let asset = rows
        .get("catalog_asset:acme|main|widget-1.0.0.bin")
        .expect("pipe-separated composite key preserved");
    assert_eq!(
        asset.get("id").and_then(Value::as_str).unwrap(),
        "catalog_asset:acme|main|widget-1.0.0.bin"
    );
}

#[tokio::test]
async fn a_divergent_row_aborts_and_names_the_culprit() {
    // AC2, mitad fail-closed: la misma identidad canonica con otro
    // contenido no se empata en silencio.
    let (sink, reference) = clean_reference().await;
    // Corromper una decision: misma clave, distinto resultado.
    let mut corrupt = sink
        .snapshot()
        .await
        .get("trust_decision:d-0002")
        .cloned()
        .expect("seeded");
    corrupt.insert("decision".to_string(), Value::String("deny".to_string()));
    sink.put("trust_decision:d-0002", &corrupt).await.unwrap();
    assert_ne!(fingerprint_of(&sink).await, reference);

    let dir = tempfile::tempdir().unwrap();
    let error = import_export(
        &package_dir(),
        sink.clone(),
        Some(fixture_reconciler()),
        &options(false, false, 4),
        &dir.path().join(".import-progress.json"),
    )
    .await
    .expect_err("divergence aborts");
    match &error {
        ImportFailure::Conflict {
            table,
            key,
            package_digest,
            destination_digest,
        } => {
            assert_eq!(table, "trust_decision");
            assert_eq!(key, "trust_decision:d-0002");
            assert!(!package_digest.is_empty() && !destination_digest.is_empty());
        }
        other => panic!("expected a typed Conflict, got {other:?}"),
    }
    // El abort es explicito y el destino conserva su fila divergente.
    let still = sink
        .lookup("trust_decision:d-0002")
        .await
        .unwrap()
        .expect("row still there");
    assert_eq!(still.get("decision").and_then(Value::as_str), Some("deny"));
}

#[tokio::test]
async fn an_authorized_overwrite_repairs_keeping_the_identity() {
    // La reparacion explicita conserva la clave canonica y converge al
    // fingerprint de la corrida limpia.
    let (sink, reference) = clean_reference().await;
    let mut corrupt = sink
        .snapshot()
        .await
        .get("iam_account:acme")
        .cloned()
        .expect("seeded");
    corrupt.insert("handle".to_string(), Value::String("renamed".to_string()));
    sink.put("iam_account:acme", &corrupt).await.unwrap();

    let dir = tempfile::tempdir().unwrap();
    let report = import_export(
        &package_dir(),
        sink.clone(),
        Some(fixture_reconciler()),
        &options(false, true, 4),
        &dir.path().join(".import-progress.json"),
    )
    .await
    .expect("overwrite repairs");
    assert_eq!(report.overwritten, 1);
    assert_eq!(report.skipped, 30);
    assert_eq!(
        report.fingerprint_after, reference,
        "repair reaches the clean fingerprint"
    );
    assert_eq!(
        sink.lookup("iam_account:acme")
            .await
            .unwrap()
            .and_then(|r| r.get("id").and_then(Value::as_str).map(str::to_string))
            .as_deref(),
        Some("iam_account:acme"),
        "the canonical identity survived the rewrite"
    );
}

/// Construye un paquete minimo y autoconsistente para las variantes de
/// filas invalidas: los hashes y los bordes se calculan de verdad.
fn write_package(dir: &Path, tables: &[(&str, &[Vec<Row>])]) -> PathBuf {
    let mut manifest_tables = Vec::new();
    let mut frontier_tables = BTreeMap::new();
    std::fs::create_dir_all(dir.join("batches")).unwrap();
    for (name, batches) in tables {
        let mut entries = Vec::new();
        let mut total = 0u64;
        for (index, rows) in batches.iter().enumerate() {
            let bytes = serde_json::to_vec(rows).unwrap();
            let file = format!("batches/{name}-{index:06}.json");
            std::fs::write(dir.join(&file), &bytes).unwrap();
            let sha = {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(&bytes);
                let digest = hasher.finalize();
                digest
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            };
            let ids: Vec<String> = rows
                .iter()
                .map(|r| {
                    r.get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string()
                })
                .collect();
            entries.push(ImportBatch {
                file,
                sha256: sha,
                rows: rows.len() as u64,
                first_id: ids[0].clone(),
                last_id: ids[ids.len() - 1].clone(),
            });
            total += rows.len() as u64;
        }
        frontier_tables.insert((*name).to_string(), total);
        manifest_tables.push(ImportTable {
            table: (*name).to_string(),
            rows: total,
            batches: entries,
        });
    }
    let mut manifest = ImportManifest {
        format: IMPORT_FORMAT,
        tool_version: "test".to_string(),
        source: ExportSource {
            engine: "surrealdb".to_string(),
            engine_version: "2.3.9".to_string(),
            backend: "mem".to_string(),
            namespace: "hodei".to_string(),
            database: "artifacts".to_string(),
        },
        frontier: ExportFrontier {
            tables: frontier_tables,
            ledger_rows: 0,
            ledger_max_position: None,
            head_rows: 0,
        },
        tables: manifest_tables,
        blobs: BlobPolicy {
            policy: "referenced-by-digest".to_string(),
            copied: false,
        },
        content_sha256: String::new(),
    };
    manifest.content_sha256 = manifest.content_digest();
    std::fs::write(
        dir.join(IMPORT_MANIFEST_FILE),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    dir.to_path_buf()
}

fn simple_row(table: &str, key: &str, value: i64) -> Row {
    let mut row = Map::new();
    row.insert("id".to_string(), Value::String(format!("{table}:{key}")));
    row.insert("v".to_string(), Value::Number(value.into()));
    row
}

#[tokio::test]
async fn invalid_rows_abort_before_touching_the_destination() {
    // El paquete se rechaza entero antes de la primera escritura: un
    // destino que ya tiene autoridades no se "prueba" con filas invalidas.

    // 1. Fila sin id: la identidad no se inventa.
    let dir = tempfile::tempdir().unwrap();
    let mut no_id = simple_row("catalog_tag", "t1", 1);
    no_id.remove("id");
    let package = write_package(dir.path(), &[("catalog_tag", &[vec![no_id.clone()]])]);
    let sink = Arc::new(MemorySink::new());
    let error = import_export(
        &package,
        sink.clone(),
        Some(Arc::new(SetReconciler::new(Vec::<String>::new()))),
        &options(false, false, 4),
        &dir.path().join(".p.json"),
    )
    .await
    .expect_err("a row without identity is refused");
    assert!(
        matches!(error, ImportFailure::RowInvalid { .. }),
        "{error:?}"
    );
    assert!(sink.is_empty().await, "the destination was never touched");

    // 2. Fila cuyo id pertenece a otra tabla.
    let dir = tempfile::tempdir().unwrap();
    let wrong_table = simple_row("catalog_tag", "t1", 1); // id = catalog_tag:t1
    let package = write_package(
        dir.path(),
        &[("catalog_asset", &[vec![wrong_table.clone()]])],
    );
    let sink = Arc::new(MemorySink::new());
    let error = import_export(
        &package,
        sink.clone(),
        Some(Arc::new(SetReconciler::new(Vec::<String>::new()))),
        &options(false, false, 4),
        &dir.path().join(".p.json"),
    )
    .await
    .expect_err("a foreign-table identity is refused");
    assert!(
        matches!(error, ImportFailure::RowInvalid { .. }),
        "{error:?}"
    );

    // 3. Identidad canonica repetida entre lotes.
    let dir = tempfile::tempdir().unwrap();
    let package = write_package(
        dir.path(),
        &[(
            "catalog_tag",
            &[
                vec![
                    simple_row("catalog_tag", "t1", 1),
                    simple_row("catalog_tag", "t2", 2),
                ],
                vec![
                    simple_row("catalog_tag", "t2", 3),
                    simple_row("catalog_tag", "t3", 4),
                ],
            ],
        )],
    );
    let sink = Arc::new(MemorySink::new());
    let error = import_export(
        &package,
        sink.clone(),
        Some(Arc::new(SetReconciler::new(Vec::<String>::new()))),
        &options(false, false, 4),
        &dir.path().join(".p.json"),
    )
    .await
    .expect_err("a repeated canonical identity is corruption");
    assert!(
        matches!(error, ImportFailure::DuplicateRow { .. }),
        "{error:?}"
    );

    // 4. Lote alterado en disco: el hash del manifiesto no miente.
    let dir = tempfile::tempdir().unwrap();
    let package = copy_package(dir.path());
    let target = package.join("batches/trust_grant-000001.json");
    let mut bytes = std::fs::read(&target).unwrap();
    bytes.truncate(bytes.len() - 2);
    std::fs::write(&target, &bytes).unwrap();
    let sink = Arc::new(MemorySink::new());
    let error = import_export(
        &package,
        sink.clone(),
        Some(fixture_reconciler()),
        &options(false, false, 4),
        &dir.path().join(".p.json"),
    )
    .await
    .expect_err("an altered batch is refused");
    assert!(
        error.to_string().contains("truncated or altered"),
        "{error}"
    );
    assert!(
        sink.is_empty().await,
        "manifest-first: nothing applied from a broken package"
    );

    // 5. Digest de contenido que el CAS no tiene.
    let dir = tempfile::tempdir().unwrap();
    let mut asset = simple_row("catalog_asset", "bin", 1);
    asset.insert(
        "sha256".to_string(),
        Value::String("sha256:deadbeef".to_string()),
    );
    let package = write_package(dir.path(), &[("catalog_asset", &[vec![asset]])]);
    let sink = Arc::new(MemorySink::new());
    let error = import_export(
        &package,
        sink.clone(),
        Some(Arc::new(SetReconciler::new(Vec::<String>::new()))),
        &options(false, false, 4),
        &dir.path().join(".p.json"),
    )
    .await
    .expect_err("a missing blob is the import's failure to raise");
    match &error {
        ImportFailure::MissingBlobs {
            first,
            referenced_by,
            ..
        } => {
            assert_eq!(first, "sha256:deadbeef");
            assert_eq!(referenced_by, "catalog_asset:bin");
        }
        other => panic!("expected MissingBlobs, got {other:?}"),
    }
    assert!(sink.is_empty().await);
}

#[tokio::test]
async fn a_manifest_that_lies_is_refused() {
    // El contrato del paquete se comprueba contra si mismo antes de nada.
    let dir = tempfile::tempdir().unwrap();
    let package = copy_package(dir.path());
    let manifest_path = package.join(IMPORT_MANIFEST_FILE);
    let mut manifest: ImportManifest =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();

    // Formato futuro.
    manifest.format = IMPORT_FORMAT + 1;
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let error =
        read_import_manifest(&package).expect_err("a future format is refused with both numbers");
    assert!(error.to_string().contains("format"), "{error}");

    // Autodigest alterado.
    let mut manifest: ImportManifest =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest.format = IMPORT_FORMAT;
    manifest.content_sha256 = "f".repeat(64);
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let error = read_import_manifest(&package).expect_err("a broken self-digest is refused");
    assert!(error.to_string().contains("content digest"), "{error}");

    // La frontera miente: promete una fila mas.
    let mut manifest: ImportManifest =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest.content_sha256 = manifest.content_digest();
    *manifest.frontier.tables.get_mut("catalog_tag").unwrap() += 1;
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let error = read_import_manifest(&package).expect_err("a lying frontier is refused");
    assert!(error.to_string().contains("frontier"), "{error}");

    // Politica de blobs inesperada: el contrato exige referenced-by-digest.
    let dir = tempfile::tempdir().unwrap();
    let package = write_package(
        dir.path(),
        &[("catalog_tag", &[vec![simple_row("catalog_tag", "t1", 1)]])],
    );
    let manifest_path = package.join(IMPORT_MANIFEST_FILE);
    let mut manifest: ImportManifest =
        serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    manifest.blobs.policy = "bytes-travel".to_string();
    manifest.content_sha256 = manifest.content_digest();
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    let sink = Arc::new(MemorySink::new());
    let error = import_export(
        &package,
        sink,
        Some(Arc::new(SetReconciler::new(Vec::<String>::new()))),
        &options(false, false, 4),
        &dir.path().join(".p.json"),
    )
    .await
    .expect_err("a package that carries bytes is not this contract");
    assert!(
        matches!(error, ImportFailure::BlobPolicy { .. }),
        "{error:?}"
    );
}

#[tokio::test]
async fn the_reconciler_cannot_be_forgotten() {
    // Sin reconciliador y sin renuncia explicita, el import aborta antes
    // de escribir: el contrato de doc-20 no se cumple a ciegas.
    let sink = Arc::new(MemorySink::new());
    let dir = tempfile::tempdir().unwrap();
    let error = import_export(
        &package_dir(),
        sink.clone(),
        None,
        &options(false, false, 4),
        &dir.path().join(".p.json"),
    )
    .await
    .expect_err("no reconciler, no waiver, no import");
    assert!(
        error.to_string().contains("allow_missing_blobs"),
        "the error names the waiver: {error}"
    );
    assert!(sink.is_empty().await);
}

#[tokio::test]
async fn resuming_a_different_package_is_refused() {
    // El progreso grabado pertenece a un contenido; reanudarlo contra otro
    // paquete seria aplicar lotes ajenos.
    let dir = tempfile::tempdir().unwrap();
    let progress = dir.path().join(".import-progress.json");
    let sink = Arc::new(MemorySink::new());
    let crash = Arc::new(CrashSink {
        inner: sink.clone(),
        budget: AtomicUsize::new(5),
    });
    let _ = import_export(
        &package_dir(),
        crash,
        Some(fixture_reconciler()),
        &options(false, false, 1),
        &progress,
    )
    .await
    .expect_err("dies midway");
    assert!(progress.exists(), "the unfinished run left its marker");

    let package_b = write_package(
        &dir.path().join("package-b"),
        &[("catalog_tag", &[vec![simple_row("catalog_tag", "z1", 9)]])],
    );
    let error = import_export(
        &package_b,
        sink,
        Some(Arc::new(SetReconciler::new(Vec::<String>::new()))),
        &options(true, false, 4),
        &progress,
    )
    .await
    .expect_err("progress from another content aborts the resume");
    assert!(
        matches!(error, ImportFailure::ProgressMismatch { .. }),
        "{error:?}"
    );
}

#[tokio::test]
async fn canonical_identity_and_digest_helpers_hold_their_contract() {
    // Helpers que el resto del modulo fija como contrato.
    let row = simple_row("catalog_asset", "acme|main|x", 1);
    assert_eq!(
        canonicalize_id("catalog_asset", &row).unwrap(),
        "catalog_asset:acme|main|x"
    );
    let mut with_extra = row.clone();
    with_extra.insert("size_bytes".to_string(), Value::Number(10.into()));
    assert_ne!(canonical_digest(&row), canonical_digest(&with_extra));
    // El fixture es integral leible por su propio manifiesto.
    let manifest = read_import_manifest(&package_dir()).expect("the fixture is sound");
    for (_table, batch) in manifest.all_batches() {
        let rows = read_batch(&package_dir(), batch).expect("batch readable");
        assert_eq!(rows.len() as u64, batch.rows);
    }
}
