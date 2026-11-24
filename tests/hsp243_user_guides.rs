//! Verification of HSP-243 user guides, links, capabilities matrix, and
//! executable recipes for Maven 2, npm, and PyPI against the real Hodei service.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use base64::Engine;
use cedar_policy::Entities;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use hodei_catalog::{RepositoryFormat, RepositoryMode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256, Sha512};
use tower::ServiceExt;

const ACCOUNT: &str = "acme";

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

fn cedar_authorizer() -> Arc<dyn AuthorizerPort> {
    let policy = r#"
        permit(
            principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
            action == Artifacts::Action::"CreateRepository",
            resource
        );
        permit(
            principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
            action == Artifacts::Action::"ReadRepository",
            resource
        );
        permit(
            principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
            action == Artifacts::Action::"UploadAsset",
            resource
        );
        permit(
            principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
            action == Artifacts::Action::"DownloadAsset",
            resource
        );
    "#;
    Arc::new(SharedAuthorizer::new(Arc::new(
        CedarAuthorizer::from_policies_unchecked(
            policy,
            Entities::empty(),
            Arc::new(HrnCedarMapper),
        )
        .expect("cedar authorizer"),
    )))
}

async fn real_app() -> (axum::Router, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut config = AppConfig::default();
    config.security.admin_api_enabled = false;
    config.security.raw_api_enabled = true;
    config.repository.durable = true;
    config.rocksdb.path = tmp.path().join("db.rocksdb").to_string_lossy().into_owned();
    config.content.cas_path = tmp.path().join("cas").to_string_lossy().into_owned();

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
        .with_authorization(cedar_authorizer())
        .with_authenticator(Arc::new(TestAuthenticator));

    (build_router(state, &config), tmp)
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn extract_markdown_links(content: &str) -> Vec<String> {
    let mut links = Vec::new();
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b']'
            && i + 1 < bytes.len()
            && bytes[i + 1] == b'('
            && let Some(end) = content[i + 2..].find(')')
        {
            let target = content[i + 2..i + 2 + end].trim();
            if !target.is_empty()
                && !target.starts_with("http://")
                && !target.starts_with("https://")
                && !target.starts_with('#')
            {
                let path_only = target.split('#').next().unwrap_or("");
                if !path_only.is_empty() {
                    links.push(path_only.to_string());
                }
            }
            i += 2 + end + 1;
            continue;
        }
        i += 1;
    }
    links
}

fn parse_workspace_members(cargo_toml: &str) -> BTreeSet<String> {
    let mut members = BTreeSet::new();
    let mut in_members = false;
    for line in cargo_toml.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("members = [") {
            in_members = true;
            continue;
        }
        if in_members {
            if trimmed == "]" {
                break;
            }
            if let Some(stripped) = trimmed.strip_prefix('"')
                && let Some(end) = stripped.find('"')
            {
                members.insert(stripped[..end].to_string());
            }
        }
    }
    members
}

#[test]
fn documentation_links_and_workspace_crates_are_valid() {
    let root = repo_root();
    let mut doc_files = vec![root.join("README.md"), root.join("README.es.md")];
    for entry in fs::read_dir(root.join("docs/community")).expect("docs/community") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|e| e.to_str()) == Some("md") {
            doc_files.push(path);
        }
    }

    for doc in &doc_files {
        let content = fs::read_to_string(doc).expect("read doc");
        let base_dir = doc.parent().expect("parent");
        for link in extract_markdown_links(&content) {
            let resolved = base_dir.join(&link);
            assert!(
                resolved.exists(),
                "broken markdown link in {}: '{}' -> {}",
                doc.display(),
                link,
                resolved.display()
            );
        }
    }

    let cargo_toml = fs::read_to_string(root.join("Cargo.toml")).expect("Cargo.toml");
    let members = parse_workspace_members(&cargo_toml);
    assert!(
        members.len() >= 20,
        "expected full workspace members list, got {}",
        members.len()
    );

    for readme_name in ["README.md", "README.es.md"] {
        let text = fs::read_to_string(root.join(readme_name)).expect(readme_name);
        for member in &members {
            let needle = if member == "." {
                "`hodei-artifacts-api`"
            } else {
                member.as_str()
            };
            assert!(
                text.contains(needle),
                "{readme_name} is missing workspace member '{member}'"
            );
        }
    }
}

