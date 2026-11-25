//! El registry de módulos privado de HSP-158 contra el cliente real
//! (`terraform`).
//!
//! El `terraform` 1.10 del entorno resuelve un módulo privado de verdad:
//! escribe una configuración cuya fuente es
//! `127.0.0.1:PORT/hodei/demo/local`, declara el registry en su config de
//! CLI (`host` + `services`, el `ForceHostServices` del disco: el cliente
//! pide el discovery siempre por https y un hub sin TLS declara el
//! servicio así), lleva su credencial como `credentials` del host, y el
//! `init` real pasa por discovery → versiones → download → go-getter, que
//! baja el tarball con la credencial de `netrc` y verifica el
//! `?checksum=sha256:` de la URL contra los bytes que llega. Hace falta
//! `terraform` 1.10.x (en `TF_CLI` o en el `PATH`): sin él, la suite
//! **falla** diciendo que falta. Cada ejecución usa su propio `HOME`, su
//! config de CLI y su `NETRC`: nada se lee ni se escribe fuera del
//! directorio del despliegue.
//!
//! El dialecto es el que fija el cliente (`internal/registry/client.go`,
//! `terraform-svchost/disco` y `go-getter` v1.7 de terraform 1.10.1); el
//! mapa del wire queda registrado en FMT-04 de `uat/UAT-FORMATS.md` y en
//! `target/uat/hsp158/evidencia.md`.

mod support;

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use flate2::write::GzEncoder;
use sha2::Digest as _;
use support::{Deployment, FIRST_ACCOUNT, SECOND_ACCOUNT};

