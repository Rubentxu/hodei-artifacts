//! HSP-314 P2 integration tests: the Cedar schema storage over the REAL
//! remote Dgraph authority.
//!
//! Environment-gated like `hsp186_dgraph_iam_it.rs`: without
//! `HODEI_GRAPH_IT_ENDPOINT` the suite skips itself (exit 0). With the
//! variables set it proves, against the server the port actually talks to:
//!
//! - a saved schema reads back (latest, by version, listed) with the
//!   version it was stored under, and a schema saved without a version
//!   reads back as `latest`;
//! - saving the same version again replaces the content (idempotent), and
//!   the newest stored schema is the latest one;
//! - `delete_schema` accepts the id `save_schema` answered and the bare
//!   version, and reports whether something was deleted;
//! - two tenants storing the same version never see each other's schema;
//! - everything survives into a brand-new connection;
//! - a server that stops answering is an error, never "no schema".
//!
//! The server persists between runs, so every fixture uses run-unique
//! tenants and no test drops data it did not create.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use hodei_graph_client::{GraphClient, GraphClientConfig, ensure_ready};
use hodei_iam::infrastructure::DgraphSchemaStore;
use hodei_policies::build_schema::ports::SchemaStoragePort;
use tokio::net::{TcpListener, TcpStream};

fn config_for(endpoint: &str) -> GraphClientConfig {
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
    GraphClientConfig::from_toml_str(&text).expect("the environment configuration must validate")
}

fn endpoint_from_env() -> Option<String> {
    std::env::var("HODEI_GRAPH_IT_ENDPOINT").ok()
}

/// A connected, logged-in client that passed the schema boot gate (which
/// declares the `hodei.cedar.*` predicates), or `None` to skip.
async fn client_for(endpoint: &str) -> Arc<GraphClient> {
    let mut client = GraphClient::connect(config_for(endpoint))
        .await
        .expect("connect to the real server");
    client.login().await.expect("the ACL login must succeed");
    ensure_ready(&client)
        .await
        .expect("the schema boot must accept the server");
    Arc::new(client)
}

async fn store_or_skip(tenant: &str) -> Option<DgraphSchemaStore> {
    let Some(endpoint) = endpoint_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set; the real-server run is gated");
        return None;
    };
    Some(DgraphSchemaStore::new(client_for(&endpoint).await, tenant))
}

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn nonce() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("the clock must be set")
        .as_nanos();
    format!("h314p2-{nanos}")
}

#[tokio::test]
async fn a_saved_schema_reads_back_with_its_version() {
    let _serial = SERIAL.lock().await;
    let tenant = nonce();
    let Some(store) = store_or_skip(&tenant).await else {
        return;
    };
    assert!(
        store.get_latest_schema().await.expect("answer").is_none(),
        "a tenant without a schema has none"
    );

    let id = store
        .save_schema("{\"v\":1}".to_string(), Some("v1".to_string()))
        .await
        .expect("the schema persists");
    assert_eq!(id, "schema:v1");

    assert_eq!(
        store.get_schema_by_version("v1").await.expect("answer"),
        Some("{\"v\":1}".to_string())
    );
    assert_eq!(
        store.get_schema_by_version("v2").await.expect("answer"),
        None
    );
    let latest = store
        .get_latest_schema_entry()
        .await
        .expect("answer")
        .expect("the schema is the latest");
    assert_eq!(latest.schema_string, "{\"v\":1}");
    assert_eq!(latest.version.as_deref(), Some("v1"));
    assert_eq!(latest.schema_id, "schema:v1");
    assert_eq!(
        store.list_schema_versions().await.expect("answer"),
        vec!["v1".to_string()]
    );

    // A schema saved without a version is stored under `latest` and says so.
    let unversioned = store
        .save_schema("{\"u\":1}".to_string(), None)
        .await
        .expect("the unversioned schema persists");
    assert_eq!(unversioned, "schema:latest");
    assert_eq!(
        store.get_schema_by_version("latest").await.expect("answer"),
        Some("{\"u\":1}".to_string())
    );
}

#[tokio::test]
async fn saving_a_version_again_replaces_it_and_the_newest_is_the_latest() {
    let _serial = SERIAL.lock().await;
    let tenant = nonce();
    let Some(store) = store_or_skip(&tenant).await else {
        return;
    };
    store
        .save_schema("first".to_string(), Some("v1".to_string()))
        .await
        .expect("save v1");
    store
        .save_schema("second".to_string(), Some("v2".to_string()))
        .await
        .expect("save v2");
    assert_eq!(
        store
            .get_latest_schema()
            .await
            .expect("answer")
            .as_deref(),
        Some("second"),
        "the most recently stored schema is the latest"
    );

    // Idempotent replacement: the same version again overwrites, it neither
    // fails nor duplicates, and it becomes the newest.
    store
        .save_schema("first-replaced".to_string(), Some("v1".to_string()))
        .await
        .expect("saving the same version again is idempotent");
    assert_eq!(
        store.get_schema_by_version("v1").await.expect("answer"),
        Some("first-replaced".to_string())
    );
    let latest = store
        .get_latest_schema_entry()
        .await
        .expect("answer")
        .expect("a latest exists");
    assert_eq!(latest.version.as_deref(), Some("v1"));
    assert_eq!(
        store.list_schema_versions().await.expect("answer"),
        vec!["v1".to_string(), "v2".to_string()],
        "the replaced version is listed once"
    );
}

