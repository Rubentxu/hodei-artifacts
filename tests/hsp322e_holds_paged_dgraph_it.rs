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
use hodei_catalog::{
    AllowAllAuthorizer, ContentHold, ContentLedger, GcRun, HoldScope, RunCandidate, RunState,
};
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

/// Hold ids placed by a test, released by the harness even if it panics.
type Placed = Arc<std::sync::Mutex<Vec<String>>>;

/// Runs `body` in its own task and ALWAYS releases the holds it placed before
/// re-raising its panic, so a failed assertion leaves no holds behind.
async fn guarded<F, Fut>(body: F)
where
    F: FnOnce(Arc<DgraphCatalogStore>, Placed) -> Fut,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let Some(store) = open_store().await else {
        return;
    };
    let placed: Placed = Arc::default();
    let outcome = tokio::spawn(body(Arc::clone(&store), Arc::clone(&placed))).await;
    let ids = placed.lock().expect("placed ids").clone();
    for id in &ids {
        let _ = store.release_hold(id).await;
    }
    if let Err(error) = outcome {
        std::panic::resume_unwind(error.into_panic());
    }
}

fn digest_hold(id: String, scope: HoldScope) -> ContentHold {
    ContentHold {
        id,
        scope,
        reason: "hsp322e bulk".to_string(),
        actor: "ops".to_string(),
        created_at: Utc::now(),
        expires_at: None,
    }
}

/// Places one digest hold per digest, ids `<tag>-00000..`, registering each.
async fn place_many(
    store: &DgraphCatalogStore,
    placed: &Placed,
    tag: &str,
    digests: &[ContentDigest],
) -> Vec<String> {
    let mut ids = Vec::new();
    for (index, digest) in digests.iter().enumerate() {
        let id = format!("{tag}-{index:05}");
        placed.lock().expect("placed ids").push(id.clone());
        store
            .place_hold(&digest_hold(
                id.clone(),
                HoldScope::Digest {
                    digest: digest.clone(),
                },
            ))
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
    guarded(|store, placed| async move {
        let tag = format!("all{}", &nonce()[8..]);
        let ids = place_many(&store, &placed, &tag, &digests_of(&tag, HOLDS)).await;

        let seen = store.holds().await.expect("the holds are listed");
        let mine: Vec<&str> = seen
            .iter()
            .filter(|hold| hold.id.starts_with(&tag))
            .map(|hold| hold.id.as_str())
            .collect();
        assert_eq!(mine.len(), HOLDS, "no hold may be silently dropped");
        assert!(ids.iter().all(|id| mine.contains(&id.as_str())));
        let seen_ids = seen.iter().map(|hold| hold.id.clone()).collect::<Vec<_>>();
        let mut sorted = seen_ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(
            sorted, seen_ids,
            "ordered by hold_id and without duplicates"
        );
    })
    .await;
}

#[tokio::test]
async fn a_gc_pass_with_more_than_1000_holds_reclaims_no_held_blob() {
    guarded(|store, placed| async move {
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
        place_many(&store, &placed, &tag, &held).await;
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
    })
    .await;
}

#[tokio::test]
async fn the_defensive_bound_fails_closed_and_the_page_size_is_irrelevant_to_completeness() {
    guarded(|store, placed| async move {
        let tag = format!("cap{}", &nonce()[8..]);
        place_many(&store, &placed, &tag, &digests_of(&tag, 30)).await;
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
        assert_eq!(
            store
                .holds_bounded(7, total)
                .await
                .expect("at the bound")
                .len(),
            total
        );
        for page in [1, 7, 4000] {
            let seen = store.holds_bounded(page, total).await.expect("paged");
            assert_eq!(seen.len(), total, "page size {page}");
        }
    })
    .await;
}

/// `first`/`after` combined with an `@filter` loses no row: 700 digest holds
/// interleaved with 700 account holds, read through the filtered pager with
/// pages of 100 (more than 500 filtered rows, 14 pages).
#[tokio::test]
async fn paging_with_a_filter_returns_every_filtered_row_and_only_those() {
    guarded(|store, placed| async move {
        let tag = format!("flt{}", &nonce()[8..]);
        let digests = digests_of(&tag, 700);
        for (index, digest) in digests.iter().enumerate() {
            let digest_id = format!("{tag}-d{index:04}");
            let account_id = format!("{tag}-a{index:04}");
            {
                let mut ids = placed.lock().expect("placed ids");
                ids.push(digest_id.clone());
                ids.push(account_id.clone());
            }
            store
                .place_hold(&digest_hold(
                    digest_id,
                    HoldScope::Digest {
                        digest: digest.clone(),
                    },
                ))
                .await
                .expect("digest hold");
            store
                .place_hold(&digest_hold(
                    account_id,
                    HoldScope::Account {
                        account: format!("acct-{tag}"),
                    },
                ))
                .await
                .expect("account hold");
        }
        let rows = store
            .all_nodes(
                "has(hodei.gc.hold.hold_id)",
                "@filter(eq(hodei.gc.hold.scope_kind, \"digest\"))",
                "hodei.gc.hold.hold_id",
                100,
                1_000_000,
            )
            .await
            .expect("filtered pages");
        let mine: Vec<&str> = rows
            .iter()
            .filter_map(|row| row["hodei.gc.hold.hold_id"].as_str())
            .filter(|id| id.starts_with(&tag))
            .collect();
        assert_eq!(
            mine.len(),
            700,
            "every filtered row, none lost between pages"
        );
        assert!(mine.iter().all(|id| id[tag.len()..].starts_with("-d")));
    })
    .await;
}

/// More than 1000 candidates of one run come back from `pending_scoped_run`
/// (the nested edge used to be read without any row bound).
#[tokio::test]
async fn a_run_with_more_than_1000_candidates_is_read_whole() {
    let Some(store) = open_store().await else {
        return;
    };
    let scope = format!("it322ec{}", &nonce()[8..]);
    let ledger = Arc::clone(&store) as Arc<dyn ContentLedger>;
    let lease = ledger
        .claim_gc_pass(&scope, "worker", Duration::from_secs(120), Utc::now())
        .await
        .expect("the claim");
    let candidates: Vec<ContentDigest> = digests_of(&format!("cand{scope}"), HOLDS);
    let run = GcRun {
        id: format!("gc-322e-{}", &nonce()[8..]),
        state: RunState::Marked,
        grace_secs: 0,
        actor: scope.clone(),
        candidates: candidates
            .iter()
            .map(|digest| RunCandidate {
                digest: digest.clone(),
                length: 3,
            })
            .collect(),
        marked_at: Utc::now(),
        deleted: 0,
        bytes_freed: 0,
        swept_at: None,
        scope: Some(scope.clone()),
        generation: Some(lease.generation),
    };
    ledger
        .mark_run_fenced(&scope, &lease, &run, &candidates)
        .await
        .expect("the mark of 1500 candidates");
    let pending = ledger
        .pending_scoped_run(&scope, Utc::now())
        .await
        .expect("the pending run")
        .expect("it is pending");
    assert_eq!(
        pending.candidates.len(),
        HOLDS,
        "no candidate may be dropped"
    );
    let _ = ledger.release_gc_lease(&scope, &lease).await;
}
