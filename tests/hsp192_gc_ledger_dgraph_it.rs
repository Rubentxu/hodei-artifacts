//! HSP-192: the GC coordinated ledger over the real Dgraph engine.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT` like all the stack ITs. What it
//! proves, against the live h186 server (TLS + ACL), with its own scope
//! keys per execution (`it192<tag><nonce>`, never a global census --
//! on the same alpha there are OCI and simulation slices in flight):
//!
//! - **AC1** the claim is a DURABLE CAS guard on the lease row
//!   (not an in-memory counter): a live foreign claim occupies the scope
//!   (`Conflict`), the take-over of the expired lease raises the generation and gives a
//!   new token, and a zombie with the old generation gets
//!   `PreconditionFailed` on EVERY pass operation -- mark,
//!   reconfirm, record, renew or close. Two clients on
//!   disjoint scopes coexist on the same alpha;
//! - **AC2/delivery 2** the mark and the tombstone carry the reference
//!   guard IN the same transaction: a candidate with a live root
//!   aborts the mark and leaves no trace, and the verdict is decided by
//!   post-commit re-read (HSP-205), never by "the upsert returned OK";
//! - **AC3** the census goes by account MEMBERSHIP (paginated, cursor =
//!   digest), and the global surfaces (`content_roots`,
//!   `pending_run`) fail closed per HSP-220;
//! - and the complete coordinated pass (`CoordinatedPass` +
//!   `FileBlobStore` under `target/uat/hsp192/`) sweeps the orphan and does not
//!   touch what is published, with the pages counted.
//!
//! It runs serialized (`--test-threads=1`): the alpha is shared.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::Utc;
use hodei_artifacts_api::gc::{CoordinatedPass, GcService, PassAction, PassConfig};
use hodei_catalog::{
    AllowAllAuthorizer, CatalogError, CatalogWriter, ContentLedger, DeletionState, DigestScope,
    ContentHold, GcDeletion, GcRun, HoldScope, PackageCoordinates, Publication, RepositoryConfig, RepositoryDescriptor,
    RepositoryFormat, RepositoryMode, RootKind, RunCandidate, RunState, repository_hrn,
};
use hodei_catalog_storage::DgraphCatalogStore;
use hodei_evidence::ContentDigest;
use hodei_graph_client::{
    GraphClient, GraphClientConfig, GraphMutationBlock, GraphMutationPort, GraphUpsertTransaction,
};
use hodei_hub::blob::{BlobStore, FileBlobStore};

/// The HSP-192 evidence directory: the test object stores
/// live here and are NEVER deleted (workspace convention).
const UAT_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/target/uat/hsp192");

const GRACE: Duration = Duration::ZERO;

fn config_from_env() -> Option<GraphClientConfig> {
    let endpoint = std::env::var("HODEI_GRAPH_IT_ENDPOINT").ok()?;
    let ca = std::env::var("HODEI_GRAPH_IT_CA").unwrap_or_default();
    let server_name = std::env::var("HODEI_GRAPH_IT_SERVER_NAME").unwrap_or_default();
    let user = std::env::var("HODEI_GRAPH_IT_USER").unwrap_or_default();
    let password = std::env::var("HODEI_GRAPH_IT_PASSWORD").unwrap_or_default();
    let mut text = format!(
        "endpoint = \"{endpoint}\"\n\n[tls]\nenabled = {enabled}\nca_cert_path = \"{ca}\"\n",
        enabled = !ca.is_empty()
    );
    if !server_name.is_empty() {
        text.push_str(&format!("server_name = \"{server_name}\"\n"));
    }
    if !user.is_empty() {
        text.push_str(&format!(
            "\n[credentials]\nuserid = \"{user}\"\npassword = \"{password}\"\n"
        ));
    }
    Some(GraphClientConfig::from_toml_str(&text).expect("the IT configuration validates"))
}

