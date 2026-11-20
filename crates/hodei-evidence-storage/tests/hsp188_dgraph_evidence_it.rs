//! HSP-188: the EvidenceStore over persistent Dgraph and the existing CAS.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use hodei_evidence::{
    ContentDigest, ContentStore, EvidenceIngestion, EvidenceStore, EvidenceType,
    EvidenceVerification, IngestionEnvelope, IngestionOutcome, IngestionRequest, Normalization,
    Producer, RejectionReason, SourceRef, StoreError,
};
use hodei_evidence_storage::{
    DgraphEventLedger, DgraphEvidenceStore, HmacEnvelopeAuthenticator, ProducerCredentials,
};
use hodei_graph_client::{GraphClient, GraphClientConfig};
use hodei_hub::blob::{BlobError, BlobStore, FileBlobStore};
use kernel::Hrn;

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

async fn connect() -> Option<Arc<GraphClient>> {
    let Some(config) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return None;
    };
    let mut client = GraphClient::connect(config).await.expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    let version = client
        .check_version()
        .await
        .expect("Dgraph reports its version");
    eprintln!("Dgraph server version: {version}");
    Some(Arc::new(client))
}

struct LocalCasContentStore(Arc<FileBlobStore>);

#[async_trait]
impl ContentStore for LocalCasContentStore {
    async fn put(&self, content: Vec<u8>) -> Result<ContentDigest, StoreError> {
        self.0
            .put(&content)
            .await
            .map_err(|error| StoreError::Unavailable(error.to_string()))
    }

    async fn get(&self, digest: &ContentDigest) -> Result<Option<Vec<u8>>, StoreError> {
        match self.0.get(digest).await {
            Ok(bytes) => Ok(Some(bytes)),
            Err(BlobError::NotFound) => Ok(None),
            Err(error) => Err(StoreError::Unavailable(error.to_string())),
        }
    }

    async fn size(&self, digest: &ContentDigest) -> Result<Option<u64>, StoreError> {
        match self.0.length(digest).await {
            Ok(length) => Ok(Some(length)),
            Err(BlobError::NotFound) => Ok(None),
            Err(error) => Err(StoreError::Unavailable(error.to_string())),
        }
    }
}

fn account() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/hsp188").expect("valid HRN")
}

fn producer() -> Producer {
    Producer::new("connector", "pipelinek").with_version("1.2.3")
}

fn envelope(external_id: &str, bytes: &[u8]) -> IngestionEnvelope {
    IngestionEnvelope {
        producer: producer(),
        tenant_hrn: account(),
        subject_refs: vec![account()],
        evidence_type: EvidenceType::Sbom,
        schema_uri: None,
        schema_version: None,
        source: SourceRef::by_external_id(external_id),
        content_digest: ContentDigest::of_bytes(bytes),
        relation_claim: None,
        observed_at: chrono::Utc::now(),
        valid_from: None,
        verification: EvidenceVerification::verified("sha256"),
        normalization: Normalization::new("pipelinek", "1.0.0"),
        correlation_id: Some("hsp188-it".to_owned()),
        signature: String::new(),
    }
}

