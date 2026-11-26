//! HSP-137: the CVE-to-distribution cycle closed over the real service.
//!
//! What is tested here is the wiring of the complete milestone, not the pieces:
//! an advisory enters through the supported entry (`POST
//! /api/v1/advisories/accept`), the engine states the reevaluation intents
//! against the real graph, and the distribution **blocks the pull of the
//! granted artifact within the same call**, because the reevaluation withdraws the
//! grant before responding. There is no external consumer that could be
//! down: the cycle runs in the process that serves the bytes.
//!
//! The cases follow the TRUST-04 UAT:
//!
//! * AC1: promoted artifact -> critical advisory enters -> new pull 404,
//!   and the admission->block time is **measured**, not declared;
//! * AC2: an irrelevant advisory (severity `other`) does not withdraw the
//!   grant of another artifact, and the policy change reevaluates the
//!   dependent subjects (drift sweep after restart with version 2.0.0);
//! * AC3: `kill -9` in the window between computing and revoking (the durable
//!   outbox keeps the surviving half) -> the restart converges through
//!   `resume` without duplicating the effect receipt.
//!
//! Honest limit, as in `hsp104_gc_e2e.rs`: the exact cut inside the
//! handler cannot be provoked from outside without a test hook in the
//! production code. The window is reproduced by writing into the outbox
//! what the process would have said before dying -- it is the same durable
//! state that `kill -9` leaves, and `resume` does not distinguish who wrote it.

mod support;

use std::time::Instant;

use hodei_trust_policy::{Requirement, RequirementKind, TrustPolicy};
use support::{Curl, Deployment, FIRST_ACCOUNT, FIRST_REPOSITORY};

const REGION: &str = "eu";

/// The graph tenant: the deployment's default template.
const TENANT: &str = "hrn:hodei:artifacts:eu:acme:artifact/acme";

/// The critical advisory of AC1/AC2, named the way the projection names it.
const CRITICAL_ADVISORY: &str = "hrn:hodei:artifacts:eu:acme:Vulnerability/GHSA-hsp137-0001";

/// The irrelevant advisory (severity `other`) of AC2.
const IRRELEVANT_ADVISORY: &str = "hrn:hodei:artifacts:eu:acme:Vulnerability/GHSA-hsp137-other";

/// The advisory of the crash window (AC3).
const CRASH_ADVISORY: &str = "hrn:hodei:artifacts:eu:acme:Vulnerability/GHSA-hsp137-crash";

/// The CycloneDX 1.7 inventory of the artifact: `web` depends on `lib-a`, which
/// depends on `lib-b` and `lib-c`. The test advisory matches `lib-a` by name,
/// and the blast radius goes up from there to the projected artifact.
const INVENTORY: &str = r#"{
  "bomFormat": "CycloneDX",
  "specVersion": "1.7",
  "version": 1,
  "metadata": {
    "timestamp": "2026-03-04T10:15:00Z",
    "component": {
      "bom-ref": "pkg:oci/web@sha256:ac9da485d6eac8c161117ecd4155cf50bdb330c8241234d669aef1edba265a06",
      "type": "container",
      "name": "web",
      "version": "1.0.0",
      "purl": "pkg:oci/web@sha256:ac9da485d6eac8c161117ecd4155cf50bdb330c8241234d669aef1edba265a06"
    }
  },
  "components": [
    { "bom-ref": "pkg:maven/org.example/lib-a@1.2.3", "type": "library", "name": "lib-a", "version": "1.2.3", "purl": "pkg:maven/org.example/lib-a@1.2.3" },
    { "bom-ref": "pkg:maven/org.example/lib-b@2.0.0", "type": "library", "name": "lib-b", "version": "2.0.0", "purl": "pkg:maven/org.example/lib-b@2.0.0" },
    { "bom-ref": "pkg:golang/github.com/example/lib-c@v1.4.2", "type": "library", "name": "lib-c", "version": "v1.4.2", "purl": "pkg:golang/github.com/example/lib-c@v1.4.2" }
  ],
  "dependencies": [
    { "ref": "pkg:oci/web@sha256:ac9da485d6eac8c161117ecd4155cf50bdb330c8241234d669aef1edba265a06", "dependsOn": ["pkg:maven/org.example/lib-a@1.2.3"] },
    { "ref": "pkg:maven/org.example/lib-a@1.2.3", "dependsOn": ["pkg:maven/org.example/lib-b@2.0.0"] },
    { "ref": "pkg:maven/org.example/lib-b@2.0.0", "dependsOn": ["pkg:golang/github.com/example/lib-c@v1.4.2"] },
    { "ref": "pkg:golang/github.com/example/lib-c@v1.4.2", "dependsOn": [] }
  ]
}"#;

