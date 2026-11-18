//! El arnes de LocalStack para las pruebas de integracion S3.
//!
//! Un contenedor compartido por todas las pruebas de este crate: se
//! reutiliza si ya esta vivo y se levanta si no. La imagen esta fijada a
//! 4.14, la ultima comunitaria que no pide token de licencias. El puerto
//! es efimero y se descubre con `docker inspect`; el estado no se toca de
//! una prueba a otra porque cada prueba usa su propio cubo, único por
//! proceso.
//!
//! Esto sustituye al emulador de AWS en modo embebido: su grafo compila
//! RustPython (roto en esta maquina) y unos trescientos crates de
//! servicios que agotan la mesa de procesos del compilador. Aqui el SDK
//! es el mismo que en produccion y el protocolo S3 lo sirve un servidor
//! real por red localhost.

use std::io::{Read, Write};
use std::process::Command;

use aws_credential_types::Credentials;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::Region;
use hodei_hub_s3::{S3BlobConfig, S3BlobStore};

const IMAGE: &str = "localstack/localstack:4.14";

/// Contra que banco corre el informe de capacidades: la etiqueta fija,
/// que es la version del servidor de pruebas.
pub const VERSION_NOTE: &str = "localstack/localstack:4.14";

/// Cuanto se le da a LocalStack para servir S3 desde que se lanza o se
/// hereda en marcha. En una maquina cargada, el arranque no cabe en un
/// minuto: se espera el doble de lo razonable y aun se falla a la vista.
const START_GRACE_SECONDS: u64 = 300;

/// El nombre del contenedor compartido. `HSP116_S3_CONTAINER` lo cambia
/// cuando dos ejecuciones de pruebas conviven en la misma maquina y no
/// deben pisarse el servidor S3 entre sí.
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

/// El puerto efimero del contenedor que ya estaba, si responde al health.
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

/// Un GET crudo al health: sin SDK ni dependencias de HTTP de prueba.
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

/// El arnes se levanta una sola vez por proceso: si no, los hilos de un
/// mismo binario de pruebas se pisan con `rm -f` el contenedor que otro
/// acaba de lanzar.
static SHARED_ENDPOINT: tokio::sync::OnceCell<String> = tokio::sync::OnceCell::const_new();

/// Si el contenedor existe y esta en ejecucion, sea nuestro o de otro.
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

/// El endpoint de S3 del contenedor compartido, levantandolo si hace falta.
pub async fn the_endpoint() -> String {
    SHARED_ENDPOINT
        .get_or_init(|| async {
            let endpoint = tokio::task::spawn_blocking(|| {
                let name = container_name();
                if let Some(port) = a_healthy_port() {
                    return Ok::<_, String>(port);
                }
                // Un contenedor en marcha se respeta aunque aun no sirva:
                // puede estar arrancando (el nuestro o el de otra prueba en
                // paralelo); solo se sustituye el que ya no vive. En esta
                // maquina, con carga, LocalStack tarda mas de lo que tarda
                // un portero impaciente en declarar el muerto.
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
/// Un cliente S3 contra el contenedor. Las credenciales son las fijas de
/// LocalStack: no son un secreto, y ni siquiera pasan por el almacen.
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

/// El nombre de cubo, unico por proceso: dos ejecuciones paralelas no se
/// pisan el estado y un listado empieza de verdad vacio.
pub fn a_unique_bucket(prefix: &str) -> String {
    format!("hsp116-{prefix}-{}", std::process::id())
}

/// Crear el cubo, tolerando el que ya exista por otra ejecucion.
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

/// Un almacen S3 sobre un cubo recien creado del contenedor compartido.
/// Levantarlo dos veces sobre el mismo cubo es el restart: el estado
/// vive en el servidor, no en el proceso.
pub async fn a_store(bucket: &str) -> S3BlobStore {
    let client = a_client().await;
    a_bucket(&client, bucket).await;
    S3BlobStore::new(client, S3BlobConfig::new(bucket)).expect("el almacen abre")
}
