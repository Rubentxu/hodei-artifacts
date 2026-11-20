//! Hugging Face hosted de HSP-218 contra el cliente real (`hf`).
//!
//! El `hf` del venv (`huggingface_hub` 1.2.3, que habla httpx) descarga
//! un modelo sembrado en la superficie hosted de Hodei: resuelve la
//! revision (`/api/models/{repo}/revision/main`), lee la metadata del
//! resolve (`302` con `X-Repo-Commit`/`X-Linked-Etag`/`X-Linked-Size` y
//! `Location` relativa) y baja el blob en streaming por la `Location`.
//! La identidad la pone el propio dialecto: el etag del fichero ES su
//! sha256, y el de lo instalado se compara con el que la superficie
//! declaro. Hace falta el `hf` 1.x del venv (el 3.15 del sistema rompe
//! toolchain viejas, leccion de HSP-168): en el `HF_BIN` (el venv de la
//! UAT) o en el `PATH`; sin el, la suite **falla** diciendo que falta.
//! Cada ejecucion usa su propio `HF_HOME`: nada se lee ni se escribe
//! fuera del directorio del despliegue.
//!
//! El dialecto es el que fija el cliente; el mapa del wire lo capturo el
//! spike de HSP-169 contra huggingface.co (ruta HTTP clasica, sin xet) y
//! contra un fixture local (FMT-07 de `uat/UAT-FORMATS.md`). El servidor
//! no emite `X-Xet-*`, asi que el cliente cae a la ruta clasica: la que
//! este release sirve.
//!
//! El test negativo documentado reproducen el hallazgo del spike (su
//! fixture en modo `CORRUPT=1`, aqui como mini-fixture embebido con la
//! misma forma): en la ruta clasica el cliente SOLO verifica el tamaño,
//! nunca el sha256 — una descarga servida corrupta es ACEPTADA. La
//! integridad la garantiza el CAS del servidor (conexion con HSP-105),
//! no el cliente.

mod support;

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::Digest as _;
use support::{Deployment, FIRST_ACCOUNT, SECOND_ACCOUNT};

/// Exige el `hf` 1.x del venv o del PATH, y devuelve su ruta.
fn the_hf_binary() -> String {
    if let Ok(path) = std::env::var("HF_BIN") {
        return path;
    }
    "hf".to_string()
}

fn require_hf() -> String {
    let binary = the_hf_binary();
    let version = Command::new(&binary)
        .arg("version")
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
        .unwrap_or_default();
    assert!(
        version.lines().any(|line| line.trim().starts_with("1.")),
        "the hf suite pins the client huggingface_hub 1.x of the UAT venv, found: {version}"
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

/// Un `hf` aislado: su `HF_HOME` de cache y su `HOME`.
struct Client {
    /// La ruta del ejecutable.
    binary: String,
    /// El directorio del despliegue.
    root: PathBuf,
    /// La cache de este cliente.
    home: PathBuf,
}

impl Client {
    /// Una cache limpia. El endpoint y la credencial van por entorno en
    /// cada comando: el dialecto no guarda el remote en una configuracion
    /// previa.
    fn new(root: &Path, name: &str) -> Self {
        let home = root.join(format!("hf-home-{name}"));
        std::fs::create_dir_all(&home).expect("the hf home");
        Self {
            binary: the_hf_binary(),
            root: root.to_path_buf(),
            home,
        }
    }

    /// Ejecuta el `hf` con su entorno aislado.
    fn run(&self, args: &[&str], endpoint: &str, token: &str) -> Run {
        let output = Command::new(&self.binary)
            .args(args)
            .env("HOME", &self.root)
            .env("HF_HOME", &self.home)
            // La ruta clasica es el dialecto servido: sin xet nativo, la
            // decision del fixture del spike que este release replica.
            .env("HF_HUB_DISABLE_XET", "1")
            .env("HF_HUB_DISABLE_TELEMETRY", "1")
            .env("HF_HUB_DISABLE_PROGRESS_BARS", "1")
            .env("HF_ENDPOINT", endpoint)
            .env("HF_TOKEN", token)
            .output()
            .expect("hf runs");
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Run {
            ok: output.status.success(),
            output: text,
        }
    }

    /// `hf download`: la resolucion real contra la superficie.
    fn download(&self, repo: &str, endpoint: &str, token: &str, extra: &[&str]) -> Run {
        let mut args = vec!["download", repo, "--quiet"];
        args.extend_from_slice(extra);
        self.run(&args, endpoint, token)
    }

    /// El snapshot instalado de un modelo: la carpeta de snapshots de su
    /// cache. El cliente nombra el modelo `models--{owner}--{name}`.
    fn the_snapshot_of(&self, repo: &str) -> PathBuf {
        let model_dir = self
            .home
            .join("hub")
            .join(format!("models--{}", repo.replace('/', "--")));
        let snapshots = model_dir.join("snapshots");
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&snapshots)
            .expect("the snapshots of the download")
            .flatten()
            .map(|entry| entry.path())
            .collect();
        entries.sort();
        assert_eq!(entries.len(), 1, "one snapshot per download: {entries:?}");
        entries.remove(0)
    }
}

/// El modelo pequeno que la suite siembra: un `config.json` y un
/// `model.safetensors` legitimo (cabecera JSON de 8 bytes + tensores), el
/// artefacto del dialecto en su forma real pero con un megabyte de pesos.
fn the_model_files(root: &Path) -> (PathBuf, PathBuf) {
    let directory = root.join("model/hodei-demo-model");
    std::fs::create_dir_all(&directory).expect("the model directory");
    let config = directory.join("config.json");
    std::fs::write(
        &config,
        "{\n  \"model_type\": \"bert\",\n  \"hidden_size\": 128,\n  \"num_hidden_layers\": 2,\n  \"vocab_size\": 30522\n}\n",
    )
    .expect("the config");
    let model = directory.join("model.safetensors");
    // Un safetensors legitimo: u64 little endian con la longitud de la
    // cabecera (multiplo de 8), la cabecera JSON y los pesos.
    let header = serde_json::json!({
        "weights": { "dtype": "F32", "shape": [256, 1024], "data_offsets": [0, 1048576] }
    })
    .to_string();
    let padding = (8 - (header.len() % 8)) % 8;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&((header.len() + padding) as u64).to_le_bytes());
    bytes.extend_from_slice(header.as_bytes());
    bytes.extend(std::iter::repeat_n(b' ', padding));
    bytes.extend((0..1048576u32).map(|index| (index % 251) as u8));
    std::fs::write(&model, bytes).expect("the safetensors");
    (config, model)
}

