//! HSP-134: the promotion risk facts come out of the real graph.
//!
//! The process starts with `DgraphRiskFacts` wired against the same
//! Dgraph namespace where the harness has seeded the delta registry and the
//! materialized view, through the real stores. What the
//! policy grants or rejects is, therefore, what the graph says -- not
//! a `RiskFacts::default()`:
//!
//! * a verified zero (view up to date, severities known, nothing
//!   reaching the subject) promotes, and the response carries the VERSION of the
//!   registry, not a borrowed zero;
//! * a critical that reaches the subject is rejected with the same version;
//! * a stale view is UNKNOWN: it is not zero, it does not promote;
//! * an advisory the catalog does not know does not establish the risk either.
//!
//! The persisted decision is read from the database itself at the end: the verdict
//! and the version the client sees are the ones that were written.

mod support;

use hodei_trust_policy::{Requirement, RequirementKind, TrustPolicy};
use support::{Curl, Deployment, FIRST_ACCOUNT, FIRST_REPOSITORY};

const REGION: &str = "eu";

/// The advisory the catalog knows and does not block.
const KNOWN_ADVISORY: &str = "hrn:hodei:artifacts:eu-west-1:acme:Vulnerability/GHSA-hsp134-known";

/// The advisory whose severity the catalog knows and DOES block.
const CRITICAL_ADVISORY: &str =
    "hrn:hodei:artifacts:eu-west-1:acme:Vulnerability/GHSA-hsp134-critical";

fn raw(repository: &str, path: &str) -> String {
    format!("/repository/{repository}/{path}")
}

fn call(
    deployment: &Deployment,
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    body: Option<(&[u8], &str)>,
) -> (u16, String) {
    let directory = deployment.directory.path();
    let into = directory.join("response.bin");
    let _ = std::fs::remove_file(&into);
    let sent = directory.join("request.bin");
    let (file, content_type) = match body {
        Some((bytes, content_type)) => {
            std::fs::write(&sent, bytes).expect("the body is written");
            (Some(sent.as_path()), Some(content_type))
        }
        None => (None, None),
    };
    let answer = Curl::call(port, method, path, token, file, content_type, Some(&into)).run();
    let text = String::from_utf8_lossy(&std::fs::read(&into).unwrap_or_default()).into_owned();
    (answer.status, text)
}

fn put_raw(deployment: &Deployment, port: u16, token: &str, path: &str, bytes: &[u8]) -> u16 {
    call(
        deployment,
        port,
        "PUT",
        path,
        token,
        Some((bytes, "application/octet-stream")),
    )
    .0
}

fn promote(
    deployment: &Deployment,
    port: u16,
    token: &str,
    repository: &str,
    path: &str,
    digest_hex: &str,
) -> (u16, String) {
    let body = serde_json::json!({
        "repository": repository,
        "path": path,
        "digest": format!("sha256:{digest_hex}"),
    })
    .to_string();
    call(
        deployment,
        port,
        "POST",
        "/api/v1/promotions",
        token,
        Some((body.as_bytes(), "application/json")),
    )
}

/// The subject of content uploaded as `path`, stated the same way the
/// handler states it: the artifact HRN by digest.
fn subject_of(content: &[u8]) -> kernel::Hrn {
    let digest = hodei_evidence::ContentDigest::of_bytes(content);
    hodei_artifacts_api::trust::hrn::artifact_hrn(REGION, FIRST_ACCOUNT, FIRST_REPOSITORY, &digest)
}

/// The graph tenant for that subject: the same template the deployment
/// starts with.
fn tenant_of(subject: &kernel::Hrn) -> kernel::Hrn {
    hodei_artifacts_api::config::InvestigationConfig::default()
        .the_graph_tenant(subject)
        .expect("the default tenant template names an HRN")
}

/// The policy that only requires the absence of reaching criticals.
fn risk_policy_file(deployment: &Deployment) -> String {
    let policy = TrustPolicy::new(
        "risk-aware",
        "1.0.0",
        vec![Requirement::denying(
            RequirementKind::NoReachableCriticalVulnerability,
        )],
    );
    let path = deployment.directory.path().join("trust-policy.json");
    std::fs::write(&path, serde_json::to_vec(&policy).expect("json"))
        .expect("the policy is written");
    path.to_string_lossy().into_owned()
}

/// The severity catalog the operator hands to the deployment.
fn catalog_file(deployment: &Deployment, severities: &[(&str, &str)]) -> String {
    let mut entries = serde_json::Map::new();
    for (advisory, severity) in severities {
        entries.insert(advisory.to_string(), serde_json::json!(severity));
    }
    let path = deployment.directory.path().join("risk-catalog.json");
    std::fs::write(
        &path,
        serde_json::to_vec_pretty(&serde_json::Value::Object(entries)).expect("json"),
    )
    .expect("the catalog is written");
    path.to_string_lossy().into_owned()
}

