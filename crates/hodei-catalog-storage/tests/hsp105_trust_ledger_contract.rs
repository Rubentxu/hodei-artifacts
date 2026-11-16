//! El contrato del libro de concesiones de confianza contra SurrealDB de verdad.
//!
//! Lo que se comprueba: una concesion activa se ve por digest; repetirla no
//! duplica ni reactiva una revocada; se revoca por decision o por digest
//! sin tocar lo ajeno; cuentas y repositorios no se mezclan; y todo
//! sobrevive a reabrir la base.

use std::sync::Arc;

use chrono::{Duration, Utc};
use hodei_catalog::{AllowAllAuthorizer, TrustGrant, TrustLedger};
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

fn grant(account: &str, repository: &str, content: &[u8], decision: &str) -> TrustGrant {
    TrustGrant {
        account: account.to_string(),
        repository: repository.to_string(),
        digest: ContentDigest::of_bytes(content),
        decision_id: decision.to_string(),
        actor: "hrn:hodei:iam:acme:User/alice".to_string(),
        granted_at: Utc::now(),
        revoked_at: None,
        revoked_by: None,
        revoke_reason: None,
    }
}

async fn fresh() -> (tempfile::TempDir, SurrealCatalogStore) {
    let tmp = tempfile::tempdir().expect("tmp");
    let store = store(open(&tmp.path().join("db")).await);
    store.initialize().await.expect("initialize");
    (tmp, store)
}

#[tokio::test]
async fn a_grant_is_active_for_its_digest_and_only_for_it() {
    let (_tmp, store) = fresh().await;
    store
        .grant(&[grant("acme", "raw", b"one", "d-1")])
        .await
        .expect("grant");

    let found = store
        .active_grant("acme", "raw", &ContentDigest::of_bytes(b"one"))
        .await
        .expect("read")
        .expect("active");
    assert_eq!(found.decision_id, "d-1");
    assert!(found.is_active());

    let other = store
        .active_grant("acme", "raw", &ContentDigest::of_bytes(b"two"))
        .await
        .expect("read");
    assert!(other.is_none(), "otro digest no hereda la concesion");
}

#[tokio::test]
async fn granting_twice_neither_duplicates_nor_revives_a_revoked_grant() {
    let (_tmp, store) = fresh().await;
    let one = grant("acme", "raw", b"one", "d-1");
    store
        .grant(std::slice::from_ref(&one))
        .await
        .expect("grant");
    store
        .grant(std::slice::from_ref(&one))
        .await
        .expect("grant again");
    assert_eq!(store.grants_of("acme", "raw").await.expect("all").len(), 1);

    let revoked = store
        .revoke_decision("acme", "raw", "d-1", "alice", "cve", Utc::now())
        .await
        .expect("revoke");
    assert_eq!(revoked, 1);

    store
        .grant(std::slice::from_ref(&one))
        .await
        .expect("grant a third time");
    let digest = ContentDigest::of_bytes(b"one");
    assert!(
        store
            .active_grant("acme", "raw", &digest)
            .await
            .expect("read")
            .is_none(),
        "repetir una concesion revocada no la reactiva"
    );
    let all = store.grants_of("acme", "raw").await.expect("all");
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].revoked_by.as_deref(), Some("alice"));
    assert_eq!(all[0].revoke_reason.as_deref(), Some("cve"));
}

#[tokio::test]
async fn a_new_decision_grants_again_after_a_revocation() {
    let (_tmp, store) = fresh().await;
    store
        .grant(&[grant("acme", "raw", b"one", "d-1")])
        .await
        .expect("grant");
    store
        .revoke_decision("acme", "raw", "d-1", "alice", "cve", Utc::now())
        .await
        .expect("revoke");
    store
        .grant(&[grant("acme", "raw", b"one", "d-2")])
        .await
        .expect("grant");
    let found = store
        .active_grant("acme", "raw", &ContentDigest::of_bytes(b"one"))
        .await
        .expect("read")
        .expect("active");
    assert_eq!(found.decision_id, "d-2");
}

#[tokio::test]
async fn revoking_a_decision_leaves_a_shared_digest_alive_under_another() {
    let (_tmp, store) = fresh().await;
    // Un layer compartido por dos manifiestos: dos decisiones, un digest.
    store
        .grant(&[
            grant("acme", "oci", b"layer", "d-1"),
            grant("acme", "oci", b"manifest-a", "d-1"),
            grant("acme", "oci", b"layer", "d-2"),
            grant("acme", "oci", b"manifest-b", "d-2"),
        ])
        .await
        .expect("grant");

    let revoked = store
        .revoke_decision("acme", "oci", "d-1", "alice", "retirado", Utc::now())
        .await
        .expect("revoke");
    assert_eq!(revoked, 2);

    let layer = ContentDigest::of_bytes(b"layer");
    let kept = store
        .active_grant("acme", "oci", &layer)
        .await
        .expect("read")
        .expect("el layer sigue concedido por d-2");
    assert_eq!(kept.decision_id, "d-2");
    assert!(
        store
            .active_grant("acme", "oci", &ContentDigest::of_bytes(b"manifest-a"))
            .await
            .expect("read")
            .is_none()
    );
    // Revocar otra vez no cuenta nada: ya no hay activas.
    let again = store
        .revoke_decision("acme", "oci", "d-1", "alice", "retirado", Utc::now())
        .await
        .expect("revoke");
    assert_eq!(again, 0);
}