/// El sha256 de un fichero, como lo verifica el protocolo.
fn sha256_of(path: &Path) -> String {
    let bytes = std::fs::read(path).expect("the file");
    format!("{:x}", sha2::Sha256::digest(&bytes))
}

/// La respuesta cruda de la superficie, por socket: estado, cabeceras en
/// minusculas y cuerpo. Los HEAD y los GET del dialecto se comprueban
/// asi, como llegan al wire.
fn the_request(
    method: &str,
    port: u16,
    path: &str,
    token: &str,
) -> (u16, HashMap<String, String>, Vec<u8>) {
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
    );
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("the socket");
    let _ = stream.write_all(request.as_bytes());
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw);
    let text = String::from_utf8_lossy(&raw).into_owned();
    let split = text.find("\r\n\r\n");
    let (head, body) = match split {
        Some(index) => (text[..index].to_string(), raw[index + 4..].to_vec()),
        None => (text.to_string(), Vec::new()),
    };
    let mut lines = head.lines();
    let status_line = lines.next().unwrap_or_default().to_string();
    let status: u16 = status_line
        .split(' ')
        .nth(1)
        .and_then(|status| status.parse().ok())
        .unwrap_or_default();
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    (status, headers, body)
}

/// El seeder del catalogo: cada suite lo trae porque el arnes reinicia
/// el binario de la suite para sembrar antes de arrancar el servicio.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

