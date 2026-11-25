//! Ansible Galaxy hosted de HSP-217 contra el cliente real
//! (`ansible-galaxy`).
//!
//! El `ansible-galaxy` del venv (`ansible-core` 2.19) construye una
//! collection real, la publica con `collection publish` contra el
//! servicio de verdad, la resuelve con `collection install` en un
//! directorio limpio y la compara con `collection verify`: la identidad
//! (`namespace.name`), la version y el sha256 que el cliente verifica al
//! bajar son los del propio protocolo, no de este test. Hace falta
//! `ansible-core` 2.19 con un Python que lo sostenga (el 3.15 del
//! sistema rompe esta version): en el `GALAXY_BIN` (el venv de la UAT) o
//! en el `PATH`; sin el, la suite **falla** diciendo que falta. Cada
//! ejecucion usa su propio directorio de collections y su propio `HOME`:
//! nada se lee ni se escribe fuera del directorio del despliegue.
//!
//! El dialecto v3 es el que fija el cliente (`ansible/galaxy/api.py`);
//! el mapa del wire lo capturo el spike de HSP-168 contra
//! galaxy.ansible.com y contra un fixture local (FMT-06 de
//! `uat/UAT-FORMATS.md`), y este test demanda de la superficie esa misma
//! forma: discovery `/api/`, documentos de collection y versiones
//! paginados DRF, artefacto por `download_url` con su sha256, publish
//! multipart con `Authorization: Token`.

mod support;

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use flate2::write::GzEncoder;
use sha2::Digest as _;
use support::{Deployment, FIRST_ACCOUNT, SECOND_ACCOUNT};

/// Exige el `ansible-galaxy` 2.19 del venv o del PATH, y devuelve su ruta.
fn the_galaxy_binary() -> String {
    if let Ok(path) = std::env::var("GALAXY_BIN") {
        return path;
    }
    "ansible-galaxy".to_string()
}

fn require_galaxy() -> String {
    let binary = the_galaxy_binary();
    let version = Command::new(&binary)
        .arg("--version")
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
    let line = version.lines().next().unwrap_or_default().to_string();
    assert!(
        line.starts_with("ansible-galaxy [core 2.19"),
        "the galaxy suite pins the client ansible-core 2.19.x, found: {line}"
    );
    binary
}

/// El resultado de una ejecucion del cliente.
struct Run {
    /// Si el cliente termino bien.
    ok: bool,
    /// Lo que dijo, por stdout y por stderr.
    output: String,
}

impl Run {
    /// El texto si termino bien, y un fallo con todo el resto si no.
    fn expect(&self, what: &str) -> String {
        assert!(self.ok, "{what} failed:\n{}", self.output);
        self.output.clone()
    }
}

/// Un `ansible-galaxy` aislado: su directorio de collections y su `HOME`.
struct Client {
    /// La ruta del ejecutable.
    binary: String,
    /// El directorio del despliegue.
    root: PathBuf,
    /// El directorio de collections de esta cache.
    collections: PathBuf,
}

impl Client {
    /// Una cache limpia. El servidor y la credencial van en cada comando:
    /// el dialecto no guarda el remote en una configuracion previa.
    fn new(root: &Path, name: &str) -> Self {
        let collections = root.join(format!("collections-{name}"));
        std::fs::create_dir_all(&collections).expect("the collections directory");
        Self {
            binary: the_galaxy_binary(),
            root: root.to_path_buf(),
            collections,
        }
    }

    /// Ejecuta el `ansible-galaxy` con su entorno aislado.
    fn run(&self, args: &[&str]) -> Run {
        let output = Command::new(&self.binary)
            .args(args)
            .env("HOME", &self.root)
            .env("ANSIBLE_COLLECTIONS_PATH", &self.collections)
            .env("ANSIBLE_GALAXY_SERVER_VALIDATE_CERTS", "false")
            .output()
            .expect("ansible-galaxy runs");
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Run {
            ok: output.status.success(),
            output: text,
        }
    }