fn nonce() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The accepted record and CAS bytes survive closing and reopening the client.
#[tokio::test]
async fn evidence_receipt_and_cas_content_survive_a_new_connection() {
    let _serial = SERIAL.lock().await;
    let Some(client) = connect().await else {
        return;
    };
    let evidence = Arc::new(
        DgraphEvidenceStore::open(client.clone())
            .await
            .expect("the evidence schema is ready"),
    );
    let ledger = Arc::new(
        DgraphEventLedger::open(client, "acme")
            .await
            .expect("the ledger is ready"),
    );
    let dir = tempfile::tempdir().expect("local CAS directory");
    let blobs = Arc::new(FileBlobStore::new(dir.path()).expect("local CAS opens"));
    let content = Arc::new(LocalCasContentStore(blobs.clone()));
    let auth = Arc::new(HmacEnvelopeAuthenticator::new());
    auth.register(
        "pipelinek",
        ProducerCredentials {
            secret: b"hsp188-it-secret".to_vec(),
            accounts: vec!["acme".to_owned()],
        },
    );
    let ingestion = EvidenceIngestion::new(content.clone(), evidence.clone(), ledger, auth.clone());
    let external_id = format!("hsp188-{}", nonce());
    let bytes = b"real evidence bytes stored outside Dgraph".to_vec();
    let mut request_envelope = envelope(&external_id, &bytes);
    request_envelope.signature = auth
        .sign(&request_envelope)
        .expect("producer is registered");
    let first = ingestion
        .ingest(IngestionRequest {
            envelope: request_envelope.clone(),
            content: bytes.clone(),
        })
        .await
        .expect("evidence ingestion succeeds");
    let evidence_id = first
        .evidence_id()
        .expect("accepted receipt has an id")
        .clone();
    assert!(matches!(first, IngestionOutcome::Accepted { .. }));

    let digest = ContentDigest::of_bytes(&bytes);
    assert_eq!(blobs.get(&digest).await.expect("CAS verifies bytes"), bytes);
    let fetched = evidence
        .get(&evidence_id)
        .await
        .expect("metadata reads")
        .expect("metadata persists");
    assert_eq!(fetched.content().digest, digest);
    assert_eq!(fetched.evidence_type(), &EvidenceType::Sbom);
    assert_eq!(fetched.tenant_hrn(), &account());
    assert!(
        fetched.verification().proves(),
        "the accepted receipt retains its verification"
    );
    assert_eq!(
        evidence
            .find_by_external_id(&producer(), &external_id)
            .await
            .expect("external lookup works"),
        Some(fetched.clone()),
    );
    let scoped = evidence
        .evidence_of_subject("acme", &account())
        .await
        .expect("scoped lookup works");
    assert!(
        scoped.contains(&fetched),
        "the matching account sees this evidence"
    );
    assert!(
        evidence
            .evidence_of_subject("other", &account())
            .await
            .expect("scope is enforced")
            .is_empty(),
        "another account cannot read this evidence"
    );

    let revision_bytes = b"a corrected, immutable evidence revision".to_vec();
    let mut revision_envelope = envelope(&external_id, &revision_bytes);
    revision_envelope.signature = auth
        .sign(&revision_envelope)
        .expect("producer is registered");
    let revision = ingestion
        .ingest(IngestionRequest {
            envelope: revision_envelope,
            content: revision_bytes.clone(),
        })
        .await
        .expect("revision ingestion succeeds");
    assert!(matches!(revision, IngestionOutcome::Revised { .. }));
    let revision_id = revision
        .evidence_id()
        .expect("revision has an identity")
        .clone();
    assert_ne!(
        revision_id, evidence_id,
        "a changed digest creates a new identity"
    );
    let current = evidence
        .find_by_external_id(&producer(), &external_id)
        .await
        .expect("external lookup returns latest revision")
        .expect("latest revision exists");
    assert_eq!(current.id(), &revision_id);
    assert_eq!(
        current.content().digest,
        ContentDigest::of_bytes(&revision_bytes)
    );
    assert_eq!(
        evidence
            .get(&evidence_id)
            .await
            .expect("prior version reads"),
        Some(fetched.clone()),
        "the previous evidence record is immutable and retained"
    );

    drop(ingestion);
    drop(evidence);
    let reopened = connect().await.expect("the server remains configured");
    let reopened_store = DgraphEvidenceStore::open(reopened)
        .await
        .expect("new connection opens");
    let after_reconnect = reopened_store
        .get(&evidence_id)
        .await
        .expect("metadata reads after reconnect");
    assert_eq!(after_reconnect, Some(fetched));
    assert_eq!(
        blobs.get(&digest).await.expect("CAS still verifies bytes"),
        bytes,
        "Dgraph stores no payload bytes; the same digest retrieves them from CAS"
    );

    let repeat = reopened_store
        .put(after_reconnect.expect("record persisted"))
        .await
        .expect("retry is idempotent");
    assert_eq!(repeat, evidence_id, "retry keeps one domain identity");
    let latest = reopened_store
        .find_by_external_id(&producer(), &external_id)
        .await
        .expect("external index survives reconnect")
        .expect("latest revision is still indexed");
    assert_eq!(
        latest.id(),
        &revision_id,
        "retrying the old receipt cannot roll back the index"
    );
}

/// A declared digest with corrupt or absent bytes is rejected before a
/// verified receipt can be written to the graph.
#[tokio::test]
async fn corrupt_or_missing_delivery_bytes_cannot_become_verified_evidence() {
    let _serial = SERIAL.lock().await;
    let Some(client) = connect().await else {
        return;
    };
    let evidence = Arc::new(
        DgraphEvidenceStore::open(client.clone())
            .await
            .expect("schema ready"),
    );
    let ledger = Arc::new(
        DgraphEventLedger::open(client, "acme")
            .await
            .expect("ledger ready"),
    );
    let dir = tempfile::tempdir().expect("local CAS directory");
    let blobs = Arc::new(FileBlobStore::new(dir.path()).expect("local CAS opens"));
    let content = Arc::new(LocalCasContentStore(blobs.clone()));
    let auth = Arc::new(HmacEnvelopeAuthenticator::new());
    auth.register(
        "pipelinek",
        ProducerCredentials {
            secret: b"hsp188-it-secret".to_vec(),
            accounts: vec!["acme".to_owned()],
        },
    );
    let ingestion = EvidenceIngestion::new(content, evidence.clone(), ledger, auth.clone());
    let expected = b"declared evidence bytes";
    let wrong = b"corrupt evidence bytes";
    for (external_id, supplied) in [
        (format!("bad-{}", nonce()), wrong.as_slice()),
        (format!("missing-{}", nonce()), b"".as_slice()),
    ] {
        let mut envelope = envelope(&external_id, expected);
        envelope.signature = auth.sign(&envelope).expect("producer is registered");
        let result = ingestion
            .ingest(IngestionRequest {
                envelope,
                content: supplied.to_vec(),
            })
            .await
            .expect("a digest mismatch is a rejection, not a storage error");
        assert!(matches!(
            result,
            IngestionOutcome::Rejected {
                reason: RejectionReason::ContentDigestMismatch
            }
        ));
        assert!(
            evidence
                .find_by_external_id(&producer(), &external_id)
                .await
                .expect("lookup works")
                .is_none()
        );
    }

    let digest = ContentDigest::of_bytes(expected);
    blobs
        .put(expected)
        .await
        .expect("store fixture in local CAS");
    tokio::fs::write(blobs.the_path_of(&digest), wrong)
        .await
        .expect("corrupt CAS fixture");
    assert!(
        blobs.get(&digest).await.is_err(),
        "CAS detects corrupt bytes"
    );
    tokio::fs::remove_file(blobs.the_path_of(&digest))
        .await
        .expect("remove CAS fixture");
    assert!(
        matches!(blobs.get(&digest).await, Err(BlobError::NotFound)),
        "CAS reports missing bytes distinctly"
    );
}
