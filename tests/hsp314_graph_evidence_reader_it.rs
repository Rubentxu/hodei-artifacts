//! HSP-314 F3b-1: the evidence reader of the graph profile, on a real Dgraph.
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT like every Dgraph IT. The evidence is
//! seeded through the `EvidenceStore` port of the SAME alpha and read back
//! through the service's `EvidenceReader`.
//!
//! - Seeded evidence of a subject is read back, whole.
//! - A subject nobody spoke about answers an EMPTY list (the contract of the
//!   port: "no evidence" is an answer, not a failure).
//! - Another account never sees the evidence (the account is part of the
//!   question), even when it names the same subject.
//! - A store that fails is an error, never an empty list. The outage is a
//!   local TCP relay between the client and the alpha that the test cuts:
//!   no shared container is stopped.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use hodei_artifacts_api::evidence::{EvidenceReader, EvidenceStoreError};
use hodei_artifacts_api::evidence_dgraph::DgraphEvidenceReader;
use hodei_evidence::{EvidenceId, EvidenceRecord, EvidenceStore, EvidenceType, SourceRef};
use hodei_evidence_storage::DgraphEvidenceStore;
use hodei_graph_client::{GraphClient, GraphClientConfig};
use kernel::Hrn;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;

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
    GraphClientConfig::from_toml_str(&text).expect("the IT configuration validates")
}

async fn connect_to(endpoint: &str) -> Arc<GraphClient> {
    let mut client = GraphClient::connect(config_for(endpoint))
        .await
        .expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    Arc::new(client)
}

fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

fn subject_of(account: &str, name: &str) -> Hrn {
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{account}:Artifact/{name}"
    ))
    .expect("an HRN")
}

fn evidence(id: &str, account: &str, subjects: Vec<Hrn>) -> EvidenceRecord {
    let tenant = Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{account}:Tenant/main"
    ))
    .expect("an HRN");
    EvidenceRecord::builder(EvidenceId::new(id), tenant)
        .subjects(subjects)
        .evidence_type(EvidenceType::Sbom)
        .source(SourceRef {
            external_id: Some(format!("ext-{id}")),
            uri: None,
        })
        .build()
}

/// A relay to the alpha that the test can cut: cutting it drops every open
/// connection and stops listening, so the client's next call fails for real.
struct Relay {
    endpoint: String,
    tasks: JoinSet<()>,
}

impl Relay {
    async fn to(target: String) -> Relay {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("relay binds");
        let endpoint = listener.local_addr().expect("relay address").to_string();
        let mut tasks = JoinSet::new();
        tasks.spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                let Ok((mut inbound, _)) = listener.accept().await else {
                    return;
                };
                let target = target.clone();
                connections.spawn(async move {
                    if let Ok(mut outbound) = TcpStream::connect(target).await {
                        let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                    }
                });
            }
        });
        Relay { endpoint, tasks }
    }

    fn cut(&mut self) {
        self.tasks.abort_all();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_graph_reader_returns_the_evidence_of_the_account_and_nothing_else() {
    let Ok(endpoint) = std::env::var("HODEI_GRAPH_IT_ENDPOINT") else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    };
    let client = connect_to(&endpoint).await;
    let store = Arc::new(
        DgraphEvidenceStore::open(client)
            .await
            .expect("the evidence store opens"),
    );
    let reader = DgraphEvidenceReader::new(store.clone());

    let tag = nonce();
    let acme = format!("acme{tag}");
    let other = format!("other{tag}");
    let subject = subject_of(&acme, "app");
    let silent = subject_of(&acme, "never-mentioned");

    let first = evidence(&format!("ev-1-{tag}"), &acme, vec![subject.clone()]);
    let second = evidence(&format!("ev-2-{tag}"), &acme, vec![subject.clone()]);
    let elsewhere = evidence(
        &format!("ev-3-{tag}"),
        &acme,
        vec![subject_of(&acme, "lib")],
    );
    for record in [&first, &second, &elsewhere] {
        store.put(record.clone()).await.expect("evidence is stored");
    }

    // Seeded evidence is read back, and only the one that names the subject.
    let read = reader
        .evidence_of_subject(&acme, &subject)
        .await
        .expect("the reader answers");
    assert_eq!(
        read.len(),
        2,
        "two pieces speak about the subject: {read:?}"
    );
    assert!(read.contains(&first) && read.contains(&second));
    assert!(!read.contains(&elsewhere));

    // Absent evidence is an empty answer, not an error.
    assert_eq!(
        reader
            .evidence_of_subject(&acme, &silent)
            .await
            .expect("the reader answers"),
        Vec::<EvidenceRecord>::new()
    );

    // Another account naming the same subject sees nothing.
    assert!(
        reader
            .evidence_of_subject(&other, &subject)
            .await
            .expect("the reader answers")
            .is_empty(),
        "the evidence of another account never comes back"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_store_is_an_error_never_an_empty_list() {
    let Ok(endpoint) = std::env::var("HODEI_GRAPH_IT_ENDPOINT") else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    };
    let mut relay = Relay::to(endpoint).await;
    let client = connect_to(&relay.endpoint).await;
    let store = Arc::new(
        DgraphEvidenceStore::open(client)
            .await
            .expect("the evidence store opens through the relay"),
    );
    let reader = DgraphEvidenceReader::new(store);

    let tag = nonce();
    let account = format!("down{tag}");
    let subject = subject_of(&account, "app");
    assert!(
        reader
            .evidence_of_subject(&account, &subject)
            .await
            .expect("the reader answers while the alpha is reachable")
            .is_empty()
    );

    relay.cut();
    match reader.evidence_of_subject(&account, &subject).await {
        Err(EvidenceStoreError::Unavailable(_)) => {}
        other => panic!("an unreachable alpha must be Unavailable, got {other:?}"),
    }
}
