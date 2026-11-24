//! HSP-209: el humo del perfil productivo sobre la autoridad única.
//!
//! AC#1 de HSP-209, probado con el **binario real** y el proceso real:
//! un despliegue con `RUN_MODE=dgraph` (el fichero `config/dgraph.toml`
//! de la distribución) arranca contra el alpha vivo y el object store
//! vivo, y sirve Raw, OCI y confianza **juntos** — el catálogo, la
//! sesión de push, la decisión de promoción y los bytes viven en las dos
//! autoridades declaradas, no en una tercera. Lo que esta suite fija:
//!
//! 1. **El arranque no toca RocksDB.** El perfil nombra un path embebido
//!    (`[rocksdb]` existe en el esquema y aquí señala un directorio
//!    fresco bajo el árbol de evidencia); tras el arranque exitoso ese
//!    path sigue sin existir y el log no abre el motor. El fallback está
//!    rechazado con nombre: la misma config con `db_type = "rocksdb"`
//!    muere en el arranque citando el rechazo, sin abrir puerto.
//! 2. **La superficie vertical funciona end-to-end sobre el grafo**:
//!    crear el repositorio (catálogo Dgraph), publicar por raw y
//!    descargar el mismo digest (bytes en S3, metadato en el grafo),
//!    abrir una sesión OCI (journal Dgraph) y promover (decisión durable
//!    en el grafo). Cada paso se relee **en el servidor** por su clave
//!    propia — nunca por censos globales: el alpha es compartido con las
//!    suites de HSP-154/155.
//! 3. **La evidencia queda escrita.** El log del servicio, las consultas
//!    DQL y las respuestas HTTP se archivan bajo `target/uat/hsp209/`
//!    junto a esta sentencia: un PASS sin ficheros en ese directorio no
//!    es un PASS.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT` como todas las suites del stack:
//! sin el alpha declarado, la suite se salta a sí misma (exit 0). El
//! object store es el contenedor compartido de la serie HSP-116 con un
//! cubo único por proceso: si el proveedor S3 real exigiera credenciales
//! humanas, su precondición queda documentada en `evidencia.md` — el
//! humo usa localstack como prueba funcional, igual que HSP-138.
//!
//! **Carril**: `--test-threads=1` (el alpha compartido y la guarda
//! `@upsert` de las claves; lección HSP-190/219).

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

/// El marcador con el que la contraseña del alpha se retira del log
/// archivado y de cualquier TOML de evidencia.
const PASSWORD_SENTINEL: &str = "__HSP209_PASSWORD_IN_ENVIRONMENT_ONLY__";

/// El directorio de evidencia de esta tarea, en la raíz del crate.
fn evidence_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/uat/hsp209")
}

/// La config del alpha desde el entorno de IT (la forma que `connect`
/// exige, igual que hsp187): endpoint con puerto, TLS con su CA y su
/// server name declarados, credenciales completas.
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
    // La contraseña real solo vive en el struct del proceso hijo; el TOML
    // con el centinela es lo que la evidencia puede mostrar.
    let mut config =
        GraphClientConfig::from_toml_str(&text).expect("the IT configuration validates");
    if let Some(credentials) = config.credentials.as_mut() {
        credentials.password = password;
    }
    Some(config)
}

/// Una conexión nueva al alpha, para la lectura de verificación.
async fn fresh_client() -> GraphClient {
    let config = config_from_env().expect("the suite is gated");
    let mut client = GraphClient::connect(config).await.expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    client
}

/// Un nonce por ejecución: ninguna prueba se encuentra con lo que otra dejó
/// sobre el alpha compartido.
fn nonce() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

/// El repositorio de esta ejecución.
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

/// El endpoint S3 del contenedor compartido, levantándolo si hace falta
/// (la política de reutilización es la de HSP-116/138).
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

/// El cubo de esta ejecución: dos ejecuciones paralelas no se pisan.
fn the_bucket() -> String {
    format!("hsp209-smoke-{}", std::process::id())
}