#[test]
fn the_hf_client_downloads_the_model_and_the_sha256_is_the_etag() {
    require_hf();
    let deployment = Deployment::new();
    let (config, model) = the_model_files(deployment.directory.path());
    deployment.seed_hf(&[(
        "hf-hosted",
        vec![
            ("artifacts/hodei/demo-model/config.json", config.clone()),
            (
                "artifacts/hodei/demo-model/model.safetensors",
                model.clone(),
            ),
        ],
    )]);
    let mut service = deployment.start(&[]);
    let port = service.port;
    let root = deployment.directory.path().to_path_buf();
    let endpoint = format!("http://127.0.0.1:{port}/hf/hf-hosted");
    let token = support::token(FIRST_ACCOUNT, "alice");
    let client = Client::new(&root, "consumer");

    // El commit del snapshot es el digest del listado que lo forma: lo
    // que la superficie declara tiene que ser el que los ficheros
    // sembrados producen.
    let config_digest = sha256_of(&config);
    let model_digest = sha256_of(&model);
    let expected_commit = the_snapshot_commit(&[
        ("config.json", &config_digest),
        ("model.safetensors", &model_digest),
    ]);

    // AC1: el documento de revision lleva `id`, `sha` y `siblings`, y el
    // commit que declara es el del snapshot sembrado. La misma revision
    // se sirve por `main` y por su commit.
    let (status, _, body) = the_request(
        "GET",
        port,
        "/hf/hf-hosted/api/models/hodei/demo-model/revision/main",
        &token,
    );
    assert_eq!(status, 200, "the revision document is served");
    let document: serde_json::Value = serde_json::from_slice(&body).expect("the revision document");
    assert_eq!(document["id"], "hodei/demo-model");
    assert_eq!(document["sha"], expected_commit);
    let siblings: Vec<&str> = document["siblings"]
        .as_array()
        .expect("the siblings")
        .iter()
        .filter_map(|sibling| sibling["rfilename"].as_str())
        .collect();
    assert!(
        siblings.contains(&"config.json"),
        "the siblings: {siblings:?}"
    );
    assert!(
        siblings.contains(&"model.safetensors"),
        "the siblings: {siblings:?}"
    );
    let (by_commit, _, body_by_commit) = the_request(
        "GET",
        port,
        &format!("/hf/hf-hosted/api/models/hodei/demo-model/revision/{expected_commit}"),
        &token,
    );
    assert_eq!(by_commit, 200, "the revision is served by its commit");
    assert_eq!(body_by_commit, body, "main and the commit are one snapshot");

    // AC1: el resolve por `HEAD` es una `302` con las cabeceras del wire
    // real y una `Location` relativa (una ruta, sin scheme ni host).
    let (status, headers, _) = the_request(
        "HEAD",
        port,
        "/hf/hf-hosted/hodei/demo-model/resolve/main/model.safetensors",
        &token,
    );
    assert_eq!(status, 302, "the resolve redirects");
    assert_eq!(
        headers.get("x-repo-commit").map(String::as_str),
        Some(expected_commit.as_str()),
        "the commit travels in the header the client reads"
    );
    let etag = headers
        .get("x-linked-etag")
        .expect("the linked etag of the wire")
        .trim_matches('"')
        .to_string();
    assert_eq!(etag, model_digest, "the etag of the blob is its sha256");
    assert_eq!(
        headers.get("x-linked-size").map(String::as_str),
        Some(
            model
                .metadata()
                .expect("the size")
                .len()
                .to_string()
                .as_str()
        ),
        "the size of the blob travels too"
    );
    let location = headers.get("location").expect("the location").to_string();
    assert!(
        location.starts_with('/') && !location.contains("://"),
        "the location is a relative path, the shape the client follows: {location}"
    );

    // AC2: el download real del cliente. El modelo entero, con la cache
    // que el propio cliente gestiona.
    client
        .download("hodei/demo-model", &endpoint, &token, &[])
        .expect("the download");

    // AC2: lo instalado es lo que la superficie declaro: el sha256 del
    // fichero instalado es el etag del resolve, y el commit del snapshot
    // instalado es el que la revision sirvio.
    let snapshot = client.the_snapshot_of("hodei/demo-model");
    let installed = snapshot.join("model.safetensors");
    assert!(installed.is_file(), "the install left the model in place");
    assert_eq!(
        sha256_of(&installed),
        etag,
        "the installed bytes are the ones the etag named"
    );
    assert_eq!(
        snapshot.file_name().and_then(|name| name.to_str()),
        Some(expected_commit.as_str()),
        "the client named the snapshot by the commit the surface declared"
    );

    // AC1: la `Location` lleva el digest y sirve los bytes del CAS: el
    // GET de la URL que la superficie construyo es el contenido entero.
    let (status, blob_headers, blob) = the_request("GET", port, &location, &token);
    assert_eq!(status, 200, "the blob is served");
    assert_eq!(
        blob_headers.get("etag").map(String::as_str),
        Some(format!("\"{model_digest}\"").as_str()),
        "the etag of the served bytes is their sha256"
    );
    assert_eq!(
        blob.len(),
        model.metadata().expect("the size").len() as usize,
        "the blob is whole"
    );
    assert_eq!(
        format!("{:x}", sha2::Sha256::digest(&blob)),
        model_digest,
        "the served bytes are the seeded content"
    );

    // La evidencia del digest que el cliente verifico, para el informe.
    println!("HSP-218 model.safetensors sha256: {model_digest}");
    support::stop(&mut service);
}