#[tokio::test]
async fn revoking_a_digest_removes_it_whatever_the_decision() {
    let (_tmp, store) = fresh().await;
    store
        .grant(&[
            grant("acme", "oci", b"layer", "d-1"),
            grant("acme", "oci", b"layer", "d-2"),
            grant("acme", "oci", b"keep", "d-2"),
        ])
        .await
        .expect("grant");
    let layer = ContentDigest::of_bytes(b"layer");
    let revoked = store
        .revoke_digest("acme", "oci", &layer, "bob", "vex", Utc::now())
        .await
        .expect("revoke");
    assert_eq!(revoked, 2);
    assert!(
        store
            .active_grant("acme", "oci", &layer)
            .await
            .expect("read")
            .is_none()
    );
    assert!(
        store
            .active_grant("acme", "oci", &ContentDigest::of_bytes(b"keep"))
            .await
            .expect("read")
            .is_some()
    );
}

#[tokio::test]
async fn accounts_and_repositories_do_not_share_grants() {
    let (_tmp, store) = fresh().await;
    store
        .grant(&[grant("acme", "raw", b"one", "d-1")])
        .await
        .expect("grant");
    let digest = ContentDigest::of_bytes(b"one");
    for (account, repository) in [("globex", "raw"), ("acme", "other")] {
        assert!(
            store
                .active_grant(account, repository, &digest)
                .await
                .expect("read")
                .is_none(),
            "{account}/{repository}"
        );
        assert!(
            store
                .grants_of(account, repository)
                .await
                .expect("all")
                .is_empty()
        );
    }
    // Revocar en otra cuenta no toca esta.
    let touched = store
        .revoke_digest("globex", "raw", &digest, "eve", "x", Utc::now())
        .await
        .expect("revoke");
    assert_eq!(touched, 0);
    assert!(
        store
            .active_grant("acme", "raw", &digest)
            .await
            .expect("read")
            .is_some()
    );
}

#[tokio::test]
async fn an_empty_batch_is_a_no_op() {
    let (_tmp, store) = fresh().await;
    store.grant(&[]).await.expect("grant");
    assert!(
        store
            .grants_of("acme", "raw")
            .await
            .expect("all")
            .is_empty()
    );
}

const RESTART_PATH: &str = "HSP105_RESTART_DB";

/// La fase de escritura del reinicio, en su propio proceso.
///
/// Es un test aparte para que el que verifica no tenga una rama que lo
/// saltee. Un reinicio real exige otro proceso: RocksDB guarda su cerrojo
/// dentro del proceso que lo abrio.
#[tokio::test]
#[ignore = "solo corre como el proceso hijo del test de reinicio"]
async fn the_grants_are_written_for_the_restart() {
    let path = std::env::var(RESTART_PATH).expect("the path of the database");
    let store = store(open(std::path::Path::new(&path)).await);
    store.initialize().await.expect("initialize");
    store
        .grant(&[
            grant("acme", "raw", b"one", "d-1"),
            grant("acme", "raw", b"two", "d-2"),
        ])
        .await
        .expect("grant");
    store
        .revoke_decision(
            "acme",
            "raw",
            "d-2",
            "alice",
            "cve",
            Utc::now() + Duration::seconds(1),
        )
        .await
        .expect("revoke");
}

#[tokio::test]
async fn grants_and_revocations_survive_a_restart() {
    let tmp = tempfile::tempdir().expect("tmp");
    let path = tmp.path().join("db");
    let status = tokio::process::Command::new(std::env::current_exe().expect("the test binary"))
        .arg("--exact")
        .arg("the_grants_are_written_for_the_restart")
        .arg("--ignored")
        .arg("--nocapture")
        .env(RESTART_PATH, path.to_string_lossy().as_ref())
        .status()
        .await
        .expect("the writing process");
    assert!(status.success(), "the writing process failed: {status}");

    let store = store(open(&path).await);
    store.initialize().await.expect("initialize again");
    assert!(
        store
            .active_grant("acme", "raw", &ContentDigest::of_bytes(b"one"))
            .await
            .expect("read")
            .is_some()
    );
    assert!(
        store
            .active_grant("acme", "raw", &ContentDigest::of_bytes(b"two"))
            .await
            .expect("read")
            .is_none(),
        "a revocation survives the restart"
    );
    assert_eq!(store.grants_of("acme", "raw").await.expect("all").len(), 2);
}
