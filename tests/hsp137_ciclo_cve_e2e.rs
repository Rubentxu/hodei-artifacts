//! HSP-137: el ciclo CVE a distribución cerrado sobre el servicio real.
//!
//! Lo que se prueba aquí es el cableado del hito completo, no las piezas:
//! un advisory entra por la entrada soportada (`POST
//! /api/v1/advisories/accept`), el motor dice los intents de reevaluación
//! contra el grafo real, y la distribución **bloquea el pull del artefacto
//! concedido dentro de la misma llamada**, porque la reevaluación retira la
//! concesión antes de responder. No hay consumidor externo que pueda estar
//! caido: el ciclo corre en el proceso que sirve los bytes.
//!
//! Los casos siguen la UAT TRUST-04:
//!
//! * AC1: artefacto promovido -> advisory crítico entra -> pull nuevo 404,
//!   y la admision->bloqueo se **mide**, no se declara;
//! * AC2: un advisory irrelevante (severidad `other`) no retira la
//!   concesion ajena, y el cambio de politica reevalua los sujetos
//!   dependientes (barrido de drift tras reinicio con version 2.0.0);
//! * AC3: `kill -9` en la ventana entre calcular y revocar (el outbox
//!   duradero guarda la mitad sobreviviente) -> el reinicio converge por
//!   `resume` sin duplicar el effect receipt.
//!
//! Limite honesto, como en `hsp104_gc_e2e.rs`: el corte exacto dentro del
//! handler no se puede provocar desde fuera sin un gancho de pruebas en el
//! codigo de produccion. La ventana se reproduce escribiendo en el outbox
//! lo que el proceso habria dicho antes de morir -- es el mismo estado
//! durable que el `kill -9` deja, y `resume` no distingue quien lo escribio.

mod support;

use std::time::Instant;

use hodei_trust_policy::{Requirement, RequirementKind, TrustPolicy};
use support::{Curl, Deployment, FIRST_ACCOUNT, FIRST_REPOSITORY};

const REGION: &str = "eu";

/// El tenant del grafo: la plantilla por defecto del despliegue.
const TENANT: &str = "hrn:hodei:artifacts:eu:acme:artifact/acme";

/// El advisory critico de AC1/AC2, nombrado como lo nombra la proyeccion.
const CRITICAL_ADVISORY: &str = "hrn:hodei:artifacts:eu:acme:Vulnerability/GHSA-hsp137-0001";

/// El advisory irrelevante (severidad `other`) de AC2.
const IRRELEVANT_ADVISORY: &str = "hrn:hodei:artifacts:eu:acme:Vulnerability/GHSA-hsp137-other";

/// El advisory de la ventana de crash (AC3).
const CRASH_ADVISORY: &str = "hrn:hodei:artifacts:eu:acme:Vulnerability/GHSA-hsp137-crash";

/// El inventario CycloneDX 1.7 del artefacto: `web` sujeta a `lib-a`, que
/// depende de `lib-b` y `lib-c`. El advisory de prueba casa por nombre con
/// `lib-a`, y el radio de impacto sube de ahi al artefacto proyectado.
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

/// El documento OSV del advisory que casa con `lib-a`.
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
// Arnés
// ---------------------------------------------------------------------------

/// El esquema con la superficie administrativa (gate Cedar del accept).
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

/// La politica Cedar: la superficie de contenido de siempre, mas la
/// administracion abierta para alice (la resource del accept la cubre).
fn the_policy() -> String {
    format!(
        "{}\npermit(principal == Iam::User::\"hrn:hodei:iam::{FIRST_ACCOUNT}:User/alice\", action == Admin::Action::\"AccessAdminApi\", resource);\n",
        support::the_policy()
    )
}

/// La politica de confianza: solo la ausencia de criticos alcanzantes.
/// `version` es lo que el drift del barrido compara con la concesion.
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

/// El catalogo de severidades del operador.
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

/// La ruta de referencia de la vista: sin ninguna ruta materializada el
/// lector dice "sin vista no hay actualidad" (el propio `is_current` de
/// HSP-134), y el cero de la promocion no se verifica. La ruta no alcanza
/// a ningun sujeto -- no tiene filas en `impact_view` -- y su marca de
/// agua esta por encima de cualquier version que el grafo de la prueba va
/// a decir, de modo que la actualidad del tenant depende solo de las
/// rutas de advisory que el ciclo reconstruye.
const BASELINE_ROUTE: &str = "hrn:hodei:artifacts:eu:acme:Vulnerability/GHSA-hsp137-baseline";

/// Un despliegue con el catalogo sembrado, el canal trusted abierto, la
/// administracion habilitada y la vista en cero verificado.
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

/// Escribe la ruta de referencia antes de levantar el proceso, por los
/// almacenes reales del grafo: una ruta materializada sin sujetos alcanzados,
/// a la versión actual del registro, que es lo que hace a la vista
/// "al día" (un cero verificado) antes de que entre ningún advisory.
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

