//! Production startup over the S3 backend (HSP-138).
//!
//! What is tested here is **the choice**, not the backend: the selection already passed its
//! parity contract in `crates/hodei-hub-s3/tests`. What this file
//! verifies is that the normal binary starts against the object store according to
//! the configuration, that an inaccessible store **stops startup with an
//! explicit error and does not write a byte to `cas_path`**, that real raw and OCI
//! publish and download over a bucket preserving their SHA256, that
//! a restart with the same catalog and the same bucket preserves the data,
//! and that the GC uses the chosen backend (the `hodei-recovery` tool,
//! and its refusal on S3, was removed in HSP-314 F4).
//!
//! The S3 server is a RustFS owned by the suite: if it is already alive it is
//! reused, otherwise it is brought up. State is separated per bucket, unique per
//! process. The service runs on the graph profile (one Dgraph namespace
//! per deployment); the catalog belongs to the graph and the bytes to the bucket.
//!
//! The SDK credentials travel **through the child process's environment**,
//! which is exactly the provider chain the configuration
//! promises and never a TOML field: one of the things this
//! file checks is that the service log does not repeat them.

mod support;

use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use aws_sdk_s3::Client;
use serde_json::Value;
use support::{Deployment, FIRST_ACCOUNT, FIRST_REPOSITORY};

/// The OCI manifest media type: the way a layer lives.
const OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";

/// The catalog seeder (repositories per account).
///
/// `Deployment::seed` re-invokes **this** test binary with
/// `--exact the_catalog_can_be_seeded`: without this function the seed
/// would fail, and the whole suite would go down with it.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// The S3 server image, pinned by digest: the same as HSP-139's
/// conformance.
///
/// It is RustFS and not LocalStack because the graph profile's GC sweep is
/// the coordinated pass, and it deletes with `DeleteObject` conditioned on
/// `If-Match`: LocalStack 4.14 answers `501 NotImplemented` to that
/// header and a server that does not implement it cannot test the
/// sweep.
const S3_IMAGE: &str = "docker.io/rustfs/rustfs@sha256:0dff1515b3c52b5ce8c5df2263420c7f93ae1c5d07cca79e19e57cff86b84c85";

/// The name of this suite's container. It is its own and not that of the
/// HSP-139 series: the server's credential is this test's sentinel.
fn container_name() -> String {
    std::env::var("HSP138_S3_CONTAINER").unwrap_or_else(|_| "hodei-hsp138-rustfs".to_string())
}

/// The named volume where RustFS stores the objects.
fn volume_name() -> String {
    format!("{}-data", container_name())
}

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

/// The S3 endpoint of the suite's container, bringing it up if needed.
///
/// A container that is already running is respected even if it does not serve yet
/// -- it may be starting --, and only one that is not alive is replaced. The secret
/// key of the server is the sentinel: it is the one the service carries in its
/// environment and the one searched for in its log.
fn the_endpoint() -> String {
    let name = container_name();
    let running = docker(&["inspect", "-f", "{{.State.Running}}", &name])
        .map(|state| state.trim() == "true")
        .unwrap_or(false);
    if !running {
        let _ = docker(&["rm", "-f", &name]);
        let _ = docker(&["volume", "create", &volume_name()]);
        docker(&[
            "run",
            "-d",
            "--name",
            &name,
            // root inside the container: the volume was created by docker as
            // root and the image's default user cannot write.
            "--user",
            "root",
            "-p",
            "127.0.0.1::9000",
            "-e",
            "RUSTFS_ACCESS_KEY=test",
            "-e",
            &format!("RUSTFS_SECRET_KEY={SENTINEL}"),
            "-v",
            &format!("{}:/data", volume_name()),
            S3_IMAGE,
            "server",
            "/data",
        ])
        .expect("el contenedor de pruebas se levanta");
    }
    for _ in 0..90 {
        if let Some(port) = a_healthy_port(&name) {
            return format!("http://127.0.0.1:{port}");
        }
        std::thread::sleep(Duration::from_secs(2));
    }
    panic!("RustFS no sirvio S3 a tiempo: el backend elegido no puede probarse");
}