/// A new authenticated connection. `None` without environment: the IT is gated.
async fn fresh_client() -> Option<Arc<GraphClient>> {
    let Some(config) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return None;
    };
    let mut client = GraphClient::connect(config).await.expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    Some(Arc::new(client))
}

fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

/// One scope (the account) per test and per execution: no run
/// runs into the leases of another, and the membership census only sees
/// what belongs to this test.
fn account(tag: &str) -> String {
    format!("it192{}{}", tag, &nonce()[8..])
}

async fn open_store(client: Arc<GraphClient>) -> Arc<DgraphCatalogStore> {
    Arc::new(
        DgraphCatalogStore::open(client.clone(), Arc::new(AllowAllAuthorizer))
            .await
            .expect("the dgraph catalog store opens"),
    )
}

/// The coordinated ledger: the same catalog row, seen as a ledger.
fn ledger(store: &Arc<DgraphCatalogStore>) -> Arc<dyn ContentLedger> {
    Arc::clone(store) as Arc<dyn ContentLedger>
}

fn a_descriptor(account: &str, repo_key: &str) -> RepositoryDescriptor {
    RepositoryDescriptor {
        hrn: repository_hrn("hodei", "artifacts", "eu", account, repo_key),
        account: account.to_string(),
        repo_key: repo_key.to_string(),
        format: RepositoryFormat::Raw,
        mode: RepositoryMode::Hosted,
        config: RepositoryConfig::new(DigestScope::Global),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn a_publication(account: &str, content: &[u8], slot: &str) -> Publication {
    Publication {
        repository: "raw".to_string(),
        asset_path: format!("app/{slot}.bin"),
        // The fixture version is the slot: two publications of the
        // same package would collide on `pkg@1.0.0` (the raw catalog
        // is immutable per version) and the second would give Conflict.
        coordinates: PackageCoordinates {
            package_name: "pkg".to_string(),
            package_version: slot.to_string(),
        },
        digest: ContentDigest::of_bytes(content),
        size_bytes: content.len() as u64,
        tags: Vec::new(),
        actor: format!("hrn:hodei:iam:{account}:User/alice"),
        correlation_id: format!("corr-hsp192-{account}-{slot}"),
        overwrite: false,
        precondition: None,
        subject: None,
    }
}

fn orphan_digest(label: &str) -> ContentDigest {
    ContentDigest::of_bytes(format!("orphan-{label}-{}", nonce()).as_bytes())
}

/// One run_id per test and per execution: the run key is the id, and
/// two runs must not step on each other's durable mark.
fn run_id(tag: &str) -> String {
    format!("gc-{tag}-{}", &nonce()[8..])
}

fn run_candidate(digest: ContentDigest) -> RunCandidate {
    RunCandidate { digest, length: 12 }
}

fn a_run(scope: &str, run_id: &str, generation: u64, candidates: Vec<RunCandidate>) -> GcRun {
    GcRun {
        id: run_id.to_string(),
        state: RunState::Marked,
        grace_secs: GRACE.as_secs(),
        actor: scope.to_string(),
        candidates,
        marked_at: Utc::now(),
        deleted: 0,
        bytes_freed: 0,
        swept_at: None,
        scope: Some(scope.to_string()),
        generation: Some(generation),
    }
}

fn a_deletion(digest: &ContentDigest, scope: &str, run_id: &str) -> GcDeletion {
    GcDeletion {
        digest: digest.clone(),
        length: 12,
        run_id: run_id.to_string(),
        actor: "worker".to_string(),
        account: scope.to_string(),
        state: DeletionState::Intended,
        at: Utc::now(),
        marker: Some(format!("mtime:test:{}", digest.value)),
    }
}

// ---------------------------------------------------------------------------
// AC1: the durable CAS claim and the zombie with the expired generation.
// ---------------------------------------------------------------------------

/// The claim lives in the authority: a live foreign claim occupies the scope,
/// the same owner renews WITHOUT changing token or generation, and two
/// disjoint scopes coexist on the same alpha with two different clients.
#[tokio::test]
async fn the_claim_is_a_durable_cas_and_a_live_foreign_lease_busy_the_scope() {
    let Some(client_a) = fresh_client().await else {
        return;
    };
    let client_b = fresh_client().await.expect("the second replica connects");
    let store_a = open_store(client_a).await;
    let store_b = open_store(client_b).await;

    let scope = account("claim");
    let foreign = account("foreign");
    let now = Utc::now();

    assert!(ledger(&store_a).supports_fenced_pass());

    // Initial claim: generation 1, durable and re-readable post-commit.
    let lease = ledger(&store_a)
        .claim_gc_pass(&scope, "worker-a", Duration::from_secs(30), now)
        .await
        .expect("the first claim");
    assert_eq!(lease.generation, 1);
    let back = ledger(&store_a)
        .current_gc_lease(&scope, Utc::now())
        .await
        .expect("the lease is readable")
        .expect("the lease is live");
    assert_eq!(back.token, lease.token);
    assert_eq!(back.generation, lease.generation);

    // Another owner with a live lease: the scope is OCCUPIED (Conflict),
    // decided by the server's guard, not by a pre-read.
    let busy = ledger(&store_b)
        .claim_gc_pass(&scope, "worker-b", Duration::from_secs(30), Utc::now())
        .await;
    assert!(
        matches!(busy, Err(CatalogError::Conflict { .. })),
        "a live foreign lease must busy the scope: {busy:?}"
    );

    // The same owner renews: same generation, same token.
    let renewed = ledger(&store_a)
        .claim_gc_pass(&scope, "worker-a", Duration::from_secs(30), Utc::now())
        .await
        .expect("the owner renews its own live lease");
    assert_eq!(renewed.generation, lease.generation);
    assert_eq!(renewed.token, lease.token);

    // Disjoint scopes on the SAME alpha with another client: they do not
    // step on each other (the lease key is per scope).
    let other = ledger(&store_b)
        .claim_gc_pass(&foreign, "worker-b", Duration::from_secs(30), Utc::now())
        .await
        .expect("a disjoint scope claims independently");
    assert_eq!(other.generation, 1);
    assert_ne!(other.token, lease.token);
}

/// The zombie of the expired generation can NOT keep deleting: mark,
/// reconfirm, record, renew and close fail with the fence, and the
/// trace of the blob the zombie meant to sweep does not exist. The
/// authority (the lease row) decided the take-over, not the worker.
#[tokio::test]
async fn a_zombie_with_an_expired_lease_cannot_keep_sweeping_the_new_generation() {
    let Some(client_a) = fresh_client().await else {
        return;
    };
    let client_b = fresh_client().await.expect("the second replica connects");
    let store_a = open_store(client_a).await;
    let store_b = open_store(client_b).await;
    let (scope_a, ledger_a, ledger_b) = {
        let scope = account("zombie");
        (scope, ledger(&store_a), ledger(&store_b))
    };

    // Generation 1 with a lease that expires on its own.
    let zombie = ledger_a
        .claim_gc_pass(&scope_a, "worker-a", Duration::from_secs(2), Utc::now())
        .await
        .expect("the zombie claims");
    assert_eq!(zombie.generation, 1);
    tokio::time::sleep(Duration::from_millis(2_500)).await;
    assert!(
        ledger_a
            .current_gc_lease(&scope_a, Utc::now())
            .await
            .expect("the lease read")
            .is_none(),
        "an expired lease is no longer live"
    );

    // Real take-over: another worker, new generation and NEW token.
    let fresh = ledger_b
        .claim_gc_pass(&scope_a, "worker-b", Duration::from_secs(30), Utc::now())
        .await
        .expect("the take-over of an expired claim");
    assert_eq!(fresh.generation, 2);
    assert_ne!(fresh.token, zombie.token);

    // The zombie presents its old token on EVERY pass operation.
    let bait = orphan_digest("zombie-bait");
    let zombie_run = run_id("zombie");
    let marked = ledger_a
        .mark_run_fenced(
            &scope_a,
            &zombie,
            &a_run(&scope_a, &zombie_run, 1, vec![run_candidate(bait.clone())]),
            std::slice::from_ref(&bait),
        )
        .await;
    assert!(
        matches!(marked, Err(CatalogError::PreconditionFailed { .. })),
        "the zombie cannot write the mark: {marked:?}"
    );
    assert!(matches!(
        ledger_a.mark_of_run(&scope_a, &zombie, &zombie_run).await,
        Err(CatalogError::PreconditionFailed { .. })
    ));
    assert!(matches!(
        ledger_a
            .record_deletion_fenced(&scope_a, &zombie, &a_deletion(&bait, &scope_a, &zombie_run))
            .await,
        Err(CatalogError::PreconditionFailed { .. })
    ));
    assert!(matches!(
        ledger_a
            .renew_gc_lease(
                &scope_a,
                &zombie,
                "worker-a",
                Duration::from_secs(30),
                Utc::now()
            )
            .await,
        Err(CatalogError::PreconditionFailed { .. })
    ));
    assert!(matches!(
        ledger_a
            .complete_scoped_run(&scope_a, &zombie, &zombie_run, 1, 12, 0, Utc::now())
            .await,
        Err(CatalogError::PreconditionFailed { .. })
    ));

    // And the final verdict is the re-read: the zombie left NO mark nor
    // trace -- it could not keep deleting the new generation.
    assert!(
        ledger_a
            .pending_scoped_run(&scope_a, Utc::now())
            .await
            .expect("the pending read")
            .is_none(),
        "the zombie left no durable mark"
    );
    assert!(
        ledger_a
            .deletion_of(&bait)
            .await
            .expect("the tombstone read")
            .is_none(),
        "the zombie left no tombstone"
    );

    // The live holder does operate under its new token.
    let live_bait = orphan_digest("holder");
    ledger_b
        .mark_run_fenced(
            &scope_a,
            &fresh,
            &a_run(
                &scope_a,
                &run_id("holder"),
                2,
                vec![run_candidate(live_bait.clone())],
            ),
            &[live_bait],
        )
        .await
        .expect("the live holder marks");
}

// ---------------------------------------------------------------------------
// AC3 + delivery 2: membership census and guards in the same transaction.
// ---------------------------------------------------------------------------

/// The census goes by account MEMBERSHIP and by bounded pages: the graph is
/// never materialized, and the global surfaces fail closed
/// (HSP-220). The foreign account does not see a single root.
#[tokio::test]
async fn the_census_pages_by_membership_and_the_global_surfaces_fail_closed() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let store = open_store(client).await;
    let scope = account("census");
    store
        .create_repository(&scope, a_descriptor(&scope, "raw"))
        .await
        .expect("the repository");

    let mut published = Vec::new();
    for slot in ["one", "two", "three"] {
        let content = format!("census-{slot}-{scope}").into_bytes();
        let publication = a_publication(&scope, &content, slot);
        published.push(publication.digest.clone());
        store
            .publish(&scope, publication)
            .await
            .expect("the publication");
    }
    // Each raw publication leaves THREE roots with a digest (asset, version and
    // reference): the membership census sees them all, nine in total.
    let expected_roots = published.len() * 3;

    // The global surfaces do not exist in this authority: a materialized
    // census would be the forbidden scan.
    assert!(matches!(
        ledger(&store).content_roots().await,
        Err(CatalogError::Unavailable { .. })
    ));
    assert!(matches!(
        ledger(&store).pending_run().await,
        Err(CatalogError::Unavailable { .. })
    ));

    // Paginated with bound 2 over 9 roots: more than one page, cursor =
    // digest, and together they cover the own census EXACTLY once
    // (without materializing and without repeating at the family crossing).
    let mut seen: Vec<(RootKind, String, ContentDigest)> = Vec::new();
    let mut cursor: Option<String> = None;
    let mut pages = 0usize;
    loop {
        let page = ledger(&store)
            .root_page(&scope, cursor.as_deref(), 2)
            .await
            .expect("the census page");
        pages += 1;
        assert!(!page.roots.is_empty(), "a bounded page must carry rows");
        // The bound is hard on the DISTINCT rows of the edge: ties of the
        // cut digest are completed on the same page
        // (they are the same census key seen through another family).
        let distinct_at_edge: Vec<String> = page
            .roots
            .iter()
            .map(|root| root.digest.to_string())
            .collect();
        let mut edge = distinct_at_edge.clone();
        edge.dedup();
        assert!(
            edge.len() <= 2,
            "the page crosses at most `limit` distinct digests: {edge:?}"
        );
        for root in &page.roots {
            assert_eq!(root.account, scope, "the census is by membership");
            seen.push((root.kind, root.subject.clone(), root.digest.clone()));
        }
        if !page.has_more {
            break;
        }
        cursor = Some(
            page.roots
                .last()
                .expect("a full page has a cursor")
                .digest
                .to_string(),
        );
        assert!(pages < 32, "the pagination must converge");
    }
    assert!(pages > 1, "nine roots with limit 2 cannot fit one page");
    assert_eq!(seen.len(), expected_roots, "the census saw every root once");
    let mut per_digest: Vec<(String, usize)> = seen
        .iter()
        .map(|(_, _, digest)| (digest.value.clone(), 1usize))
        .collect();
    per_digest.sort_by(|a, b| a.0.cmp(&b.0));
    let mut merged: Vec<(String, usize)> = Vec::new();
    for (value, one) in per_digest {
        match merged.last_mut() {
            Some((previous, count)) if *previous == value => *count += one,
            _ => merged.push((value, one)),
        }
    }
    assert!(
        merged.iter().all(|(_, count)| *count == 3),
        "each publication contributes exactly its three roots: {merged:?}"
    );
    for digest in &published {
        let mut mine: Vec<_> = seen
            .iter()
            .filter(|(_, _, found)| found == digest)
            .map(|(kind, _, _)| *kind)
            .collect();
        // The order within the same digest is that of the row key,
        // not that of the family: it is compared as a set.
        mine.sort_by_key(|kind| kind.as_str().to_owned());
        assert_eq!(
            mine,
            [RootKind::Asset, RootKind::Reference, RootKind::Version],
            "each publication contributes its three membership roots"
        );
    }

    // A foreign account sees NOTHING: the page goes by membership.
    let foreign = account("census-foreign");
    let page = ledger(&store)
        .root_page(&foreign, None, 10)
        .await
        .expect("the foreign page");
    assert!(page.roots.is_empty());
    assert!(!page.has_more);

    // The fresh-references probe: what is published is alive, the
    // orphan is not.
    assert!(
        ledger(&store)
            .live_reference_count(&published[0])
            .await
            .expect("the reference probe")
            > 0
    );
    assert_eq!(
        ledger(&store)
            .live_reference_count(&orphan_digest("uncatalogued"))
            .await
            .expect("the reference probe"),
        0
    );
}

/// The guards travel IN the transaction and the verdict is the post-commit
/// re-read: a candidate with a live root aborts the mark; a tombstone
/// over referenced content is not written (even if the lease is
/// live), and what the authority says on reading is the only thing that counts.
#[tokio::test]
async fn the_reference_guard_lives_in_the_mark_and_the_verdict_is_the_post_commit_read() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let store = open_store(client).await;
    let scope = account("guard");
    store
        .create_repository(&scope, a_descriptor(&scope, "raw"))
        .await
        .expect("the repository");
    let content = format!("guarded-{scope}").into_bytes();
    let publication = a_publication(&scope, &content, "guarded");
    let referenced = publication.digest.clone();
    store
        .publish(&scope, publication)
        .await
        .expect("the publication");

    let lease = ledger(&store)
        .claim_gc_pass(&scope, "worker", Duration::from_secs(30), Utc::now())
        .await
        .expect("the claim");
    let free = orphan_digest("truly-orphan");

    // Mark with a referenced candidate: the guard aborts the WHOLE
    // mark, the orphan does not sneak in half-way.
    let guarded_run = run_id("guarded");
    let marked = ledger(&store)
        .mark_run_fenced(
            &scope,
            &lease,
            &a_run(
                &scope,
                &guarded_run,
                lease.generation,
                vec![
                    run_candidate(free.clone()),
                    run_candidate(referenced.clone()),
                ],
            ),
            &[free.clone(), referenced.clone()],
        )
        .await;
    assert!(
        matches!(marked, Err(CatalogError::PreconditionFailed { .. })),
        "a referenced candidate aborts the mark: {marked:?}"
    );

    // The tombstone of what is referenced, WITH a live lease: the reference
    // guard blocks it in the upsert itself and the re-read says
    // it did not stay -- a post-commit verdict, not faith in the commit.
    let wrote = ledger(&store)
        .record_deletion_fenced(
            &scope,
            &lease,
            &a_deletion(&referenced, &scope, &guarded_run),
        )
        .await;
    assert!(
        matches!(wrote, Err(CatalogError::Unavailable { .. })),
        "the tombstone of live content must not land: {wrote:?}"
    );
    assert!(
        ledger(&store)
            .deletion_of(&referenced)
            .await
            .expect("the tombstone read")
            .is_none()
    );

    // The free candidate does: the mark goes through, and the trace is written and
    // confirmed under the live token.
    let clean_run = run_id("clean");
    let marked = ledger(&store)
        .mark_run_fenced(
            &scope,
            &lease,
            &a_run(
                &scope,
                &clean_run,
                lease.generation,
                vec![run_candidate(free.clone())],
            ),
            std::slice::from_ref(&free),
        )
        .await
        .expect("the clean mark");
    assert_eq!(marked.len(), 1);
    ledger(&store)
        .record_deletion_fenced(&scope, &lease, &a_deletion(&free, &scope, &clean_run))
        .await
        .expect("the intention of the orphan lands");
    let trace = ledger(&store)
        .deletion_of(&free)
        .await
        .expect("the tombstone read")
        .expect("the tombstone is there after commit");
    assert_eq!(trace.state, DeletionState::Intended);

    // The pending mark is DURABLE: another client of the same scope sees it
    // under the live generation (the authority rules, not the process).
    let pending = ledger(&store)
        .pending_scoped_run(&scope, Utc::now())
        .await
        .expect("the pending read");
    assert_eq!(
        pending.map(|run| run.id).as_deref(),
        Some(clean_run.as_str())
    );
}

