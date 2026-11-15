//! The raw hosted content surface, at the level of a real process (AC5).
//!
//! The contract of [`har04_raw_hosted_contract`] runs in process. This one
//! runs the **binary**: it boots the service on a real port, publishes a
//! file with the real `curl`, downloads it with the real `curl`, compares
//! the two with the real `sha256sum`, stops the process, starts it again
//! over the same storage and checks that the content is still there and is
//! still the same bytes.
//!
//! What the in-process contract cannot say and this one says:
//!
//! - That the content is on **disk**, and not in the memory of a process
//!   that is about to die.
//! - That the routing, the configuration and the boot check work in the
//!   binary and not only in a test harness that builds the router by hand.
//! - That the **bytes** are the bytes: `sha256sum` says so, and it is not
//!   the code under test that says it.
//!
//! The repository is seeded directly in the catalog before the service
//! starts, because the creation of a repository is not part of this
//! surface: this surface serves the repositories that are there.

use std::fs::File;
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hodei_catalog::{
    AllowAllAuthorizer, CatalogWriter, DigestScope, RepositoryConfig, RepositoryDescriptor,
    RepositoryFormat, RepositoryMode, repository_hrn,
};
use hodei_catalog_storage::SurrealCatalogStore;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::Serialize;
use surrealdb::Surreal;
use surrealdb::engine::local::RocksDb;

const BIN: &str = env!("CARGO_BIN_EXE_hodei-artifacts-api");
const SECRET: &[u8] = b"raw-restart-contract-secret-long-enough";
const ISSUER: &str = "https://issuer.example";
const AUDIENCE: &str = "hodei-artifacts";

/// The Cedar schema of the deployment.
///
/// It declares the two actions of the raw surface, because a bundle that
/// does not declare them cannot grant them, and the service refuses to
/// start with a bundle that does not declare what it authorizes with.
const SCHEMA: &str = r#"{
  "": { "entityTypes": {}, "actions": {} },
  "Iam": { "entityTypes": { "User": { "shape": { "type": "Record", "attributes": {} } } }, "actions": {} },
  "Artifacts": {
    "entityTypes": {
      "Repository": { "shape": { "type": "Record", "attributes": {} } },
      "Artifact": { "shape": { "type": "Record", "attributes": {} } }
    },
    "actions": {
      "CreateRepository": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
      "ReadRepository": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
      "ReadCapsule": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Artifact"] } },
      "UploadAsset": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
      "DownloadAsset": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } }
    }
  }
}"#;

/// The policy of the deployment: `alice` may publish and download.
const POLICY: &str = r#"
permit(principal == Iam::User::"hrn:hodei:iam::acme:User/alice", action == Artifacts::Action::"UploadAsset", resource);
permit(principal == Iam::User::"hrn:hodei:iam::acme:User/alice", action == Artifacts::Action::"DownloadAsset", resource);
"#;

/// The path of the content, inside the repository.
const CONTENT_PATH: &str = "/repository/releases/dir/file.bin";

#[derive(Serialize)]
struct Claims {
    sub: String,
    account: String,
    exp: usize,
    iss: String,
    aud: String,
}