#[test]
fn capabilities_table_matches_catalog_and_conformance() {
    let root = repo_root();
    let docs = [
        ("README.md", "Yes", "No"),
        ("README.es.md", "Sí", "No"),
        ("docs/community/ecosystems.md", "Sí", "No"),
    ];

    for (rel_path, yes, no) in docs {
        let content = fs::read_to_string(root.join(rel_path)).expect(rel_path);
        for format in RepositoryFormat::served() {
            let modes = format.serves_modes();
            let hosted = if modes.contains(&RepositoryMode::Hosted) {
                yes
            } else {
                no
            };
            let proxy = if modes.contains(&RepositoryMode::Proxy) {
                yes
            } else {
                no
            };
            let group = if modes.contains(&RepositoryMode::Group) {
                yes
            } else {
                no
            };
            let expected_prefix =
                format!("| `{}` | {hosted} | {proxy} | {group} |", format.as_str());
            assert!(
                content.contains(&expected_prefix),
                "{rel_path} capabilities row mismatch for '{}': expected '{expected_prefix}'",
                format.as_str()
            );
        }
        let helm_row = format!("| `helm` | {no} | {no} | {no} |");
        assert!(
            content.contains(&helm_row),
            "{rel_path} must declare helm as not served: '{helm_row}'"
        );
    }
}

#[test]
fn guides_have_required_metadata_and_no_insecure_production_advice() {
    let root = repo_root();
    let ecosystems =
        fs::read_to_string(root.join("docs/community/ecosystems.md")).expect("ecosystems.md");

    let required_sections = [
        "### Maven 2 (`maven2`)",
        "### npm (`npm`)",
        "### PyPI (`pypi`)",
        "### OCI (`oci`)",
        "### Cargo (`cargo`)",
        "### NuGet (`nuget`)",
        "### Go (`go`)",
        "### APT (`apt`)",
        "### RPM / Yum (`rpm`)",
        "### Helm-OCI (`oci` + `helm`)",
    ];

    for (idx, heading) in required_sections.iter().enumerate() {
        let start = ecosystems
            .find(heading)
            .unwrap_or_else(|| panic!("missing section '{heading}'"));
        let end = required_sections
            .get(idx + 1)
            .and_then(|next| ecosystems[start..].find(next).map(|o| start + o))
            .unwrap_or(ecosystems.len());
        let section = &ecosystems[start..end];

        assert!(
            section.contains("- **Estado:** `VERIFIED`")
                || section.contains("- **Estado:** `NOT_RUN`"),
            "section '{heading}' must declare Estado VERIFIED or NOT_RUN"
        );
        assert!(
            section.contains("- **Versión de cliente:**"),
            "section '{heading}' must declare Versión de cliente"
        );
        assert!(
            section.contains("- **Política de upstream:**")
                && section.contains("proxy.allowed_hosts")
                && section.contains("settings.upstream"),
            "section '{heading}' must declare Política de upstream with proxy.allowed_hosts and settings.upstream"
        );
        assert!(
            section.contains("**Ejemplo offline"),
            "section '{heading}' must include an offline example"
        );
    }

    // Ensure no production HTTP-without-TLS or signature-disable advice exists.
    for rel_path in [
        "README.md",
        "README.es.md",
        "docs/community/quickstart.md",
        "docs/community/ecosystems.md",
    ] {
        let text = fs::read_to_string(root.join(rel_path)).expect(rel_path);
        for line in text.lines() {
            let lower = line.to_lowercase();
            // Any mention of insecure flags must be an explicit prohibition ("nunca" / "never" / "no uses").
            for forbidden in [
                "insecure-registries",
                "strict-ssl=false",
                "[trusted=yes]",
                "gpgcheck=0",
                "sslverify=0",
                "--insecure",
            ] {
                if lower.contains(forbidden) {
                    assert!(
                        lower.contains("nunca")
                            || lower.contains("never")
                            || lower.contains("no uses"),
                        "{rel_path} contains '{forbidden}' without explicit prohibition: {line}"
                    );
                }
            }
            if lower.contains("http://") {
                assert!(
                    lower.contains("127.0.0.1")
                        || lower.contains("localhost")
                        || lower.contains("ejemplo")
                        || lower.contains("example")
                        || lower.contains("local")
                        || lower.contains("https://"),
                    "{rel_path} contains non-local http:// URL: {line}"
                );
            }
        }
    }

    // Verify backup/restore and upgrade operations are linked.
    for rel_path in ["README.md", "README.es.md", "docs/community/ecosystems.md"] {
        let text = fs::read_to_string(root.join(rel_path)).expect(rel_path);
        assert!(
            text.contains("UAT-OPERATIONS.md") && text.contains("hodei-recovery"),
            "{rel_path} must link backup/restore operations (UAT-OPERATIONS.md / hodei-recovery)"
        );
        assert!(
            text.contains("package-release.sh"),
            "{rel_path} must link upgrade/release packaging (package-release.sh)"
        );
    }
}

