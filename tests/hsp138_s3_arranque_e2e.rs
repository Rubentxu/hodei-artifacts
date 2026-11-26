//! El arranque productivo sobre el backend S3 (HSP-138).
//!
//! Aquí se prueba **la elección**, no el backend: la selección ya pasó su
//! contrato de paridad en `crates/hodei-hub-s3/tests`. Lo que este fichero
//! certify es que el binario normal arranca contra el object store según
//! la configuración, que un almacén inaccesible **para el arranque con
//! error explícito y no escribe un byte en `cas_path`**, que raw y OCI
//! reales publican y descargan sobre un cubo conservando su SHA256, que
//! el reinicio con el mismo catálogo y el mismo cubo conserva los datos,
//! y que el GC usa el backend elegido (la herramienta `hodei-recovery`,
//! y su negativa sobre S3, se retiró en HSP-314 F4).
//!
//! El servidor S3 es un RustFS propio de la suite: si ya esta vivo se
//! reutiliza, si no se levanta. El estado se separa por cubo, único por
//! proceso. El servicio corre sobre el perfil de grafo (un namespace de
//! Dgraph por despliegue); el catalogo es del grafo y los bytes son del cubo.
//!
//! Las credenciales del SDK viajan **por el entorno del proceso hijo**,
//! que es exactamente la cadena de proveedores que la configuración
//! promete y que nunca un campo de TOML: una de las cosas que este
//! fichero comprueba es que el log del servicio no las repite.

mod support;

use std::net::TcpStream;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use aws_sdk_s3::Client;
use serde_json::Value;
use support::{Deployment, FIRST_ACCOUNT, FIRST_REPOSITORY};

/// El medio tipo del manifiesto OCI: la via por la que una capa vive.
const OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";

/// El seeder del catalogo (repositorios por cuenta).
///
/// `Deployment::seed` reinvoca **este** binario de pruebas con
/// `--exact the_catalog_can_be_seeded`: sin esta funcion el seed
/// fallaria, y toda la suite se iría detrás con él.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// La imagen del servidor S3, fijada por digest: la misma de la conformidad
/// de HSP-139.
///
/// Es RustFS y no LocalStack porque el barrido del GC del perfil de grafo es
/// la pasada coordinada, y esta borra con `DeleteObject` condicionado por
/// `If-Match`: LocalStack 4.14 contesta `501 NotImplemented` a ese
/// encabezado y un servidor que no lo implementa no puede probar el
/// barrido.
const S3_IMAGE: &str =
    "docker.io/rustfs/rustfs@sha256:0dff1515b3c52b5ce8c5df2263420c7f93ae1c5d07cca79e19e57cff86b84c85";

/// El nombre del contenedor de esta suite. Es propio y no el de la serie
/// HSP-139: la credencial del servidor es el centinela de esta prueba.
fn container_name() -> String {
    std::env::var("HSP138_S3_CONTAINER").unwrap_or_else(|_| "hodei-hsp138-rustfs".to_string())
}

/// El volumen con nombre donde RustFS guarda los objetos.
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

/// El endpoint S3 del contenedor de la suite, levantandolo si hace falta.
///
/// Un contenedor que ya esta en marcha se respeta aunque aun no sirva
/// --puede estar arrancando--, y solo se sustituye el que no vive. La clave
/// secreta del servidor es el centinela: es la que el servicio lleva en su
/// entorno y la que se busca en su log.
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
            // root dentro del contenedor: el volumen lo creo docker como
            // root y el usuario por defecto de la imagen no puede escribir.
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

/// El cubo de este proceso: dos ejecuciones paralelas no se pisan.
fn the_bucket() -> String {
    format!("hsp138-arranque-{}", std::process::id())
}

/// La credencia con la que el servicio de pruebas va al cubo. No es un
/// secreto --es la del servidor de pruebas-- pero su valor se busca en el log del
/// servicio como se buscara la clave real de un bucket policy: si el
/// servicio escribe el valor que el SDK le dio, el log es una fuga.
const SENTINEL: &str = "hsp138-sentinel-secret-not-for-logs";

/// El cliente S3 de la observacion externa: lo que el servicio escribe
/// en el cubo se comprueba desde fuera del servicio, no por lo que el
/// servicio dice de si mismo.
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

/// Crea el cubo, tolerando el que ya existe por otra ejecucion.
async fn a_bucket(client: &Client, name: &str) {
    let _ = client.create_bucket().bucket(name).send().await;
}

/// Las claves vivas bajo el prefijo de los blobs.
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

/// Si la clave existe: la presencia de un blob se comprueba de fuera,
/// y su ausencia despues del sweep tambien.
async fn a_key_exists(client: &Client, bucket: &str, key: &str) -> bool {
    client
        .head_object()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .is_ok()
}

/// La clave que el backend S3 da a un digest: el prefijo del almacen,
/// dos niveles por seguridad de listado, y el hex.
fn the_key_of(digest_hex: &str) -> String {
    format!(
        "blobs/{}/{}/{}",
        &digest_hex[..2],
        &digest_hex[2..4],
        digest_hex
    )
}

/// El despliegue con la administracion abierta: la politica extendida y
/// el esquema con el recurso `Admin`, como en la serie de GC.
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