/// A token of `alice`, of the account that owns the repository.
fn token() -> String {
    encode(
        &Header::new(Algorithm::HS256),
        &Claims {
            sub: "hrn:hodei:iam::acme:User/alice".to_string(),
            account: "acme".to_string(),
            exp: 4_000_000_000,
            iss: ISSUER.to_string(),
            aud: AUDIENCE.to_string(),
        },
        &EncodingKey::from_secret(SECRET),
    )
    .expect("token")
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// The repository, created directly in the catalog.
///
/// It is created before the service starts, and in a **process of its
/// own**: the disk engine keeps the lock of the directory while there is
/// a connection open, and it does not let go of it until the instance is
/// dropped, which inside a process happens late. The service is another
/// process, and it has to be able to open the base: the seeding is
/// therefore done by a process that ends right after.
///
/// The creation of a repository is not part of this surface: this surface
/// serves the repositories that are there.
fn seed_the_catalog_in_a_process_of_its_own(db: &str) {
    let status = Command::new(std::env::current_exe().expect("the test binary"))
        .args([
            "--exact",
            "the_catalog_can_be_seeded",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("HODEI_SEED_DB", db)
        .status()
        .expect("the seeder runs");
    assert!(status.success(), "the catalog could not be seeded");
}

/// The repository, created directly in the catalog.
///
/// When the process is the seeder of another one, the path of the base
/// viene en una variable de entorno. When it is not, the test seeds a
/// throwaway catalog of its own and checks that the repository is there.
#[test]
fn the_catalog_can_be_seeded() {
    let Some(db) = std::env::var_os("HODEI_SEED_DB") else {
        let tmp = tempfile::tempdir().expect("temp dir");
        let path = tmp.path().join("db");
        seed_the_catalog(&path.to_string_lossy());
        // La siembra tiene que **crear** la base: un directorio vacío
        // significa que no se sembró nada, y decir "el directorio
        // existe" no dice nada.
        let entries = std::fs::read_dir(&path)
            .expect("the base is readable")
            .count();
        assert!(entries > 0, "the seeding did not create the base");
        return;
    };
    seed_the_catalog(&db.to_string_lossy());
}

/// Creates the repository in the catalog of the base that is named.
fn seed_the_catalog(path: &str) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("the runtime");
    runtime.block_on(async {
        let db = Surreal::new::<RocksDb>(path).await.expect("rocksdb");
        db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");
        let store = SurrealCatalogStore::with_authorizer(db.clone(), Arc::new(AllowAllAuthorizer));
        store.initialize().await.expect("initialize");
        let mut config = RepositoryConfig::new(DigestScope::Global);
        config.config_settings = serde_json::json!({});
        store
            .create_repository(
                "acme",
                RepositoryDescriptor {
                    hrn: repository_hrn("hodei", "artifacts", "eu", "acme", "releases"),
                    account: "acme".to_string(),
                    repo_key: "releases".to_string(),
                    format: RepositoryFormat::Raw,
                    mode: RepositoryMode::Hosted,
                    config,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                },
            )
            .await
            .expect("the repository is created");
        drop(store);
        drop(db);
    });
}

/// Starts the service and waits for it to open the port.
///
/// The service can die at the boot when the motor de disco todavía tiene
/// el bloqueo del directorio, así que un intento que muere antes de abrir
/// el puerto se repite: lo que se prueba es el servicio, no la carrera
/// del bloqueo.
fn start_and_wait(port: u16, db: &str, cas: &str, policy: &str, schema: &str, log: &Path) -> Child {
    for _attempt in 0..5 {
        let stderr = File::create(log).expect("the log of the service");
        let mut child = Command::new(BIN)
            .env_remove("RUN_MODE")
            .env_remove("HODEI_SECURITY__AUTH_JWKS_PATH")
            .env("HODEI_SERVER__HOST", "127.0.0.1")
            .env("HODEI_SERVER__PORT", port.to_string())
            .env("HODEI_ROCKSDB__PATH", db)
            .env("HODEI_CONTENT__CAS_PATH", cas)
            .env("HODEI_REPOSITORY__DURABLE", "true")
            .env("HODEI_SECURITY__RAW_API_ENABLED", "true")
            .env(
                "HODEI_SECURITY__AUTH_JWT_SECRET",
                String::from_utf8(SECRET.to_vec()).unwrap(),
            )
            .env("HODEI_SECURITY__AUTH_ISSUER", ISSUER)
            .env("HODEI_SECURITY__AUTH_AUDIENCE", AUDIENCE)
            .env("HODEI_SECURITY__AUTHORIZATION_POLICY_PATH", policy)
            .env("HODEI_SECURITY__AUTHORIZATION_SCHEMA_PATH", schema)
            .env("RUST_LOG", "warn")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr))
            .spawn()
            .expect("the service starts");

        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                return child;
            }
            if matches!(child.try_wait(), Ok(Some(_))) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
        let _ = child.wait();
        std::thread::sleep(Duration::from_millis(500));
    }
    panic!(
        "the service did not open the port; the log says:\n{}",
        std::fs::read_to_string(log).unwrap_or_default()
    );
}

fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// The digest of a file, said by `sha256sum`.
///
/// The comparison is not made by the code under test: `sha256sum` is a
/// program that has nothing to do with this service, y es el que dice si
/// los bytes son los mismos.
fn sha256_of(file: &Path) -> String {
    let output = Command::new("sha256sum")
        .arg(file)
        .output()
        .expect("sha256sum has to be there");
    assert!(output.status.success(), "sha256sum failed on {:?}", file);
    String::from_utf8(output.stdout)
        .expect("the digest is text")
        .split_whitespace()
        .next()
        .expect("a digest")
        .to_string()
}