async fn send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    content_type: Option<&str>,
    body: Vec<u8>,
) -> (StatusCode, Vec<u8>) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:8080")
        .header("authorization", "Bearer alice");
    if let Some(ct) = content_type {
        builder = builder.header("content-type", ct);
    }
    let req = builder.body(Body::from(body)).expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 10 * 1024 * 1024)
        .await
        .expect("body bytes")
        .to_vec();
    (status, bytes)
}

#[tokio::test]
async fn documented_maven_npm_pypi_recipes_work_against_real_service() {
    let (app, _tmp) = real_app().await;

    // 0. Live /api/v1/capabilities matches RepositoryFormat::served() & serves_modes().
    let (status, cap_bytes) = send(&app, "GET", "/api/v1/capabilities", None, Vec::new()).await;
    assert_eq!(status, StatusCode::OK);
    let cap_json: Value = serde_json::from_slice(&cap_bytes).expect("capabilities json");
    let cap_formats = cap_json["formats"].as_array().expect("formats array");
    assert_eq!(cap_formats.len(), RepositoryFormat::served().len());
    for (idx, format) in RepositoryFormat::served().iter().enumerate() {
        assert_eq!(cap_formats[idx]["name"], format.as_str());
        let expected_modes: Vec<&str> = format.serves_modes().iter().map(|m| m.as_str()).collect();
        let actual_modes: Vec<&str> = cap_formats[idx]["modes"]
            .as_array()
            .expect("modes")
            .iter()
            .map(|v| v.as_str().expect("mode str"))
            .collect();
        assert_eq!(actual_modes, expected_modes);
    }

    // 1. Maven 2 recipe (doc-maven)
    let (status, body) = send(
        &app,
        "POST",
        "/api/v1/catalog/repositories",
        Some("application/json"),
        json!({
            "name": "doc-maven",
            "format": "maven2",
            "mode": "hosted",
            "settings": {"maven": {"version_policy": "release"}}
        })
        .to_string()
        .into_bytes(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );

    let pom = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<project><modelVersion>4.0.0</modelVersion><groupId>com.acme</groupId><artifactId>demo</artifactId><version>1.0.0</version></project>\n";
    let (status, body) = send(
        &app,
        "PUT",
        "/repository/doc-maven/com/acme/demo/1.0.0/demo-1.0.0.pom",
        Some("application/xml"),
        pom.to_vec(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );

    let jar = b"PK\x03\x04demo-jar-content";
    let (status, body) = send(
        &app,
        "PUT",
        "/repository/doc-maven/com/acme/demo/1.0.0/demo-1.0.0.jar",
        Some("application/java-archive"),
        jar.to_vec(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );

    let (status, downloaded_jar) = send(
        &app,
        "GET",
        "/repository/doc-maven/com/acme/demo/1.0.0/demo-1.0.0.jar",
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(downloaded_jar, jar);

    // 2. npm recipe (doc-npm)
    let (status, body) = send(
        &app,
        "POST",
        "/api/v1/catalog/repositories",
        Some("application/json"),
        json!({"name": "doc-npm", "format": "npm", "mode": "hosted"})
            .to_string()
            .into_bytes(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );

    let tarball = b"demo-npm-tarball-bytes";
    let integrity = format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(Sha512::digest(tarball))
    );
    let npm_payload = json!({
        "_id": "@acme/demo",
        "name": "@acme/demo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "@acme/demo",
                "version": "1.0.0",
                "_id": "@acme/demo@1.0.0",
                "dist": { "integrity": integrity }
            }
        },
        "_attachments": {
            "@acme/demo-1.0.0.tgz": {
                "content_type": "application/octet-stream",
                "data": base64::engine::general_purpose::STANDARD.encode(tarball),
                "length": tarball.len(),
            }
        }
    });
    let (status, body) = send(
        &app,
        "PUT",
        "/npm/doc-npm/@acme%2fdemo",
        Some("application/json"),
        npm_payload.to_string().into_bytes(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );

    let (status, packument) =
        send(&app, "GET", "/npm/doc-npm/@acme%2fdemo", None, Vec::new()).await;
    assert_eq!(status, StatusCode::OK);
    let packument_json: Value = serde_json::from_slice(&packument).expect("packument");
    assert_eq!(packument_json["dist-tags"]["latest"], "1.0.0");

    let (status, downloaded_tgz) = send(
        &app,
        "GET",
        "/npm/doc-npm/@acme/demo/-/demo-1.0.0.tgz",
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(downloaded_tgz, tarball);

    // 3. PyPI recipe (doc-pypi)
    let (status, body) = send(
        &app,
        "POST",
        "/api/v1/catalog/repositories",
        Some("application/json"),
        json!({"name": "doc-pypi", "format": "pypi", "mode": "hosted"})
            .to_string()
            .into_bytes(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );

    let wheel = b"PK\x03\x04demo-wheel-bytes";
    let sha256_hex: String = Sha256::digest(wheel)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let boundary = "----hsp243boundary";
    let mut multipart = Vec::new();
    for (field, value) in [
        (":action", "file_upload"),
        ("protocol_version", "1"),
        ("name", "demo-pkg"),
        ("version", "1.0.0"),
        ("metadata_version", "2.1"),
        ("sha256_digest", sha256_hex.as_str()),
    ] {
        multipart.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{field}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    multipart.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"content\"; filename=\"demo_pkg-1.0.0-py3-none-any.whl\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    multipart.extend_from_slice(wheel);
    multipart.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let (status, body) = send(
        &app,
        "POST",
        "/pypi/doc-pypi/",
        Some(&format!("multipart/form-data; boundary={boundary}")),
        multipart,
    )
    .await;
    assert!(
        status == StatusCode::OK || status == StatusCode::CREATED,
        "pypi upload failed with {status}: {}",
        String::from_utf8_lossy(&body)
    );

    let (status, simple_html) = send(
        &app,
        "GET",
        "/pypi/doc-pypi/simple/demo-pkg/",
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let html_str = String::from_utf8_lossy(&simple_html);
    assert!(
        html_str.contains("demo_pkg-1.0.0-py3-none-any.whl"),
        "simple index missing wheel: {html_str}"
    );
}

#[test]
fn readme_headers_link_the_visual_demo_and_its_generator() {
    let root = repo_root();
    let asset = "docs/community/quickstart-demo.gif";
    for readme_name in ["README.md", "README.es.md"] {
        let text = fs::read_to_string(root.join(readme_name)).expect(readme_name);
        let header = text.split("\n## ").next().expect("header");
        assert!(
            header.contains(&format!("]({asset})")),
            "{readme_name} must link the demo asset in its header"
        );
        assert!(
            text.contains("scripts/render-demo.sh"),
            "{readme_name} must point to the demo generator"
        );
    }
    for rel in [asset, "docs/community/quickstart-demo.cast", "scripts/render-demo.sh"] {
        let meta = fs::metadata(root.join(rel)).unwrap_or_else(|_| panic!("{rel} must exist"));
        assert!(meta.len() > 0, "{rel} must not be empty");
    }
    let gif = fs::read(root.join(asset)).expect(asset);
    assert!(gif.starts_with(b"GIF8"), "{asset} must be a GIF");
    assert!(
        !root.join("docs/community/quickstart-demo.tape").exists(),
        "the obsolete VHS tape must not coexist with render-demo.sh"
    );
}