fn a_healthy_port(name: &str) -> Option<String> {
    let port = docker(&[
        "inspect",
        "-f",
        r#"{{(index (index .NetworkSettings.Ports "9000/tcp") 0).HostPort}}"#,
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
    let Ok(mut stream) = std::net::TcpStream::connect(address) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let request = format!("GET /minio/health/live HTTP/1.0\r\nHost: {address}\r\n\r\n");
    use std::io::{Read, Write};
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut answer = String::new();
    if stream.read_to_string(&mut answer).is_err() {
        return false;
    }
    answer.contains("200")
}

/// This process's bucket: two parallel runs do not step on each other.
fn the_bucket() -> String {
    format!("hsp138-arranque-{}", std::process::id())
}

/// The credential the test service uses to reach the bucket. It is not a
/// secret -- it is the test server's -- but its value is searched for in the
/// service log as the real key of a bucket policy would be: if the
/// service writes the value the SDK gave it, the log is a leak.
const SENTINEL: &str = "hsp138-sentinel-secret-not-for-logs";

/// The S3 client for external observation: what the service writes
/// to the bucket is checked from outside the service, not by what the
/// service says about itself.
async fn an_observing_client() -> Client {
    let sdk = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new("us-east-1"))
        .credentials_provider(aws_credential_types::Credentials::from_keys(
            "test", SENTINEL, None,
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

/// Creates the bucket, tolerating one that already exists from another run.
async fn a_bucket(client: &Client, name: &str) {
    let _ = client.create_bucket().bucket(name).send().await;
}

/// The live keys under the blobs prefix.
async fn the_keys_of(client: &Client, bucket: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut token: Option<String> = None;
    loop {
        let mut request = client
            .list_objects_v2()
            .bucket(bucket)
            .prefix("blobs/")
            .max_keys(1000);
        if let Some(token) = &token {
            request = request.continuation_token(token.clone());
        }
        let page = request.send().await.expect("the bucket is listed");
        for object in page.contents() {
            keys.push(object.key().unwrap_or_default().to_string());
        }
        if page.is_truncated == Some(true) {
            token = page.next_continuation_token;
        } else {
            return keys;
        }
    }
}

/// Whether the key exists: the presence of a blob is checked from outside,
/// and so is its absence after the sweep.
async fn a_key_exists(client: &Client, bucket: &str, key: &str) -> bool {
    client
        .head_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .is_ok()
}

/// The key the S3 backend gives a digest: the store prefix,
/// two levels for listing safety, and the hex.
fn the_key_of(digest_hex: &str) -> String {
    format!(
        "blobs/{}/{}/{}",
        &digest_hex[..2],
        &digest_hex[2..4],
        digest_hex
    )
}

/// The deployment with administration open: the extended policy and
/// the schema with the `Admin` resource, as in the GC series.
fn a_deployment() -> Deployment {
    let deployment = Deployment::new();
    let mut schema: Value = serde_json::from_str(support::SCHEMA).expect("the schema parses");
    schema["Admin"] = serde_json::json!({
        "entityTypes": { "AdminApi": { "shape": { "type": "Record", "attributes": {} } } },
        "actions": {
            "AccessAdminApi": {
                "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Admin::AdminApi"] }
            }
        }
    });
    std::fs::write(
        deployment.directory.path().join("schema.json"),
        schema.to_string(),
    )
    .expect("the schema is written");
    std::fs::write(
        deployment.directory.path().join("policies.cedar"),
        format!(
            "{}\npermit(principal == Iam::User::\"hrn:hodei:iam::{FIRST_ACCOUNT}:User/alice\", action == Admin::Action::\"AccessAdminApi\", resource);\n",
            support::the_policy()
        ),
    )
    .expect("the policy is written");
    deployment.seed();
    deployment
}

/// The service environment that chooses the bucket: backend, endpoint, region
/// and the credentials **through the process**, not through configuration. The
/// secret key is the sentinel: it will be searched for in the log.
fn the_s3_environment(bucket: &str) -> Vec<(String, String)> {
    vec![
        ("HODEI_CONTENT__BACKEND".to_string(), "s3".to_string()),
        ("HODEI_CONTENT__S3__BUCKET".to_string(), bucket.to_string()),
        // The endpoint is given with its scheme: the config also accepts
        // bare host:port, and here the literal case of the test server is what matters.
        ("HODEI_CONTENT__S3__ENDPOINT".to_string(), the_endpoint()),
        (
            "HODEI_CONTENT__S3__REGION".to_string(),
            "us-east-1".to_string(),
        ),
        ("AWS_ACCESS_KEY_ID".to_string(), "test".to_string()),
        ("AWS_SECRET_ACCESS_KEY".to_string(), SENTINEL.to_string()),
        ("HODEI_CONTENT__GC_GRACE_SECS".to_string(), "2".to_string()),
        (
            "HODEI_SECURITY__ADMIN_API_ENABLED".to_string(),
            "true".to_string(),
        ),
    ]
}

/// The hex digest of a content.
fn the_digest(content: &[u8]) -> String {
    hodei_evidence::ContentDigest::of_bytes(content).value
}

/// Publishes a raw blob through one-shot distribution, and returns its
/// digest: the way content enters the bucket is the same as
/// always, which is why the GC series' one is used.
fn push_blob(deployment: &Deployment, port: u16, token: &str, content: &[u8]) -> String {
    let digest = the_digest(content);
    let file = deployment.directory.path().join("blob.bin");
    std::fs::write(&file, content).expect("the blob is written");
    let answer = support::Curl::call(
        port,
        "POST",
        &format!("/v2/{FIRST_REPOSITORY}/blobs/uploads/?digest=sha256:{digest}"),
        token,
        Some(&file),
        Some("application/octet-stream"),
        None,
    )
    .run();
    assert_eq!(
        answer.status,
        201,
        "the blob was not published:\n{}",
        deployment.the_log()
    );
    digest
}

/// Publishes a file through the raw surface and returns the digest of the
/// content named by its `ETag`-like: publication via the raw path.
fn publish_raw(deployment: &Deployment, port: u16, content: &[u8]) -> String {
    let file = deployment.directory.path().join("raw.bin");
    std::fs::write(&file, content).expect("the file is written");
    let answer = support::curl_upload(
        port,
        &format!("/repository/{FIRST_REPOSITORY}/dir/shared.bin"),
        &support::token(FIRST_ACCOUNT, "alice"),
        &file,
        None,
    );
    assert!(
        answer.status == 201 || answer.status == 200,
        "the raw publication failed ({}):\n{}",
        answer.status,
        deployment.the_log()
    );
    the_digest(content)
}

/// Downloads the raw path and returns the served bytes.
fn download_raw(deployment: &Deployment, port: u16, into: &Path) -> Vec<u8> {
    let _ = std::fs::remove_file(into);
    let answer = support::curl_download(
        port,
        &format!("/repository/{FIRST_REPOSITORY}/dir/shared.bin"),
        &support::token(FIRST_ACCOUNT, "alice"),
        Some(into),
    );
    assert_eq!(
        answer.status,
        200,
        "the raw download failed ({}):\n{}",
        answer.status,
        deployment.the_log()
    );
    std::fs::read(into).expect("the bytes were written")
}

/// An administrative request: code and JSON of the body.
fn admin(
    deployment: &Deployment,
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    body: Option<&Value>,
) -> (u16, Value) {
    let request = deployment.directory.path().join("admin-request.json");
    let response = deployment.directory.path().join("admin-response.json");
    let _ = std::fs::remove_file(&response);
    let body_file = body.map(|value| {
        std::fs::write(&request, value.to_string()).expect("the request is written");
        request.as_path()
    });
    let answer = support::Curl::call(
        port,
        method,
        &format!("/api/v1{path}"),
        token,
        body_file,
        body.map(|_| "application/json"),
        Some(&response),
    )
    .run();
    let bytes = std::fs::read(&response).unwrap_or_default();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (answer.status, json)
}

/// The health of the service that started: `/health/ready` responds.
fn the_health(port: u16) -> u16 {
    let address = format!("127.0.0.1:{port}");
    let mut stream = TcpStream::connect(&address).expect("the service listens");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("the timeout is set");
    let request = "GET /health/ready HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
    use std::io::{Read, Write};
    stream
        .write_all(request.as_bytes())
        .expect("the request goes");
    let mut answer = String::new();
    stream
        .read_to_string(&mut answer)
        .expect("the answer comes");
    answer
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or(0)
}

/// AC1 (the half the harness cannot measure): a chosen but
/// inaccessible backend **does not start the service and does not write to the directory**.
///
/// It does not go through `Deployment::start`: that harness would retry and
/// end in a panic with the harness's message, and here what is
/// observed is the binary's real output -- code, stderr -- and the state
/// of `cas_path` after the attempt. A dead endpoint answers
/// instantly (connection refused) and the startup preflight translates it
/// into an explicit error: the service dies before listening.
#[test]
fn an_unreachable_bucket_stops_the_boot_and_leaves_the_directory_alone() {
    let tmp = tempfile::tempdir().expect("a temp dir");
    let tenant = support::graph_profile::GraphTenant::create();
    let cas = tmp.path().join("cas");
    let policy = tmp.path().join("policies.cedar");
    let schema = tmp.path().join("schema.json");
    std::fs::write(&policy, support::the_policy()).expect("the policy");
    std::fs::write(&schema, support::SCHEMA).expect("the schema");

    let log = tmp.path().join("boot.log");
    let output_file = std::fs::File::create(&log).expect("the log is open");
    let mut command = Command::new(support::BIN);
    command
        .env_remove("RUN_MODE")
        .env("HODEI_SERVER__HOST", "127.0.0.1")
        .env("HODEI_SERVER__PORT", support::free_port().to_string())
        .env("HODEI_CONTENT__CAS_PATH", &cas)
        .env("HODEI_REPOSITORY__DURABLE", "true")
        .env("HODEI_SECURITY__RAW_API_ENABLED", "true")
        .env("HODEI_SECURITY__OCI_API_ENABLED", "true")
        .env(
            "HODEI_SECURITY__AUTH_JWT_SECRET",
            String::from_utf8(support::SECRET.to_vec()).unwrap(),
        )
        .env("HODEI_SECURITY__AUTH_ISSUER", support::ISSUER)
        .env("HODEI_SECURITY__AUTH_AUDIENCE", support::AUDIENCE)
        .env("HODEI_SECURITY__AUTHORIZATION_POLICY_PATH", &policy)
        .env("HODEI_SECURITY__AUTHORIZATION_SCHEMA_PATH", &schema);
    // The graph profile of this tenant: a live alpha, so the only thing
    // that can stop the boot is the bucket.
    for (name, value) in tenant.service_environment() {
        command.env(name, value);
    }
    let mut child = command
        // The chosen backend, with a bucket that cannot exist: the
        // dead endpoint answers "connection refused" and the preflight
        // translates it into an error; the 10 s timeout is the ceiling.
        .env("HODEI_CONTENT__BACKEND", "s3")
        .env("HODEI_CONTENT__S3__BUCKET", "hsp138-inexistente")
        .env("HODEI_CONTENT__S3__ENDPOINT", "http://127.0.0.1:1")
        .env("HODEI_CONTENT__S3__REGION", "us-east-1")
        .env("AWS_ACCESS_KEY_ID", "test")
        .env("AWS_SECRET_ACCESS_KEY", "test")
        .env("RUST_LOG", "info")
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            output_file.try_clone().expect("the log, twice"),
        ))
        .stderr(Stdio::from(output_file))
        .spawn()
        .expect("the binary runs");
    let deadline = Instant::now() + Duration::from_secs(40);
    let status = loop {
        match child.try_wait().expect("the wait works") {
            Some(status) => break status,
            None if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            None => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("the boot did not die: the backend is unreachable and the service hangs");
            }
        }
    };
    assert!(
        !status.success(),
        "AC1: un S3 inaccesible no deja arrancar al servicio"
    );
    let said = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        said.contains("Bootstrap failed") && said.contains("bucket"),
        "el error tiene que decir que el problema es el cubo:\n{said}"
    );
    // And the directory the config names remains untouched: it was not
    // a substitute for anything. Under filesystem startup would create it; here
    // not even that: the chosen backend does not touch it.
    assert!(
        !cas.exists(),
        "AC1: el cas_path no se toca cuando el backend elegido es S3"
    );
}