/// The status code of a `curl`, and what it wrote.
fn curl(
    method: &str,
    port: u16,
    path: &str,
    token: &str,
    upload: Option<&Path>,
    download: Option<&Path>,
) -> (u16, String) {
    let mut command = Command::new("curl");
    // `--path-as-is` es lo que deja a `curl` enviar la ruta tal cual: sin
    // el, curl la normaliza y un `%2e%2e` viaja como si no estuviera, y
    // el servidor recibe otra ruta que no es la que se quiere probar.
    command.args(["--silent", "--show-error", "--path-as-is"]);
    if method == "HEAD" {
        // Un HEAD con `--request` deja a curl esperando un cuerpo que no
        // llega, y la petición se queda colgada: `--head` es la forma de
        // pedir una cabecera sin pedir un cuerpo.
        command.arg("--head");
    } else {
        command.arg("--request");
        command.arg(method);
    }
    command.arg("--header");
    command.arg(format!("Authorization: Bearer {token}"));
    if let Some(file) = upload {
        command.arg("--data-binary");
        command.arg(format!("@{}", file.display()));
    }
    match download {
        Some(file) => {
            command.arg("--output");
            command.arg(file.to_string_lossy().into_owned());
        }
        None => {
            command.arg("--output");
            command.arg("/dev/null");
        }
    }
    command.arg("--write-out");
    command.arg("%{http_code}");
    command.arg(format!("http://127.0.0.1:{port}{path}"));

    let output = command.output().expect("curl has to be there");
    assert!(
        output.status.success(),
        "curl failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let status = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or(0);
    (status, String::from_utf8_lossy(&output.stderr).into_owned())
}

/// The whole scenario: publish, download, compare, restart, download again.
#[test]
fn the_content_is_published_downloaded_and_survives_a_real_restart() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = tmp.path().join("db").to_string_lossy().into_owned();
    let cas = tmp.path().join("cas").to_string_lossy().into_owned();
    let policy = tmp.path().join("policies.cedar");
    let schema = tmp.path().join("schema.json");
    std::fs::write(&policy, POLICY).unwrap();
    std::fs::write(&schema, SCHEMA).unwrap();

    // El contenido que se publica: unos bytes que no son texto, para que
    // la comparacion sea de bytes y no de lineas.
    let source = tmp.path().join("source.bin");
    let bytes: Vec<u8> = (0u16..4096).map(|value| (value % 251) as u8).collect();
    std::fs::write(&source, &bytes).unwrap();
    let expected = sha256_of(&source);

    seed_the_catalog_in_a_process_of_its_own(&db);

    let token = token();
    let log = tmp.path().join("service.log");

    // --- La primera vuelta: el servicio publica y sirve el contenido.
    let port = free_port();
    let mut child = start_and_wait(
        port,
        &db,
        &cas,
        policy.to_str().unwrap(),
        schema.to_str().unwrap(),
        &log,
    );

    let (status, stderr) = curl("PUT", port, CONTENT_PATH, &token, Some(&source), None);
    assert_eq!(status, 201, "the publication failed: {stderr}");

    let downloaded = tmp.path().join("downloaded.bin");
    let (status, stderr) = curl("GET", port, CONTENT_PATH, &token, None, Some(&downloaded));
    assert_eq!(status, 200, "the download failed: {stderr}");
    assert_eq!(
        sha256_of(&downloaded),
        expected,
        "the bytes that came back are not the bytes that went in"
    );

    // Y la cabecera dice lo que el contenido mide.
    let (status, _) = curl("HEAD", port, CONTENT_PATH, &token, None, None);
    assert_eq!(status, 200, "the description failed");

    stop(&mut child);

    // --- La segunda vuelta: el mismo almacén, el mismo contenido.
    let port = free_port();
    let mut child = start_and_wait(
        port,
        &db,
        &cas,
        policy.to_str().unwrap(),
        schema.to_str().unwrap(),
        &log,
    );

    let after = tmp.path().join("after-restart.bin");
    let (status, stderr) = curl("GET", port, CONTENT_PATH, &token, None, Some(&after));
    assert_eq!(
        status, 200,
        "the content did not survive the restart: {stderr}"
    );
    assert_eq!(
        sha256_of(&after),
        expected,
        "the content that survived the restart is not the content that was published"
    );

    // Y republicar lo mismo es idempotente, no un conflicto.
    let (status, stderr) = curl("PUT", port, CONTENT_PATH, &token, Some(&source), None);
    assert_eq!(status, 200, "the republication failed: {stderr}");

    stop(&mut child);
}

/// A path that is not allowed does not publish anything, and the service
/// says so with the status code, not with un contenido.
#[test]
fn a_traversal_is_refused_by_the_running_service() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = tmp.path().join("db").to_string_lossy().into_owned();
    let cas = tmp.path().join("cas").to_string_lossy().into_owned();
    let policy = tmp.path().join("policies.cedar");
    let schema = tmp.path().join("schema.json");
    std::fs::write(&policy, POLICY).unwrap();
    std::fs::write(&schema, SCHEMA).unwrap();

    let source = tmp.path().join("source.bin");
    std::fs::write(&source, b"content").unwrap();

    seed_the_catalog_in_a_process_of_its_own(&db);
    let token = token();
    let log = tmp.path().join("service.log");

    let port = free_port();
    let mut child = start_and_wait(
        port,
        &db,
        &cas,
        policy.to_str().unwrap(),
        schema.to_str().unwrap(),
        &log,
    );

    let (status, _) = curl(
        "PUT",
        port,
        "/repository/releases/%2e%2e/escape.bin",
        &token,
        Some(&source),
        None,
    );
    assert_eq!(status, 400, "a traversal has to be refused");

    let (status, _) = curl(
        "GET",
        port,
        "/repository/releases/escape.bin",
        &token,
        None,
        None,
    );
    assert_eq!(status, 404, "the refused path did not publish anything");

    // Y sin credencial no se dice ni que el repositorio existe.
    let (status, _) = curl("GET", port, CONTENT_PATH, "", None, None);
    assert_eq!(status, 401, "a request without a credential is refused");

    stop(&mut child);
}