/// Exige el `terraform` 1.10.x del entorno y devuelve la ruta **real** del
/// ejecutable. El shim de asdf resuelve la version desde el directorio de
/// trabajo, y el despliegue no la tiene: se resuelve una vez, desde la
/// carpeta del repo, y el cliente corre con el binario absoluto.
fn require_terraform() -> String {
    let binary = std::env::var("TF_CLI").unwrap_or_else(|_| "terraform".to_string());
    let version = Command::new(&binary)
        .arg("version")
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
    let line = version.lines().next().unwrap_or_default().to_string();
    assert!(
        line.starts_with("Terraform v1.10."),
        "the terraform suite pins the client terraform 1.10.x, found: {line}"
    );
    let resolved = Command::new("asdf")
        .args(["which", "terraform"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|path| !path.is_empty() && Path::new(path).is_file());
    resolved.unwrap_or(binary)
}

/// El resultado de una ejecución del cliente.
struct Run {
    /// Si el cliente terminó bien.
    ok: bool,
    /// Lo que dijo, por stdout y por stderr.
    output: String,
}

impl Run {
    /// El texto si terminó bien, y un fallo con todo el resto si no.
    fn expect(&self, what: &str) -> String {
        assert!(self.ok, "{what} failed:\n{}", self.output);
        self.output.clone()
    }
}

/// Un `terraform` aislado: su directorio de configuración, su config de
/// CLI y su `NETRC`.
struct Client {
    /// La ruta del ejecutable.
    binary: String,
    /// El directorio del despliegue.
    root: PathBuf,
    /// El directorio de trabajo de esta configuración.
    work: PathBuf,
    /// El host y puerto del hub, tal y como la fuente lo nombra.
    host: String,
}

impl Client {
    /// Un cliente limpio para una cuenta. La config de CLI declara el
    /// servicio `modules.v1` del host (el discovery por https no es
    /// alcanzable en un hub http) y la credencial del host; el `NETRC`
    /// lleva la credencial que go-getter manda al bajar el artefacto.
    fn new(root: &Path, name: &str, port: u16, token: &str, the_galaxy_terraform: String) -> Self {
        let work = root.join(format!("tf-{name}"));
        std::fs::create_dir_all(&work).expect("the work directory");
        let host = format!("127.0.0.1:{port}");
        let config = format!(
            "host \"{host}\" {{\n  services = {{\n    \"modules.v1\" = \"http://{host}/terraform/modules-hosted/v1/modules/\"\n  }}\n}}\n\ncredentials \"{host}\" {{\n  token = \"{token}\"\n}}\n"
        );
        std::fs::write(work.join("terraform.rc"), &config).expect("the CLI config");
        // go-getter busca la entrada de netrc por , que lleva el
        // puerto: la entrada nombra al host con su puerto.
        let netrc = format!("machine 127.0.0.1:{port} login hodei password {token}\n");
        std::fs::write(work.join("netrc"), &netrc).expect("the netrc");
        Self {
            binary: the_galaxy_terraform,
            root: root.to_path_buf(),
            work,
            host,
        }
    }

    /// Escribe la configuración del módulo privado que este cliente
    /// resuelve, con su restricción de versión.
    fn write_config(&self, version: Option<&str>) -> PathBuf {
        let constraint = version
            .map(|version| format!("  version = \"{version}\"\n"))
            .unwrap_or_default();
        let config = format!(
            "module \"demo\" {{\n  source  = \"{}/hodei/demo/local\"\n{}}}\n",
            self.host, constraint
        );
        std::fs::write(self.work.join("main.tf"), &config).expect("the module config");
        self.work.clone()
    }

    /// Ejecuta el `terraform` con su entorno aislado.
    fn run(&self, args: &[&str]) -> Run {
        let output = Command::new(&self.binary)
            .args(args)
            .current_dir(&self.work)
            .env("HOME", &self.root)
            .env("TF_CLI_CONFIG_FILE", self.work.join("terraform.rc"))
            .env("NETRC", self.work.join("netrc"))
            .env("TF_INPUT", "0")
            .env("TF_IN_AUTOMATION", "1")
            .output()
            .expect("terraform runs");
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Run {
            ok: output.status.success(),
            output: text,
        }
    }

    /// `init`: la resolución real contra la superficie.
    fn init(&self) -> Run {
        self.run(&["init", "-input=false", "-no-color"])
    }

    /// El módulo instalado por este cliente: el directorio que el
    /// instalador nombra con la clave del bloque .
    fn installed(&self) -> PathBuf {
        self.work.join(".terraform").join("modules").join("demo")
    }

    /// El manifiesto que el instalador escribe.
    fn manifest(&self) -> String {
        std::fs::read_to_string(
            self.work
                .join(".terraform")
                .join("modules")
                .join("modules.json"),
        )
        .expect("the modules manifest")
    }
}

/// El sha256 de unos bytes, como lo verifica el protocolo.
fn sha256_of(bytes: &[u8]) -> String {
    format!("{:x}", sha2::Sha256::digest(bytes))
}

/// Un miembro ustar escrito como el formato lo lee: cabecera, contenido y
/// el relleno que redondea el registro.
fn the_member_bytes(name: &str, content: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0u8; 512];
    bytes[..name.len()].copy_from_slice(name.as_bytes());
    // El modo va en la cabecera, y go-getter lo respeta al extraer: un
    // modo 0000 es un fichero que el cliente no puede leer.
    bytes[100..108].copy_from_slice(b"0000644\0");
    let size = format!("{:011o}", content.len());
    bytes[124..124 + size.len()].copy_from_slice(size.as_bytes());
    bytes[148..156].copy_from_slice(b"        ");
    bytes[156] = b'0';
    bytes[257..262].copy_from_slice(b"ustar");
    let checksum: u32 = bytes.iter().map(|byte| *byte as u32).sum();
    let checksum = format!("{:06o}\0 ", checksum);
    bytes[148..156].copy_from_slice(checksum.as_bytes());
    let mut member = bytes;
    member.extend_from_slice(content);
    let padding = (512 - (content.len() % 512)) % 512;
    member.extend(std::iter::repeat_n(0u8, padding));
    member
}

/// El tarball de un módulo: los ficheros en la raíz del archivo, como el
/// registry los guarda y go-getter los descomprime.
fn the_module_tarball(root: &Path, name: &str, content: &str) -> Vec<u8> {
    let mut tar = Vec::new();
    tar.extend_from_slice(&the_member_bytes(
        "main.tf",
        format!("# {name}\nvariable \"region\" {{\n  type    = string\n  default = \"eu\"\n}}\n{content}\n")
            .as_bytes(),
    ));
    tar.extend_from_slice(&the_member_bytes(
        "outputs.tf",
        b"output \"region\" {\n  value = var.region\n}\n",
    ));
    tar.extend_from_slice(&[0u8; 512]);
    let mut gzipped = GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzipped.write_all(&tar).expect("the gzip write");
    let bytes = gzipped.finish().expect("the gzip finish");
    let path = root.join(format!("{name}.tar.gz"));
    std::fs::write(&path, &bytes).expect("the tarball");
    bytes
}