/// What is seeded in the graph before raising the process: the registry
/// that speaks, the view paths with their watermark and the rows that
/// reach a subject.
struct Seed {
    /// The version of the delta registry for the account.
    ledger_seq: u64,
    /// The materialized paths: advisory and watermark.
    routes: Vec<(String, u64)>,
    /// The view rows: advisory, reached subject.
    rows: Vec<(String, String)>,
}

/// Writes the seed into the deployment's Dgraph namespace, through the
/// real stores: the delta registry states the version and the view is
/// materialized with that watermark.
///
/// It is done BEFORE starting the process, as the operator would.
fn seed(deployment: &Deployment, seed: &Seed) {
    let tenant = tenant_of(&subject_of(b"whatever"));
    let plan = support::graph_risk::RiskSeed {
        ledger_seq: seed.ledger_seq,
        routes: seed.routes.clone(),
        rows: seed.rows.clone(),
    };
    let graph = deployment.graph.as_ref().expect("the graph profile");
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime to seed")
        .block_on(support::graph_risk::seed_risk(graph, &tenant, &plan));
}

/// The last persisted decision, read from the graph with the service already stopped.
struct Persisted {
    subject: String,
    outcome: hodei_trust_policy::Outcome,
    risk_version: u64,
}

fn last_decision(deployment: &Deployment) -> Persisted {
    let graph = deployment.graph.as_ref().expect("the graph profile");
    let subject = subject_of(b"risk-free artifact").to_string();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime to read")
        .block_on(async {
            use hodei_trust_policy::DecisionStore as _;
            let store = hodei_trust_policy_storage::DgraphDecisionStore::open(graph.client().await)
                .await
                .expect("the decision store opens");
            let decision = store
                .list_for_subject(FIRST_ACCOUNT, &subject)
                .await
                .expect("the decisions read")
                .into_iter()
                .next()
                .expect("a persisted decision");
            Persisted {
                subject: decision.request.subject.to_string(),
                outcome: decision.outcome,
                risk_version: decision.trust.risk_version,
            }
        })
}

fn a_deployment(plan: &Seed) -> Deployment {
    let deployment = Deployment::new();
    deployment.seed_trusted(&[], &[FIRST_REPOSITORY]);
    seed(&deployment, plan);
    deployment
}

/// The catalog seeder: the harness relaunches this binary under that exact
/// name to create the repositories before raising the service.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

// ---------------------------------------------------------------------------
// A verified zero promotes with the registry version.
// ---------------------------------------------------------------------------

#[test]
fn a_verified_zero_promotes_and_carries_the_ledger_version() {
    let content = b"risk-free artifact".to_vec();
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let subject = subject_of(&content);
    let deployment = a_deployment(&Seed {
        ledger_seq: 7,
        routes: vec![(KNOWN_ADVISORY.to_string(), 7)],
        rows: vec![],
    });
    let policy = risk_policy_file(&deployment);
    let catalog = catalog_file(&deployment, &[(KNOWN_ADVISORY, "other")]);
    let mut service = deployment.start(&[
        ("HODEI_TRUST__POLICY_FILE", &policy),
        ("HODEI_TRUST__RISK_CATALOG_FILE", &catalog),
    ]);

    assert_eq!(
        put_raw(
            &deployment,
            service.port,
            &alice,
            &raw(FIRST_REPOSITORY, "app/risk-free.bin"),
            &content,
        ),
        201
    );
    let digest = hodei_evidence::ContentDigest::of_bytes(&content).value;
    let (status, body) = promote(
        &deployment,
        service.port,
        &alice,
        FIRST_REPOSITORY,
        "app/risk-free.bin",
        &digest,
    );
    assert_eq!(status, 200, "a verified zero refused: {body}");
    let promoted: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(promoted["promoted"], true);
    assert_eq!(
        promoted["risk_version"], 7,
        "the wire answered with the version of the ledger it read"
    );
    support::stop(&mut service);

    let decision = last_decision(&deployment);
    assert_eq!(
        decision.subject,
        subject.to_string(),
        "the subject the handler promoted is the subject the view was seeded for"
    );
    assert_eq!(
        decision.risk_version, 7,
        "the persisted decision carries the same version"
    );
    assert_eq!(
        decision.outcome,
        hodei_trust_policy::Outcome::Allow,
        "the verdict is the one Cedar said"
    );
}

// ---------------------------------------------------------------------------
// A critical that reaches the subject is rejected, and the version travels.
// ---------------------------------------------------------------------------