#[test]
fn the_warm_cache_resolves_when_the_service_is_down() {
    require_hf();
    let deployment = Deployment::new();
    let (config, model) = the_model_files(deployment.directory.path());
    deployment.seed_hf(&[(
        "hf-hosted",
        vec![
            ("artifacts/hodei/demo-model/config.json", config.clone()),
            (
                "artifacts/hodei/demo-model/model.safetensors",
                model.clone(),
            ),
        ],
    )]);
    let mut service = deployment.start(&[]);
    let port = service.port;
    let root = deployment.directory.path().to_path_buf();
    let endpoint = format!("http://127.0.0.1:{port}/hf/hf-hosted");
    let token = support::token(FIRST_ACCOUNT, "alice");
    let client = Client::new(&root, "consumer");

    client
        .download("hodei/demo-model", &endpoint, &token, &[])
        .expect("the first download warms the cache");
    support::stop(&mut service);

    // El servicio esta muerto: cualquier peticion del cliente fallaria.
    // Que el download de la cache caliente acabe bien ES la prueba de
    // que el cliente no vuelve a pedir nada.
    let warm = client.download("hodei/demo-model", &endpoint, &token, &[]);
    assert!(
        warm.ok,
        "the warm cache resolves without the remote: {}",
        warm.output
    );
    let snapshot = client.the_snapshot_of("hodei/demo-model");
    assert_eq!(
        sha256_of(&snapshot.join("model.safetensors")),
        sha256_of(&model),
        "the warm snapshot is the served content"
    );

    // Y un modelo que no tiene falla: sin red, no hay resolucion que
    // invente lo que el hub no sirvio.
    let missing = client.download("hodei/other-model", &endpoint, &token, &[]);
    assert!(
        !missing.ok,
        "the offline download of a model nobody served fails: {}",
        missing.output
    );
}

#[test]
fn another_principal_does_not_see_the_model_of_the_first_account() {
    require_hf();
    let deployment = Deployment::new();
    let (config, model) = the_model_files(deployment.directory.path());
    deployment.seed_hf(&[(
        "hf-hosted",
        vec![
            ("artifacts/hodei/demo-model/config.json", config.clone()),
            (
                "artifacts/hodei/demo-model/model.safetensors",
                model.clone(),
            ),
        ],
    )]);
    let mut service = deployment.start(&[]);
    let port = service.port;
    let root = deployment.directory.path().to_path_buf();
    let endpoint = format!("http://127.0.0.1:{port}/hf/hf-hosted");

    // El scoping es por cuenta: el repositorio vive en la cuenta primera,
    // y un principal de otra cuenta no lo ve.
    let token = support::token(FIRST_ACCOUNT, "alice");
    let stranger_token = support::token(SECOND_ACCOUNT, "bob");
    let stranger = Client::new(&root, "stranger");
    let stranger_download = stranger.download("hodei/demo-model", &endpoint, &stranger_token, &[]);
    assert!(
        !stranger_download.ok,
        "another principal does not see the model of the first account: {}",
        stranger_download.output
    );
    let _ = token;
    support::stop(&mut service);
}

/// El commit del snapshot: el digest de las lineas `rfilename sha256`
/// ordenadas, la derivacion que la superficie declara.
fn the_snapshot_commit(entries: &[(&str, &str)]) -> String {
    let mut listing: Vec<String> = entries
        .iter()
        .map(|(filename, digest)| format!("{filename} {digest}"))
        .collect();
    listing.sort();
    let mut hasher = sha2::Sha256::new();
    for line in listing {
        hasher.update(line.as_bytes());
        hasher.update(b"\n");
    }
    format!("{:x}", hasher.finalize())
}