// ---------------------------------------------------------------------------
// The complete coordinated pass over the real authority + the file CAS.
// ---------------------------------------------------------------------------

/// The plan above in action: `CoordinatedPass` over the Dgraph ledger
/// and a `FileBlobStore` under `target/uat/hsp192/` sweeps ONLY the orphan,
/// does not touch what is published, and the census is counted by pages.
#[tokio::test]
async fn the_coordinated_pass_sweeps_the_orphan_and_leaves_the_referenced_alone() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let store = open_store(client).await;
    let scope = account("sweep");

    let cas_root = std::path::Path::new(UAT_ROOT).join(format!("cas-{scope}"));
    std::fs::create_dir_all(&cas_root).expect("the UAT evidence directory");
    let content = Arc::new(FileBlobStore::new(&cas_root).expect("the file CAS"));

    store
        .create_repository(&scope, a_descriptor(&scope, "raw"))
        .await
        .expect("the repository");
    let mut referenced = Vec::new();
    for slot in ["keep-one", "keep-two", "keep-three"] {
        let bytes = format!("sweep-{slot}-{scope}").into_bytes();
        content
            .put(&bytes)
            .await
            .expect("the blob of a published asset");
        referenced.push(ContentDigest::of_bytes(&bytes));
        store
            .publish(&scope, a_publication(&scope, &bytes, slot))
            .await
            .expect("the publication");
    }
    let orphan = content
        .put(b"orphan sweep bait")
        .await
        .expect("the orphan blob");

    let service = Arc::new(
        GcService::new(
            ledger(&store),
            Arc::clone(&content) as Arc<dyn BlobStore>,
            GRACE,
        )
        .expect("the file store reclaims"),
    );
    let pass = CoordinatedPass::new(
        Arc::clone(&service),
        PassConfig::new(scope.clone(), "worker", Duration::from_secs(30)).with_page_limit(2),
    );

    let report = pass.run().await.expect("the coordinated pass");
    assert_eq!(report.deleted, 1, "{report:?}");
    assert_eq!(report.bytes_freed, b"orphan sweep bait".len() as u64);
    assert_eq!(report.unknown, 0);
    // Nine own roots with page bound 2: the census is walked
    // in SEVERAL bounded pages, never all at once. The exact number
    // depends on where the edge ties fall, it is not the
    // property; the property is that there is more than one page.
    assert!(report.census_pages > 1, "{report:?}");
    assert_eq!(
        report.actions.get(&orphan.to_string()),
        Some(&PassAction::Deleted)
    );
    assert!(
        !content.exists(&orphan).await.expect("the listing"),
        "the orphan is gone"
    );
    for digest in &referenced {
        assert!(
            !report.actions.contains_key(&digest.to_string()),
            "referenced content must not even be a candidate"
        );
        assert!(
            content.exists(digest).await.expect("the listing"),
            "the referenced blob stays"
        );
    }

    // The trace confirmed in the authority: post-effect, post-commit.
    let trace = ledger(&store)
        .deletion_of(&orphan)
        .await
        .expect("the tombstone read")
        .expect("the sweep left its trace");
    assert_eq!(trace.state, DeletionState::Confirmed);

    // A second pass finds nothing to sweep: the census is
    // stable and the orphan is no longer there.
    let second = pass.run().await.expect("the second pass");
    assert_eq!(second.marked, 0, "{second:?}");
    assert_eq!(second.deleted, 0);
}