/// The OSV document of the advisory that matches `lib-a`.
fn osv(id: &str) -> serde_json::Value {
    serde_json::json!({
        "schema_version": "1.6.0",
        "id": id,
        "modified": "2026-11-18T00:00:00Z",
        "affected": [
            { "package": { "ecosystem": "Maven", "name": "lib-a", "purl": "pkg:maven/org.example/lib-a@1.2.3" } }
        ]
    })
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// The schema with the administrative surface (Cedar gate of the accept).
fn the_schema() -> String {
    let mut schema: serde_json::Value =
        serde_json::from_str(support::SCHEMA).expect("the schema parses");
    schema["Admin"] = serde_json::json!({
        "entityTypes": { "AdminApi": { "shape": { "type": "Record", "attributes": {} } } },
        "actions": {
            "AccessAdminApi": {
                "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Admin::AdminApi"] }
            }
        }
    });
    schema.to_string()
}

/// The Cedar policy: the usual content surface, plus
/// administration open for alice (the accept's resource covers it).
fn the_policy() -> String {
    format!(
        "{}\npermit(principal == Iam::User::\"hrn:hodei:iam::{FIRST_ACCOUNT}:User/alice\", action == Admin::Action::\"AccessAdminApi\", resource);\n",
        support::the_policy()
    )
}

/// The trust policy: only the absence of reaching criticals.
/// `version` is what the sweep's drift compares with the grant.
fn risk_policy_file(deployment: &Deployment, version: &str) -> String {
    let policy = TrustPolicy::new(
        "default",
        version,
        vec![Requirement::denying(
            RequirementKind::NoReachableCriticalVulnerability,
        )],
    );
    let path = deployment.directory.path().join("trust-policy.json");
    std::fs::write(&path, serde_json::to_vec(&policy).expect("json")).expect("written");
    path.to_string_lossy().into_owned()
}

/// The operator's severity catalog.
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

/// The reference route of the view: with no materialized route the
/// reader says "no view means no currency" (HSP-134's own `is_current`),
/// and the zero of the promotion is not verified. The route reaches
/// no subject -- it has no rows in `impact_view` -- and its
/// watermark is above any version that the test's graph is going
/// to state, so that the tenant's currency depends only on the
/// advisory routes that the cycle rebuilds.
const BASELINE_ROUTE: &str = "hrn:hodei:artifacts:eu:acme:Vulnerability/GHSA-hsp137-baseline";

/// A deployment with the catalog seeded, the trusted channel open,
/// administration enabled and the view at verified zero.
fn a_deployment() -> Deployment {
    let deployment = Deployment::new();
    std::fs::write(
        deployment.directory.path().join("schema.json"),
        the_schema(),
    )
    .expect("schema");
    std::fs::write(
        deployment.directory.path().join("policies.cedar"),
        the_policy(),
    )
    .expect("policy");
    deployment.seed_trusted(&[], &[FIRST_REPOSITORY]);
    seed_baseline(&deployment);
    deployment
}

/// Writes the reference route before raising the process, through the
/// real graph stores: a materialized route without reached subjects,
/// at the registry's current version, which is what makes the view
/// "up to date" (a verified zero) before any advisory enters.
fn seed_baseline(deployment: &Deployment) {
    let plan = support::graph_risk::RiskSeed {
        ledger_seq: 0,
        routes: vec![(BASELINE_ROUTE.to_string(), 0)],
        rows: Vec::new(),
    };
    let graph = deployment.graph.as_ref().expect("the graph profile");
    let tenant = hodei_evidence_tenant();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime to seed")
        .block_on(support::graph_risk::seed_risk(graph, &tenant, &plan));
}

fn hodei_evidence_tenant() -> kernel::Hrn {
    kernel::Hrn::from_string(TENANT).expect("the tenant is an HRN")
}