/// Una publicación hecha a mano sobre el socket: la superficie de
/// publicación la define el hub (el CLI no sabe publicar módulos).
/// Devuelve el estado y el cuerpo de la respuesta.
#[allow(clippy::too_many_arguments)]
fn publish_module(
    port: u16,
    repository: &str,
    token: &str,
    version: &str,
    bytes: &[u8],
    declared_sha256: Option<&str>,
) -> (u16, String) {
    let mut request = format!(
        "POST /terraform/{repository}/publish/hodei/demo/local/{version} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/gzip\r\n"
    );
    if let Some(declared) = declared_sha256 {
        request.push_str(&format!("X-Hodei-Sha256: {declared}\r\n"));
    }
    request.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        bytes.len()
    ));
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("the socket");
    let _ = stream.write_all(request.as_bytes());
    let _ = stream.write_all(bytes);
    let mut answer = String::new();
    let _ = stream.read_to_string(&mut answer);
    let status = answer
        .lines()
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .and_then(|status| status.parse().ok())
        .expect("the status line");
    let body = answer
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or_default()
        .to_string();
    (status, body)
}

/// Una petición GET hecha a mano sobre el socket, con sus cabeceras y el
/// cuerpo **en bytes**: la que registra el wire del protocolo para la
/// evidencia, y la que compara el tarball servido con el publicado.
fn socket_get(port: u16, path: &str, authorization: Option<&str>) -> (u16, String, Vec<u8>) {
    let mut request =
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n");
    if let Some(token) = authorization {
        request.push_str(&format!("Authorization: {token}\r\n"));
    }
    request.push_str("\r\n");
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("the socket");
    let _ = stream.write_all(request.as_bytes());
    let mut answer = Vec::new();
    let _ = stream.read_to_end(&mut answer);
    let split = answer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("the head and the body separate");
    let head = String::from_utf8_lossy(&answer[..split]).into_owned();
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .and_then(|status| status.parse().ok())
        .expect("the status line");
    let headers = head.lines().skip(1).collect::<Vec<_>>().join("\n");
    (status, headers, answer[split + 4..].to_vec())
}