/// A finished pass hands its claim back, so the next owner takes the scope
/// without waiting for the lease to run out; a stale token cannot release.
#[tokio::test]
async fn a_released_claim_is_free_for_the_next_owner_and_only_the_live_token_releases() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let store = open_store(client).await;
    let scope = account("release");

    let first = ledger(&store)
        .claim_gc_pass(&scope, "worker-a", Duration::from_secs(300), Utc::now())
        .await
        .expect("the first claim");
    let busy = ledger(&store)
        .claim_gc_pass(&scope, "worker-b", Duration::from_secs(300), Utc::now())
        .await;
    assert!(matches!(busy, Err(CatalogError::Conflict { .. })), "{busy:?}");

    let mut stale = first.clone();
    stale.token = "not-the-live-token".to_string();
    assert!(matches!(
        ledger(&store).release_gc_lease(&scope, &stale).await,
        Err(CatalogError::PreconditionFailed { .. })
    ));

    ledger(&store)
        .release_gc_lease(&scope, &first)
        .await
        .expect("the live token releases");
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let next = ledger(&store)
        .claim_gc_pass(&scope, "worker-b", Duration::from_secs(300), Utc::now())
        .await
        .expect("a released scope is free");
    assert_eq!(next.generation, first.generation + 1, "a new generation");
}