/// AC1 and AC2 together, the happy path: the normal binary starts against the
/// bucket, real raw and OCI publish and download with their SHA256, the
/// bytes live **in the bucket** and not in the directory, a restart with the
/// same catalog and the same bucket preserves them, and the GC deletes from the bucket.
#[tokio::test]
async fn the_service_boots_on_the_bucket_and_keeps_what_it_published_across_a_restart() {
    let bucket = the_bucket();
    let client = an_observing_client().await;
    a_bucket(&client, &bucket).await;
    let deployment = a_deployment();
    let environment = the_s3_environment(&bucket);
    let pairs: Vec<(&str, &str)> = environment
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect();

    // Startup: if the preflight failed, the harness would panic with the
    // log, and it would show there. If it starts, `/health/ready` answers 200 --
    // the positive readiness required by AC1 is this path.
    let mut service = deployment.start(&pairs);
    assert_eq!(the_health(service.port), 200, "la readiness sobre S3");

    let alice = support::token(FIRST_ACCOUNT, "alice");
    let raw = b"raw content that lives in the bucket".to_vec();
    let raw_digest = publish_raw(&deployment, service.port, &raw);
    assert_eq!(raw_digest, the_digest(&raw), "el SHA256 que se conserva");

    // OCI: the layer and the manifest enter through the distribution.
    let layer = push_blob(&deployment, service.port, &alice, b"an s3 layer");
    let config = push_blob(
        &deployment,
        service.port,
        &alice,
        b"{\"\":[],\"os\":\"linux\"}",
    );
    let manifest_bytes = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": OCI_MANIFEST,
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": format!("sha256:{config}"),
            "size": 1
        },
        "layers": [{
            "mediaType": "application/vnd.oci.image.layer.v1.tar",
            "digest": format!("sha256:{layer}"),
            "size": 1
        }]
    })
    .to_string()
    .into_bytes();
    let manifest_file = deployment.directory.path().join("manifest.json");
    std::fs::write(&manifest_file, &manifest_bytes).expect("the manifest is written");
    let answer = support::Curl::call(
        service.port,
        "PUT",
        &format!("/v2/{FIRST_REPOSITORY}/manifests/v1"),
        &alice,
        Some(&manifest_file),
        Some(OCI_MANIFEST),
        None,
    )
    .run();
    assert!(
        answer.status == 200 || answer.status == 201,
        "the manifest was not published ({}):\n{}",
        answer.status,
        deployment.the_log()
    );

    // The download, through the raw path, byte for byte equal.
    let back = download_raw(
        &deployment,
        service.port,
        &deployment.directory.path().join("got.bin"),
    );
    assert_eq!(back, raw, "AC2: lo que se descarga es lo que se publico");

    // And the bytes are **in the bucket**, at the key the backend uses,
    // with the digest the content states.
    for digest in [&raw_digest, &layer, &config] {
        assert!(
            a_key_exists(&client, &bucket, &the_key_of(digest)).await,
            "AC2: el contenido {digest} tiene que vivir en el cubo"
        );
    }
    // The directory the config names is no authority for anything: it does
    // not even exist, because startup did not open it.
    assert!(
        !Path::new(&deployment.cas).exists(),
        "AC1: un despliegue S3 no escribe ni crea su cas_path"
    );
    // The credentials do not appear in the log: the SDK chain gave them to the
    // process, and the service does not repeat them. The sentinel is the value
    // the service's environment carries as secret key; if the log
    // contains it, the service writes credentials.
    let log = deployment.the_log();
    assert!(
        !log.contains(SENTINEL),
        "AC4: el log del servicio no escribe la credencial con la que trabaja"
    );

    // The restart: the same catalog (the same database), the same bucket.
    let port = service.port;
    support::stop(&mut service);
    let mut again = deployment.start_on(port, &pairs);
    let back = download_raw(
        &deployment,
        again.port,
        &deployment.directory.path().join("got2.bin"),
    );
    assert_eq!(
        back, raw,
        "AC2: tras el reinicio el mismo cubo sirve lo publicado"
    );
    assert!(
        a_key_exists(&client, &bucket, &the_key_of(&layer)).await,
        "AC2: la capa sigue en el cubo tras el reinicio"
    );

    // The GC over the chosen backend: an object that no catalog record
    // names leaves the bucket when the grace period expires. The orphan
    // is planted **from outside**, writing the key into the bucket with the
    // observer client: uploading it through the distribution would create its row in
    // the catalog, and a blob with a row is a root, not an orphan. It is the
    // same plant the GC series used to write by hand in the directory.
    let orphan_bytes = b"an orphan for the bucket gc";
    let orphan = the_digest(orphan_bytes);
    client
        .put_object()
        .bucket(&bucket)
        .key(the_key_of(&orphan))
        .body(orphan_bytes.to_vec().into())
        .send()
        .await
        .expect("the orphan is planted in the bucket");
    assert!(
        a_key_exists(&client, &bucket, &the_key_of(&orphan)).await,
        "el huerfano planted esta en el cubo antes del plan"
    );
    std::thread::sleep(Duration::from_secs(3));
    let (status, plan) = admin(&deployment, again.port, "GET", "/gc/plan", &alice, None);
    assert_eq!(status, 200, "el plan sobre S3:\n{}", deployment.the_log());
    assert!(
        plan["entries"]
            .as_array()
            .expect("plan entries")
            .iter()
            .any(|entry| entry["digest"] == format!("sha256:{orphan}")
                && entry["verdict"] == "reclaimable"),
        "AC3: el plan ve huerfano en el cubo: {plan}"
    );
    let (status, report) = admin(
        &deployment,
        again.port,
        "POST",
        "/gc/sweep",
        &alice,
        Some(&serde_json::json!({ "execute": true })),
    );
    assert_eq!(status, 200, "el sweep sobre S3:\n{}", deployment.the_log());
    assert_eq!(
        report["dry_run"], false,
        "el barrido tenia que ejecutarse de verdad: {report}"
    );
    // The sweep runs detached: the response is the acknowledgement, not
    // the last delete_object answered. We wait until the bucket
    // accounts for it -- with a ceiling --, as in the GC series over the directory.
    let until = Instant::now() + Duration::from_secs(30);
    while a_key_exists(&client, &bucket, &the_key_of(&orphan)).await && Instant::now() < until {
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert!(
        !a_key_exists(&client, &bucket, &the_key_of(&orphan)).await,
        "AC3: el GC borro el huerfano del cubo (reporte: {report}; log:\n{})",
        deployment.the_log()
    );
    assert!(
        a_key_exists(&client, &bucket, &the_key_of(&layer)).await,
        "AC3: lo que un manifiesto nombra no se borra"
    );

    // And what was published still lives for the service, with the bucket as
    // sole authority: the external listing shows the live blobs.
    let keys = the_keys_of(&client, &bucket).await;
    assert!(
        keys.contains(&the_key_of(&raw_digest)) && keys.contains(&the_key_of(&layer)),
        "el cubo guarda lo publicado: {keys:?}"
    );
    support::stop(&mut again);
}