#[tokio::test]
async fn delete_accepts_both_spellings_and_reports_what_it_deleted() {
    let _serial = SERIAL.lock().await;
    let tenant = nonce();
    let Some(store) = store_or_skip(&tenant).await else {
        return;
    };
    let id = store
        .save_schema("a".to_string(), Some("va".to_string()))
        .await
        .expect("save");
    store
        .save_schema("b".to_string(), Some("vb".to_string()))
        .await
        .expect("save");
    assert!(
        store.delete_schema(&id).await.expect("delete by returned id"),
        "the returned id deletes the record"
    );
    assert!(
        !store.delete_schema(&id).await.expect("second delete"),
        "deleting what is gone reports false"
    );
    assert_eq!(store.get_schema_by_version("va").await.expect("answer"), None);
    assert!(
        store.delete_schema("vb").await.expect("delete by bare version"),
        "the bare version deletes the record too"
    );
    assert!(store.get_latest_schema().await.expect("answer").is_none());
}

#[tokio::test]
async fn two_tenants_never_see_each_others_schema() {
    let _serial = SERIAL.lock().await;
    let run = nonce();
    let (tenant_a, tenant_b) = (format!("{run}-a"), format!("{run}-b"));
    let Some(a) = store_or_skip(&tenant_a).await else {
        return;
    };
    let b = store_or_skip(&tenant_b).await.expect("same server");

    a.save_schema("schema-of-a".to_string(), Some("v1".to_string()))
        .await
        .expect("a saves");
    assert_eq!(b.get_schema_by_version("v1").await.expect("answer"), None);
    assert!(b.get_latest_schema().await.expect("answer").is_none());
    assert!(b.list_schema_versions().await.expect("answer").is_empty());

    // The SAME version in the other tenant is another node, not a replace.
    b.save_schema("schema-of-b".to_string(), Some("v1".to_string()))
        .await
        .expect("b saves the same version");
    assert_eq!(
        a.get_schema_by_version("v1").await.expect("answer"),
        Some("schema-of-a".to_string())
    );
    assert_eq!(
        b.get_schema_by_version("v1").await.expect("answer"),
        Some("schema-of-b".to_string())
    );
    // Deleting in one tenant leaves the other's untouched.
    assert!(b.delete_schema("v1").await.expect("b deletes"));
    assert_eq!(
        a.get_schema_by_version("v1").await.expect("answer"),
        Some("schema-of-a".to_string())
    );
}

#[tokio::test]
async fn the_schema_survives_a_brand_new_connection() {
    let _serial = SERIAL.lock().await;
    let tenant = nonce();
    let Some(store) = store_or_skip(&tenant).await else {
        return;
    };
    let content = "{\"line\":\"with \\\"quotes\\\"\\nand a newline\"}";
    store
        .save_schema(content.to_string(), Some("durable".to_string()))
        .await
        .expect("save");
    drop(store);

    let reopened = store_or_skip(&tenant).await.expect("same server");
    let latest = reopened
        .get_latest_schema_entry()
        .await
        .expect("answer")
        .expect("the schema survived");
    assert_eq!(latest.schema_string, content);
    assert_eq!(latest.version.as_deref(), Some("durable"));
}

/// A TCP forwarder in front of the alpha, so a test can make a CONNECTED
/// client lose its server without touching the shared container.
struct Forwarder {
    address: SocketAddr,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Forwarder {
    async fn start(target: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("address");
        let accept = tokio::spawn(async move {
            // A JoinSet aborts every forwarded connection when it is dropped,
            // which is what aborting the accept task does.
            let mut connections = tokio::task::JoinSet::new();
            while let Ok((mut inbound, _)) = listener.accept().await {
                let target = target.clone();
                connections.spawn(async move {
                    if let Ok(mut outbound) = TcpStream::connect(target).await {
                        let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                    }
                });
            }
        });
        Self {
            address,
            tasks: vec![accept],
        }
    }
}

#[tokio::test]
async fn a_server_that_stops_answering_is_an_error_not_an_absent_schema() {
    let _serial = SERIAL.lock().await;
    let Some(real) = endpoint_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set; the real-server run is gated");
        return;
    };
    let tenant = nonce();
    let forwarder = Forwarder::start(real.clone()).await;
    let client = client_for(&forwarder.address.to_string()).await;
    let store = DgraphSchemaStore::new(client, tenant.clone());
    store
        .save_schema("held".to_string(), Some("v1".to_string()))
        .await
        .expect("the schema persists while the server answers");

    // The server goes away: every connection through the forwarder dies.
    for task in &forwarder.tasks {
        task.abort();
    }
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    assert!(
        store.get_latest_schema().await.is_err(),
        "an unreachable authority must not read as 'no schema'"
    );
    assert!(store.get_schema_by_version("v1").await.is_err());
    assert!(store.list_schema_versions().await.is_err());
    assert!(
        store
            .save_schema("lost".to_string(), Some("v2".to_string()))
            .await
            .is_err(),
        "a write against a dead authority must not claim success"
    );
    assert!(store.delete_schema("v1").await.is_err());

    // The data written before the outage is intact on the real server.
    let direct = store_or_skip(&tenant).await.expect("same server");
    assert_eq!(
        direct.get_schema_by_version("v1").await.expect("answer"),
        Some("held".to_string())
    );
}
