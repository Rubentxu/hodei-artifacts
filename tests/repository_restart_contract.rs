//! Process-level acceptance: JWT + durable repository survives a real restart.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::Serialize;
use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_hodei-artifacts-api");
const SECRET: &[u8] = b"restart-contract-secret-long-enough";
const ISSUER: &str = "https://issuer.example";
const AUDIENCE: &str = "hodei-artifacts";

/// The public key the service verifies with, published the way a real
/// deployment publishes it: as a JWKS the service reads at boot.
const JWKS: &str = include_str!("fixtures/jwks.json");
/// The private key that signs the tokens of this scenario.
const RSA_PRIVATE_KEY: &str = include_str!("fixtures/rsa-private.pem");
/// The key id the JWKS publishes and the tokens name.
const KEY_ID: &str = "test-key";

const SCHEMA: &str = r#"{
  "": { "entityTypes": {}, "actions": {} },
  "Iam": { "entityTypes": { "User": { "shape": { "type": "Record", "attributes": {} } } }, "actions": {} },
  "Artifacts": {
    "entityTypes": { "Repository": { "shape": { "type": "Record", "attributes": {} } } },
    "actions": {
      "CreateRepository": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
      "ReadRepository": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } }
    }
  }
}"#;

const POLICY: &str = r#"
permit(principal == Iam::User::"hrn:hodei:iam::acme:User/alice", action == Artifacts::Action::"CreateRepository", resource);
permit(principal == Iam::User::"hrn:hodei:iam::acme:User/alice", action == Artifacts::Action::"ReadRepository", resource);
"#;

#[derive(Serialize)]
struct Claims {
    sub: String,
    account: String,
    exp: usize,
    iss: String,
    aud: String,
}

/// A token signed with RS256 and the key the JWKS publishes.
///
/// The key id travels in the header, which is how the service knows which of
/// the published keys verifies the token.
fn rs256_token(sub: &str, account: &str) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(KEY_ID.to_string());
    encode(
        &header,
        &Claims {
            sub: sub.to_string(),
            account: account.to_string(),
            exp: 4_000_000_000,
            iss: ISSUER.to_string(),
            aud: AUDIENCE.to_string(),
        },
        &EncodingKey::from_rsa_pem(RSA_PRIVATE_KEY.as_bytes()).expect("the rsa key"),
    )
    .unwrap()
}

