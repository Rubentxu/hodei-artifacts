//! The cursor of the keyset contract (HSP-191) on the HTTP surface (HSP-222).
//!
//! The storage already answers pages by key; what is measured here is
//! the **surface**: that `GET /api/v1/search` accepts the opaque cursor
//! and propagates it to the store, that it echoes it and hands out the
//! key of the next page, that a cursor that cannot be read is refused by
//! name instead of silently sliding to offset zero, that a cursor naming
//! a repository the principal may not read never reaches the query — and
//! that without the cursor the answer is the one the surface has always
//! given.
//!
//! The store is a fake on purpose: it serves the keyset exactly as the
//! contract promises, and one variant of it ignores the cursor the way
//! an engine that does not serve it does. The real engines' conformance
//! to the contract is HSP-147 and HSP-191; the surface is what the fake
//! pins, because a page built while ignoring the cursor is **indistinguishable**
//! from a walked one by content alone — only the surface knows that the
//! window it echoes must start after the key it was asked with.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use hodei_artifacts_api::authorization::AuthorizerPort;
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::{
    AccountId, AuthenticationError, AuthorizationOutcome, Credentials, PrincipalAuthenticator,
    PrincipalClaims, PrincipalId,
};
use hodei_catalog::search::{SearchHit, SearchPage, SearchQuery, parse_search_cursor};
use hodei_catalog::{
    CatalogError, CatalogSearch, DigestScope, RepositoryConfig, RepositoryDescriptor,
    RepositoryFormat, RepositoryMode, TrustState, repository_hrn,
};
use hodei_evidence::ContentDigest;
use serde_json::json;
use tower::ServiceExt;

/// The account of the principal that searches.
const ACCOUNT: &str = "acme";
/// Another account, whose keys must never be paged by this one.
const OTHER: &str = "other";

/// An asset of the fake catalog.
struct Seeded {
    account: &'static str,
    repository: &'static str,
    asset_path: &'static str,
}

/// The whole catalog of the fake, in the total order of the contract
/// (`repository`, `asset_path`). Two repositories of the searching
/// account, and one of the other: its keys are named only by its own
/// account.
const ASSETS: &[Seeded] = &[
    Seeded {
        account: ACCOUNT,
        repository: "apps",
        asset_path: "a/00",
    },
    Seeded {
        account: ACCOUNT,
        repository: "apps",
        asset_path: "a/01",
    },
    Seeded {
        account: ACCOUNT,
        repository: "libs",
        asset_path: "q/00",
    },
    Seeded {
        account: ACCOUNT,
        repository: "libs",
        asset_path: "q/01",
    },
    Seeded {
        account: ACCOUNT,
        repository: "libs",
        asset_path: "q/02",
    },
    Seeded {
        account: ACCOUNT,
        repository: "libs",
        asset_path: "q/03",
    },
    Seeded {
        account: ACCOUNT,
        repository: "libs",
        asset_path: "q/04",
    },
    Seeded {
        account: ACCOUNT,
        repository: "libs",
        asset_path: "q/05",
    },
    Seeded {
        account: ACCOUNT,
        repository: "libs",
        asset_path: "q/06",
    },
    Seeded {
        account: OTHER,
        repository: "secrets",
        asset_path: "x/00",
    },
    Seeded {
        account: OTHER,
        repository: "secrets",
        asset_path: "x/01",
    },
];