/// El entorno del servicio que elige el cubo: backend, endpoint, region
/// y las credenciales **por el proceso**, no por la configuracion. La
/// clave secreta es el centinela: se buscara en el log.
fn the_s3_environment(bucket: &str) -> Vec<(String, String)> {
    vec![
        ("HODEI_CONTENT__BACKEND".to_string(), "s3".to_string()),
        ("HODEI_CONTENT__S3__BUCKET".to_string(), bucket.to_string()),
        // El endpoint se da con su esquema: la config tambien admite el
        // bare host:port, y aqui interesa el caso literal del servidor de pruebas.
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

/// El digest hex de un contenido.
fn the_digest(content: &[u8]) -> String {
    hodei_evidence::ContentDigest::of_bytes(content).value
}

/// Publica un blob crudo por la distribucion one-shot, y devuelve su
/// digest: la via por la que el contenido entra al cubo es la misma de
/// siempre, y por eso se usa la de la serie de GC.
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

/// Publica un fichero por la superficie raw y devuelve el digest del
/// contenido que nombra su `ETag`-like: la publicacion de la via cruda.
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

/// Descarga la ruta raw y devuelve los bytes servidos.
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

/// Una peticion administrativa: codigo y JSON del cuerpo.
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

/// El health del servicio que arranco: `/health/ready` responde.
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

/// AC1 (la mitad que el harness no puede medir): un backend elegido e
/// inaccesible **no arranca el servicio y no escribe en el directorio**.
///
/// No pasa por `Deployment::start`: ese arnes reintentaria y
/// terminaria en panic con el mensaje del harness, y aqui lo que se
/// observa es la salida real del binario --codigo, stderr-- y el estado
/// del `cas_path` despues del intento. Un endpoint muerto contesta al
/// instante (conexion rechazada) y el preflight del arranque la traduce
/// en un error explicito: el servicio muere antes de escuchar.
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
        // El backend elegido, con un cubo que no puede existir: el
        // endpoint muerto contesta "conexion rechazada" y el preflight
        // lo traduce en error; el timeout de 10 s es el techo.
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
    // Y el directorio que la config nombra sigue intacto: no fue
    // sustituto de nada. Bajo filesystem el arranque lo crearia; aqui
    // ni eso: el backend elegido no lo toca.
    assert!(
        !cas.exists(),
        "AC1: el cas_path no se toca cuando el backend elegido es S3"
    );
}

/// AC1 y AC2 juntos, el caso feliz: el binario normal arranca contra el
/// cubo, el raw y el OCI reales publican y descargan con su SHA256, los
/// bytes viven **en el cubo** y no en el directorio, el reinicio con el
/// mismo catalogo y el mismo cubo conserva, y el GC borra desde el cubo.
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

    // El arranque: si el preflight fallara, el harness panicaria con el
    // log, y ahi se veria. Si arranca, `/health/ready` contesta 200 --
    // la readiness positiva pedida por AC1 es este camino.
    let mut service = deployment.start(&pairs);
    assert_eq!(the_health(service.port), 200, "la readiness sobre S3");

    let alice = support::token(FIRST_ACCOUNT, "alice");
    let raw = b"raw content that lives in the bucket".to_vec();
    let raw_digest = publish_raw(&deployment, service.port, &raw);
    assert_eq!(raw_digest, the_digest(&raw), "el SHA256 que se conserva");

    // OCI: la capa y el manifiesto entran por la distribucion.
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

    // La descarga, por la via cruda, byte a byte igual.
    let back = download_raw(
        &deployment,
        service.port,
        &deployment.directory.path().join("got.bin"),
    );
    assert_eq!(back, raw, "AC2: lo que se descarga es lo que se publico");

    // Y los bytes estan **en el cubo**, en la clave que el backend usa,
    // con el digest que el contenido dice.
    for digest in [&raw_digest, &layer, &config] {
        assert!(
            a_key_exists(&client, &bucket, &the_key_of(digest)).await,
            "AC2: el contenido {digest} tiene que vivir en el cubo"
        );
    }
    // El directorio que la config nombra no es autoridad de nada: no
    // existe siquiera, porque el arranque no lo abrio.
    assert!(
        !Path::new(&deployment.cas).exists(),
        "AC1: un despliegue S3 no escribe ni crea su cas_path"
    );
    // Las credenciales no salen en el log: la cadena del SDK las dio al
    // proceso, y el servicio no las repite. El centinela es el valor
    // que el entorno del servicio lleva como clave secreta; si el log
    // lo contiene, el servicio escribe credenciales.
    let log = deployment.the_log();
    assert!(
        !log.contains(SENTINEL),
        "AC4: el log del servicio no escribe la credencial con la que trabaja"
    );

    // El reinicio: el mismo catalogo (la misma base), el mismo cubo.
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

    // El GC sobre el backend elegido: un objeto que ningun registro del
    // catalogo nombra se va del cubo cuando vence la gracia. El huerfano
    // se planta **desde fuera**, escribiendo la clave en el cubo con el
    // cliente observador: subirlo por la distribucion crearia su fila en
    // el catalogo, y un blob con fila es una raiz, no un huerfano. Es la
    // misma planta que la serie de GC escribia a mano en el directorio.
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
    // El barrido corre detached: la respuesta es el reconocimiento, no
    // la ultima delete_object contestada. Se espera a que el cubo lo
    // cuente --con techo--, como en la serie de GC sobre el directorio.
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

    // Y lo publicado sigue viviendole al servicio, con el cubo como
    // unica autoridad: el listado externo muestra los blobs vivos.
    let keys = the_keys_of(&client, &bucket).await;
    assert!(
        keys.contains(&the_key_of(&raw_digest)) && keys.contains(&the_key_of(&layer)),
        "el cubo guarda lo publicado: {keys:?}"
    );
    support::stop(&mut again);
}
