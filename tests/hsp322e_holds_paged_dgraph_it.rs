//! HSP-322 tramo e: the hold reads are never silently truncated.
//!
//! Over the real Dgraph engine (gated on `HODEI_GRAPH_IT_ENDPOINT`):
//!
//! - more than 1000 holds are all returned by `ContentLedger::holds()`;
//! - a coordinated GC pass with more than 1000 digest holds reclaims NO
//!   held blob (before the fix the holds past the first 1000 were invisible
//!   to the plan and their blobs were swept);
//! - the defensive bound fails closed instead of returning a partial list.
//!
//! It runs serialized (`--test-threads=1`): the alpha is shared.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::Utc;
use hodei_artifacts_api::gc::{CoordinatedPass, GcService, PassConfig};
use hodei_catalog::{AllowAllAuthorizer, ContentHold, ContentLedger, HoldScope};
use hodei_catalog_storage::DgraphCatalogStore;
use hodei_evidence::ContentDigest;
use hodei_graph_client::{GraphClient, GraphClientConfig};
use hodei_hub::blob::{BlobStore, FileBlobStore};

const UAT_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/target/uat/hsp322e");

/// More than the 1000 rows that used to be returned.
const HOLDS: usize = 1500;

fn config_from_env() -> Option<GraphClientConfig> {
    let endpoint = std::env::var("HODEI_GRAPH_IT_ENDPOINT").ok()?;
    let user = std::env::var("HODEI_GRAPH_IT_USER").unwrap_or_default();
    let password = std::env::var("HODEI_GRAPH_IT_PASSWORD").unwrap_or_default();
    let mut text =
        format!("endpoint = \"{endpoint}\"\n\n[tls]\nenabled = false\nca_cert_path = \"\"\n");
    if !user.is_empty() {
        text.push_str(&format!(
            "\n[credentials]\nuserid = \"{user}\"\npassword = \"{password}\"\n"
        ));
    }
    Some(GraphClientConfig::from_toml_str(&text).expect("the IT configuration validates"))
}

async fn open_store() -> Option<Arc<DgraphCatalogStore>> {
    let Some(config) = config_from_env() else {
        assert!(
            std::env::var("HODEI_REQUIRE_IT").is_err(),
            "HODEI_REQUIRE_IT is set but HODEI_GRAPH_IT_ENDPOINT is not"
        );
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return None;
    };
    let mut client = GraphClient::connect(config).await.expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    Some(Arc::new(
        DgraphCatalogStore::open(Arc::new(client), Arc::new(AllowAllAuthorizer))
            .await
            .expect("the dgraph catalog store opens"),
    ))
}

fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

fn digest_hold(id: String, digest: ContentDigest) -> ContentHold {
    ContentHold {
        id,
        scope: HoldScope::Digest { digest },
        reason: "hsp322e bulk".to_string(),
        actor: "ops".to_string(),
        created_at: Utc::now(),
        expires_at: None,
    }
}

/// Places `HOLDS` digest holds with ids `<tag>-0000..` and returns the digests.
async fn place_many(
    store: &DgraphCatalogStore,
    tag: &str,
    digests: &[ContentDigest],
) -> Vec<String> {
    let mut ids = Vec::new();
    for (index, digest) in digests.iter().enumerate() {
        let id = format!("{tag}-{index:05}");
        store
            .place_hold(&digest_hold(id.clone(), digest.clone()))
            .await
            .expect("place hold");
        ids.push(id);
    }
    ids
}

fn digests_of(tag: &str, count: usize) -> Vec<ContentDigest> {
    (0..count)
        .map(|index| ContentDigest::of_bytes(format!("hsp322e-{tag}-{index}").as_bytes()))
        .collect()
}