/// Las credenciales del SDK del proceso hijo. No son un secreto —es
/// LocalStack— pero su valor se busca en el log archivado como se
/// buscaría el de un bucket policy: si el servicio repite en el log lo
/// que el SDK le dio, el log es una fuga.
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

/// La clave S3 de un digest, la convención del CAS: `blobs/aa/bb/hex`.
fn blob_key(digest: &str) -> String {
    let hex = digest.strip_prefix("sha256:").unwrap_or(digest);
    format!("blobs/{}/{}/{}", &hex[0..2], &hex[2..4], hex)
}

/// El digest hex de un contenido.
fn the_digest(content: &[u8]) -> String {
    hodei_evidence::ContentDigest::of_bytes(content).value
}

// ————————————————————————————————————————————————————— El arranque real

/// El puerto que el log dice que el servicio ocupó. El perfil del grafo
/// loguea en JSON, así que la dirección va seguida de la comilla que
/// cierra el mensaje: se corta en el primer carácter que no es de
/// dirección.
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

/// Quita las secuencias de escape del log (la del arnés embebido es la
/// misma regla: un puerto con un ANSI pegado no se lee como número).
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

/// El proceso del perfil productivo, con la config **emitida**
/// (`config/dgraph.toml` vía `RUN_MODE=dgraph`) y el entorno del
/// operador. A diferencia del arranque embebido del arnés, el perfil del
/// grafo decide su propio `repository.durable` (falso, y el humo lo
/// levanta a true solo donde la superficie legacy lo exige) y abre su
/// bundle de seguridad con los ficheros que `Deployment` ya escribió.
struct GraphBoot {
    port: u16,
    child: std::process::Child,
    log: PathBuf,
}