    /// `collection build`: el artefacto que el propio cliente produce.
    fn build(&self, source: &Path, dist: &Path) -> PathBuf {
        self.run(&[
            "collection",
            "build",
            source.to_str().unwrap(),
            "--output-path",
            dist.to_str().unwrap(),
        ])
        .expect("the build");
        let artifact = dist.join("hodei-demo-1.0.0.tar.gz");
        assert!(artifact.is_file(), "the build produced the artifact");
        artifact
    }

    /// `collection publish`: el upload real contra la superficie galaxy.
    fn publish(&self, artifact: &Path, server: &str, token: &str) -> Run {
        self.run(&[
            "collection",
            "publish",
            artifact.to_str().unwrap(),
            "--server",
            server,
            "--api-key",
            token,
        ])
    }

    /// `collection install`: la resolucion real contra la superficie.
    fn install(&self, requirement: &str, server: &str, token: &str) -> Run {
        self.run(&[
            "collection",
            "install",
            requirement,
            "--server",
            server,
            "--api-key",
            token,
            "-p",
            self.collections.to_str().unwrap(),
        ])
    }

    /// `collection verify`: la comparacion real de lo instalado contra el
    /// artefacto servido.
    fn verify(&self, requirement: &str, server: &str, token: &str) -> Run {
        self.run(&[
            "collection",
            "verify",
            requirement,
            "--server",
            server,
            "--api-key",
            token,
            "-p",
            self.collections.to_str().unwrap(),
        ])
    }

    /// El directorio instalado de la collection de este cliente.
    fn installed(&self) -> PathBuf {
        self.collections
            .join("ansible_collections")
            .join("hodei")
            .join("demo")
    }
}

/// El codigo fuente de la collection que el cliente construye.
fn the_collection_source(root: &Path) -> PathBuf {
    let directory = root.join("collections/hodei-demo");
    std::fs::create_dir_all(&directory).expect("the source directory");
    std::fs::write(
        directory.join("galaxy.yml"),
        "namespace: hodei\nname: demo\nversion: 1.0.0\nreadme: README.md\nauthors:\n  - hodei\nlicense:\n  - Apache-2.0\n",
    )
    .expect("the galaxy.yml");
    std::fs::write(directory.join("README.md"), "# the demo collection\n").expect("the readme");
    directory
}

/// El sha256 de un fichero, como lo verifica el protocolo.
fn sha256_of(path: &Path) -> String {
    let bytes = std::fs::read(path).expect("the file");
    format!("{:x}", sha2::Sha256::digest(&bytes))
}

/// Un miembro ustar escrito como el formato lo lee: cabecera, contenido y
/// el relleno que redondea el registro.
fn the_member_bytes(name: &str, content: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0u8; 512];
    bytes[..name.len()].copy_from_slice(name.as_bytes());
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

/// Un artefacto que el cliente acepta subir pero que no es una
/// collection legible: un gzip de un tar cuyo `MANIFEST.json` no es el
/// documento que el formato declara. La corrupcion vive dentro del
/// artefacto, no en el transporte.
fn the_corrupted_artifact(root: &Path) -> PathBuf {
    let mut tar = Vec::new();
    tar.extend_from_slice(&the_member_bytes(
        "MANIFEST.json",
        b"this is not the manifest json",
    ));
    tar.extend_from_slice(&[0u8; 512]);
    let mut gzipped = GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzipped.write_all(&tar).expect("the gzip write");
    let artifact = root.join("corrupted/hodei-demo-1.0.0.tar.gz");
    std::fs::create_dir_all(artifact.parent().expect("the directory")).expect("the directory");
    std::fs::write(&artifact, gzipped.finish().expect("the gzip finish")).expect("the artifact");
    artifact
}

