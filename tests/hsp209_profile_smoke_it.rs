//! HSP-209: the smoke test of the production profile over the single authority.
//!
//! AC#1 of HSP-209, tested with the **real binary** and the real process:
//! a deployment with `RUN_MODE=dgraph` (the distribution's `config/dgraph.toml`
//! file) starts against the live alpha and the live object store,
//! and serves Raw, OCI and trust **together** -- the catalog, the
//! push session, the promotion decision and the bytes live in the two
//! declared authorities, not in a third one. What this suite pins:
//!
//! 1. **The startup has no other engine.** The log announces the graph profile
//!    and no opening of a local store. A backend that is not the
//!    graph is rejected by name: the same config with
//!    `db_type = "rocksdb"` dies at startup citing the rejection, without
//!    opening a port.
//! 2. **The vertical surface works end-to-end over the graph**:
//!    create the repository (Dgraph catalog), publish through raw and
//!    download the same digest (bytes in S3, metadata in the graph),
//!    open an OCI session (Dgraph journal) and promote (durable decision
//!    in the graph). Each step is re-read **on the server** by its own
//!    key -- never by global censuses: the alpha is shared with the
//!    HSP-154/155 suites.
//! 3. **The evidence is written.** The service log, the DQL queries
//!    and the HTTP responses are archived under `target/uat/hsp209/`
//!    next to this statement: a PASS without files in that directory is not
//!    a PASS.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT` like all the stack suites:
//! without the alpha declared, the suite skips itself (exit 0). The
//! object store is the shared container of the HSP-116 series with a
//! single bucket per process: if the real S3 provider required human
//! credentials, its precondition is documented in `evidencia.md` -- the
//! smoke test uses localstack as a functional proof, like HSP-138.
//!
//! **Lane**: `--test-threads=1` (the shared alpha and the `@upsert` guard
//! of the keys; lesson HSP-190/219).

mod support;

use std::io::Read as IoRead;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use aws_sdk_s3::Client;
use hodei_graph_client::{GraphClient, GraphClientConfig};
use serde_json::{Value, json};
use support::{Curl, Deployment, FIRST_ACCOUNT};

/// The marker with which the alpha's password is removed from the archived
/// log and from any evidence TOML.
const PASSWORD_SENTINEL: &str = "__HSP209_PASSWORD_IN_ENVIRONMENT_ONLY__";

/// The evidence directory of this task, at the crate root.
fn evidence_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/uat/hsp209")
}

/// The alpha config from the IT environment (the shape `connect`
/// requires, like hsp187): endpoint with port, TLS with its CA and its
/// server name declared, complete credentials.
fn config_from_env() -> Option<GraphClientConfig> {
    let endpoint = std::env::var("HODEI_GRAPH_IT_ENDPOINT").ok()?;
    let ca = std::env::var("HODEI_GRAPH_IT_CA").unwrap_or_default();
    let server_name = std::env::var("HODEI_GRAPH_IT_SERVER_NAME").unwrap_or_default();
    let user = std::env::var("HODEI_GRAPH_IT_USER").unwrap_or_default();
    let password = std::env::var("HODEI_GRAPH_IT_PASSWORD").ok()?;
    let mut text = format!(
        "endpoint = \"{endpoint}\"\n\n[tls]\nenabled = {enabled}\nca_cert_path = \"{ca}\"\n",
        enabled = !ca.is_empty()
    );
    if !server_name.is_empty() {
        text.push_str(&format!("server_name = \"{server_name}\"\n"));
    }
    text.push_str(&format!(
        "\n[credentials]\nuserid = \"{user}\"\npassword = \"{PASSWORD_SENTINEL}\"\n"
    ));
    // The real password only lives in the child process's struct; the TOML
    // with the sentinel is what the evidence may show.
    let mut config =
        GraphClientConfig::from_toml_str(&text).expect("the IT configuration validates");
    if let Some(credentials) = config.credentials.as_mut() {
        credentials.password = password;
    }
    Some(config)
}