impl GraphBoot {
    fn launch(
        evidence: &Path,
        cas_path: &Path,
        rocksdb_path: &Path,
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
            .env("HODEI_ROCKSDB__PATH", rocksdb_path)
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
            // El operador de IT apunta el perfil al alpha real por el
            // entorno — la misma cadena que HODEI_* permite en producción.
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

    /// El log con los centinelas rojos, listo para archivar: ni la
    /// contraseña del alpha ni el secret del SDK viajan al fichero.
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

/// Escribe la evidencia de una pasada: cabecera con lo observable (sin
/// secretos) y el log del servicio saneado.
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

/// La lectura propia en el grafo: DQL por clave exacta sobre un
/// predicado con índice `@index(exact)`. Devuelve la consulta y si la
/// clave existe. **Nunca censos globales**: el alpha es compartido.
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

/// La política del humo: la del arnés (publicar, descargar, promover)
/// más la creación y lectura de repositorios para alice, que el perfil
/// del grafo sí sirve (el catálogo Dgraph es la autoridad del metadato).
fn the_policy() -> String {
    let mut policy = support::the_policy();
    for action in ["CreateRepository", "ReadRepository"] {
        policy.push_str(&format!(
            "permit(principal == Iam::User::\"hrn:hodei:iam::{FIRST_ACCOUNT}:User/alice\", action == Artifacts::Action::\"{action}\", resource);\n"
        ));
    }
    policy
}

/// El fichero de política de confianza del humo: `default` sin
/// requisitos — lo que todo software cumple (el registro vacío del
/// perfil niega con `PolicyUnavailable`, y el humo necesita la decisión
/// concedida para leer la clave durable en el grafo).
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

/// Escribe un cuerpo JSON en el directorio del despliegue y devuelve su
/// ruta: el arnés manda los cuerpos por fichero (`--data-binary @file`).
fn write_json(deployment: &Deployment, name: &str, body: &Value) -> PathBuf {
    let path = deployment.directory.path().join(name);
    std::fs::write(&path, body.to_string()).expect("the JSON body is written");
    path
}

// ——————————————————————————————————————————————————————————————————— tests

/// S0: la puerta de la suite. Sin alpha declarado, todo lo demás se salta
/// (exit 0), igual que el resto de suites gated del stack.
#[test]
fn the_suite_is_gated_on_a_real_graph() {
    match config_from_env() {
        Some(_) => println!("HSP-209 smoke: alpha declarado, la suite corre"),
        None => println!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set"),
    }
}

/// AC#1: el arranque productivo completo. El binario real, con la config
/// emitida y el alpha + localstack vivos:
///
/// - el proceso arranca y anuncia el perfil del grafo en el log,
/// - el path embebido que el perfil nombra **no existe** tras el arranque,
/// - el log no abre el motor (sin "RocksDB" en claro en el camino de
///   arranque del grafo),
/// - el bundle de seguridad del perfil (ficheros Cedar del arnés,
///   esquema desde fichero — el persistido sobre grafo es el bloqueador
///   registrado) permite la superficie completa.
///
/// y la superficie vertical **junta** sobre la autoridad única:
/// crear repositorio → publicar raw → descargar el mismo digest →
/// objeto en S3 por su clave de CAS → sesión OCI → promoción → decisión
/// durable. Cada hecho, releído en el servidor por su clave propia.
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
    // El bundle de seguridad del perfil: los ficheros Cedar del arnés y
    // el esquema **desde fichero** — la persistencia del esquema sobre el
    // grafo es el bloqueador registrado, y el gate (los cinco campos
    // juntos o el arranque muere) se abre aquí igual que en producción.
    let cas = evidence.join("cas-superficie");
    let rocksdb_claimed = evidence.join("rocksdb-debe-no-existir");
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
        &rocksdb_claimed,
        &bucket,
        &[
            ("HODEI_REPOSITORY__DURABLE", "true"),
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

    // La prueba de arranque: el log anuncia el perfil del grafo, y el
    // path embebido que el fichero nombra no fue tocado.
    let log = boot.redacted_log();
    assert!(
        log.contains("Bootstrap completed successfully on the graph profile"),
        "the boot must announce the graph profile:\n{log}"
    );
    assert!(
        !rocksdb_claimed.exists(),
        "the graph profile must not create the embedded path it names"
    );
    // S0 observable del rechazo: el motor embebido no se abre. El eco
    // de la config al arrancar (main.rs) nombra el path siempre — lo que
    // NO puede aparecer en el perfil del grafo es la apertura de la
    // tienda: "Initializing SurrealDB with RocksDB".
    assert!(
        !log.contains("Initializing SurrealDB with RocksDB"),
        "the graph profile must never open the embedded engine:\n{log}"
    );

    // 1. El repositorio: creado por la API, persistido en el grafo.
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

    // 2. Raw: publicar un fichero por la superficie cruda.
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

    // 3. El byte está en S3 por su clave de CAS — la autoridad de los
    // bytes es el cubo, no el disco local.
    let in_s3 = head_object(&s3, &bucket, &blob_key(&digest)).await;
    assert!(in_s3, "the blob must live in the S3 CAS");

    // 4. OCI: la sesión de push abre y su journal vive en el grafo.
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

    // 5. Confianza: promover el digest publicado.
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

    // 6. La lectura en el servidor, por clave propia y solo por clave
    // propia (membresía; jamás censos sobre el alpha compartido).
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
                    "path_embebido_creado": rocksdb_claimed.exists(),
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

/// AC#1 (la mitad explícita del rechazo): el **mismo** arranque real que
/// arriba, declarando el motor embebido (`HODEI_DATABASE__DB_TYPE=rocksdb`
/// — la forma en que los perfiles emitidos reclaman RocksDB) sobre el
/// perfil del grafo, debe morir citando el rechazo, sin abrir puerto y
/// sin crear el path.
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
    let rocksdb_claimed = evidence.join("rocksdb-reclamado-no-debe-existir");
    let outcome = GraphBoot::launch(
        &evidence,
        &cas,
        &rocksdb_claimed,
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
    assert!(
        !rocksdb_claimed.exists(),
        "the refused boot must not create the embedded path"
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
                "path_creado": rocksdb_claimed.exists(),
            }),
        )],
        Some(&redacted),
    );
}
