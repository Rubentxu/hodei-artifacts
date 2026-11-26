//! The matrix of capabilities is executable, and a client of it fails
//! when the declaration is fraudulent.
//!
//! The registry is the single origin of what a format and a modality
//! mean in this release: [`RepositoryFormat::serves_modes`] says what is
//! served, the capabilities endpoint says it out loud for the console,
//! the creation of a repository refuses what the registry refuses, and
//! the conformance profile (`hodei_catalog::conformance`) names the
//! suites that accredit every served cell.
//!
//! What is measured here is that the three voices stay one voice:
//!
//! - what the capabilities endpoint offers is exactly what the registry
//!   serves, cell by cell, with nothing offered that cannot be served
//!   and nothing served that is not offered;
//! - a configuration that the registry does not support is refused in
//!   the creation with a dialect error that names the format and the
//!   modality, for every declared combination;
//! - the suites the conformance profile selects exist in the workspace
//!   and speak of the format — and of the modality, when the modality is
//!   a composed one — they accredit;
//! - a fraudulent declaration fails the client: cargo accredited by the
//!   generic proxy suite, a modality promised without evidence, a suite
//!   erased, evidence that does not speak of what it accredits.
//!
//! The selection is written to `target/uat/hsp142-capability-matrix.md`,
//! stamped with the version of the profile and the commit of the product
//! that ran it: which build accredits each cell is read, not remembered.
//! The report does not say the suites were executed here — the stages of
//! the pipeline execute them; it says they were selected and that the
//! declaration that selects them is coherent with the code on disk.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use cedar_policy::Entities;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use hodei_catalog::conformance::{
    Accreditation, AuditFailure, ConformanceCell, PROFILE_VERSION, accredits, audit, refuses,
    served_pairs,
};
use hodei_catalog::{RepositoryFormat, RepositoryMode};
use serde_json::{Value, json};
use tower::ServiceExt;

// The graph profile of the harness: a REAL Dgraph namespace per application.
#[path = "support/graph_profile.rs"]
mod graph_profile;
use graph_profile::Workdir;

/// The account of the person who creates repositories in this test.
const ACCOUNT: &str = "acme";

/// The credential verifier of the test.
///
/// The token is the name of the person: there is no secret here, and the
/// account comes from the token and not from a header.
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
        let (account, name) = match token {
            "alice" => (ACCOUNT, "alice"),
            _ => return Err(AuthenticationError::InvalidCredentials),
        };
        Ok(PrincipalClaims::new(
            PrincipalId::new(format!("hrn:hodei:iam::{account}:User/{name}")).unwrap(),
            AccountId::new(account).unwrap(),
        ))
    }
}

/// The policy of the test: alice may create and read repositories.
fn cedar() -> Arc<dyn AuthorizerPort> {
    let policy = r#"permit(
        principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
        action == Artifacts::Action::"CreateRepository",
        resource
    );
    permit(
        principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
        action == Artifacts::Action::"ReadRepository",
        resource
    );"#;
    Arc::new(SharedAuthorizer::new(Arc::new(
        CedarAuthorizer::from_policies_unchecked(
            policy,
            Entities::empty(),
            Arc::new(HrnCedarMapper),
        )
        .expect("authorizer"),
    )))
}

/// The application with a real catalog behind it.
async fn app_with(authorizer: Arc<dyn AuthorizerPort>) -> (axum::Router, Workdir) {
    let tmp = Workdir::new();
    let store = Arc::new(tmp.tenant().catalog_store().await);

    let mut config = AppConfig::default();
    config.security.admin_api_enabled = false;
    tmp.apply(&mut config);

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
    let state = state
        .with_authorization(authorizer)
        .with_repository_auth(
            Arc::new(TestAuthenticator),
            Arc::new(hodei_artifacts_api::repository::InMemoryRepositoryReader::empty()),
        )
        .with_catalog(store.clone(), store.clone())
        .with_catalog_search(store.clone())
        .with_trust_ledger(Arc::new(tmp.tenant().trust_store().await));

    (build_router(state, &config), tmp)
}

