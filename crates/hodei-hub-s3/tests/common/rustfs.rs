//! El arnés de RustFS para las pruebas de conformidad S3 (HSP-139).
//!
//! RustFS es un servidor S3 real escrito en Rust (AGPLv3), no un
//! emulador: es el segundo banco de la serie, junto al LocalStack de la
//! HSP-116, para que la conformidad del backend no dependa de la
//! benevolencia de un único simulacro. La imagen se fija **por digest**,
//! no por etiqueta: `latest` es una promesa que cambia de contenido bajo
//! los pies, y un informe de capacidades por versión (AC4) que no dice
//! contra qué bytes exactos corrió no es un informe. El digest aquí
//! escrito es el que `rustfs/rustfs:latest` tenía el día que se escribió
//! esta prueba.
//!
//! El patrón de contenedor compartido es el del módulo `localstack`:
//! se reutiliza si ya está vivo, se levanta si no, el puerto es efímero
//! y se descubre con `docker inspect`, y el estado se separa por cubo
//! único por proceso. Dos detalles son propios de este servidor y no del
//! emulador:
//!
//! - El almacenamiento es un **volumen con nombre** de docker, no un
//!   bind-mount: RustFS corre su servidor sobre `/data` y rechaza el
//!   bind-mount de un directorio del host que no pertenece al usuario del
//!   proceso dentro del contenedor ("File access denied"); con volumen
//!   gestionado por docker la propiedad la decide docker.
//! - La credencial se pasa por el entorno del servidor
//!   (`RUSTFS_ACCESS_KEY` / `RUSTFS_SECRET_KEY`), que es lo que el
//!   cliente firma después. Son fijas y de prueba: como las de
//!   LocalStack, no viajan por el almacén ni aparecen en ningún log del
//!   servicio bajo prueba.

use std::io::{Read, Write};
use std::process::Command;

use aws_credential_types::Credentials;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::Region;
use hodei_hub_s3::{S3BlobConfig, S3BlobStore};

/// La imagen, fijada por digest de manifiesto (índice de plataforma).
const IMAGE: &str =
    "rustfs/rustfs@sha256:0dff1515b3c52b5ce8c5df2263420c7f93ae1c5d07cca79e19e57cff86b84c85";

/// La etiqueta de la que se resolvió ese digest, solo para el informe:
/// lo que fija la prueba es el digest, lo que nombra el informe es esto.
pub const VERSION_NOTE: &str = "rustfs/rustfs:latest (digest 0dff1515)";

/// El puerto S3 interno del contenedor RustFS.
const INTERNAL_PORT: &str = "9000";

/// La credencial fija del servidor de pruebas. No es un secreto: igual
/// que `test`/`test` de LocalStack, es lo que el propio contenedor
/// publica en su entorno.
pub const ACCESS_KEY: &str = "test";
pub const SECRET_KEY: &str = "testsecret";

/// Cuanto se le da a RustFS para servir S3 desde que se lanza.
const START_GRACE_SECONDS: u64 = 180;

/// El nombre del contenedor compartido, independiente del de LocalStack:
/// los dos bancos pueden convivir en la misma máquina sin pisarse.
fn container_name() -> String {
    std::env::var("HSP139_S3_CONTAINER").unwrap_or_else(|_| "hodei-hsp139-rustfs".to_string())
}

/// El volumen con nombre donde RustFS guarda los objetos.
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

/// El puerto efímero del contenedor que ya estaba, si responde al health.
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

/// Un GET crudo al health de RustFS (compatibilidad MinIO) o a su raíz
/// sin firmar: un 403 de la raíz también dice que hay un servidor S3
/// escuchando. No hace falta el SDK para saber que hay puerta.
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

/// El arnés se levanta una sola vez por proceso.
static SHARED_ENDPOINT: tokio::sync::OnceCell<String> = tokio::sync::OnceCell::const_new();

/// Si el contenedor existe y está en ejecución, sea nuestro o de otro.
fn the_container_is_running(name: &str) -> bool {
    docker(&["inspect", "-f", "{{.State.Running}}", name])
        .map(|state| state.trim() == "true")
        .unwrap_or(false)
}

/// Espera a que haya un puerto sano, sondeando cada dos segundos.
fn a_healthy_port_within(seconds: u64) -> Option<String> {
    for _ in 0..(seconds / 2) {
        if let Some(port) = a_healthy_port() {
            return Some(port);
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    a_healthy_port()
}

/// El endpoint S3 del contenedor compartido, levantándolo si hace falta.
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
                    // El volumen con nombre sobrevive entre ejecuciones:
                    // RustFS lo recluta y su estado no se pierde; la
                    // separación entre pruebas la hace el cubo.
                    let _ = docker(&["volume", "create", &volume_name()]);
                    docker(&[
                        "run",
                        "-d",
                        "--name",
                        &name,
                        // root dentro del contenedor: el volumen lo creó
                        // docker como root y el usuario por defecto de la
                        // imagen no puede escribir en él.
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

/// Un cliente S3 contra RustFS con sus credenciales fijas de prueba.
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

/// El nombre de cubo, único por proceso.
pub fn a_unique_bucket(prefix: &str) -> String {
    format!("hsp139-{prefix}-{}", std::process::id())
}

/// Crear el cubo, tolerando el que ya exista por otra ejecución.
///
/// El `create_bucket` de RustFS es transitoriamente brusco bajo carga:
/// con la prueba anterior terminando de soltar conexiones, responde un
/// `service error` que no es ni "ya existe" ni un fallo del almacén.
/// Reintentar con calma es lo que un cliente real haría; reventar el
/// test sería confundir la torpeza del servidor con un defecto propio.
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

/// Un almacén S3 sobre un cubo recién creado del RustFS compartido.
pub async fn a_store(bucket: &str) -> S3BlobStore {
    let client = a_client().await;
    a_bucket(&client, bucket).await;
    S3BlobStore::new(client, S3BlobConfig::new(bucket)).expect("el almacen abre")
}