/// The hold lifecycle on the graph: placing it, listing it, releasing it
/// (once), and the account census that feeds the platform-wide sweep.
#[tokio::test]
async fn a_hold_is_released_once_and_the_census_lists_the_accounts_with_repositories() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let store = open_store(client).await;
    let owner = account("hold");

    let hold = ContentHold {
        id: format!("hold-{}", nonce()),
        scope: HoldScope::Account {
            account: owner.clone(),
        },
        reason: "release lifecycle".to_string(),
        actor: "ops".to_string(),
        created_at: Utc::now(),
        expires_at: None,
    };
    ledger(&store).place_hold(&hold).await.expect("place");
    assert!(
        ledger(&store)
            .holds()
            .await
            .expect("list")
            .iter()
            .any(|seen| seen.id == hold.id)
    );
    assert!(ledger(&store).release_hold(&hold.id).await.expect("release"));
    assert!(
        !ledger(&store).release_hold(&hold.id).await.expect("again"),
        "releasing twice is not a second release"
    );
    assert!(
        !ledger(&store)
            .holds()
            .await
            .expect("list")
            .iter()
            .any(|seen| seen.id == hold.id)
    );

    store
        .create_repository(&owner, a_descriptor(&owner, "raw"))
        .await
        .expect("the repository is created");
    let accounts = ledger(&store).census_accounts().await.expect("accounts");
    assert_eq!(
        accounts.iter().filter(|seen| **seen == owner).count(),
        1,
        "one row per account, not per repository: {accounts:?}"
    );
}