/// La entrada soportada de advisories.
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

/// El sujeto del ciclo: el HRN del artefacto por digest, dicho igual que
/// lo dicen el handler y la promocion.
fn subject_of(content: &[u8]) -> String {
    let digest = hodei_evidence::ContentDigest::of_bytes(content);
    hodei_artifacts_api::trust::hrn::artifact_hrn(REGION, FIRST_ACCOUNT, FIRST_REPOSITORY, &digest)
        .to_string()
}

fn hex_of(content: &[u8]) -> String {
    hodei_evidence::ContentDigest::of_bytes(content).value
}

// ---------------------------------------------------------------------------
// Cirugia del grafo (la ventana de crash y la lectura de la evidencia)
// ---------------------------------------------------------------------------

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime")
}

/// Siembra la mitad sobreviviente del crash: el crítico alcanzando al sujeto
/// en el registro y la vista (con severidad conocida) y el intent de
/// reevaluación PENDIENTE en el outbox duradero -- exactamente lo que queda
/// vivo cuando el proceso muere después de planificar y antes de revocar.
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
        // El intent dicho por el planificador antes de morir. La identidad y
        // la clave son las MISMAS que el plan del crate habría escrito:
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

/// La evidencia persistida que la prueba lee con el servicio ya parado.
#[derive(Debug)]
struct DecisionRow {
    subject: String,
    outcome: String,
    policy_version: String,
}

/// Las decisiones del sujeto, de la más antigua a la más reciente.
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

/// Cuántas veces se aplicó el efecto de esa clave.
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

/// El seeder del catalogo.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

// ---------------------------------------------------------------------------
// AC1 (UAT TRUST-04): advisory critico -> el pull nuevo queda bloqueado,
// y el tiempo admision->bloqueo se mide.
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

    // La admision corre el ciclo completo: proyeccion, vista, intents,
    // reevaluacion, revocacion. Cuando responde, la concesion ya esta
    // retirada; el pull que la media es el primer pull BLOQUEADO.
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

    // SLO: el objetivo declarado es p99 <= 10 s en un nodo. Aqui se mide
    // un valor, no se declara el cumplimiento de una distribucion.
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

    // La readmision del mismo advisory es el mismo hecho dicho dos veces:
    // el ciclo lo dice en lugar de fingir otra vez, y no abre nada.
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

    // La decision persistida: el sujeto promovido y luego negado por el
    // critico alcanzante, en el mismo orden que el wire conto.
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
// AC2: un CVE irrelevante no retira concesiones ajenas.
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

    // El advisory de severidad `other` toca mi artefacto (el inventario
    // casa con lib-a): la reevaluacion ocurre y AUTORIZA -- la politica
    // solo niega criticos. El artefacto ajeno ni siquiera entra en el
    // radio: no se reevalua lo que el advisory no alcanza.
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

    // La evidencia: mi sujeto fue reevaluado (dos ALLOW: promocion y
    // ciclo) y el ajeno conserva UNA sola decision -- nadie lo toco.
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
// AC2 (derivada): el cambio de politica reevalua los sujetos dependientes.
// El barrido de drift detecta la version activa nueva tras el reinicio.
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

    // El operador sube la politica a 2.0.0 y reinicia. La concesion
    // escrita dice 1.0.0: el barrido (intervalo 1 s) la ve obsoleta y
    // reevalua el sujeto contra la version activa.
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
    // La distribucion sigue sirviendo: la reevaluacion AUTORIZA contra la
    // politica nueva (el riesgo sigue siendo cero verificado).
    assert_eq!(
        call(&deployment, service.port, "GET", &path, &alice, None).0,
        200,
        "the drift reevaluation denied what it should have kept"
    );
    support::stop(&mut service);

    // La evidencia: una decision nueva con la version 2.0.0, emitida por
    // el ciclo (actor reactor) y no por un cliente.
    let rows = decisions(&deployment, &subject_of(&content));
    let versions: Vec<&str> = rows.iter().map(|row| row.policy_version.as_str()).collect();
    assert_eq!(
        versions,
        vec!["1.0.0", "2.0.0"],
        "the policy change did not reevaluate the subject it concerns: {rows:?}"
    );
}

// ---------------------------------------------------------------------------
// AC3: kill -9 en la ventana entre calcular y revocar. El reinicio
// converge desde el outbox sin duplicar el effect receipt.
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

    // Lo que el proceso muerto dejo atras: el delta dicho, la vista
    // alcanzando al sujeto y el intent de reevaluacion pendiente, sin
    // recibo. Es la ventana exacta que el `kill -9` sobrevive.
    let subject = subject_of(&content);
    plant_crash_window(&deployment, &subject);

    // El arranque converge ANTES de abrir al trafico: `resume` reclamo
    // el intent pendiente, la reevaluacion nego por el critico y la
    // revocacion llego al libro mayor.
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

    // El recibo del efecto es UNO: la convergencia no toco dos veces.
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