/// El test negativo documentado: el cliente clasico SOLO verifica el
/// tamaño.
///
/// Un mini-fixture con la misma forma que el del spike de HSP-169 en su
/// modo `CORRUPT=1` (oraculo de documentos y cabeceras del hub): sirve un
/// repo `hodei/spike-model` con un `config.json` cuyo `X-Linked-Etag`
/// declara el sha256 del fichero ORIGINAL, pero cuyos bytes servidos
/// llevan un byte volteado, con el `Content-Length` correcto. El cliente
/// real ACEPTA la descarga: el sha256 declarado no lo comprueba, y solo
/// un `sha256sum` externo detecta la corrupcion. Por eso la integridad en
/// hosted la garantiza el CAS del servidor (HSP-105), que no entrega
/// bytes que no son su digest.
#[test]
fn the_classic_client_only_verifies_the_size_never_the_sha256() {
    require_hf();
    let deployment = Deployment::new();
    let root = deployment.directory.path().to_path_buf();

    // El fichero original: el que el etag del fixture declara.
    let original = root.join("spike/config.json");
    std::fs::create_dir_all(original.parent().expect("the directory")).expect("the directory");
    std::fs::write(&original, "{\n  \"model_type\": \"bert\"\n}\n").expect("the original");
    let declared = sha256_of(&original);

    // El fixture del oraculo: el dialecto del hub clasico con un byte
    // servido corrupto. stdlib de python, sin dependencias: el mismo
    // hueco que el `CORRUPT=1` del spike.
    let fixture = root.join("corrupt_fixture.py");
    std::fs::write(&fixture, CORRUPT_FIXTURE).expect("the fixture");
    let port = support::free_port();
    let mut server = Command::new("python3")
        .arg(&fixture)
        .env("FIXTURE_PORT", port.to_string())
        .env(
            "FIXTURE_ROOT",
            root.join("spike").to_string_lossy().into_owned(),
        )
        .env("FIXTURE_SHA256", &declared)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("the fixture runs");
    // El fixture dice cuando escucha, y lo dice escribiendo el puerto en
    // su propia salida... por socket: se espera a que el puerto abra.
    let mut ready = false;
    for _ in 0..100 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            ready = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(ready, "the corrupt fixture opened its port");

    // El cliente real contra el oraculo: la descarga corrupta es
    // ACEPTADA. Ese es el hallazgo documentado.
    let client = Client::new(&root, "corrupted");
    let endpoint = format!("http://127.0.0.1:{port}");
    let accepted = client.download(
        "hodei/spike-model",
        &endpoint,
        "a-token-the-fixture-ignores",
        &["config.json"],
    );
    let killed = server.kill();
    let _ = server.wait();
    let _ = killed;
    assert!(
        accepted.ok,
        "the classic client accepts a download whose bytes are not the declared sha256: {}",
        accepted.output
    );

    // Y la corrupcion la ve quien mira fuera del cliente: lo instalado
    // ya no es lo que el etag declaro.
    let snapshot = client.the_snapshot_of("hodei/spike-model");
    let installed = sha256_of(&snapshot.join("config.json"));
    assert_ne!(
        installed, declared,
        "the accepted download is not the content the etag named"
    );
    println!("HSP-218 negative: accepted sha256={installed}, declared etag={declared}");
}

/// El fixture del test negativo: el dialecto del hub clasico en su modo
/// `CORRUPT=1`, la misma forma que el fixture del spike de HSP-169.
const CORRUPT_FIXTURE: &str = r#"import hashlib, os, sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROOT = os.environ["FIXTURE_ROOT"]
DECLARED = os.environ["FIXTURE_SHA256"]
PORT = int(os.environ["FIXTURE_PORT"])
REPO = "hodei/spike-model"
FILE = "config.json"

original = open(os.path.join(ROOT, FILE), "rb").read()
size = len(original)
# El primer byte servido se voltea: el fichero en disco sigue siendo el
# que el etag declara, y el tamano declarado no cambia.
corrupted = bytes([original[0] ^ 0xFF]) + original[1:]


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_HEAD(self):
        if self.path == "/api/models/%s/revision/main" % REPO:
            body = ('{"id": "%s", "sha": "%s", "siblings": [{"rfilename": "%s"}]}'
                    % (REPO, "f1cee5eed1e02b6a5c7d2e1e17a0d8b13d5c3a91", FILE)).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        if self.path.startswith("/%s/resolve/" % REPO):
            self.send_response(302)
            self.send_header("X-Repo-Commit", "f1cee5eed1e02b6a5c7d2e1e17a0d8b13d5c3a91")
            self.send_header("X-Linked-Etag", '"%s"' % DECLARED)
            self.send_header("X-Linked-Size", str(size))
            self.send_header("Location", "/cdn-lfs/%s/%s" % (DECLARED, FILE))
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        if self.path.startswith("/cdn-lfs/"):
            # La metadata final del redirect: de aqui la lee el cliente.
            self.send_response(200)
            self.send_header("X-Repo-Commit", "f1cee5eed1e02b6a5c7d2e1e17a0d8b13d5c3a91")
            self.send_header("X-Linked-Etag", '"%s"' % DECLARED)
            self.send_header("X-Linked-Size", str(size))
            self.send_header("Content-Length", str(size))
            self.end_headers()
            return
        self.send_response(404)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_GET(self):
        if self.path == "/api/models/%s/revision/main" % REPO:
            self.do_HEAD()
            return
        if self.path.startswith("/cdn-lfs/"):
            self.send_response(200)
            self.send_header("Content-Type", "application/octet-stream")
            self.send_header("ETag", '"%s"' % DECLARED)
            self.send_header("Content-Length", str(size))
            self.end_headers()
            try:
                self.wfile.write(corrupted)
            except (BrokenPipeError, ConnectionResetError):
                pass
            return
        self.send_response(404)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def log_message(self, fmt, *args):
        pass


ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
"#;