/// An account whose rows still root a digest but that has no repository row
/// (a repository removed without cascade, a direct seed, a migration) is
/// part of the census: its digest stays retained, never reclaimable.
#[tokio::test]
async fn an_account_with_roots_and_no_repository_row_still_retains_its_digest() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let store = open_store(Arc::clone(&client)).await;
    let owner = account("norepo");

    let cas_root = std::path::Path::new(UAT_ROOT).join(format!("cas-{owner}"));
    std::fs::create_dir_all(&cas_root).expect("the UAT evidence directory");
    let content = Arc::new(FileBlobStore::new(&cas_root).expect("the file CAS"));

    store
        .create_repository(&owner, a_descriptor(&owner, "raw"))
        .await
        .expect("the repository");
    let bytes = format!("rooted-without-repository-{owner}").into_bytes();
    content.put(&bytes).await.expect("the blob");
    store
        .publish(&owner, a_publication(&owner, &bytes, "rooted"))
        .await
        .expect("the publication");

    // Remove ONLY the repository row: the asset, version and reference rows
    // of the account stay, as after a removal without cascade.
    client
        .upsert(GraphUpsertTransaction {
            query: format!(
                "{{ r as var(func: eq(hodei.catalog.repository.account, \"{owner}\")) }}"
            ),
            blocks: vec![GraphMutationBlock {
                cond: Some("eq(len(r), 1)".to_owned()),
                set_nquads: String::new(),
                delete_nquads: "uid(r) * * .\n".to_owned(),
            }],
            commit_now: true,
        })
        .await
        .expect("the repository row is removed");

    let accounts = ledger(&store).census_accounts().await.expect("accounts");
    assert!(
        accounts.contains(&owner),
        "the census must include an account that only has roots: {accounts:?}"
    );

    let service = GcService::new(
        ledger(&store),
        Arc::clone(&content) as Arc<dyn BlobStore>,
        GRACE,
    )
    .expect("the file store reclaims");
    let plan = serde_json::to_value(&*service.dry_run().await.expect("the dry-run"))
        .expect("the plan serialises");
    let digest = ContentDigest::of_bytes(&bytes).to_string();
    let entry = plan["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .find(|entry| entry["digest"] == digest.as_str())
        .unwrap_or_else(|| panic!("the plan lists {digest}"));
    assert_eq!(entry["verdict"], "retained", "{entry:#}");
}