/// The application with the policy that creates repositories.
async fn app() -> (axum::Router, Workdir) {
    app_with(cedar()).await
}

/// A request with a credential.
fn request(method: &str, path: &str, token: &str, body: Option<Value>) -> Request<Body> {
    let builder = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {token}"));
    match body {
        Some(body) => builder
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .expect("a request"),
        None => builder.body(Body::empty()).expect("a request"),
    }
}

/// The body of an answer, read as JSON.
async fn the_body(response: axum::response::Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("the body");
    serde_json::from_slice(&bytes).expect("json")
}

/// The integration test targets of every member of the workspace, by
/// file stem, with the path of the file.
///
/// The members are read from the workspace manifest, not assumed: a
/// crate that joins the workspace brings its own test targets, and the
/// conformance profile names suites that may live in any of them.
fn test_targets() -> BTreeMap<String, PathBuf> {
    let mut found: BTreeMap<String, PathBuf> = BTreeMap::new();
    for member in workspace_members() {
        let Ok(entries) = fs::read_dir(member.join("tests")) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|extension| extension == "rs")
                && let Some(name) = path.file_stem()
            {
                found.insert(name.to_string_lossy().into_owned(), path);
            }
        }
    }
    found
}

/// The directories of the members of the workspace.
fn workspace_members() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest = fs::read_to_string(root.join("Cargo.toml"))
        .unwrap_or_else(|error| panic!("cannot read the workspace manifest: {error}"));

    let mut members = vec![root.clone()];
    let mut inside = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with("members") {
            inside = true;
            continue;
        }
        if inside {
            if line.starts_with(']') {
                break;
            }
            let member = line.trim().trim_end_matches(',').trim();
            let member = member.trim_matches('"');
            if !member.is_empty() && !member.starts_with('#') {
                members.push(root.join(member));
            }
        }
    }
    members
}

/// The commit of the product that runs this test.
///
/// The evidence of the matrix names the build that produced it: a cell
/// accredited without a version is a cell nobody can audit later. The
/// commit is read from git, best effort —a tree without git still runs,
/// and the report says so instead of lying.
fn the_commit() -> String {
    let answer = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output();
    match answer {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => "unknown (no git in the tree that ran this test)".to_string(),
    }
}

/// The path of the report of the selection.
fn the_report_path() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"))
        .join("uat")
        .join("hsp142-capability-matrix.md")
}

/// The report of the selection, in the language of the product.
fn the_report(cells: &[ConformanceCell], commit: &str) -> String {
    let mut report = String::new();
    report.push_str("# Matriz de capacidades ejecutable — informe de selección\n\n");
    report.push_str(&format!(
        "- Perfil del registro: `{PROFILE_VERSION}`\n- Commit del producto que ejecutó la selección: `{commit}`\n\n"
    ));
    report.push_str(
        "**Lo que este informe certifica y lo que no.** Certifica que la \
         declaración del registro es coherente consigo misma: cada celda \
         servida tiene suites que la acreditan, cada celda rechazada lo \
         está también en la configuración, y cada suite seleccionada \
         existe en el workspace y nombra al formato —y a la modalidad \
         compuesta— que acredita. **No** ejecuta aquí las suites de \
         conformance por formato (eso lo hacen los stages de la \
         pipeline), ni certifica clientes reales, versiones o red: la \
         ejecución de cada suite es su propia evidencia.\n\n",
    );
    report.push_str(
        "| Formato | Modalidad | Estado | Suites que acreditan | Suites que prueban el rechazo | Nota |\n",
    );
    report.push_str("|---|---|---|---|---|---|\n");
    for cell in cells {
        report.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            cell.format.as_str(),
            cell.mode.as_str(),
            cell.status.as_str(),
            if cell.evidence.is_empty() {
                "—".to_string()
            } else {
                cell.evidence.join(", ")
            },
            if cell.refusal_evidence.is_empty() {
                "—".to_string()
            } else {
                cell.refusal_evidence.join(", ")
            },
            if cell.note.is_empty() {
                "—"
            } else {
                cell.note
            },
        ));
    }
    report
}

