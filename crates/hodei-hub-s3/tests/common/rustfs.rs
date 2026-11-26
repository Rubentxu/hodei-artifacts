//! The RustFS harness for the S3 conformance tests (HSP-139).
//!
//! RustFS is a real S3 server written in Rust (AGPLv3), not an
//! emulator: it is the second bench of the series, next to the LocalStack of
//! HSP-116, so that the backend's conformance does not depend on the
//! benevolence of a single mock. The image is pinned **by digest**,
//! not by tag: `latest` is a promise whose content changes under
//! your feet, and a per-version capabilities report (AC4) that does not say
//! against which exact bytes it ran is not a report. The digest written
//! here is the one `rustfs/rustfs:latest` had the day this test
//! was written.
//!
//! The shared-container pattern is that of the `localstack` module:
//! it is reused if already alive, started if not, the port is ephemeral
//! and discovered with `docker inspect`, and state is separated by a bucket
//! unique per process. Two details are specific to this server and not to the
//! emulator:
//!
//! - Storage is a docker **named volume**, not a
//!   bind-mount: RustFS runs its server over `/data` and rejects the
//!   bind-mount of a host directory that does not belong to the user of the
//!   process inside the container ("File access denied"); with a volume
//!   managed by docker, ownership is decided by docker.
//! - The credential is passed through the server's environment
//!   (`RUSTFS_ACCESS_KEY` / `RUSTFS_SECRET_KEY`), which is what the
//!   client signs with afterwards. They are fixed and for testing: like those of
//!   LocalStack, they do not travel through the store nor appear in any log of the
//!   service under test.

use std::io::{Read, Write};
use std::process::Command;

use aws_credential_types::Credentials;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::Region;
use hodei_hub_s3::{S3BlobConfig, S3BlobStore};

/// The image, pinned by manifest digest (platform index).
const IMAGE: &str =
    "rustfs/rustfs@sha256:0dff1515b3c52b5ce8c5df2263420c7f93ae1c5d07cca79e19e57cff86b84c85";

/// The tag from which that digest was resolved, only for the report:
/// what the test pins is the digest, what the report names is this.
pub const VERSION_NOTE: &str = "rustfs/rustfs:latest (digest 0dff1515)";

/// The internal S3 port of the RustFS container.
const INTERNAL_PORT: &str = "9000";

/// The fixed credential of the test server. It is not a secret: just like
/// LocalStack's `test`/`test`, it is what the container itself
/// publishes in its environment.
pub const ACCESS_KEY: &str = "test";
pub const SECRET_KEY: &str = "testsecret";

/// How long RustFS is given to serve S3 from when it is launched.
const START_GRACE_SECONDS: u64 = 180;

/// The name of the shared container, independent of LocalStack's:
/// the two benches can coexist on the same machine without trampling each other.
fn container_name() -> String {
    std::env::var("HSP139_S3_CONTAINER").unwrap_or_else(|_| "hodei-hsp139-rustfs".to_string())
}

/// The named volume where RustFS keeps the objects.
fn volume_name() -> String {
    std::env::var("HSP139_S3_VOLUME").unwrap_or_else(|_| "hodei-hsp139-rustfs-data".to_string())
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
        &format!(
            r#"{{{{(index (index .NetworkSettings.Ports "{INTERNAL_PORT}/tcp") 0).HostPort}}}}"#
        ),
        &name,
    ])
    .ok()?;
    let port = port.trim();
    if port.is_empty() || !s3_is_alive(&format!("127.0.0.1:{port}")) {
        return None;
    }
    Some(port.to_string())
}

/// A raw GET to RustFS's health endpoint (MinIO compatibility) or to its unsigned
/// root: a 403 from the root also says an S3 server is
/// listening. The SDK is not needed to know there is a door.
fn s3_is_alive(address: &str) -> bool {
    let Ok(mut stream) = std::net::TcpStream::connect(address) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(3)));
    let request = format!("GET /minio/health/live HTTP/1.0\r\nHost: {address}\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut answer = String::new();
    if stream.read_to_string(&mut answer).is_err() {
        return false;
    }
    answer.contains("200")
}

/// The harness is started only once per process.
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
                if !the_container_is_running(&name) {
                    let _ = docker(&["rm", "-f", &name]);
                    // The named volume survives between runs:
                    // RustFS picks it up and its state is not lost; the
                    // separation between tests is done by the bucket.
                    let _ = docker(&["volume", "create", &volume_name()]);
                    docker(&[
                        "run",
                        "-d",
                        "--name",
                        &name,
                        // root inside the container: the volume was created by
                        // docker as root and the image's default user
                        // cannot write to it.
                        "--user",
                        "root",
                        "-p",
                        &format!("127.0.0.1:0:{INTERNAL_PORT}"),
                        "-e",
                        &format!("RUSTFS_ACCESS_KEY={ACCESS_KEY}"),
                        "-e",
                        &format!("RUSTFS_SECRET_KEY={SECRET_KEY}"),
                        "-v",
                        &format!("{}:/data", volume_name()),
                        IMAGE,
                        "server",
                        "/data",
                    ])?;
                }
                a_healthy_port_within(START_GRACE_SECONDS)
                    .ok_or_else(|| format!("RustFS no sirvio S3 en {START_GRACE_SECONDS} s"))
            })
            .await
            .expect("el sondeo no paniquea");

            match endpoint {
                Ok(port) => format!("http://127.0.0.1:{port}"),
                Err(reason) => panic!("el servidor S3 real de pruebas no arranco: {reason}"),
            }
        })
        .await
        .clone()
}

/// An S3 client against RustFS with its fixed test credentials.
pub async fn a_client() -> Client {
    let endpoint = the_endpoint().await;
    let credentials = Credentials::from_keys(ACCESS_KEY, SECRET_KEY, None);
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

/// The bucket name, unique per process.
pub fn a_unique_bucket(prefix: &str) -> String {
    format!("hsp139-{prefix}-{}", std::process::id())
}

/// Create the bucket, tolerating one that already exists from another run.
///
/// RustFS's `create_bucket` is transiently abrupt under load:
/// with the previous test finishing releasing connections, it answers a
/// `service error` that is neither "already exists" nor a store failure.
/// Retrying calmly is what a real client would do; blowing up the
/// test would be confusing the server's clumsiness with a defect of our own.
pub async fn a_bucket(client: &Client, name: &str) {
    let mut attempts = 0;
    loop {
        attempts += 1;
        match client.create_bucket().bucket(name).send().await {
            Ok(_) => return,
            Err(error) => {
                let code = aws_sdk_s3::error::ProvideErrorMetadata::code(&error)
                    .unwrap_or_default()
                    .to_string();
                if code == "BucketAlreadyOwnedByYou" || code == "BucketAlreadyExists" {
                    return;
                }
                assert!(attempts < 5, "crear el cubo de pruebas {name}: {error}");
                tokio::time::sleep(std::time::Duration::from_millis(200 * attempts)).await;
            }
        }
    }
}

/// An S3 store over a freshly created bucket of the shared RustFS.
pub async fn a_store(bucket: &str) -> S3BlobStore {
    let client = a_client().await;
    a_bucket(&client, bucket).await;
    S3BlobStore::new(client, S3BlobConfig::new(bucket)).expect("el almacen abre")
}
