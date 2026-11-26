//! The catalog search of the graph profile, against a REAL Dgraph alpha:
//! the trusted channel, the trust state and the text query.
//!
//! The surface suite (`hsp113_search_e2e`) measures the routes; this one
//! measures the adapter: what the search reads from the grant ledger, what
//! it refuses to read, and what a repository larger than one query keeps
//! of its order and its total. Every test owns an ACL namespace, so the
//! fixed accounts do not leak between tests. The suite skips itself (exit
//! 0) without `HODEI_GRAPH_IT_ENDPOINT`.

use std::sync::Arc;

use chrono::{Duration, Utc};
use hodei_catalog::search::{SearchPage, SearchQuery, search_cursor};
use hodei_catalog::{
    CatalogError, CatalogSearch, CatalogWriter, DigestScope, PackageCoordinates, Publication,
    RepositoryConfig, RepositoryDescriptor, RepositoryFormat, RepositoryMode, TrustGrant,
    TrustLedger, TrustState, repository_hrn,
};
use hodei_catalog_storage::DgraphCatalogStore;
use hodei_evidence::ContentDigest;

#[path = "support/graph_profile.rs"]
mod graph_profile;
use graph_profile::GraphTenant;

const ACCOUNT: &str = "acme";
const OTHER: &str = "other";
const TRUSTED: &str = "tools";
const OPEN: &str = "libs";