/// El seeder del catálogo: cada suite lo trae porque el arnés reinicia el
/// binario de la suite para sembrar antes de arrancar el servicio.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// AC1 + AC2: el flujo completo del primer módulo privado, con el cliente
/// real: publicación versionada e inmutable, `init` autenticado que
/// resuelve por versión y verifica el checksum del tarball, mutación
/// incompatible rechazada y resolución de otra cuenta denegada.
#[test]
fn the_terraform_client_inits_a_private_module() {
    require_terraform();
    let deployment = Deployment::new();
    deployment.seed_terraform(&[("modules-hosted", serde_json::Value::Null)]);
    let mut service = deployment.start(&[]);
    let port = service.port;
    let root = deployment.directory.path().to_path_buf();

    let the_binary = require_terraform();
    let token = support::token(FIRST_ACCOUNT, "alice");
    let v1 = the_module_tarball(&root, "module-v1", "# version one");
    let v1_digest = sha256_of(&v1);

    // AC2: la publicación es un recibo con el sha256 que el hub calculó
    // al commit; el mismo que luego declara la descarga.
    let (status, receipt) = publish_module(port, "modules-hosted", &token, "1.0.0", &v1, None);
    assert_eq!(
        status, 201,
        "the publish of a new version is a receipt: {receipt}"
    );
    let receipt: serde_json::Value = serde_json::from_str(&receipt).expect("the receipt");
    assert_eq!(
        receipt["sha256"], v1_digest,
        "the receipt names the sha256 of the bytes"
    );
    assert_eq!(
        receipt["size_bytes"],
        v1.len() as u64,
        "the receipt names the size"
    );

    // AC1: el `init` real. El cliente pide versiones con su credencial,
    // resuelve la restricción, sigue el `X-Terraform-Get` y go-getter baja
    // el tarball con la credencial de netrc verificando `?checksum=`.
    let author = Client::new(&root, "author", port, &token, the_binary.clone());
    author.write_config(Some("1.0.0"));
    author.init().expect("the init of the private module");
    assert!(
        author.installed().join("main.tf").is_file(),
        "the init left the module in place: {}",
        author.installed().display()
    );
    let manifest = author.manifest();
    assert!(
        manifest.contains("\"Version\":\"1.0.0\""),
        "the manifest records the resolved version: {manifest}"
    );

    // AC1: la versión es INMUTABLE. Repetir la publicación —con los mismos
    // bytes— es un rechazo, no un eco.
    let (status, again) = publish_module(port, "modules-hosted", &token, "1.0.0", &v1, None);
    assert_eq!(
        status, 409,
        "the republish of the same version is rejected: {again}"
    );
    // AC2: la mutación incompatible — misma versión, otro contenido — es
    // otro rechazo.
    let tampered = the_module_tarball(&root, "module-v1-tampered", "# version one, changed");
    let (status, mutated) =
        publish_module(port, "modules-hosted", &token, "1.0.0", &tampered, None);
    assert_eq!(
        status, 409,
        "the same version with other content is rejected: {mutated}"
    );

    // AC2: lo publicado es el recibo. El tarball servido es byte a byte el
    // que entró, y la descarga declara el checksum del recibo.
    let (_, headers, _body) = socket_get(
        port,
        "/terraform/modules-hosted/v1/modules/hodei/demo/local/1.0.0/download",
        Some(&format!("Bearer {token}")),
    );
    assert!(
        headers.to_lowercase().contains("x-terraform-get"),
        "the download answers with the go-getter source: {headers}"
    );
    let location = headers
        .lines()
        .find(|line| line.to_lowercase().starts_with("x-terraform-get"))
        .and_then(|line| line.split_once(':'))
        .map(|(_, value)| value.trim().to_string())
        .expect("the location");
    assert!(
        location.ends_with(&format!(
            "artifacts/hodei/demo/local/1.0.0.tar.gz?checksum=sha256:{v1_digest}"
        )),
        "the download points at the artifact with the receipt checksum: {location}"
    );
    let (status, served_headers, artifact) =
        socket_get(port, &location, Some(&format!("Bearer {token}")));
    assert_eq!(status, 200, "the artifact is served: {served_headers}");
    assert!(
        served_headers.contains("content-type: application/gzip"),
        "the artifact is served as a tarball: {served_headers}"
    );
    assert_eq!(
        sha256_of(&artifact),
        v1_digest,
        "the bytes served are the bytes published"
    );

    // AC1: el scoping es por cuenta. Otro principal no resuelve el módulo:
    // el repositorio vive en otra cuenta, y su init no encuentra nada.
    let other = support::token(SECOND_ACCOUNT, "bob");
    let stranger = Client::new(&root, "stranger", port, &other, the_binary.clone());
    stranger.write_config(Some("1.0.0"));
    let denied = stranger.init();
    assert!(
        !denied.ok,
        "another account does not resolve the module of the first account: {}",
        denied.output
    );
    let (status, _, _) = socket_get(
        port,
        "/terraform/modules-hosted/v1/modules/hodei/demo/local/versions",
        Some(&format!("Bearer {other}")),
    );
    assert_eq!(
        status, 404,
        "the versions of another account's repository do not exist for it"
    );
    let (status, _) = publish_module(port, "modules-hosted", &other, "2.0.0", &tampered, None);
    assert_eq!(
        status, 404,
        "the repository of another account does not exist for the stranger"
    );

    // AC2: el índice de versiones. La segunda versión entra y la
    // resolución por versión la alcanza.
    let v2 = the_module_tarball(&root, "module-v2", "# version two");
    let (status, receipt2) = publish_module(port, "modules-hosted", &token, "2.0.0", &v2, None);
    assert_eq!(status, 201, "the second version publishes: {receipt2}");
    let (_, _, index) = socket_get(
        port,
        "/terraform/modules-hosted/v1/modules/hodei/demo/local/versions",
        Some(&format!("Bearer {token}")),
    );
    let index: serde_json::Value = serde_json::from_slice(&index).expect("the index");
    let versions: Vec<&str> = index["modules"][0]["versions"]
        .as_array()
        .expect("the versions")
        .iter()
        .filter_map(|version| version["version"].as_str())
        .collect();
    assert_eq!(
        versions,
        ["1.0.0", "2.0.0"],
        "the index lists the versions in order"
    );

    let pinned = Client::new(&root, "pinned", port, &token, the_binary.clone());
    pinned.write_config(Some("2.0.0"));
    pinned.init().expect("the init of the second version");
    assert!(pinned.installed().join("main.tf").is_file());

    // AC4: el discovery del host nombra al único registry de la cuenta, y
    // solo modules.v1: AC3 no implementado, nada se anuncia. Es el
    // documento que el cliente pide por https en un despliegue con TLS y
    // por la config de CLI en este.
    let (status, _, document) = socket_get(
        port,
        "/.well-known/terraform.json",
        Some(&format!("Bearer {token}")),
    );
    assert_eq!(
        status, 200,
        "the discovery of a single-registry account answers"
    );
    let document: serde_json::Value =
        serde_json::from_slice(&document).expect("the discovery document");
    assert_eq!(
        document["modules.v1"], "/terraform/modules-hosted/v1/modules/",
        "the discovery names the repository of the account"
    );
    assert!(
        document.get("providers.v1").is_none(),
        "AC3: the provider registry is not announced because it is not implemented"
    );

    // AC4: el modo offline. Con el hub parado, el init de una caché
    // templada no necesita red (el instalador reutiliza lo instalado) y
    // una resolución que no tiene caché falla, sin caer a nada.
    support::stop(&mut service);
    let warm = author.init();
    assert!(
        warm.ok,
        "the offline init of a warm cache needs no remote: {}",
        warm.output
    );
    let cold = Client::new(&root, "cold", port, &token, the_binary.clone());
    cold.write_config(Some("1.0.0"));
    let missing = cold.init();
    assert!(
        !missing.ok,
        "the offline init of a cold cache fails: {}",
        missing.output
    );

    // La evidencia del digest que el cliente verificó, para el informe.
    println!("HSP-158 module v1 sha256: {v1_digest}");
}