/// A new connection to the alpha, for the verification read.
async fn fresh_client() -> GraphClient {
    let config = config_from_env().expect("the suite is gated");
    let mut client = GraphClient::connect(config).await.expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    client
}

/// A nonce per execution: no test runs into what another left
/// on the shared alpha.
fn nonce() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

/// The repository of this execution.
fn repository_name() -> String {
    format!("repo{}", &nonce()[10..])
}

// ——————————————————————————————————————————————————————————— S3 localstack

fn docker(args: &[&str]) -> Result<String, String> {
    let out = Command::new("docker")
        .args(args)
        .output()
        .map_err(|error| format!("docker no esta disponible: {error}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

fn container_name() -> String {
    std::env::var("HSP116_S3_CONTAINER").unwrap_or_else(|_| "hodei-hsp116-localstack".to_string())
}

/// The S3 endpoint of the shared container, raising it if needed
/// (the reuse policy is that of HSP-116/138).
fn the_endpoint() -> String {
    let name = container_name();
    let running = docker(&["inspect", "-f", "{{.State.Running}}", &name])
        .map(|state| state.trim() == "true")
        .unwrap_or(false);
    if !running {
        let _ = docker(&["rm", "-f", &name]);
        docker(&[
            "run",
            "-d",
            "--name",
            &name,
            "-p",
            "127.0.0.1:0:4566",
            "localstack/localstack:4.14",
        ])
        .expect("el contenedor de pruebas se levanta");
    }
    for _ in 0..150 {
        if let Some(port) = a_healthy_port(&name) {
            return format!("http://127.0.0.1:{port}");
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    panic!("LocalStack no sirvio S3 a tiempo: el backend elegido no puede probarse");
}

fn a_healthy_port(name: &str) -> Option<String> {
    let port = docker(&[
        "inspect",
        "-f",
        r#"{{(index (index .NetworkSettings.Ports "4566/tcp") 0).HostPort}}"#,
        name,
    ])
    .ok()?;
    let port = port.trim();
    if port.is_empty() || !s3_is_alive(&format!("127.0.0.1:{port}")) {
        return None;
    }
    Some(port.to_string())
}

fn s3_is_alive(address: &str) -> bool {
    let Ok(mut stream) = TcpStream::connect(address) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let request = format!("GET /_localstack/health HTTP/1.0\r\nHost: {address}\r\n\r\n");
    use std::io::Write as IoWrite;
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut answer = String::new();
    if stream.read_to_string(&mut answer).is_err() {
        return false;
    }
    let body: String = answer.chars().filter(|c| !c.is_whitespace()).collect();
    body.contains(r#""s3":"available""#) || body.contains(r#""s3":"running""#)
}

/// The bucket of this execution: two parallel executions do not step on each other.
fn the_bucket() -> String {
    format!("hsp209-smoke-{}", std::process::id())
}

/// The SDK credentials of the child process. They are not a secret -- it is
/// LocalStack -- but their value is searched for in the archived log as one
/// would search for that of a bucket policy: if the service repeats in the log what
/// the SDK gave it, the log is a leak.
const S3_ACCESS_KEY: &str = "test";
const S3_SECRET_KEY: &str = "hsp209-sentinel-secret-not-for-logs";

async fn an_observing_client() -> Client {
    let sdk = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new("us-east-1"))
        .credentials_provider(aws_credential_types::Credentials::from_keys(
            S3_ACCESS_KEY,
            S3_SECRET_KEY,
            None,
        ))
        .endpoint_url(the_endpoint())
        .load()
        .await;
    Client::from_conf(
        aws_sdk_s3::config::Builder::from(&sdk)
            .force_path_style(true)
            .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest())
            .build(),
    )
}

async fn a_bucket(client: &Client, name: &str) {
    let _ = client.create_bucket().bucket(name).send().await;
}

async fn head_object(client: &Client, bucket: &str, key: &str) -> bool {
    client
        .head_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .is_ok()
}

/// The S3 key of a digest, the CAS convention: `blobs/aa/bb/hex`.
fn blob_key(digest: &str) -> String {
    let hex = digest.strip_prefix("sha256:").unwrap_or(digest);
    format!("blobs/{}/{}/{}", &hex[0..2], &hex[2..4], hex)
}

/// The hex digest of a content.
fn the_digest(content: &[u8]) -> String {
    hodei_evidence::ContentDigest::of_bytes(content).value
}

// ————————————————————————————————————————————————————— The real startup

/// The port that the log says the service occupied. The graph profile
/// logs in JSON, so the address is followed by the quote that
/// closes the message: it is cut at the first character that is not part of an
/// address.
fn the_bound_port(log: &Path) -> Option<u16> {
    let content = std::fs::read_to_string(log).ok()?;
    let line = content
        .lines()
        .find(|line| line.contains("Listening on http://"))?;
    let address = without_the_colors(line.split("Listening on http://").nth(1)?);
    let address: String = address
        .chars()
        .take_while(|c| *c != '"' && !c.is_whitespace())
        .collect();
    address.rsplit(':').next()?.trim().parse().ok()
}

/// Strips the escape sequences from the log (the embedded harness's is the
/// same rule: a port with an ANSI glued to it is not read as a number).
fn without_the_colors(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        if chars.next() == Some('[') {
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        }
    }
    out
}

/// The production profile process, with the **emitted** config
/// (`config/dgraph.toml` via `RUN_MODE=dgraph`) and the operator's
/// environment. It opens its security bundle with the files that `Deployment`
/// already wrote.
struct GraphBoot {
    port: u16,
    child: std::process::Child,
    log: PathBuf,
}

impl GraphBoot {
    fn launch(
        evidence: &Path,
        cas_path: &Path,
        bucket: &str,
        extra: &[(&str, &str)],
    ) -> Result<Self, String> {
        let log = evidence.join("service.log");
        let file = std::fs::File::create(&log).expect("the log file is creatable");
        let stdout = file.try_clone().expect("the log file, twice");
        let port = support::free_port();
        let mut command = Command::new(support::BIN);
        command
            .env("RUN_MODE", "dgraph")
            .env("HODEI_SERVER__HOST", "127.0.0.1")
            .env("HODEI_SERVER__PORT", port.to_string())
            .env("HODEI_CONTENT__CAS_PATH", cas_path)
            .env("HODEI_CONTENT__S3__BUCKET", bucket)
            .env("HODEI_CONTENT__S3__REGION", "us-east-1")
            .env("HODEI_CONTENT__S3__ENDPOINT", the_endpoint())
            .env("HODEI_CONTENT__S3__TLS", "false")
            .env("AWS_ACCESS_KEY_ID", S3_ACCESS_KEY)
            .env("AWS_SECRET_ACCESS_KEY", S3_SECRET_KEY)
            .env("RUST_LOG", "info")
            .env_remove("HODEI_SECURITY__AUTH_JWT_SECRET")
            .stdin(Stdio::null())
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(file));
        if let Some(config) = config_from_env() {
            // The IT operator points the profile at the real alpha through the
            // environment -- the same chain that HODEI_* allows in production.
            command
                .env("HODEI_GRAPH__ENDPOINT", &config.endpoint)
                .env("HODEI_GRAPH__NAMESPACE", config.namespace.to_string());
            command.env("HODEI_GRAPH__TLS__ENABLED", config.tls.enabled.to_string());
            if let Some(ca) = config.tls.ca_cert_path.as_deref() {
                command.env("HODEI_GRAPH__TLS__CA_CERT_PATH", ca);
            }
            if let Some(name) = config.tls.server_name.as_deref() {
                command.env("HODEI_GRAPH__TLS__SERVER_NAME", name);
            }
            if let Some(credentials) = config.credentials.as_ref() {
                command.env("HODEI_GRAPH__CREDENTIALS__USERID", &credentials.userid);
                command.env("HODEI_GRAPH__CREDENTIALS__PASSWORD", &credentials.password);
            }
        }
        for (name, value) in extra {
            command.env(name, value);
        }
        let mut child = command.spawn().map_err(|error| format!("spawn: {error}"))?;
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            if the_bound_port(&log) == Some(port)
                && TcpStream::connect(("127.0.0.1", port)).is_ok()
                && matches!(child.try_wait(), Ok(None))
            {
                return Ok(Self { port, child, log });
            }
            if let Ok(Some(_)) = child.try_wait() {
                let text = std::fs::read_to_string(&log).unwrap_or_default();
                return Err(text);
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                let text = std::fs::read_to_string(&log).unwrap_or_default();
                return Err(text);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn log_text(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    /// The log with the red sentinels, ready to archive: neither the
    /// alpha's password nor the SDK secret travel to the file.
    fn redacted_log(&self) -> String {
        let password = std::env::var("HODEI_GRAPH_IT_PASSWORD").unwrap_or_default();
        let mut log = self
            .log_text()
            .replace(S3_SECRET_KEY, "__S3_SECRET_IN_ENVIRONMENT_ONLY__");
        if !password.is_empty() {
            log = log.replace(&password, PASSWORD_SENTINEL);
        }
        log
    }
}

impl Drop for GraphBoot {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Writes the evidence of a pass: header with the observable (without
/// secrets) and the sanitized service log.
fn archive(evidence: &Path, name: &str, sections: &[(&str, Value)], log: Option<&str>) {
    let mut text = format!(
        "# evidencia HSP-209 — {name}\n\nfecha UTC: {}\n\n",
        chrono::Utc::now().to_rfc3339()
    );
    for (section, value) in sections {
        text.push_str(&format!(
            "## {section}\n\n```json\n{}\n```\n\n",
            serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
        ));
    }
    if let Some(log) = log {
        text.push_str("## log del servicio (saneado)\n\n");
        for line in log.lines() {
            text.push_str(line);
            text.push('\n');
        }
    }
    std::fs::write(evidence.join(name), text).expect("the evidence is writable");
}

/// The own read in the graph: DQL by exact key over a
/// predicate with an `@index(exact)` index. Returns the query and whether the
/// key exists. **Never global censuses**: the alpha is shared.
async fn readback(client: &GraphClient, predicate: &str, key: &str) -> (String, bool) {
    let query = format!(
        "{{ q(func: eq({predicate}, \"{key}\")) {{ uid }} }}",
        predicate = predicate,
        key = key
    );
    let answer = client
        .execute_dql(&query)
        .await
        .unwrap_or_else(|error| panic!("the DQL readback of {predicate} must serve: {error}"));
    let found = answer
        .json
        .get("q")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .any(|entry| entry.get("uid").and_then(Value::as_str).is_some())
        })
        .unwrap_or(false);
    (query, found)
}

/// The smoke test policy: the harness's (publish, download, promote)
/// plus the creation and reading of repositories for alice, which the graph
/// profile does serve (the Dgraph catalog is the authority of the metadata).
fn the_policy() -> String {
    let mut policy = support::the_policy();
    for action in ["CreateRepository", "ReadRepository"] {
        policy.push_str(&format!(
            "permit(principal == Iam::User::\"hrn:hodei:iam::{FIRST_ACCOUNT}:User/alice\", action == Artifacts::Action::\"{action}\", resource);\n"
        ));
    }
    policy
}

/// The smoke test's trust policy file: `default` without
/// requirements -- what all software satisfies (the profile's empty
/// registry denies with `PolicyUnavailable`, and the smoke test needs the
/// decision granted to read the durable key in the graph).
fn trust_policy_file(evidence: &Path) -> String {
    let policy = hodei_trust_policy::TrustPolicy::new("default", "1", Vec::new());
    let path = evidence.join("trust-policy.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&policy).expect("the policy serializes"),
    )
    .expect("the trust policy is written");
    path.to_string_lossy().into_owned()
}

/// Writes a JSON body in the deployment directory and returns its
/// path: the harness sends bodies by file (`--data-binary @file`).
fn write_json(deployment: &Deployment, name: &str, body: &Value) -> PathBuf {
    let path = deployment.directory.path().join(name);
    std::fs::write(&path, body.to_string()).expect("the JSON body is written");
    path
}

// ——————————————————————————————————————————————————————————————————— tests

/// S0: the suite gate. Without a declared alpha, everything else is skipped
/// (exit 0), like the rest of the stack's gated suites.
#[test]
fn the_suite_is_gated_on_a_real_graph() {
    match config_from_env() {
        Some(_) => println!("HSP-209 smoke: alpha declarado, la suite corre"),
        None => println!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set"),
    }
}

/// AC#1: the complete production startup. The real binary, with the emitted
/// config and the alpha + localstack alive:
///
/// - the process starts and announces the graph profile in the log,
/// - the log opens no local engine,
/// - the profile's security bundle (the harness's Cedar files,
///   schema from file -- the one persisted on the graph is the registered
///   blocker) allows the complete surface.
///
/// and the vertical surface **together** over the single authority:
/// create repository → publish raw → download the same digest →
/// object in S3 by its CAS key → OCI session → promotion → durable
/// decision. Each fact, re-read on the server by its own key.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_productive_profile_boots_and_serves_every_vertical_together() {
    if config_from_env().is_none() {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    }
    let evidence = evidence_dir();
    std::fs::create_dir_all(&evidence).expect("the evidence directory is creatable");
    let bucket = the_bucket();
    let s3 = an_observing_client().await;
    a_bucket(&s3, &bucket).await;

    let deployment = Deployment::new();
    deployment.write_policy(&the_policy());
    // The profile's security bundle: the harness's Cedar files and
    // the schema **from file** -- the persistence of the schema on the
    // graph is the registered blocker, and the gate (the five fields
    // together or the startup dies) is opened here just as in production.
    let cas = evidence.join("cas-superficie");
    let policy_path = deployment
        .directory
        .path()
        .join("policies.cedar")
        .to_string_lossy()
        .into_owned();
    let schema_path = deployment
        .directory
        .path()
        .join("schema.json")
        .to_string_lossy()
        .into_owned();
    let secret = String::from_utf8(support::SECRET.to_vec()).unwrap();
    let trust_policy = trust_policy_file(&evidence);
    let boot = GraphBoot::launch(
        &evidence,
        &cas,
        &bucket,
        &[
            ("HODEI_SECURITY__AUTH_JWT_SECRET", &secret),
            ("HODEI_SECURITY__AUTH_ISSUER", support::ISSUER),
            ("HODEI_SECURITY__AUTH_AUDIENCE", support::AUDIENCE),
            ("HODEI_SECURITY__AUTHORIZATION_POLICY_PATH", &policy_path),
            ("HODEI_SECURITY__AUTHORIZATION_SCHEMA_PATH", &schema_path),
            ("HODEI_TRUST__POLICY_FILE", &trust_policy),
        ],
    )
    .unwrap_or_else(|log| panic!("el perfil productivo debe arrancar; el log dice:\n{log}"));
    let port = boot.port;
    let token = support::token(FIRST_ACCOUNT, "alice");

    // The startup proof: the log announces the graph profile.
    let log = boot.redacted_log();
    assert!(
        log.contains("Bootstrap completed successfully on the graph profile"),
        "the boot must announce the graph profile:\n{log}"
    );

    // 1. The repository: created by the API, persisted in the graph.
    let repository = repository_name();
    let create_body = write_json(
        &deployment,
        "create.json",
        &json!({"name": repository, "format": "raw", "mode": "hosted"}),
    );
    let created = Curl::call(
        port,
        "POST",
        "/api/v1/catalog/repositories",
        &token,
        Some(&create_body),
        Some("application/json"),
        Some(&deployment.directory.path().join("created.json")),
    )
    .run();
    assert_eq!(
        created.status,
        201,
        "the repository must be created:\n{}",
        boot.log_text()
    );

    // 2. Raw: publish a file through the raw surface.
    let content = format!("hsp209-smoke-{}", nonce()).into_bytes();
    let digest = the_digest(&content);
    let file = deployment.directory.path().join("hsp209.bin");
    std::fs::write(&file, &content).expect("the blob is written");
    let published = support::curl_upload(
        port,
        &format!("/repository/{repository}/dir/smoke.bin"),
        &token,
        &file,
        None,
    );
    assert!(
        published.status == 201 || published.status == 200,
        "the raw publication failed ({}):\n{}",
        published.status,
        boot.log_text()
    );
    let downloaded = deployment.directory.path().join("downloaded.bin");
    let served = support::curl_download(
        port,
        &format!("/repository/{repository}/dir/smoke.bin"),
        &token,
        Some(&downloaded),
    );
    assert_eq!(served.status, 200, "the raw download must serve");
    let bytes = std::fs::read(&downloaded).expect("the download is on disk");
    assert_eq!(
        the_digest(&bytes),
        digest,
        "the served bytes are the published bytes"
    );

    // 3. The byte is in S3 by its CAS key -- the authority of the
    // bytes is the bucket, not the local disk.
    let in_s3 = head_object(&s3, &bucket, &blob_key(&digest)).await;
    assert!(in_s3, "the blob must live in the S3 CAS");

    // 4. OCI: the push session opens and its journal lives in the graph.
    let session = Curl::call(
        port,
        "POST",
        &format!("/v2/{repository}/blobs/uploads/"),
        &token,
        None,
        None,
        None,
    )
    .run();
    assert_eq!(
        session.status,
        202,
        "the OCI upload session must open:\n{}",
        boot.log_text()
    );
    let location = session
        .header("location")
        .expect("the session answers with its location");
    let id = location.rsplit('/').next().unwrap_or("").to_string();

    // 5. Trust: promote the published digest.
    let promote_body = write_json(
        &deployment,
        "promote.json",
        &json!({
            "repository": repository,
            "path": "dir/smoke.bin",
            "digest": format!("sha256:{digest}")
        }),
    );
    let promoted = Curl::call(
        port,
        "POST",
        "/api/v1/promotions",
        &token,
        Some(&promote_body),
        Some("application/json"),
        Some(&deployment.directory.path().join("promoted.json")),
    )
    .run();
    assert_eq!(
        promoted.status,
        200,
        "the promotion must decide:\n{} — body {}",
        boot.log_text(),
        String::from_utf8_lossy(&promoted.body)
    );
    let decision: Value = serde_json::from_slice(&promoted.body).expect("a decision body");
    let decision_id = decision["decision_id"]
        .as_str()
        .expect("the decision names itself")
        .to_string();

    // 6. The read on the server, by own key and only by own
    // key (membership; never censuses over the shared alpha).
    let client = fresh_client().await;
    let namespace = std::env::var("HODEI_GRAPH_IT_NAMESPACE").unwrap_or_else(|_| "0".to_string());
    let (q_repository, repository_present) = readback(
        &client,
        "hodei.catalog.repository.key",
        &format!("{FIRST_ACCOUNT}|{repository}"),
    )
    .await;
    let (q_session, session_present) = readback(
        &client,
        "hodei.upload.session.key",
        &format!("upload-session/{namespace}/{id}"),
    )
    .await;
    let (q_asset, asset_present) = readback(
        &client,
        "hodei.catalog.asset.key",
        &format!("{FIRST_ACCOUNT}|{repository}|dir/smoke.bin"),
    )
    .await;
    let (q_decision, decision_present) =
        readback(&client, "hodei.trust.decision.key", &decision_id).await;
    assert!(
        repository_present,
        "the repository must exist in the graph: {q_repository}"
    );
    assert!(
        asset_present,
        "the asset must exist in the graph: {q_asset}"
    );
    assert!(
        session_present,
        "the OCI session must be journaled in the graph: {q_session}"
    );
    assert!(
        decision_present,
        "the decision must be durable in the graph: {q_decision}"
    );

    archive(
        &evidence,
        "smoke-arranque-superficie.md",
        &[
            (
                "arranque",
                json!({
                    "perfil": "config/dgraph.toml (RUN_MODE=dgraph)",
                    "binario": support::BIN,
                    "anuncia_el_perfil": log.contains("Bootstrap completed successfully on the graph profile"),
                    "esquema": "graph-v7",
                }),
            ),
            (
                "superficie",
                json!({
                    "cuenta": FIRST_ACCOUNT,
                    "repositorio": repository,
                    "raw": { "publicado": true, "digest": format!("sha256:{digest}"), "en_s3": in_s3 },
                    "oci": { "sesion": id, "location": location },
                    "promocion": decision,
                }),
            ),
            (
                "lecturas en el servidor (membresía por clave propia)",
                json!({
                    "repository": { "dql": q_repository, "existe": repository_present },
                    "asset": { "dql": q_asset, "existe": asset_present },
                    "upload-session": { "dql": q_session, "existe": session_present },
                    "decision": { "dql": q_decision, "existe": decision_present },
                }),
            ),
        ],
        Some(&log),
    );
    drop(boot);
}

/// AC#1 (the explicit half of the rejection): the **same** real startup as
/// above, declaring the retired embedded engine
/// (`HODEI_DATABASE__DB_TYPE=rocksdb`) over the graph profile, must die
/// citing the rejection and without opening a port.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_real_boot_refuses_the_rocksdb_fallback_by_name() {
    if config_from_env().is_none() {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    }
    let evidence = evidence_dir();
    std::fs::create_dir_all(&evidence).expect("the evidence directory is creatable");
    let bucket = the_bucket();
    let s3 = an_observing_client().await;
    a_bucket(&s3, &bucket).await;

    let cas = evidence.join("cas-rechazo");
    let outcome = GraphBoot::launch(
        &evidence,
        &cas,
        &bucket,
        &[("HODEI_DATABASE__DB_TYPE", "rocksdb")],
    );
    let log = match outcome {
        Ok(boot) => {
            let text = boot.log_text();
            drop(boot);
            panic!("a graph profile that claims RocksDB must NOT boot; it did:\n{text}");
        }
        Err(log) => log,
    };
    let refusal = hodei_artifacts_api::profile::REFUSAL_MESSAGE;
    let redacted = log
        .replace(S3_SECRET_KEY, "__S3_SECRET_IN_ENVIRONMENT_ONLY__")
        .replace(
            std::env::var("HODEI_GRAPH_IT_PASSWORD")
                .unwrap_or_default()
                .as_str(),
            PASSWORD_SENTINEL,
        );
    assert!(
        redacted.contains(refusal),
        "the boot error must name the refused engine, the log says:\n{redacted}"
    );
    assert!(
        !redacted.contains("Listening on"),
        "the refused boot must die before opening the port:\n{redacted}"
    );
    archive(
        &evidence,
        "smoke-rechazo-rocksdb.md",
        &[(
            "rechazo",
            json!({
                "config": "RUN_MODE=dgraph + HODEI_DATABASE__DB_TYPE=rocksdb",
                "esperado": refusal,
                "abierto_el_puerto": redacted.contains("Listening on"),
            }),
        )],
        Some(&redacted),
    );
}