#[test]
fn a_critical_reaching_the_subject_is_refused_with_its_version() {
    let content = b"vulnerable artifact".to_vec();
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let subject = subject_of(&content).to_string();
    let deployment = a_deployment(&Seed {
        ledger_seq: 7,
        routes: vec![
            (KNOWN_ADVISORY.to_string(), 7),
            (CRITICAL_ADVISORY.to_string(), 7),
        ],
        rows: vec![(CRITICAL_ADVISORY.to_string(), subject)],
    });
    let policy = risk_policy_file(&deployment);
    let catalog = catalog_file(
        &deployment,
        &[(KNOWN_ADVISORY, "other"), (CRITICAL_ADVISORY, "critical")],
    );
    let mut service = deployment.start(&[
        ("HODEI_TRUST__POLICY_FILE", &policy),
        ("HODEI_TRUST__RISK_CATALOG_FILE", &catalog),
    ]);

    assert_eq!(
        put_raw(
            &deployment,
            service.port,
            &alice,
            &raw(FIRST_REPOSITORY, "app/sick.bin"),
            &content,
        ),
        201
    );
    let digest = hodei_evidence::ContentDigest::of_bytes(&content).value;
    let (status, body) = promote(
        &deployment,
        service.port,
        &alice,
        FIRST_REPOSITORY,
        "app/sick.bin",
        &digest,
    );
    assert_eq!(status, 422, "a reaching critical promoted: {body}");
    let refused: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(refused["promoted"], false);
    assert_eq!(refused["risk_version"], 7);
    let reasons: Vec<&str> = refused["reasons"]
        .as_array()
        .expect("reasons")
        .iter()
        .map(|reason| reason.as_str().expect("a string"))
        .collect();
    assert!(
        reasons
            .iter()
            .any(|reason| reason.contains("RiskReachableCriticalVulnerability")),
        "the refusal must name the reachable critical, and it said {reasons:?}"
    );
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// The stale view is UNKNOWN: it is not zero and does not promote.
// ---------------------------------------------------------------------------

#[test]
fn a_stale_view_leaves_the_risk_unknown_and_refuses() {
    let content = b"lagging artifact".to_vec();
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let deployment = a_deployment(&Seed {
        // The registry speaks version 9; the view stayed at 7.
        ledger_seq: 9,
        routes: vec![(KNOWN_ADVISORY.to_string(), 7)],
        rows: vec![],
    });
    let policy = risk_policy_file(&deployment);
    let catalog = catalog_file(&deployment, &[(KNOWN_ADVISORY, "other")]);
    let mut service = deployment.start(&[
        ("HODEI_TRUST__POLICY_FILE", &policy),
        ("HODEI_TRUST__RISK_CATALOG_FILE", &catalog),
    ]);

    assert_eq!(
        put_raw(
            &deployment,
            service.port,
            &alice,
            &raw(FIRST_REPOSITORY, "app/lag.bin"),
            &content,
        ),
        201
    );
    let digest = hodei_evidence::ContentDigest::of_bytes(&content).value;
    let (status, body) = promote(
        &deployment,
        service.port,
        &alice,
        FIRST_REPOSITORY,
        "app/lag.bin",
        &digest,
    );
    assert_eq!(status, 422, "a stale view promoted: {body}");
    let refused: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(
        refused["risk_version"], 9,
        "the version travels even when the view lags"
    );
    let reasons = refused["reasons"].as_array().expect("reasons");
    assert!(
        reasons
            .iter()
            .any(|reason| reason.as_str().unwrap_or_default().contains("RiskUnknown")),
        "the refusal must say the risk is unknown, and it said {refused}"
    );
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// Without a catalog there is no known severity: UNKNOWN, not zero.
// ---------------------------------------------------------------------------

#[test]
fn an_uncovered_advisory_never_establishes_a_clean_risk() {
    let content = b"uncatalogued artifact".to_vec();
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let deployment = a_deployment(&Seed {
        ledger_seq: 7,
        routes: vec![(CRITICAL_ADVISORY.to_string(), 7)],
        // The advisory reaches the subject, but the catalog does not know it.
        rows: vec![(
            CRITICAL_ADVISORY.to_string(),
            subject_of(&content).to_string(),
        )],
    });
    let policy = risk_policy_file(&deployment);
    let catalog = catalog_file(&deployment, &[]);
    let mut service = deployment.start(&[
        ("HODEI_TRUST__POLICY_FILE", &policy),
        ("HODEI_TRUST__RISK_CATALOG_FILE", &catalog),
    ]);

    assert_eq!(
        put_raw(
            &deployment,
            service.port,
            &alice,
            &raw(FIRST_REPOSITORY, "app/unknown.bin"),
            &content,
        ),
        201
    );
    let digest = hodei_evidence::ContentDigest::of_bytes(&content).value;
    let (status, body) = promote(
        &deployment,
        service.port,
        &alice,
        FIRST_REPOSITORY,
        "app/unknown.bin",
        &digest,
    );
    assert_eq!(status, 422, "an unknown severity promoted: {body}");
    let refused: serde_json::Value = serde_json::from_str(&body).expect("json");
    let reasons = refused["reasons"].as_array().expect("reasons");
    assert!(
        reasons
            .iter()
            .any(|reason| reason.as_str().unwrap_or_default().contains("RiskUnknown")),
        "the absence of knowledge is UNKNOWN, and it said {refused}"
    );
    support::stop(&mut service);
}