/// AC4: la superficie de service discovery del hub y el resto del wire
/// que el cliente real no alcanza sobre http (el discovery va siempre por
/// https): el documento que nombra al registry de la cuenta, y el límite
/// de otra cuenta.
#[test]
fn the_discovery_names_the_module_registry_of_the_account() {
    let deployment = Deployment::new();
    deployment.seed_terraform(&[
        ("modules-hosted", serde_json::Value::Null),
        ("modules-second", serde_json::Value::Null),
    ]);
    let service = deployment.start(&[]);
    let port = service.port;
    let token = support::token(FIRST_ACCOUNT, "alice");

    // Dos registries de la misma cuenta: el discovery no puede elegir
    // solo, y lo dice.
    let (status, _, body) = socket_get(
        port,
        "/.well-known/terraform.json",
        Some(&format!("Bearer {token}")),
    );
    assert_eq!(
        status,
        409,
        "an account with several registries is told to name one: {}",
        String::from_utf8_lossy(&body)
    );

    // Una cuenta sin registries: no hay nada que anunciar.
    let other = support::token(SECOND_ACCOUNT, "bob");
    let (status, _, body) = socket_get(
        port,
        "/.well-known/terraform.json",
        Some(&format!("Bearer {other}")),
    );
    assert_eq!(
        status,
        404,
        "an account without registries announces nothing: {}",
        String::from_utf8_lossy(&body)
    );

    // Sin credencial no hay discovery: el documento nombra repositorios de
    // una cuenta, y una cuenta se verifica, no se declara. La credencial
    // del discovery es la del registry (Bearer); el Basic del netrc es el
    // del artefacto, y aquí no vale.
    let (status, _, _) = socket_get(port, "/.well-known/terraform.json", None);
    assert_eq!(status, 401, "the discovery needs a credential");
    let (status, _, _) = socket_get(
        port,
        "/.well-known/terraform.json",
        Some(&format!("Basic {}", base64_of(&format!("hodei:{other}")))),
    );
    assert_eq!(
        status, 401,
        "the discovery wants the registry credential, not a netrc one"
    );
}

/// El base64 de unos bytes, como lo escribe el cliente que manda Basic.
fn base64_of(value: &str) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(value)
}