/// A repository of the fake catalog, on the open channel.
fn descriptor(account: &str, key: &str) -> RepositoryDescriptor {
    RepositoryDescriptor {
        hrn: repository_hrn("hodei", "artifacts", "eu", account, key),
        account: account.to_string(),
        repo_key: key.to_string(),
        format: RepositoryFormat::Raw,
        mode: RepositoryMode::Hosted,
        config: RepositoryConfig::new(DigestScope::Global),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

/// The search of the fake, serving the keyset exactly as the contract
/// of HSP-191 promises: a keyed page starts **after** the key of the
/// cursor, in the order the pages are walked.
///
/// The `serves_cursor` variant is an engine that does not serve the
/// cursor — the way Surreal answers today: a cursor on the query is
/// ignored and the page comes from the offset window. The surface has to
/// tell the difference: a window that slid back to an offset is not a
/// keyset page, and it is refused by name, not echoed with a
/// `next_cursor` the client would walk with a lie.
struct FakeSearcher {
    /// Whether this engine serves the cursor.
    serves_cursor: bool,
    /// How many times the search was asked: the account that does not
    /// reach the query leaves no trace here.
    searches: AtomicUsize,
}

impl FakeSearcher {
    fn new(serves_cursor: bool) -> Self {
        Self {
            serves_cursor,
            searches: AtomicUsize::new(0),
        }
    }
}

fn hit(asset: &Seeded) -> SearchHit {
    SearchHit {
        repository: asset.repository.to_string(),
        format: RepositoryFormat::Raw,
        asset_path: asset.asset_path.to_string(),
        package_name: "labpkg".to_string(),
        package_version: "1.0".to_string(),
        digest: ContentDigest::sha256("bb".repeat(32)),
        size_bytes: 10,
        trust_state: TrustState::Quarantined,
        updated_at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH,
    }
}

#[async_trait]
impl CatalogSearch for FakeSearcher {
    async fn repositories(
        &self,
        principal_account: &str,
    ) -> Result<Vec<RepositoryDescriptor>, CatalogError> {
        let mut keys: Vec<&str> = ASSETS
            .iter()
            .filter(|asset| asset.account == principal_account)
            .map(|asset| asset.repository)
            .collect();
        keys.sort_unstable();
        keys.dedup();
        Ok(keys
            .into_iter()
            .map(|key| descriptor(principal_account, key))
            .collect())
    }

    async fn search(
        &self,
        principal_account: &str,
        repositories: &[RepositoryDescriptor],
        query: &SearchQuery,
    ) -> Result<SearchPage, CatalogError> {
        self.searches.fetch_add(1, Ordering::SeqCst);
        query
            .validate()
            .map_err(|error| CatalogError::invalid(error.to_string()))?;

        // The account and the permission come first: a row of another
        // account does not exist here, not even as a counted one.
        let permitted: Vec<&str> = repositories.iter().map(|r| r.repo_key.as_str()).collect();
        let mut rows: Vec<SearchHit> = ASSETS
            .iter()
            .filter(|asset| {
                asset.account == principal_account && permitted.contains(&asset.repository)
            })
            .map(hit)
            .collect();
        rows.sort_by(|left, right| {
            (&left.repository, &left.asset_path).cmp(&(&right.repository, &right.asset_path))
        });
        let total = rows.len();

        if let Some(cursor) = query.cursor.as_deref().filter(|_| self.serves_cursor) {
            let (repository, asset_path) = parse_search_cursor(cursor)
                .map_err(|error| CatalogError::invalid(error.to_string()))?;
            // The keyed window: everything **after** the seen key.
            rows.retain(|row| {
                (row.repository.as_str(), row.asset_path.as_str())
                    > (repository.as_str(), asset_path.as_str())
            });
        } else {
            // The offset window, and the cursor-less keyset-less engine:
            // an engine that does not serve the cursor answers from the
            // offset as if the cursor had not been named.
            rows.drain(..query.offset.min(rows.len()));
        }

        let more = rows.len() > query.limit;
        rows.truncate(query.limit);
        Ok(SearchPage {
            total: Some(total),
            more,
            limit: query.limit,
            offset: query.offset,
            results: rows,
        })
    }
}

/// Everything is readable by the principal: Cedar is exercised by
/// `hsp113_search_e2e`, and here what matters is that the key of a
/// repository **of another account** — never listed for this one, never
/// permitted — is what the surface refuses.
struct AllowAll;

impl AuthorizerPort for AllowAll {
    fn authorize(&self, _request: &hodei_authz::AuthorizationRequest) -> AuthorizationOutcome {
        AuthorizationOutcome::Permit
    }
}

struct TestAuthenticator;

#[async_trait]
impl PrincipalAuthenticator for TestAuthenticator {
    async fn authenticate(
        &self,
        credentials: &Credentials,
    ) -> Result<PrincipalClaims, AuthenticationError> {
        let token = match credentials {
            Credentials::Bearer(token) => token.as_str(),
            _ => return Err(AuthenticationError::InvalidCredentials),
        };
        if token != "alice" {
            return Err(AuthenticationError::InvalidCredentials);
        }
        Ok(PrincipalClaims::new(
            PrincipalId::new(format!("hrn:hodei:iam::{ACCOUNT}:User/alice")).unwrap(),
            AccountId::new(ACCOUNT).unwrap(),
        ))
    }
}

/// The application over the fake store.
async fn app(serves_cursor: bool) -> (axum::Router, Arc<FakeSearcher>, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let mut config = AppConfig::default();
    config.security.admin_api_enabled = false;
    config.rocksdb.path = tmp
        .path()
        .join("schema.rocksdb")
        .to_string_lossy()
        .into_owned();

    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    .expect("bootstrap");
    let searcher = Arc::new(FakeSearcher::new(serves_cursor));
    let state = state
        .with_authorization(Arc::new(AllowAll))
        .with_repository_auth(
            Arc::new(TestAuthenticator),
            Arc::new(hodei_artifacts_api::repository::InMemoryRepositoryReader::empty()),
        )
        .with_catalog_search(searcher.clone());

    (build_router(state, &config), searcher, tmp)
}

/// A search request.
fn search_request(query: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(format!("/api/v1/search{query}"))
        .header("authorization", "Bearer alice")
        .body(Body::empty())
        .expect("a request")
}

/// The body of the response, as JSON.
async fn the_body(response: axum::response::Response) -> serde_json::Value {
    serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .expect("the body"),
    )
    .expect("the body is JSON")
}

/// The paths of a page, in the order it came.
fn paths(body: &serde_json::Value) -> Vec<String> {
    body["results"]
        .as_array()
        .expect("the results")
        .iter()
        .map(|result| {
            format!(
                "{}|{}",
                result["repository"].as_str().expect("a repository"),
                result["asset_path"].as_str().expect("a path")
            )
        })
        .collect()
}

/// A page asked by cursor is the keyset window the cursor names: it
/// starts after the seen key, it echoes the cursor it was asked with,
/// and it hands out the key of its last result as the next one.
///
/// This cannot pass if the surface ignores the cursor: an ignored cursor
/// answers with the first page again, whose first key is not after the
/// one that was seen.
#[tokio::test]
async fn a_keyed_page_walks_after_the_cursor_and_hands_out_the_next_key() {
    let (app, searcher, _tmp) = app(true).await;

    // The first page, asked without a cursor, already names the key the
    // next one starts after.
    let response = app
        .clone()
        .oneshot(search_request("?limit=3"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;
    assert_eq!(body["total"], json!(9));
    assert_eq!(paths(&body), ["apps|a/00", "apps|a/01", "libs|q/00"]);
    assert_eq!(body["more"], json!(true));
    assert!(
        body.get("cursor").is_none(),
        "a page without a cursor echoes none"
    );
    let first_cursor = body["next_cursor"].as_str().expect("the next cursor");
    assert_eq!(first_cursor, "libs|q/00");

    // The second page, asked with that cursor.
    let response = app
        .clone()
        .oneshot(search_request(&format!(
            "?limit=3&cursor={}",
            first_cursor.replace('|', "%7C")
        )))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;
    assert_eq!(body["cursor"], json!(first_cursor), "the cursor is echoed");
    assert_eq!(paths(&body), ["libs|q/01", "libs|q/02", "libs|q/03"]);
    assert_eq!(
        body["offset"],
        json!(0),
        "a keyed page leaves nothing behind"
    );
    assert_eq!(body["next_cursor"], json!("libs|q/03"));

    // The last page names no next key: there is nothing after.
    let response = app
        .clone()
        .oneshot(search_request("?limit=10&cursor=libs%7Cq/03"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;
    assert_eq!(paths(&body), ["libs|q/04", "libs|q/05", "libs|q/06"]);
    assert_eq!(body["more"], json!(false));
    assert!(
        body.get("next_cursor").is_none(),
        "the last page hands out no key"
    );

    // A cursor at the very end of the catalog is a real window with
    // nothing left in it: an empty page, not a refusal and not a fall
    // back to the first page.
    let response = app
        .oneshot(search_request("?limit=3&cursor=libs%7Cq/06"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;
    assert!(
        paths(&body).is_empty(),
        "the keyed walk ends at the last key"
    );
    assert_eq!(body["more"], json!(false));
    assert_eq!(
        body["total"],
        json!(9),
        "the total does not move with the key"
    );
    assert_eq!(searcher.searches.load(Ordering::SeqCst), 4);
}

/// The total of a keyed page counts what the principal may read and not
/// one row more: the same invariance of HSP-147 held by the plain page
/// also holds when the page is asked by key.
#[tokio::test]
async fn a_keyed_page_counts_only_what_the_principal_may_read() {
    let (app, _searcher, _tmp) = app(true).await;

    let response = app
        .oneshot(search_request("?limit=2&cursor=apps|a%2F01"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;
    // The two of "secrets" belong to the other account and are neither
    // walked nor counted, keyed or not.
    assert_eq!(body["total"], json!(9));
    assert_eq!(paths(&body), ["libs|q/00", "libs|q/01"]);
}

/// A cursor that names a repository the principal may not read is
/// refused by name, and the other account never reaches the query: what
/// is not named is not leaked by the shape of the answer either.
#[tokio::test]
async fn a_cursor_of_another_account_is_refused_by_name_and_never_reaches_the_query() {
    let (app, searcher, _tmp) = app(true).await;

    let response = app
        .oneshot(search_request("?cursor=secrets|x%2F00"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = the_body(response).await;
    assert_eq!(body["error"], json!("invalid"));
    let message = body["message"].as_str().expect("a message");
    assert!(
        message.contains("secrets|x/00"),
        "the refused cursor is named: {message}"
    );
    // The refusal does not enumerate what the key could unlock.
    assert!(
        !message.contains("x/01"),
        "the next key of the other account is not named"
    );
    assert_eq!(
        searcher.searches.load(Ordering::SeqCst),
        0,
        "the other account never reaches the query"
    );
}

/// A cursor that cannot be read is refused by name: it does not answer
/// from the first page. Falling back to offset zero would silently serve
/// results the client already saw — a page that says it walked from a
/// key it did not read is worse than a refusal.
#[tokio::test]
async fn a_cursor_that_cannot_be_read_is_refused_not_answered_from_the_start() {
    let (app, _searcher, _tmp) = app(true).await;

    // "%7C" decodes to the separator: `x|` reads as a cursor with an
    // empty key and is refused as one, not answered from the start.
    for bad in ["notacursor", "a|b|c", "|b", "a|", "x%7C"] {
        let response = app
            .clone()
            .oneshot(search_request(&format!("?cursor={bad}")))
            .await
            .expect("the request");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{bad:?}");
        let body = the_body(response).await;
        assert!(
            body["message"]
                .as_str()
                .expect("a message")
                .contains("cursor"),
            "the failure names the cursor for {bad:?}"
        );
    }
}

/// A cursor and an offset on the same query say different things — one
/// starts after a key, the other counts results left behind — and the
/// query that says both is refused, not guessed at.
#[tokio::test]
async fn a_cursor_and_an_offset_do_not_go_together() {
    let (app, searcher, _tmp) = app(true).await;

    let response = app
        .oneshot(search_request("?cursor=libs|q%2F00&offset=3"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = the_body(response).await;
    let message = body["message"].as_str().expect("a message");
    assert!(
        message.contains("cursor") && message.contains("offset"),
        "{message}"
    );
    assert_eq!(searcher.searches.load(Ordering::SeqCst), 0);
}

/// An engine that does not serve the cursor does not get to answer a
/// keyed page from the offset window: the page is refused by name. A
/// surface that echoed this window as a keyset page — with its
/// `next_cursor` — would make the client re-walk the catalog in circles
/// without ever knowing, which is the silent deep-offset degradation the
/// contract refuses.
#[tokio::test]
async fn an_engine_that_ignores_the_cursor_is_not_a_keyset_page() {
    let (app, searcher, _tmp) = app(false).await;

    // Without a cursor the engine works: the surface behaves as it did.
    let response = app
        .clone()
        .oneshot(search_request("?limit=2&offset=2"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;
    assert_eq!(paths(&body), ["libs|q/00", "libs|q/01"]);

    // With one, the window that ignores it is caught, not served.
    let response = app
        .oneshot(search_request("?limit=2&cursor=libs|q%2F03"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = the_body(response).await;
    assert!(
        body["message"]
            .as_str()
            .expect("a message")
            .contains("cursor"),
        "the refusal names the cursor"
    );
    // The engine was asked — the surface cannot know the engine lied
    // before it answers — and its answer was refused by name.
    assert_eq!(searcher.searches.load(Ordering::SeqCst), 2);
}

/// The offset window keeps working as it always did, and the answer
/// keeps saying how many results were left behind (HSP-298, backwards
/// compatibility). A page without a cursor echoes none.
#[tokio::test]
async fn the_offset_window_still_works_and_the_response_keeps_the_offset() {
    let (app, _searcher, _tmp) = app(true).await;

    let response = app
        .clone()
        .oneshot(search_request("?limit=2&offset=3"))
        .await
        .expect("the request");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;
    assert_eq!(paths(&body), ["libs|q/01", "libs|q/02"]);
    assert_eq!(body["offset"], json!(3), "the offset is kept in the answer");
    assert_eq!(body["limit"], json!(2));
    assert_eq!(body["more"], json!(true));
    assert!(
        body.get("cursor").is_none(),
        "an offset page echoes no cursor"
    );

    // The last window of the offset walk is complete and closed.
    let response = app
        .oneshot(search_request("?limit=5&offset=6"))
        .await
        .expect("the request");
    let body = the_body(response).await;
    assert_eq!(
        paths(&body),
        ["libs|q/04", "libs|q/05", "libs|q/06"].map(String::from)
    );
    assert_eq!(body["offset"], json!(6));
    assert_eq!(body["more"], json!(false));
}

/// Walking every page through `next_cursor` visits each result exactly
/// once: consecutive pages neither overlap nor skip, whatever the page
/// size. This fails if the surface ignores the cursor, because the same
/// first page would come back forever (or the walk would never end).
#[tokio::test]
async fn walking_every_page_by_cursor_is_the_exact_union_without_overlap() {
    let expected: Vec<String> = [
        "apps|a/00",
        "apps|a/01",
        "libs|q/00",
        "libs|q/01",
        "libs|q/02",
        "libs|q/03",
        "libs|q/04",
        "libs|q/05",
        "libs|q/06",
    ]
    .map(String::from)
    .to_vec();

    for limit in [1usize, 2, 4, 9, 20] {
        let (app, _searcher, _tmp) = app(true).await;
        let mut seen: Vec<String> = Vec::new();
        let mut cursor: Option<String> = None;
        let mut pages = 0usize;
        loop {
            pages += 1;
            assert!(
                pages <= expected.len() + 1,
                "the walk does not end (limit {limit})"
            );
            let query = match &cursor {
                Some(cursor) => format!("?limit={limit}&cursor={}", cursor.replace('|', "%7C")),
                None => format!("?limit={limit}"),
            };
            let response = app
                .clone()
                .oneshot(search_request(&query))
                .await
                .expect("the request");
            assert_eq!(response.status(), StatusCode::OK);
            let body = the_body(response).await;
            let page = paths(&body);
            assert!(!page.is_empty(), "no empty page in the middle of the walk");
            assert!(page.len() <= limit);
            // No overlap with what was already seen, and strictly forward.
            for path in &page {
                assert!(!seen.contains(path), "{path} came twice (limit {limit})");
            }
            seen.extend(page);
            match body.get("next_cursor").and_then(|value| value.as_str()) {
                Some(next) => {
                    assert_eq!(body["more"], json!(true));
                    cursor = Some(next.to_string());
                }
                None => {
                    assert_eq!(body["more"], json!(false));
                    break;
                }
            }
        }
        assert_eq!(seen, expected, "union exact and in order (limit {limit})");
    }
}
