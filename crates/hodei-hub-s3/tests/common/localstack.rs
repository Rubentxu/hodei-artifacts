//! The LocalStack harness for the S3 integration tests.
//!
//! One container shared by all the tests of this crate: it is
//! reused if it is already alive and started if not. The image is pinned to
//! 4.14, the last community one that does not ask for a license token. The port
//! is ephemeral and discovered with `docker inspect`; state is not shared from
//! one test to another because each test uses its own bucket, unique per
//! process.
//!
//! This replaces the AWS emulator in embedded mode: its graph compiles
//! RustPython (broken on this machine) and some three hundred service
//! crates that exhaust the compiler's process table. Here the SDK
//! is the same as in production and the S3 protocol is served by a real
//! server over the localhost network.

use std::io::{Read, Write};
use std::process::Command;

use aws_credential_types::Credentials;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::Region;
use hodei_hub_s3::{S3BlobConfig, S3BlobStore};

const IMAGE: &str = "localstack/localstack:4.14";

/// Which bench the capabilities report runs against: the fixed tag,
/// which is the version of the test server.
pub const VERSION_NOTE: &str = "localstack/localstack:4.14";

/// How long LocalStack is given to serve S3 from when it is launched or
/// inherited already running. On a loaded machine, startup does not fit in a
/// minute: twice the reasonable time is awaited and it still fails in plain sight.
const START_GRACE_SECONDS: u64 = 300;

/// The name of the shared container. `HSP116_S3_CONTAINER` changes it
/// when two test runs coexist on the same machine and must not
/// trample each other's S3 server.
fn container_name() -> String {
    std::env::var("HSP116_S3_CONTAINER").unwrap_or_else(|_| "hodei-hsp116-localstack".to_string())
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

/// The ephemeral port of the container that was already there, if it answers the health check.
fn a_healthy_port() -> Option<String> {
    let name = container_name();
    let port = docker(&[
        "inspect",
        "-f",
        r#"{{(index (index .NetworkSettings.Ports "4566/tcp") 0).HostPort}}"#,
        &name,
    ])
    .ok()?;
    let port = port.trim();
    if port.is_empty() || !s3_is_alive(&format!("127.0.0.1:{port}")) {
        return None;
    }
    Some(port.to_string())
}

/// A raw GET to the health endpoint: without an SDK or HTTP test dependencies.
fn s3_is_alive(address: &str) -> bool {
    let Ok(mut stream) = std::net::TcpStream::connect(address) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(3)));
    let request = format!("GET /_localstack/health HTTP/1.0\r\nHost: {address}\r\n\r\n");
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

/// The harness is started only once per process: otherwise the threads of one
/// test binary trample each other with `rm -f` on the container another one
/// has just launched.
static SHARED_ENDPOINT: tokio::sync::OnceCell<String> = tokio::sync::OnceCell::const_new();

/// Whether the container exists and is running, whether ours or someone else's.
fn the_container_is_running(name: &str) -> bool {
    docker(&["inspect", "-f", "{{.State.Running}}", name])
        .map(|state| state.trim() == "true")
        .unwrap_or(false)
}

/// Waits for a healthy port to appear, polling every two seconds.
fn a_healthy_port_within(seconds: u64) -> Option<String> {
    for _ in 0..(seconds / 2) {
        if let Some(port) = a_healthy_port() {
            return Some(port);
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    a_healthy_port()
}

/// The S3 endpoint of the shared container, starting it if needed.
pub async fn the_endpoint() -> String {
    SHARED_ENDPOINT
        .get_or_init(|| async {
            let endpoint = tokio::task::spawn_blocking(|| {
                let name = container_name();
                if let Some(port) = a_healthy_port() {
                    return Ok::<_, String>(port);
                }
                // A running container is respected even if it does not serve yet:
                // it may be starting up (ours or another test's running in
                // parallel); only one that is no longer alive is replaced. On this
                // machine, under load, LocalStack takes longer than an impatient doorman
                // takes to declare it dead.
                if !the_container_is_running(&name) {
                    let _ = docker(&["rm", "-f", &name]);
                    docker(&[
                        "run",
                        "-d",
                        "--name",
                        &name,
                        "-p",
                        "127.0.0.1:0:4566",
                        IMAGE,
                    ])?;
                }
                a_healthy_port_within(START_GRACE_SECONDS)
                    .ok_or_else(|| format!("LocalStack no sirvio S3 en {START_GRACE_SECONDS} s"))
            })
            .await
            .expect("el sondeo no paniquea");

            match endpoint {
                Ok(port) => format!("http://127.0.0.1:{port}"),
                Err(reason) => panic!("el servidor S3 de pruebas no arranco: {reason}"),
            }
        })
        .await
        .clone()
}
/// An S3 client against the container. The credentials are LocalStack's fixed
/// ones: they are not a secret, and do not even go through the store.
pub async fn a_client() -> Client {
    let endpoint = the_endpoint().await;
    let credentials = Credentials::from_keys("test", "test", None);
    let sdk = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .credentials_provider(credentials)
        .load()
        .await;
    let s3_config = aws_sdk_s3::config::Builder::from(&sdk)
        .endpoint_url(&endpoint)
        .force_path_style(true)
        .behavior_version(aws_sdk_s3::config::BehaviorVersion::latest())
        .build();
    Client::from_conf(s3_config)
}

/// The bucket name, unique per process: two parallel runs do not
/// trample each other's state and a listing starts truly empty.
pub fn a_unique_bucket(prefix: &str) -> String {
    format!("hsp116-{prefix}-{}", std::process::id())
}

/// Create the bucket, tolerating one that already exists from another run.
pub async fn a_bucket(client: &Client, name: &str) {
    match client.create_bucket().bucket(name).send().await {
        Ok(_) => {}
        Err(error) => {
            let code = aws_sdk_s3::error::ProvideErrorMetadata::code(&error)
                .unwrap_or_default()
                .to_string();
            assert!(
                code == "BucketAlreadyOwnedByYou" || code == "BucketAlreadyExists",
                "crear el cubo de pruebas {name}: {error}"
            );
        }
    }
}

/// An S3 store over a freshly created bucket of the shared container.
/// Starting it twice over the same bucket is the restart: the state
/// lives on the server, not in the process.
pub async fn a_store(bucket: &str) -> S3BlobStore {
    let client = a_client().await;
    a_bucket(&client, bucket).await;
    S3BlobStore::new(client, S3BlobConfig::new(bucket)).expect("el almacen abre")
}