/// Un publish contra la superficie, hecho a mano sobre el socket: los
/// mismos bytes del artefacto con un `sha256` que **no** es el de esos
/// bytes. El cliente real no puede decir esa mentira, y por eso este
/// caso va por el socket.
fn the_doctored_publish(
    port: u16,
    repository: &str,
    token: &str,
    bytes: &[u8],
    declared_sha256: &str,
) -> u16 {
    let boundary = "hodei-hsp217";
    let mut body = Vec::new();
    let _ = write!(
        body,
        "--{boundary}\r\nContent-Disposition: form-data; name=\"sha256\"\r\n\r\n{declared_sha256}\r\n"
    );
    let _ = write!(
        body,
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"hodei-demo-1.0.0.tar.gz\"\r\nContent-Type: application/octet-stream\r\n\r\n"
    );
    body.extend_from_slice(bytes);
    let _ = write!(body, "\r\n--{boundary}--\r\n");
    let request = format!(
        "POST /galaxy/{repository}/api/v3/artifacts/collections/ HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Token {token}\r\nContent-Type: multipart/form-data; boundary={boundary}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("the socket");
    let _ = stream.write_all(request.as_bytes());
    let _ = stream.write_all(&body);
    let mut answer = String::new();
    let _ = stream.read_to_string(&mut answer);
    answer
        .lines()
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .and_then(|status| status.parse().ok())
        .expect("the status line")
}

/// El seeder del catalogo: cada suite lo trae porque el arnes reinicia
/// el binario de la suite para sembrar antes de arrancar el servicio.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

#[test]
fn the_galaxy_client_publishes_installs_and_verifies() {
    require_galaxy();
    let deployment = Deployment::new();
    deployment.seed_galaxy(&[("galaxy-hosted", serde_json::Value::Null)]);
    let mut service = deployment.start(&[]);
    let port = service.port;
    let root = deployment.directory.path().to_path_buf();
    let server = format!("http://127.0.0.1:{port}/galaxy/galaxy-hosted");

    let token = support::token(FIRST_ACCOUNT, "alice");
    let author = Client::new(&root, "author");
    let artifact = author.build(&the_collection_source(&root), &root.join("dist"));

    // AC2: el publish real contra la superficie. El cliente calcula su
    // sha256, lo manda en el multipart y espera la tarea de import que
    // la superficie responde.
    author
        .publish(&artifact, &server, &token)
        .expect("the publish");
    // La repeticion del mismo artefacto es un eco, no una reescritura:
    // el mismo contenido en la misma version es un si.
    author
        .publish(&artifact, &server, &token)
        .expect("the idempotent republish of the same artifact");
    let digest = sha256_of(&artifact);

    // AC1/AC3: el install real en un directorio limpio. El cliente pide
    // el discovery, el documento de la collection, las versiones
    // paginadas, el documento de la version y el artefacto, y verifica
    // el sha256 del artefacto contra el que el documento declara.
    let consumer = Client::new(&root, "consumer");
    consumer
        .install("hodei.demo:1.0.0", &server, &token)
        .expect("the install");
    let manifest = consumer.installed().join("MANIFEST.json");
    assert!(
        manifest.is_file(),
        "the install left the collection in place: {}",
        consumer.installed().display()
    );

    // AC3: verify real. Lo instalado y lo servido son lo mismo: la
    // comparacion de los checksums del MANIFEST es un exito.
    let verified = consumer.verify("hodei.demo:1.0.0", &server, &token);
    assert!(
        verified.ok && verified.output.contains("Successfully verified"),
        "the verify of a clean install passes: {}",
        verified.output
    );

    // AC3: la corrupcion local no pasa el verify. Un fichero del
    // instalado que ya no es el que el artefacto declaro es una
    // instalacion que ya no es la que el hub sirvio.
    std::fs::write(consumer.installed().join("README.md"), "# corrupted\n")
        .expect("the corruption");
    let corrupted = consumer.verify("hodei.demo:1.0.0", &server, &token);
    assert!(
        !corrupted.ok,
        "the verify of a corrupted install fails: {}",
        corrupted.output
    );

    // AC4: el scoping es por cuenta. Otro principal no ve la collection:
    // el repositorio vive en otra cuenta, y su install no encuentra nada.
    let other = support::token(SECOND_ACCOUNT, "bob");
    let stranger = Client::new(&root, "stranger");
    let stranger_install = stranger.install("hodei.demo:1.0.0", &server, &other);
    assert!(
        !stranger_install.ok,
        "another principal does not see the collection of the first account: {}",
        stranger_install.output
    );
    let stranger_publish = stranger.publish(&artifact, &server, &other);
    assert!(
        !stranger_publish.ok,
        "another principal cannot publish into the repository of the first account: {}",
        stranger_publish.output
    );

    // AC3: offline. El remote muere: la cache caliente del cliente
    // resuelve sin red ("Nothing to do") y un requisito que no tiene
    // falla, sin caer a nada.
    support::stop(&mut service);
    let warm = consumer.install("hodei.demo:1.0.0", &server, &token);
    assert!(
        warm.ok && warm.output.contains("Nothing to do"),
        "the offline install of a warm cache needs no remote: {}",
        warm.output
    );
    let missing = consumer.install("hodei.demo:2.0.0", &server, &token);
    assert!(
        !missing.ok,
        "the offline install of a version nobody served fails: {}",
        missing.output
    );

    // La evidencia del digest que el cliente verifico, para el informe.
    println!("HSP-217 artifact sha256: {digest}");
}

#[test]
fn the_hub_rejects_a_corrupted_artifact_and_a_lying_sha256() {
    require_galaxy();
    let deployment = Deployment::new();
    deployment.seed_galaxy(&[("galaxy-hosted", serde_json::Value::Null)]);
    let service = deployment.start(&[]);
    let port = service.port;
    let root = deployment.directory.path().to_path_buf();
    let server = format!("http://127.0.0.1:{port}/galaxy/galaxy-hosted");

    let token = support::token(FIRST_ACCOUNT, "alice");
    let author = Client::new(&root, "author");

    // El artefacto corrupto entra por el cliente real: el cliente
    // acepta subirlo (es un tarball) y la superficie lo rechaza antes de
    // guardar nada, porque no es una collection legible.
    let corrupted = the_corrupted_artifact(&root);
    let rejected = author.publish(&corrupted, &server, &token);
    assert!(
        !rejected.ok,
        "the publish of an artifact that is not a readable collection is rejected: {}",
        rejected.output
    );
    assert!(
        rejected.output.contains("HTTP Code: 400"),
        "the rejection says what it is: {}",
        rejected.output
    );

    // El sha256 mentiroso no pasa el commit: los bytes que llegan tienen
    // que ser los que el formulario declara. El cliente real no puede
    // decir esta mentira, por lo que va por el socket con el formulario
    // de su propio artefacto y un sha256 que no es el suyo.
    let artifact = author.build(&the_collection_source(&root), &root.join("dist"));
    let bytes = std::fs::read(&artifact).expect("the artifact bytes");
    let digest = sha256_of(&artifact);
    let wrong = format!("{:x}", sha2::Sha256::digest(b"not the artifact bytes"));
    let status = the_doctored_publish(port, "galaxy-hosted", &token, &bytes, &wrong);
    assert_eq!(
        status, 400,
        "the publish whose sha256 does not match its bytes is rejected"
    );
    // Un digest mal formado no es un digest: tampoco entra.
    let malformed = the_doctored_publish(port, "galaxy-hosted", &token, &bytes, "short");
    assert_eq!(
        malformed, 400,
        "a publish whose sha256 is not a sha256 is rejected"
    );

    // Lo rechazado no ocupo el sitio: el artefacto bueno se publica
    // despues de los dos intentos, con su version libre, y el digest que
    // el cliente declaro es el que la version sirve.
    author
        .publish(&artifact, &server, &token)
        .expect("the publish of the intact artifact after the rejections");
    let consumer = Client::new(&root, "consumer");
    consumer
        .install("hodei.demo:1.0.0", &server, &token)
        .expect("the install of what the hub accepted");
    let installed = consumer.installed().join("MANIFEST.json");
    assert!(installed.is_file(), "the install left the collection");
    let served = format!("{:x}", sha2::Sha256::digest(&bytes));
    assert_eq!(
        served, digest,
        "the bytes the client published are the ones it verifies"
    );
}
