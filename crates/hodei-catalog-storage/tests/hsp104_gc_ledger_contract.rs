//! El contrato del libro de la recoleccion contra SurrealDB de verdad.
//!
//! Lo que se comprueba: el censo trae las raices de las filas reales
//! (asset, referencia, version, subject); las retenciones se guardan, se
//! listan y se liberan; la pasada marcada sobrevive a reabrir la base; y
//! confirmar un borrado deja rastro y un solo evento de auditoria.

use std::sync::Arc;

use chrono::{Duration, Utc};
use hodei_catalog::{
    AllowAllAuthorizer, CatalogWriter, ContentHold, ContentLedger, DeletionState, DigestScope,
    GcDeletion, GcRun, HoldScope, PackageCoordinates, Publication, RepositoryConfig,
    RepositoryDescriptor, RepositoryFormat, RepositoryMode, RootKind, RunCandidate, RunState,
    repository_hrn,
};
use hodei_catalog_storage::SurrealCatalogStore;
use hodei_evidence::ContentDigest;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

async fn open(path: &std::path::Path) -> Surreal<Db> {
    let mut last = String::new();
    for _ in 0..20 {
        match Surreal::new::<RocksDb>(path).await {
            Ok(db) => {
                db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");
                return db;
            }
            Err(error) => {
                last = error.to_string();
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
    panic!("the database could not be opened: {last}");
}

fn store(db: Surreal<Db>) -> SurrealCatalogStore {
    SurrealCatalogStore::with_authorizer(db, Arc::new(AllowAllAuthorizer))
}

fn descriptor(account: &str, repo: &str) -> RepositoryDescriptor {
    RepositoryDescriptor {
        hrn: repository_hrn("hodei", "artifacts", "eu", account, repo),
        account: account.to_string(),
        repo_key: repo.to_string(),
        format: RepositoryFormat::Raw,
        mode: RepositoryMode::Hosted,
        config: RepositoryConfig::new(DigestScope::Global),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn publication(
    repository: &str,
    path: &str,
    content: &[u8],
    subject: Option<ContentDigest>,
) -> Publication {
    Publication {
        repository: repository.to_string(),
        asset_path: path.to_string(),
        coordinates: PackageCoordinates {
            package_name: "pkg".to_string(),
            package_version: "1.0.0".to_string(),
        },
        digest: ContentDigest::of_bytes(content),
        size_bytes: content.len() as u64,
        tags: Vec::new(),
        actor: "hrn:hodei:iam:acme:User/alice".to_string(),
        correlation_id: "corr-hsp104".to_string(),
        overwrite: false,
        precondition: None,
        subject,
    }
}

#[tokio::test]
async fn the_census_has_a_root_per_real_row() {
    let tmp = tempfile::tempdir().expect("tmp");
    let store = store(open(&tmp.path().join("db")).await);
    store.initialize().await.expect("initialize");
    store
        .create_repository("acme", descriptor("acme", "raw"))
        .await
        .expect("repo");
    let subject = ContentDigest::of_bytes(b"the-subject");
    store
        .publish(
            "acme",
            publication("raw", "a/one.bin", b"one", Some(subject.clone())),
        )
        .await
        .expect("publish");

    let roots = store.content_roots().await.expect("roots");
    let one = ContentDigest::of_bytes(b"one");
    let kinds: Vec<_> = roots
        .iter()
        .filter(|r| r.digest == one)
        .map(|r| r.kind)
        .collect();
    assert!(kinds.contains(&RootKind::Asset), "{roots:?}");
    assert!(kinds.contains(&RootKind::Reference), "{roots:?}");
    assert!(kinds.contains(&RootKind::Version), "{roots:?}");
    assert!(
        roots
            .iter()
            .any(|r| r.kind == RootKind::Subject && r.digest == subject),
        "el subject del referrer mantiene vivo su destino: {roots:?}"
    );
    assert!(
        roots
            .iter()
            .all(|r| r.account == "acme" && r.repository == "raw")
    );
}

#[tokio::test]
async fn holds_are_stored_listed_replaced_and_released() {
    let tmp = tempfile::tempdir().expect("tmp");
    let store = store(open(&tmp.path().join("db")).await);
    store.initialize().await.expect("initialize");
    let digest = ContentDigest::of_bytes(b"held");
    let hold = ContentHold {
        id: "legal-1".to_string(),
        scope: HoldScope::Digest {
            digest: digest.clone(),
        },
        reason: "litigio 42".to_string(),
        actor: "admin".to_string(),
        created_at: Utc::now(),
        expires_at: Some(Utc::now() + Duration::days(30)),
    };
    store.place_hold(&hold).await.expect("place");
    store
        .place_hold(&ContentHold {
            id: "repo-1".to_string(),
            scope: HoldScope::Repository {
                account: "acme".to_string(),
                repository: "raw".to_string(),
            },
            reason: "auditoria".to_string(),
            actor: "admin".to_string(),
            created_at: Utc::now(),
            expires_at: None,
        })
        .await
        .expect("place repo");

    let holds = store.holds().await.expect("holds");
    assert_eq!(holds.len(), 2);
    assert_eq!(holds[0].id, "legal-1");
    assert_eq!(holds[0].scope, HoldScope::Digest { digest });
    assert!(holds[0].is_active(Utc::now()));
    assert!(!holds[0].is_active(Utc::now() + Duration::days(31)));

    // Repetir el id reemplaza.
    let mut again = hold.clone();
    again.reason = "otro motivo".to_string();
    store.place_hold(&again).await.expect("replace");
    let holds = store.holds().await.expect("holds");
    assert_eq!(holds.len(), 2);
    assert_eq!(holds[0].reason, "otro motivo");

    assert!(store.release_hold("legal-1").await.expect("release"));
    assert!(!store.release_hold("legal-1").await.expect("release again"));
    assert_eq!(store.holds().await.expect("holds").len(), 1);
}

#[tokio::test]
async fn a_marked_run_is_pending_until_it_is_completed() {
    let tmp = tempfile::tempdir().expect("tmp");
    let path = tmp.path().join("db");
    let run = GcRun {
        id: "run-1".to_string(),
        state: RunState::Marked,
        grace_secs: 3600,
        actor: "admin".to_string(),
        candidates: vec![
            RunCandidate {
                digest: ContentDigest::of_bytes(b"x"),
                length: 1,
            },
            RunCandidate {
                digest: ContentDigest::of_bytes(b"yy"),
                length: 2,
            },
        ],
        marked_at: Utc::now(),
        deleted: 0,
        bytes_freed: 0,
        swept_at: None,
        scope: None,
        generation: None,
    };
    let store = store(open(&path).await);
    store.initialize().await.expect("initialize");
    assert!(store.pending_run().await.expect("pending").is_none());
    store.open_run(&run).await.expect("open");
    // Idempotente: marcar dos veces la misma pasada no la duplica.
    store.open_run(&run).await.expect("open again");
    let pending = store
        .pending_run()
        .await
        .expect("pending")
        .expect("the run is still there");
    assert_eq!(pending.id, "run-1");
    assert_eq!(pending.candidates, run.candidates);
    assert_eq!(pending.grace_secs, 3600);

    store
        .complete_run("run-1", 2, 3, Utc::now())
        .await
        .expect("complete");
    assert!(store.pending_run().await.expect("pending").is_none());
    let last = store
        .last_swept_run()
        .await
        .expect("last")
        .expect("a swept run");
    assert_eq!(
        (last.state, last.deleted, last.bytes_freed),
        (RunState::Swept, 2, 3)
    );
}

#[tokio::test]
async fn confirming_a_deletion_leaves_a_trace_and_one_audit_event() {
    let tmp = tempfile::tempdir().expect("tmp");
    let db = open(&tmp.path().join("db")).await;
    let store = store(db.clone());
    store.initialize().await.expect("initialize");
    let digest = ContentDigest::of_bytes(b"gone");
    assert!(store.deletion_of(&digest).await.expect("none").is_none());

    let mut deletion = GcDeletion {
        digest: digest.clone(),
        length: 4,
        run_id: "run-9".to_string(),
        actor: "admin".to_string(),
        account: "hodei-admin".to_string(),
        state: DeletionState::Intended,
        at: Utc::now(),
        marker: None,
    };
    store.record_deletion(&deletion).await.expect("intended");
    assert_eq!(
        store
            .deletion_of(&digest)
            .await
            .expect("some")
            .expect("trace")
            .state,
        DeletionState::Intended
    );
    assert_eq!(audit_events(&db).await, 0, "la intencion no es un borrado");

    deletion.state = DeletionState::Confirmed;
    store.record_deletion(&deletion).await.expect("confirmed");
    store
        .record_deletion(&deletion)
        .await
        .expect("confirmed twice");
    let trace = store
        .deletion_of(&digest)
        .await
        .expect("some")
        .expect("trace");
    assert_eq!(trace.state, DeletionState::Confirmed);
    assert_eq!(trace.run_id, "run-9");
    assert_eq!(audit_events(&db).await, 1, "un borrado, un evento");
}

async fn audit_events(db: &Surreal<Db>) -> usize {
    let mut response = db
        .query("SELECT action FROM catalog_audit_outbox WHERE action = 'gc.delete'")
        .await
        .expect("query");
    #[derive(serde::Deserialize)]
    struct Row {
        #[allow(dead_code)]
        action: String,
    }
    let rows: Vec<Row> = response.take(0).expect("rows");
    rows.len()
}