fn raw(path: &str) -> String {
    format!("/repository/{FIRST_REPOSITORY}/{path}")
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
    path: &str,
    digest_hex: &str,
) -> (u16, String) {
    let body = serde_json::json!({
        "repository": FIRST_REPOSITORY,
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

/// The supported advisory entry.
fn admit(
    deployment: &Deployment,
    port: u16,
    token: &str,
    digest_hex: &str,
    advisory: serde_json::Value,
) -> (u16, String) {
    let body = serde_json::json!({
        "repository": FIRST_REPOSITORY,
        "digest": format!("sha256:{digest_hex}"),
        "advisory": advisory,
        "inventory": serde_json::from_str::<serde_json::Value>(INVENTORY).expect("inventory"),
    })
    .to_string();
    call(
        deployment,
        port,
        "POST",
        "/api/v1/advisories/accept",
        token,
        Some((body.as_bytes(), "application/json")),
    )
}

/// The cycle subject: the artifact HRN by digest, stated the same way
/// the handler and the promotion state it.
fn subject_of(content: &[u8]) -> String {
    let digest = hodei_evidence::ContentDigest::of_bytes(content);
    hodei_artifacts_api::trust::hrn::artifact_hrn(REGION, FIRST_ACCOUNT, FIRST_REPOSITORY, &digest)
        .to_string()
}

fn hex_of(content: &[u8]) -> String {
    hodei_evidence::ContentDigest::of_bytes(content).value
}

// ---------------------------------------------------------------------------
// Graph surgery (the crash window and the evidence reading)
// ---------------------------------------------------------------------------

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime")
}

/// Seeds the surviving half of the crash: the critical reaching the subject
/// in the registry and the view (with known severity) and the PENDING
/// reevaluation intent in the durable outbox -- exactly what stays
/// alive when the process dies after planning and before revoking.
fn plant_crash_window(deployment: &Deployment, subject: &str) {
    use hodei_reactive::intent::{EventOrigin, Intent, IntentOrigin};
    use hodei_reactive::intent::IntentOutbox as _;
    let graph = deployment.graph.as_ref().expect("the graph profile");
    let advisory = CRASH_ADVISORY;
    // The view says the same version as the ledger, for the baseline route
    // too: the lagging route would make the whole tenant stale.
    let plan = support::graph_risk::RiskSeed {
        ledger_seq: 5,
        routes: vec![(BASELINE_ROUTE.to_string(), 5), (advisory.to_string(), 5)],
        rows: vec![(advisory.to_string(), subject.to_string())],
    };
    runtime().block_on(async {
        support::graph_risk::seed_risk(graph, &hodei_evidence_tenant(), &plan).await;
        // The intent stated by the planner before dying. The identity and
        // the key are the SAME as the crate plan would have written:
        // `intent:vulnerability_impact:{event}:{key}`.
        let event_id = "evt-hsp137-crash-window";
        let key = format!("decision:{advisory}:{subject}");
        let intent = Intent::new(
            IntentOrigin {
                behavior: "vulnerability_impact".to_string(),
                event: EventOrigin::new(event_id, Vec::new()),
            },
            "hodei",
            "decision_reevaluation",
            key,
            event_id,
            serde_json::json!({
                "subject": subject,
                "advisory": advisory,
                "reason": "vulnerability_impacted",
                "at": "2026-11-18T00:00:00Z",
            }),
        );
        hodei_trust_policy_storage::DgraphIntentStore::open(graph.client().await)
            .await
            .expect("the intent store opens")
            .put_pending(&intent)
            .await
            .expect("the intent is said");
    });
}

/// The persisted evidence that the test reads with the service already stopped.
#[derive(Debug)]
struct DecisionRow {
    subject: String,
    outcome: String,
    policy_version: String,
}

/// The subject's decisions, from oldest to most recent.
fn decisions(deployment: &Deployment, subject: &str) -> Vec<DecisionRow> {
    use hodei_trust_policy::DecisionStore as _;
    let graph = deployment.graph.as_ref().expect("the graph profile");
    runtime().block_on(async {
        let store = hodei_trust_policy_storage::DgraphDecisionStore::open(graph.client().await)
            .await
            .expect("the decision store opens");
        let mut rows = store
            .list_for_subject(FIRST_ACCOUNT, subject)
            .await
            .expect("the decisions read");
        rows.sort_by_key(|decision| decision.decided_at);
        rows.into_iter()
            .map(|decision| DecisionRow {
                subject: decision.request.subject.clone(),
                outcome: format!("{:?}", decision.outcome).to_uppercase(),
                policy_version: decision.trust.policy_version.clone(),
            })
            .collect()
    })
}

/// How many times the effect of that key was applied.
fn effect_receipts(deployment: &Deployment, key: &str) -> u64 {
    use hodei_reactive::runtime::ExecutionStore as _;
    let graph = deployment.graph.as_ref().expect("the graph profile");
    runtime().block_on(async {
        let store = hodei_trust_policy_storage::DgraphIntentStore::open(graph.client().await)
            .await
            .expect("the intent store opens");
        store
            .applied_effects()
            .await
            .iter()
            .filter(|effect| effect.idempotency_key == key)
            .count() as u64
    })
}

/// The catalog seeder.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

// ---------------------------------------------------------------------------
// AC1 (UAT TRUST-04): critical advisory -> the new pull is blocked,
// and the admission->block time is measured.
// ---------------------------------------------------------------------------

#[test]
fn a_critical_advisory_blocks_the_pull_it_should() {
    let content = b"hsp137 artifact".to_vec();
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let deployment = a_deployment();
    let policy = risk_policy_file(&deployment, "1.0.0");
    let catalog = catalog_file(&deployment, &[(CRITICAL_ADVISORY, "critical")]);
    let mut service = deployment.start(&[
        ("HODEI_TRUST__POLICY_FILE", &policy),
        ("HODEI_TRUST__RISK_CATALOG_FILE", &catalog),
        ("HODEI_SECURITY__ADMIN_API_ENABLED", "true"),
    ]);
    let path = raw("app/server.bin");
    let digest = hex_of(&content);
    assert_eq!(
        put_raw(&deployment, service.port, &alice, &path, &content),
        201
    );
    let (status, body) = promote(&deployment, service.port, &alice, "app/server.bin", &digest);
    assert_eq!(status, 200, "the zero did not promote: {body}");
    assert_eq!(
        call(&deployment, service.port, "GET", &path, &alice, None).0,
        200
    );

    // The admission runs the complete cycle: projection, view, intents,
    // reevaluation, revocation. When it responds, the grant is already
    // withdrawn; the pull that measures it is the first BLOCKED pull.
    let admitted_at = Instant::now();
    let (status, body) = admit(
        &deployment,
        service.port,
        &alice,
        &digest,
        osv("GHSA-hsp137-0001"),
    );
    assert_eq!(
        status,
        200,
        "the admission refused: {body}\nlog: {}",
        deployment.the_log()
    );
    let admission: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(admission["advisory"], CRITICAL_ADVISORY);
    assert_eq!(admission["duplicate"], false);
    assert!(admission["behaviors_ran"].as_u64().unwrap_or(0) >= 1);
    let (status, _) = call(&deployment, service.port, "GET", &path, &alice, None);
    let blocked_in = admitted_at.elapsed();
    assert_eq!(
        status, 404,
        "the critical reached the artifact and the pull still served"
    );

    // SLO: the declared target is p99 <= 10 s on a node. Here a value
    // is measured, compliance of a distribution is not declared.
    let elapsed_millis = admission["elapsed_millis"].as_u64().expect("elapsed");
    println!(
        "HSP-137 SLO sample: admission={elapsed_millis} ms, admission->blocked pull={} ms",
        blocked_in.as_millis()
    );
    assert!(
        blocked_in.as_millis() < 10_000,
        "the lab sample blew the 10 s objective: {blocked_in:?}"
    );
    assert!(
        elapsed_millis < 10_000,
        "the admission itself blew the objective: {elapsed_millis} ms"
    );

    // Readmitting the same advisory is the same fact stated twice:
    // the cycle says so instead of pretending again, and opens nothing.
    let (status, body) = admit(
        &deployment,
        service.port,
        &alice,
        &digest,
        osv("GHSA-hsp137-0001"),
    );
    assert_eq!(status, 200, "the redelivery refused: {body}");
    let again: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(
        again["duplicate"], true,
        "the same event said twice ran twice"
    );
    support::stop(&mut service);

    // The persisted decision: the subject promoted and then denied by the
    // reaching critical, in the same order the wire counted.
    let rows = decisions(&deployment, &subject_of(&content));
    assert!(
        rows.iter().all(|row| row.subject == subject_of(&content)),
        "the trail names another subject: {rows:?}"
    );
    let outcomes: Vec<&str> = rows.iter().map(|row| row.outcome.as_str()).collect();
    assert_eq!(
        outcomes,
        vec!["ALLOW", "DENY"],
        "the decision trail says {rows:?}"
    );
    assert_eq!(
        rows[1].policy_version, "1.0.0",
        "the reevaluation decided with the active policy"
    );
}

// ---------------------------------------------------------------------------
// AC2: an irrelevant CVE does not withdraw other grants.
// ---------------------------------------------------------------------------

#[test]
fn an_irrelevant_advisory_keeps_every_concession_it_should() {
    let content = b"hsp137 irrelevant".to_vec();
    let foreign = b"hsp137 untouched".to_vec();
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let deployment = a_deployment();
    let policy = risk_policy_file(&deployment, "1.0.0");
    let catalog = catalog_file(
        &deployment,
        &[
            (CRITICAL_ADVISORY, "critical"),
            (IRRELEVANT_ADVISORY, "other"),
        ],
    );
    let mut service = deployment.start(&[
        ("HODEI_TRUST__POLICY_FILE", &policy),
        ("HODEI_TRUST__RISK_CATALOG_FILE", &catalog),
        ("HODEI_SECURITY__ADMIN_API_ENABLED", "true"),
    ]);
    let mine = raw("app/mine.bin");
    let theirs = raw("app/theirs.bin");
    let mine_digest = hex_of(&content);
    let theirs_digest = hex_of(&foreign);
    assert_eq!(
        put_raw(&deployment, service.port, &alice, &mine, &content),
        201
    );
    assert_eq!(
        put_raw(&deployment, service.port, &alice, &theirs, &foreign),
        201
    );
    assert_eq!(
        promote(
            &deployment,
            service.port,
            &alice,
            "app/mine.bin",
            &mine_digest
        )
        .0,
        200
    );
    assert_eq!(
        promote(
            &deployment,
            service.port,
            &alice,
            "app/theirs.bin",
            &theirs_digest
        )
        .0,
        200
    );

    // The `other`-severity advisory touches my artifact (the inventory
    // matches lib-a): the reevaluation happens and AUTHORIZES -- the policy
    // only denies criticals. The other artifact does not even enter the
    // radius: what the advisory does not reach is not reevaluated.
    let (status, body) = admit(
        &deployment,
        service.port,
        &alice,
        &mine_digest,
        osv("GHSA-hsp137-other"),
    );
    assert_eq!(status, 200, "the admission refused: {body}");
    assert_eq!(
        call(&deployment, service.port, "GET", &mine, &alice, None).0,
        200,
        "an `other` severity revoked"
    );
    assert_eq!(
        call(&deployment, service.port, "GET", &theirs, &alice, None).0,
        200,
        "a foreign concession fell"
    );
    support::stop(&mut service);

    // The evidence: my subject was reevaluated (two ALLOWs: promotion and
    // cycle) and the other keeps a SINGLE decision -- nobody touched it.
    let mine_rows = decisions(&deployment, &subject_of(&content));
    assert_eq!(
        mine_rows.len(),
        2,
        "the trail of the reached subject: {mine_rows:?}"
    );
    assert!(mine_rows.iter().all(|row| row.outcome == "ALLOW"));
    let foreign_rows = decisions(&deployment, &subject_of(&foreign));
    assert_eq!(
        foreign_rows.len(),
        1,
        "the foreign subject was reevaluated: {foreign_rows:?}"
    );
}

// ---------------------------------------------------------------------------
// AC2 (derived): the policy change reevaluates the dependent subjects.
// The drift sweep detects the new active version after the restart.
// ---------------------------------------------------------------------------

#[test]
fn a_policy_change_reevaluates_the_subjects_it_concerns() {
    let content = b"hsp137 drift".to_vec();
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let deployment = a_deployment();
    let policy = risk_policy_file(&deployment, "1.0.0");
    let catalog = catalog_file(&deployment, &[]);
    let mut service = deployment.start(&[
        ("HODEI_TRUST__POLICY_FILE", &policy),
        ("HODEI_TRUST__RISK_CATALOG_FILE", &catalog),
        ("HODEI_SECURITY__ADMIN_API_ENABLED", "true"),
    ]);
    let path = raw("app/drift.bin");
    let digest = hex_of(&content);
    assert_eq!(
        put_raw(&deployment, service.port, &alice, &path, &content),
        201
    );
    assert_eq!(
        promote(&deployment, service.port, &alice, "app/drift.bin", &digest).0,
        200
    );
    let port = service.port;
    support::stop(&mut service);

    // The operator raises the policy to 2.0.0 and restarts. The written grant
    // says 1.0.0: the sweep (1 s interval) sees it as obsolete and
    // reevaluates the subject against the active version.
    std::fs::write(
        deployment.directory.path().join("trust-policy.json"),
        serde_json::to_vec(&TrustPolicy::new(
            "default",
            "2.0.0",
            vec![Requirement::denying(
                RequirementKind::NoReachableCriticalVulnerability,
            )],
        ))
        .expect("json"),
    )
    .expect("the policy is raised");
    let mut service = deployment.start_on(
        port,
        &[
            ("HODEI_TRUST__POLICY_FILE", &policy),
            ("HODEI_TRUST__RISK_CATALOG_FILE", &catalog),
            ("HODEI_SECURITY__ADMIN_API_ENABLED", "true"),
            ("HODEI_TRUST__SWEEP_INTERVAL_SECS", "1"),
        ],
    );
    std::thread::sleep(std::time::Duration::from_millis(2500));
    // The distribution keeps serving: the reevaluation AUTHORIZES against the
    // new policy (the risk is still a verified zero).
    assert_eq!(
        call(&deployment, service.port, "GET", &path, &alice, None).0,
        200,
        "the drift reevaluation denied what it should have kept"
    );
    support::stop(&mut service);

    // The evidence: a new decision with version 2.0.0, issued by
    // the cycle (reactor actor) and not by a client.
    let rows = decisions(&deployment, &subject_of(&content));
    let versions: Vec<&str> = rows.iter().map(|row| row.policy_version.as_str()).collect();
    assert_eq!(
        versions,
        vec!["1.0.0", "2.0.0"],
        "the policy change did not reevaluate the subject it concerns: {rows:?}"
    );
}

// ---------------------------------------------------------------------------
// AC3: kill -9 in the window between computing and revoking. The restart
// converges from the outbox without duplicating the effect receipt.
// ---------------------------------------------------------------------------

#[test]
fn a_crash_between_planning_and_revoking_converges_once() {
    let content = b"hsp137 crash window".to_vec();
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let deployment = a_deployment();
    let policy = risk_policy_file(&deployment, "1.0.0");
    let catalog = catalog_file(&deployment, &[(CRASH_ADVISORY, "critical")]);
    let mut service = deployment.start(&[
        ("HODEI_TRUST__POLICY_FILE", &policy),
        ("HODEI_TRUST__RISK_CATALOG_FILE", &catalog),
        ("HODEI_SECURITY__ADMIN_API_ENABLED", "true"),
        ("HODEI_TRUST__SWEEP_INTERVAL_SECS", "0"),
    ]);
    let path = raw("app/crash.bin");
    let digest = hex_of(&content);
    assert_eq!(
        put_raw(&deployment, service.port, &alice, &path, &content),
        201
    );
    assert_eq!(
        promote(&deployment, service.port, &alice, "app/crash.bin", &digest).0,
        200
    );
    assert_eq!(
        call(&deployment, service.port, "GET", &path, &alice, None).0,
        200
    );
    let port = service.port;
    support::kill(&mut service);

    // What the dead process left behind: the stated delta, the view
    // reaching the subject and the pending reevaluation intent, without
    // a receipt. It is the exact window that `kill -9` survives.
    let subject = subject_of(&content);
    plant_crash_window(&deployment, &subject);

    // The boot converges BEFORE opening to traffic: `resume` claimed
    // the pending intent, the reevaluation denied because of the critical and the
    // revocation reached the ledger.
    let mut service = deployment.start_on(
        port,
        &[
            ("HODEI_TRUST__POLICY_FILE", &policy),
            ("HODEI_TRUST__RISK_CATALOG_FILE", &catalog),
            ("HODEI_SECURITY__ADMIN_API_ENABLED", "true"),
            ("HODEI_TRUST__SWEEP_INTERVAL_SECS", "0"),
        ],
    );
    assert_eq!(
        call(&deployment, service.port, "GET", &path, &alice, None).0,
        404,
        "the boot did not converge the pending reevaluation"
    );
    support::stop(&mut service);

    // The effect receipt is ONE: the convergence did not touch twice.
    let key = format!("decision:{CRASH_ADVISORY}:{subject}");
    assert_eq!(
        effect_receipts(&deployment, &key),
        1,
        "the effect ran twice"
    );
    let rows = decisions(&deployment, &subject);
    assert_eq!(
        rows.iter().filter(|row| row.outcome == "DENY").count(),
        1,
        "the crash converged with more than one denial: {rows:?}"
    );
}