/// A token signed with the shared secret.
fn hs256_token(sub: &str, account: &str) -> String {
    encode(
        &Header::new(Algorithm::HS256),
        &Claims {
            sub: sub.to_string(),
            account: account.to_string(),
            exp: 4_000_000_000,
            iss: ISSUER.to_string(),
            aud: AUDIENCE.to_string(),
        },
        &EncodingKey::from_secret(SECRET),
    )
    .unwrap()
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// How the service is told which credential to trust.
#[derive(Clone, Copy)]
enum Credential {
    /// A shared secret, verified with HS256.
    SharedSecret,
    /// A published key set, verified with RS256.
    Jwks,
}

fn start(
    port: u16,
    db: &str,
    policy: &str,
    schema: &str,
    credential: Credential,
    jwks_path: Option<&str>,
) -> Child {
    let mut command = Command::new(BIN);
    command
        .env_remove("RUN_MODE")
        .env("HODEI_SERVER__HOST", "127.0.0.1")
        .env("HODEI_SERVER__PORT", port.to_string())
        .env("HODEI_ROCKSDB__PATH", db)
        .env("HODEI_REPOSITORY__DURABLE", "true")
        .env("HODEI_SECURITY__AUTH_ISSUER", ISSUER)
        .env("HODEI_SECURITY__AUTH_AUDIENCE", AUDIENCE)
        .env("HODEI_SECURITY__AUTHORIZATION_POLICY_PATH", policy)
        .env("HODEI_SECURITY__AUTHORIZATION_SCHEMA_PATH", schema)
        .env("RUST_LOG", "warn");
    match credential {
        Credential::SharedSecret => {
            command.env(
                "HODEI_SECURITY__AUTH_JWT_SECRET",
                String::from_utf8(SECRET.to_vec()).unwrap(),
            );
        }
        Credential::Jwks => {
            // The key set is a file the service reads at boot, the way a
            // deployment publishes it.
            let jwks = jwks_path.expect("the key set path");
            command.env("HODEI_SECURITY__AUTH_JWKS_PATH", jwks);
        }
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

unsafe fn kill(pid: i32, signal: i32) {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    unsafe {
        kill(pid, signal);
    }
}

fn stop(child: &mut Child) {
    unsafe { kill(child.id() as i32, 15) };
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn wait_health(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("binary did not open the port");
}

fn request(
    port: u16,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (u16, Value) {
    let body = body.map(|value| value.to_string()).unwrap_or_default();
    let auth = token
        .map(|token| format!("Authorization: Bearer {token}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let status = response
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    let json_start = response.find('{').unwrap_or(response.len());
    let value = serde_json::from_str(&response[json_start..]).unwrap_or(Value::Null);
    (status, value)
}

/// The restart scenario, run with one credential.
///
/// The scenario starts the service, creates a repository, checks that the
/// contract holds (no token, no permission, no duplicate), stops the process
/// and starts it again over the same storage: the repository must still be
/// there and the credentials must still be refused or accepted as before.
fn restart_scenario(credential: Credential) {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("db").to_string_lossy().into_owned();
    let policy = tmp.path().join("policies.cedar");
    let schema = tmp.path().join("schema.json");
    std::fs::write(&policy, POLICY).unwrap();
    std::fs::write(&schema, SCHEMA).unwrap();

    let token = match credential {
        Credential::SharedSecret => hs256_token,
        Credential::Jwks => rs256_token,
    };

    // The key set is a file of its own, published the way a deployment
    // publishes it, and it lives in the scenario's own directory so two
    // scenarios running at once do not share it.
    let jwks = tmp.path().join("jwks.json");
    std::fs::write(&jwks, JWKS).unwrap();
    let jwks_path = jwks.to_string_lossy().into_owned();

    let port = free_port();
    let mut child = start(
        port,
        &db,
        policy.to_str().unwrap(),
        schema.to_str().unwrap(),
        credential,
        Some(&jwks_path),
    );
    wait_health(port);

    let alice = token("hrn:hodei:iam::acme:User/alice", "acme");
    let bob = token("hrn:hodei:iam::other:User/bob", "other");
    let (status, created) = request(
        port,
        "POST",
        "/api/v1/repositories",
        Some(&alice),
        Some(serde_json::json!({"name": "releases", "region": "eu"})),
    );
    assert_eq!(status, 201, "create failed: {created:?}");
    let hrn = created["hrn"].as_str().unwrap().to_string();

    let (status, _) = request(port, "GET", "/api/v1/repositories", None, None);
    assert_eq!(status, 401);
    let (status, _) = request(
        port,
        "POST",
        "/api/v1/repositories",
        Some(&bob),
        Some(serde_json::json!({"name": "releases"})),
    );
    assert_eq!(status, 403);
    let (status, _) = request(
        port,
        "POST",
        "/api/v1/repositories",
        Some(&alice),
        Some(serde_json::json!({"name": "releases", "region": "eu"})),
    );
    assert_eq!(status, 409);
    stop(&mut child);

    let port = free_port();
    let mut child = start(
        port,
        &db,
        policy.to_str().unwrap(),
        schema.to_str().unwrap(),
        credential,
        Some(&jwks_path),
    );
    wait_health(port);
    let (status, body) = request(
        port,
        "GET",
        &format!("/api/v1/repositories?hrn={hrn}"),
        Some(&alice),
        None,
    );
    assert_eq!(status, 200, "restart lost the repository: {body:?}");
    assert_eq!(body["name"], "releases");
    stop(&mut child);
}

/// The restart scenario with the published key set, verified with RS256.
///
/// The credentials a deployment really uses are a published key set, not a
/// secret shared with the service, so the scenario that proves the storage
/// survives a restart has to run with them.
#[test]
fn repository_survives_a_real_process_restart_with_jwks_rs256() {
    restart_scenario(Credential::Jwks);
}

/// The same scenario with the shared secret, so both credential paths are
/// covered by the restart contract.
#[test]
fn repository_survives_a_real_process_restart_with_the_shared_secret() {
    restart_scenario(Credential::SharedSecret);
}