#[tokio::test]
async fn holds_returns_every_hold_past_the_old_1000_row_ceiling() {
    let Some(store) = open_store().await else {
        return;
    };
    let tag = format!("all{}", &nonce()[8..]);
    let ids = place_many(&store, &tag, &digests_of(&tag, HOLDS)).await;

    let seen = store.holds().await.expect("the holds are listed");
    let mine: Vec<&str> = seen
        .iter()
        .filter(|hold| hold.id.starts_with(&tag))
        .map(|hold| hold.id.as_str())
        .collect();
    assert_eq!(mine.len(), HOLDS, "no hold may be silently dropped");
    assert!(ids.iter().all(|id| mine.contains(&id.as_str())));
    let mut sorted = seen.iter().map(|hold| hold.id.clone()).collect::<Vec<_>>();
    let unsorted = sorted.clone();
    sorted.sort();
    assert_eq!(sorted, unsorted, "the list keeps its hold_id order");

    for id in &ids {
        store.release_hold(id).await.expect("release");
    }
}

#[tokio::test]
async fn a_gc_pass_with_more_than_1000_holds_reclaims_no_held_blob() {
    let Some(store) = open_store().await else {
        return;
    };
    let tag = format!("gc{}", &nonce()[8..]);
    let scope = format!("it322e{tag}");
    let cas_root = std::path::Path::new(UAT_ROOT).join(format!("cas-{tag}"));
    std::fs::create_dir_all(&cas_root).expect("the UAT evidence directory");
    let content = Arc::new(FileBlobStore::new(&cas_root).expect("the file CAS"));

    // HOLDS orphan blobs, each protected by its own digest hold, plus one
    // genuine orphan with no hold that the pass must still reclaim.
    let mut held = Vec::new();
    for index in 0..HOLDS {
        held.push(
            content
                .put(format!("hsp322e-{tag}-{index}").as_bytes())
                .await
                .expect("a held blob"),
        );
    }
    let ids = place_many(&store, &tag, &held).await;
    let unheld = content
        .put(format!("hsp322e-unheld-{tag}").as_bytes())
        .await
        .expect("the unheld orphan");

    let ledger = Arc::clone(&store) as Arc<dyn ContentLedger>;
    let service = Arc::new(
        GcService::new(
            ledger,
            Arc::clone(&content) as Arc<dyn BlobStore>,
            Duration::ZERO,
        )
        .expect("the file store reclaims"),
    );
    let pass = CoordinatedPass::new(
        service,
        PassConfig::new(scope, "worker", Duration::from_secs(60)).with_page_limit(50),
    );
    let report = pass.run().await.expect("the coordinated pass");

    let mut lost = 0usize;
    for digest in &held {
        if !content.exists(digest).await.expect("the listing") {
            lost += 1;
        }
    }
    assert_eq!(lost, 0, "{lost} held blobs were reclaimed: {report:?}");
    assert!(
        !content.exists(&unheld).await.expect("the listing"),
        "the unheld orphan is still reclaimed: {report:?}"
    );

    for id in &ids {
        store.release_hold(id).await.expect("release");
    }
}

#[tokio::test]
async fn the_defensive_bound_fails_closed_and_the_page_size_is_irrelevant_to_completeness() {
    let Some(store) = open_store().await else {
        return;
    };
    let tag = format!("cap{}", &nonce()[8..]);
    let ids = place_many(&store, &tag, &digests_of(&tag, 30)).await;
    let total = store.holds().await.expect("all holds").len();
    assert!(total >= 30);

    // A bound below what exists is an error, never a partial list.
    let refused = store.holds_bounded(7, total - 1).await;
    assert!(
        matches!(
            refused,
            Err(hodei_catalog::CatalogError::Unavailable { .. })
        ),
        "the bound must fail closed: {refused:?}"
    );
    // Exactly at the bound it is complete.
    assert_eq!(
        store
            .holds_bounded(7, total)
            .await
            .expect("at the bound")
            .len(),
        total
    );
    // Odd page sizes, one row per page and a page bigger than the server's
    // old row ceiling all return the same complete list.
    for page in [1, 7, 4000] {
        let seen = store.holds_bounded(page, total).await.expect("paged");
        assert_eq!(seen.len(), total, "page size {page}");
    }

    for id in &ids {
        store.release_hold(id).await.expect("release");
    }
}