/// What the capabilities endpoint offers, by format name.
async fn the_offered(app: &axum::Router) -> BTreeMap<String, Vec<String>> {
    let response = app
        .clone()
        .oneshot(request("GET", "/api/v1/capabilities", "alice", None))
        .await
        .expect("the response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = the_body(response).await;
    let mut offered = BTreeMap::new();
    for format in body["formats"].as_array().expect("the formats") {
        let name = format["name"].as_str().expect("the name").to_string();
        let modes = format["modes"]
            .as_array()
            .expect("the modes")
            .iter()
            .map(|mode| mode.as_str().expect("the mode").to_string())
            .collect();
        offered.insert(name, modes);
    }
    offered
}

/// What the release serves, by format name: the registry itself.
fn the_servable() -> BTreeMap<String, Vec<String>> {
    let mut servable = BTreeMap::new();
    for (format, mode) in served_pairs() {
        servable
            .entry(format.as_str().to_string())
            .or_insert_with(Vec::new)
            .push(mode.as_str().to_string());
    }
    servable
}

/// What the capabilities endpoint offers is exactly what the registry
/// serves: nothing offered that cannot be served, nothing served that is
/// not offered, and no format declared but unserved on the wire.
#[tokio::test]
async fn the_capabilities_offer_exactly_what_the_release_serves() {
    let (app, _tmp) = app().await;
    assert_eq!(
        the_offered(&app).await,
        the_servable(),
        "the wire and the registry are one voice"
    );
    for format in RepositoryFormat::declared() {
        if !RepositoryFormat::served().contains(format) {
            assert!(
                !the_servable().contains_key(format.as_str()),
                "a format the release does not serve is not servable: {}",
                format.as_str()
            );
        }
    }
}

/// A configuration the registry does not support is refused in the
/// creation with a dialect error that names the format and the
/// modality, for every declared combination — and the cargo proxy and
/// group the encargo names are among the refusals, with the helm
/// formats that are declared and not served.
#[tokio::test]
async fn every_unsupported_config_is_refused_with_a_dialect_error() {
    let (app, _tmp) = app().await;

    let mut refused = Vec::new();
    for &format in RepositoryFormat::declared() {
        for mode in [
            RepositoryMode::Hosted,
            RepositoryMode::Proxy,
            RepositoryMode::Group,
        ] {
            if format.supports(&mode) {
                continue;
            }
            let name = format!("reject-{}-{}", format.as_str(), mode.as_str());
            let response = app
                .clone()
                .oneshot(request(
                    "POST",
                    "/api/v1/catalog/repositories",
                    "alice",
                    Some(json!({
                        "name": name,
                        "format": format.as_str(),
                        "mode": mode.as_str(),
                    })),
                ))
                .await
                .expect("the response");
            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "the configuration {}/{name} is refused",
                format.as_str()
            );
            let body = the_body(response).await;
            let message = body["message"].as_str().unwrap_or_default().to_string();
            assert_eq!(
                body["error"], "invalid",
                "the refusal is a dialect error: {message}"
            );
            assert!(
                message.contains(format.as_str()) && message.contains(mode.as_str()),
                "the refusal names the format and the modality: {message}"
            );
            refused.push((format, mode));
        }
    }

    // The refusals cover every declared combination the release does not
    // serve, and not one more: the loop is the walk of the matrix, not a
    // sample of it.
    let expected: Vec<(RepositoryFormat, RepositoryMode)> = {
        let modes = [
            RepositoryMode::Hosted,
            RepositoryMode::Proxy,
            RepositoryMode::Group,
        ];
        let mut pairs: Vec<_> = RepositoryFormat::declared()
            .iter()
            .flat_map(|format| modes.iter().map(move |mode| (*format, *mode)))
            .filter(|(format, mode)| !format.supports(mode))
            .collect();
        pairs.sort_by_key(|(format, mode)| (format.as_str(), mode.as_str()));
        pairs
    };
    let mut actual = refused;
    actual.sort_by_key(|(format, mode)| (format.as_str(), mode.as_str()));
    assert_eq!(actual, expected, "every declared combination is walked");

    // Cargo proxy and group are refused until a client-verified upstream
    // exists, and the registry says so with its own note.
    for mode in [RepositoryMode::Proxy, RepositoryMode::Group] {
        let cargo_proxy = hodei_catalog::conformance_cell(RepositoryFormat::Cargo, mode);
        assert_eq!(
            cargo_proxy.status,
            Accreditation::NotSupported,
            "cargo {mode} is not supported until HSP-156 verifies it against a client"
        );
        assert!(
            !cargo_proxy.note.is_empty(),
            "the refusal of cargo {} carries its limit",
            mode.as_str()
        );
    }

    // And one combination that the registry serves is accepted: the loop
    // is not a walk that refuses everything.
    let response = app
        .oneshot(request(
            "POST",
            "/api/v1/catalog/repositories",
            "alice",
            Some(json!({ "name": "accepted", "format": "raw", "mode": "hosted" })),
        ))
        .await
        .expect("the response");
    assert_eq!(response.status(), StatusCode::CREATED);
}