fn descriptor(account: &str, key: &str, trusted: bool) -> RepositoryDescriptor {
    let mut config = RepositoryConfig::new(DigestScope::Global);
    if trusted {
        config.config_settings = serde_json::json!({ "trust": { "serve": "trusted" } });
    }
    RepositoryDescriptor {
        hrn: repository_hrn("hodei", "artifacts", "eu", account, key),
        account: account.to_string(),
        repo_key: key.to_string(),
        format: RepositoryFormat::Raw,
        mode: RepositoryMode::Hosted,
        config,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

/// A tenant with a store that reads the ledger and the ledger itself.
struct Fixture {
    _tenant: GraphTenant,
    store: DgraphCatalogStore,
    trust: Arc<hodei_trust_policy_storage::DgraphTrustStore>,
}

async fn fixture() -> Fixture {
    let tenant = GraphTenant::create();
    let trust = Arc::new(tenant.trust_store().await);
    let store = tenant
        .catalog_store()
        .await
        .with_trust_ledger(trust.clone());
    for (account, key, trusted) in [
        (ACCOUNT, OPEN, false),
        (ACCOUNT, TRUSTED, true),
        (OTHER, TRUSTED, true),
        (OTHER, OPEN, false),
    ] {
        store
            .create_repository(account, descriptor(account, key, trusted))
            .await
            .expect("the repository is created");
    }
    Fixture {
        _tenant: tenant,
        store,
        trust,
    }
}

/// Publishes one asset whose content is its path, and returns its digest.
async fn publish(
    store: &DgraphCatalogStore,
    account: &str,
    repository: &str,
    package: &str,
    path: &str,
) -> ContentDigest {
    let digest = ContentDigest::of_bytes(path.as_bytes());
    store
        .publish(
            account,
            Publication {
                repository: repository.to_string(),
                asset_path: path.to_string(),
                coordinates: PackageCoordinates {
                    package_name: package.to_string(),
                    package_version: "1.0".to_string(),
                },
                digest: digest.clone(),
                size_bytes: path.len() as u64,
                tags: Vec::new(),
                actor: format!("hrn:hodei:iam::{account}:User/alice"),
                correlation_id: "corr-hsp314-p5".to_string(),
                overwrite: false,
                precondition: None,
                subject: None,
            },
        )
        .await
        .expect("published");
    digest
}

fn grant_of(
    account: &str,
    repository: &str,
    digest: &ContentDigest,
    decision: &str,
    valid_for: Duration,
) -> TrustGrant {
    TrustGrant {
        account: account.to_string(),
        repository: repository.to_string(),
        digest: digest.clone(),
        decision_id: decision.to_string(),
        actor: format!("hrn:hodei:iam::{account}:User/alice"),
        granted_at: Utc::now(),
        revoked_at: None,
        revoked_by: None,
        revoke_reason: None,
        subject: None,
        policy_version: Some("1.0.0".to_string()),
        capsule_hash: None,
        risk_version: None,
        valid_until: Some(Utc::now() + valid_for),
    }
}

async fn search(fixture: &Fixture, account: &str, query: &SearchQuery) -> SearchPage {
    let repositories = fixture
        .store
        .repositories(account)
        .await
        .expect("the repositories");
    fixture
        .store
        .search(account, &repositories, query)
        .await
        .expect("the search answers")
}

fn paths(page: &SearchPage) -> Vec<&str> {
    page.results
        .iter()
        .map(|hit| hit.asset_path.as_str())
        .collect()
}

/// The trusted channel lists what has a live grant and nothing else, and
/// reads the ledger at the time of the request.
#[tokio::test]
async fn the_trusted_channel_lists_only_what_has_an_active_grant() {
    let fixture = fixture().await;
    let granted = publish(&fixture.store, ACCOUNT, TRUSTED, "granted", "granted.bin").await;
    let revoked = publish(&fixture.store, ACCOUNT, TRUSTED, "revoked", "revoked.bin").await;
    let expired = publish(&fixture.store, ACCOUNT, TRUSTED, "expired", "expired.bin").await;
    publish(&fixture.store, ACCOUNT, TRUSTED, "silent", "silent.bin").await;
    fixture
        .trust
        .grant(&[
            grant_of(ACCOUNT, TRUSTED, &granted, "dec-a", Duration::hours(1)),
            grant_of(ACCOUNT, TRUSTED, &revoked, "dec-b", Duration::hours(1)),
            grant_of(ACCOUNT, TRUSTED, &expired, "dec-c", Duration::seconds(-60)),
        ])
        .await
        .expect("the grants");
    fixture
        .trust
        .revoke_decision(ACCOUNT, TRUSTED, "dec-b", "bob", "test", Utc::now())
        .await
        .expect("the revocation");

    let page = search(
        &fixture,
        ACCOUNT,
        &SearchQuery::new().with_repository(TRUSTED),
    )
    .await;
    assert_eq!(paths(&page), vec!["granted.bin"]);
    assert_eq!(page.total, Some(1), "what is not granted is not counted");
    assert_eq!(page.results[0].trust_state, TrustState::Trusted);

    // A revocation shows up in the next request: nothing is rewritten.
    fixture
        .trust
        .revoke_digest(ACCOUNT, TRUSTED, &granted, "bob", "test", Utc::now())
        .await
        .expect("the revocation");
    let page = search(
        &fixture,
        ACCOUNT,
        &SearchQuery::new().with_repository(TRUSTED),
    )
    .await;
    assert!(page.results.is_empty());
    assert_eq!(page.total, Some(0));
}

/// The grants of another account or another repository grant nothing here.
#[tokio::test]
async fn a_grant_of_another_account_or_repository_grants_nothing() {
    let fixture = fixture().await;
    let digest = publish(&fixture.store, ACCOUNT, TRUSTED, "pkg", "pkg.bin").await;
    publish(&fixture.store, OTHER, TRUSTED, "pkg", "pkg.bin").await;
    // The same content is granted to the other account and, in this
    // account, in the OPEN repository: neither grants the trusted one.
    fixture
        .trust
        .grant(&[
            grant_of(OTHER, TRUSTED, &digest, "dec-other", Duration::hours(1)),
            grant_of(ACCOUNT, OPEN, &digest, "dec-open", Duration::hours(1)),
        ])
        .await
        .expect("the grants");

    let page = search(
        &fixture,
        ACCOUNT,
        &SearchQuery::new().with_repository(TRUSTED),
    )
    .await;
    assert!(page.results.is_empty(), "{:?}", paths(&page));
    assert_eq!(page.total, Some(0));

    // The other account sees its own grant, and only its own asset.
    let page = search(
        &fixture,
        OTHER,
        &SearchQuery::new().with_repository(TRUSTED),
    )
    .await;
    assert_eq!(paths(&page), vec!["pkg.bin"]);
    assert_eq!(page.total, Some(1));
}

/// The trust state of an open repository comes from the ledger.
#[tokio::test]
async fn the_trust_state_of_an_open_repository_comes_from_the_ledger() {
    let fixture = fixture().await;
    let promoted = publish(&fixture.store, ACCOUNT, OPEN, "a", "a.bin").await;
    publish(&fixture.store, ACCOUNT, OPEN, "b", "b.bin").await;
    fixture
        .trust
        .grant(&[grant_of(
            ACCOUNT,
            OPEN,
            &promoted,
            "dec-1",
            Duration::hours(1),
        )])
        .await
        .expect("the grant");

    let page = search(&fixture, ACCOUNT, &SearchQuery::new().with_repository(OPEN)).await;
    let states: Vec<(&str, TrustState)> = page
        .results
        .iter()
        .map(|hit| (hit.asset_path.as_str(), hit.trust_state))
        .collect();
    assert_eq!(
        states,
        vec![
            ("a.bin", TrustState::Trusted),
            ("b.bin", TrustState::Quarantined)
        ]
    );
    // The open channel still lists both: the grant is a state, not a filter.
    assert_eq!(page.total, Some(2));

    fixture
        .trust
        .revoke_decision(ACCOUNT, OPEN, "dec-1", "bob", "test", Utc::now())
        .await
        .expect("the revocation");
    let page = search(&fixture, ACCOUNT, &SearchQuery::new().with_repository(OPEN)).await;
    assert!(
        page.results
            .iter()
            .all(|hit| hit.trust_state == TrustState::Quarantined)
    );
}

/// A repository with more grants than one query names keeps the order of
/// the path, the exact total, and its pages by offset and by key.
#[tokio::test]
async fn a_trusted_repository_larger_than_one_query_keeps_order_total_and_pages() {
    let fixture = fixture().await;
    // More than the digests one query names (64), so the rows come from
    // more than one group and are merged in the adapter.
    let count = 70usize;
    let mut grants = Vec::new();
    for index in 0..count {
        let path = format!("asset-{index:03}.bin");
        let digest = publish(
            &fixture.store,
            ACCOUNT,
            TRUSTED,
            &format!("p{index}"),
            &path,
        )
        .await;
        let mut grant = grant_of(ACCOUNT, TRUSTED, &digest, "dec-bulk", Duration::hours(1));
        // The ledger lists grants by the time they were granted: the later
        // the path, the earlier the grant, so the first group of digests
        // holds the LAST paths and the groups only merge into the order of
        // the path if the adapter merges them.
        grant.granted_at = Utc::now() - Duration::seconds(index as i64);
        grants.push(grant);
    }
    publish(&fixture.store, ACCOUNT, TRUSTED, "silent", "zz-silent.bin").await;
    fixture.trust.grant(&grants).await.expect("the grants");

    let everything = SearchQuery::new().with_repository(TRUSTED).with_limit(100);
    let page = search(&fixture, ACCOUNT, &everything).await;
    let expected: Vec<String> = (0..count)
        .map(|index| format!("asset-{index:03}.bin"))
        .collect();
    assert_eq!(page.results.len(), count);
    assert_eq!(page.total, Some(count));
    assert!(!page.more);
    assert_eq!(
        paths(&page),
        expected.iter().map(String::as_str).collect::<Vec<_>>(),
        "the order is the one of the path across the groups"
    );

    // Pages by offset neither repeat nor lose a result.
    let mut walked: Vec<String> = Vec::new();
    for offset in (0..count).step_by(30) {
        let page = search(
            &fixture,
            ACCOUNT,
            &SearchQuery::new()
                .with_repository(TRUSTED)
                .with_limit(30)
                .with_offset(offset),
        )
        .await;
        walked.extend(paths(&page).into_iter().map(str::to_string));
        assert_eq!(page.more, offset + 30 < count);
    }
    assert_eq!(walked, expected);

    // Pages by key do too.
    let mut walked: Vec<String> = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut query = SearchQuery::new().with_repository(TRUSTED).with_limit(25);
        if let Some(cursor) = &cursor {
            query = query.with_cursor(cursor.clone());
        }
        let page = search(&fixture, ACCOUNT, &query).await;
        walked.extend(paths(&page).into_iter().map(str::to_string));
        match page.results.last() {
            Some(last) if page.more => {
                cursor = Some(search_cursor(&last.repository, &last.asset_path));
            }
            _ => break,
        }
    }
    assert_eq!(walked, expected);
}

/// A search by digest in the trusted channel only finds the granted digest.
#[tokio::test]
async fn a_digest_search_in_the_trusted_channel_needs_its_grant() {
    let fixture = fixture().await;
    let granted = publish(&fixture.store, ACCOUNT, TRUSTED, "granted", "granted.bin").await;
    let silent = publish(&fixture.store, ACCOUNT, TRUSTED, "silent", "silent.bin").await;
    fixture
        .trust
        .grant(&[grant_of(
            ACCOUNT,
            TRUSTED,
            &granted,
            "dec-a",
            Duration::hours(1),
        )])
        .await
        .expect("the grant");

    let page = search(
        &fixture,
        ACCOUNT,
        &SearchQuery::new().with_digest(granted.clone()),
    )
    .await;
    assert_eq!(paths(&page), vec!["granted.bin"]);
    let page = search(&fixture, ACCOUNT, &SearchQuery::new().with_digest(silent)).await;
    assert!(page.results.is_empty());
    assert_eq!(page.total, Some(0));
}

/// Without a ledger the trusted channel cannot be searched: it fails
/// closed instead of listing everything or nothing.
#[tokio::test]
async fn the_trusted_channel_without_a_ledger_fails_closed() {
    let tenant = GraphTenant::create();
    let store = tenant.catalog_store().await;
    store
        .create_repository(ACCOUNT, descriptor(ACCOUNT, TRUSTED, true))
        .await
        .expect("the repository");
    publish(&store, ACCOUNT, TRUSTED, "pkg", "pkg.bin").await;
    let repositories = store.repositories(ACCOUNT).await.expect("the repositories");

    let outcome = store
        .search(ACCOUNT, &repositories, &SearchQuery::new())
        .await;
    assert!(
        matches!(outcome, Err(CatalogError::Unavailable { .. })),
        "{outcome:?}"
    );
}

/// The text is a case-insensitive substring of the package name or of the
/// path, and what the client writes is text, never a pattern.
#[tokio::test]
async fn the_text_is_a_literal_case_insensitive_substring() {
    let fixture = fixture().await;
    publish(&fixture.store, ACCOUNT, OPEN, "LabPkg", "dist/one.whl").await;
    publish(&fixture.store, ACCOUNT, OPEN, "other", "dist/LAB-two.whl").await;
    publish(&fixture.store, ACCOUNT, OPEN, "a.b", "dist/dot.whl").await;
    publish(&fixture.store, ACCOUNT, OPEN, "axb", "dist/x.whl").await;
    publish(&fixture.store, ACCOUNT, OPEN, "paren(1)", "dist/paren.whl").await;
    publish(&fixture.store, OTHER, OPEN, "labpkg", "dist/foreign.whl").await;

    // The package name, in another case.
    let page = search(&fixture, ACCOUNT, &SearchQuery::new().with_text("labpkg")).await;
    assert_eq!(paths(&page), vec!["dist/one.whl"]);
    // The path, in another case.
    let page = search(&fixture, ACCOUNT, &SearchQuery::new().with_text("lab-t")).await;
    assert_eq!(paths(&page), vec!["dist/LAB-two.whl"]);
    // Both fields at once, and a short text.
    let page = search(&fixture, ACCOUNT, &SearchQuery::new().with_text("lab")).await;
    assert_eq!(paths(&page), vec!["dist/LAB-two.whl", "dist/one.whl"]);
    assert_eq!(page.total, Some(2), "the other account does not add up");
    // A dot is a dot and not "any character".
    let page = search(&fixture, ACCOUNT, &SearchQuery::new().with_text("a.b")).await;
    assert_eq!(paths(&page), vec!["dist/dot.whl"]);
    // Parentheses, slashes and a backslash do not break the query.
    let page = search(&fixture, ACCOUNT, &SearchQuery::new().with_text("(1)")).await;
    assert_eq!(paths(&page), vec!["dist/paren.whl"]);
    let page = search(&fixture, ACCOUNT, &SearchQuery::new().with_text("dist/dot")).await;
    assert_eq!(paths(&page), vec!["dist/dot.whl"]);
    let page = search(&fixture, ACCOUNT, &SearchQuery::new().with_text("a\\b")).await;
    assert!(page.results.is_empty());
    // A text with a control character is not a text.
    let repositories = fixture.store.repositories(ACCOUNT).await.expect("repos");
    let refused = fixture
        .store
        .search(
            ACCOUNT,
            &repositories,
            &SearchQuery::new().with_text("a\nb"),
        )
        .await;
    assert!(matches!(refused, Err(CatalogError::InvalidRequest { .. })));
}

/// The text narrows the trusted channel too: it never shows what has no
/// grant, whatever it matches.
#[tokio::test]
async fn the_text_does_not_widen_the_trusted_channel() {
    let fixture = fixture().await;
    let granted = publish(
        &fixture.store,
        ACCOUNT,
        TRUSTED,
        "needle-ok",
        "needle-ok.bin",
    )
    .await;
    publish(
        &fixture.store,
        ACCOUNT,
        TRUSTED,
        "needle-silent",
        "needle-silent.bin",
    )
    .await;
    fixture
        .trust
        .grant(&[grant_of(
            ACCOUNT,
            TRUSTED,
            &granted,
            "dec-a",
            Duration::hours(1),
        )])
        .await
        .expect("the grant");

    let page = search(&fixture, ACCOUNT, &SearchQuery::new().with_text("NEEDLE")).await;
    assert_eq!(paths(&page), vec!["needle-ok.bin"]);
    assert_eq!(page.total, Some(1));
}

/// The ledger answers a page in one read: the active subset of the digests
/// that are asked, in the repository of the account that asks.
#[tokio::test]
async fn the_ledger_answers_the_active_subset_of_a_page() {
    let fixture = fixture().await;
    let live = ContentDigest::of_bytes(b"live");
    let revoked = ContentDigest::of_bytes(b"revoked");
    let expired = ContentDigest::of_bytes(b"expired");
    let never = ContentDigest::of_bytes(b"never");
    fixture
        .trust
        .grant(&[
            grant_of(ACCOUNT, OPEN, &live, "dec-live", Duration::hours(1)),
            grant_of(ACCOUNT, OPEN, &revoked, "dec-revoked", Duration::hours(1)),
            grant_of(
                ACCOUNT,
                OPEN,
                &expired,
                "dec-expired",
                Duration::seconds(-60),
            ),
            grant_of(OTHER, OPEN, &never, "dec-other", Duration::hours(1)),
        ])
        .await
        .expect("the grants");
    fixture
        .trust
        .revoke_decision(ACCOUNT, OPEN, "dec-revoked", "bob", "test", Utc::now())
        .await
        .expect("the revocation");

    let asked = [live.clone(), revoked, expired, never.clone()];
    let active = fixture
        .trust
        .active_digests(ACCOUNT, OPEN, &asked, Utc::now())
        .await
        .expect("the answer");
    assert_eq!(active, vec![live.clone()]);

    // Another repository and another account have no grant for it.
    let active = fixture
        .trust
        .active_digests(ACCOUNT, TRUSTED, &[live.clone()], Utc::now())
        .await
        .expect("the answer");
    assert!(active.is_empty());
    let active = fixture
        .trust
        .active_digests(OTHER, OPEN, &[live, never.clone()], Utc::now())
        .await
        .expect("the answer");
    assert_eq!(active, vec![never]);
}