/// The conformance profile selects the suites that accredit every cell,
/// and every suite it selects exists in the workspace and speaks of the
/// format — and of the composed modality — it accredits. The selection
/// is written to `target/uat/`, stamped with the version of the profile
/// and the commit of the product.
#[test]
fn the_matrix_selects_the_suites_that_accredit_each_cell() {
    let matrix = hodei_catalog::conformance_matrix();
    audit(&matrix, RepositoryFormat::declared(), &served_pairs())
        .expect("the registry is coherent: every served cell is accredited and every accreditation is served");

    let targets = test_targets();
    let mut selected = 0;
    let mut failures = Vec::new();
    for cell in &matrix {
        for stem in cell.suites() {
            selected += 1;
            let Some(path) = targets.get(stem) else {
                failures.push(format!(
                    "the cell {}/{} selects `{stem}`, which is no test target of this workspace",
                    cell.format.as_str(),
                    cell.mode.as_str()
                ));
                continue;
            };
            let content =
                fs::read_to_string(path).unwrap_or_else(|error| panic!("read {path:?}: {error}"));
            let answer = match cell.status {
                Accreditation::Accredited => {
                    accredits(cell, stem, &content).map_err(|failure| failure.detail)
                }
                Accreditation::NotSupported => {
                    refuses(cell, stem, &content).map_err(|failure| failure.detail)
                }
            };
            if let Err(detail) = answer {
                failures.push(format!(
                    "the cell {}/{} names `{stem}` and the suite does not speak of it: {detail}",
                    cell.format.as_str(),
                    cell.mode.as_str()
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "the declaration of the registry does not hold against the code on disk:\n  - {}",
        failures.join("\n  - ")
    );
    assert!(
        selected >= 30,
        "the profile names {selected} suites; the matrix that was accredited named more, \
         and an evidence table that shrinks is an evidence table that was gutted"
    );

    let nuget_group =
        hodei_catalog::conformance_cell(RepositoryFormat::NuGet, RepositoryMode::Group);
    assert_eq!(
        nuget_group.evidence,
        &["hsp231_nuget_group"],
        "nuget group is accredited by its group conformance suite"
    );

    let commit = the_commit();
    let report = the_report(&matrix, &commit);
    let path = the_report_path();
    fs::create_dir_all(path.parent().expect("the report has a directory")).expect("the directory");
    fs::write(&path, report).expect("the report is written");
}

/// A fraudulent declaration fails the client, and the client is told
/// why.
///
/// The four frauds are the ones the matrix has to survive: a cell
/// accredited outside the service, a modality promised without
/// evidence, an accreditation whose suite was erased, and —the one the
/// encargo names— cargo hosted accredited by the generic proxy suite,
/// whose text this test reads from the workspace to be sure the real
/// content fails the real rule.
#[test]
fn a_fraudulent_declaration_fails_the_client() {
    // A cell accredited for a modality the release does not serve.
    let mut matrix = hodei_catalog::conformance_matrix();
    for cell in matrix.iter_mut() {
        if cell.format == RepositoryFormat::Cargo && cell.mode == RepositoryMode::Proxy {
            cell.status = Accreditation::Accredited;
            cell.evidence = &["hsp106_proxy_e2e"];
            cell.note = "";
        }
    }
    let failure = audit(&matrix, RepositoryFormat::declared(), &served_pairs())
        .expect_err("cargo proxy is not served: the accreditation is a fraud");
    assert!(
        failure.to_string().contains("cargo") && failure.to_string().contains("proxy"),
        "the explanation names the cell: {failure}"
    );

    // A modality the declaration promises without evidence.
    let mut served = served_pairs();
    served.push((RepositoryFormat::Cargo, RepositoryMode::Proxy));
    let failure = audit(
        &hodei_catalog::conformance_matrix(),
        RepositoryFormat::declared(),
        &served,
    )
    .expect_err("no suite accredits the promise of cargo proxy");
    assert_eq!(
        failure,
        AuditFailure::ServedWithoutAccreditation {
            format: "cargo",
            mode: "proxy"
        },
        "the promise is refused with the rule it broke"
    );

    // An accreditation whose suite was erased.
    let mut matrix = hodei_catalog::conformance_matrix();
    for cell in matrix.iter_mut() {
        if cell.format == RepositoryFormat::Cargo && cell.mode == RepositoryMode::Hosted {
            cell.evidence = &[];
        }
    }
    let failure = audit(&matrix, RepositoryFormat::declared(), &served_pairs())
        .expect_err("cargo hosted is accredited without a single suite");
    assert!(
        failure.to_string().contains("cargo"),
        "the explanation names the cell whose proof was erased: {failure}"
    );

    // Cargo hosted accredited by the generic proxy suite: the shape is
    // right, the names are right, and the suite does not speak of
    // cargo. The content is the real content of the real suite.
    let targets = test_targets();
    let stem = "hsp106_proxy_e2e";
    let path = targets
        .get(stem)
        .unwrap_or_else(|| panic!("the suite {stem} exists in this workspace"));
    let content = fs::read_to_string(path).expect("the content of the suite");
    let fraudulent = ConformanceCell {
        evidence: &["hsp106_proxy_e2e"],
        ..hodei_catalog::conformance_cell(RepositoryFormat::Cargo, RepositoryMode::Hosted)
    };
    let mismatch = accredits(&fraudulent, stem, &content)
        .expect_err("the generic proxy suite does not speak of cargo");
    assert!(
        mismatch.detail.contains("cargo"),
        "the explanation names the format: {mismatch}"
    );

    // And the honest cell passes the same rule with the same content
    // rule applied where it belongs: the npm proxy suite speaks of npm
    // and of the proxy it exercises.
    let npm_proxy = hodei_catalog::conformance_cell(RepositoryFormat::Npm, RepositoryMode::Proxy);
    let stem = "hsp111_npm_cli";
    let path = targets
        .get(stem)
        .unwrap_or_else(|| panic!("the suite {stem} exists in this workspace"));
    let content = fs::read_to_string(path).expect("the content of the suite");
    accredits(&npm_proxy, stem, &content)
        .expect("the npm proxy suite accredits the npm proxy cell");
}
