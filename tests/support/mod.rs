//! El arnés de la conformidad: un servicio **de verdad**, un `curl` de
//! verdad y un `sha256sum` de verdad.
//!
//! Lo que se comprueba aquí no es una función de Rust, sino un proceso
//! que arranca, escucha en un puerto y sirve bytes: por eso todo lo que
//! se usa para hablar con él es una herramienta del sistema y no el
//! código que se está probando.
//!
//! La comparación de los bytes la hace `sha256sum`, que no tiene nada que
//! ver con este servicio. Si el hash que vuelve no es el hash que entró,
//! el contenido no es el mismo, y eso no lo decide el código bajo prueba.
//!
//! Y el resultado se **escribe**: un informe con la versión, el comando de
//! cada caso, el resultado real y los límites. Una conformidad que solo
//! existe en el log de una CI es una conformidad que nadie puede leer
//! dentro de seis meses.

#![allow(dead_code)]

use std::fs::{File, FileTimes};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use hodei_catalog::CatalogWriter;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::Serialize;

/// El binario del servicio, tal y como lo compila la CI.
pub const BIN: &str = env!("CARGO_BIN_EXE_hodei-artifacts-api");

/// El secreto de los tokens de la prueba.
///
/// Es un secreto de prueba, y está aquí a propósito: el token lo firma
/// el propio arnés, y un token que no se puede falsificar no probaría
/// nada.
pub const SECRET: &[u8] = b"raw-conformity-secret-long-enough";

/// El emisor que los tokens declaran.
pub const ISSUER: &str = "https://issuer.example";

/// La audiencia que los tokens declaran.
pub const AUDIENCE: &str = "hodei-artifacts";

/// La cuenta de la primera persona.
pub const FIRST_ACCOUNT: &str = "acme";

/// La cuenta de la segunda persona.
pub const SECOND_ACCOUNT: &str = "other";

/// La clave del repositorio de la primera cuenta.
pub const FIRST_REPOSITORY: &str = "releases";

/// La clave del repositorio de la segunda cuenta.
pub const SECOND_REPOSITORY: &str = "releases";

/// La clave de un repositorio que solo tiene la primera cuenta.
///
/// Sirve para el caso de la cuenta ajena: un repositorio que existe en
/// una cuenta y no en la otra. La respuesta de la cuenta que no lo tiene
/// no puede decir si existe.
pub const FIRST_ONLY_REPOSITORY: &str = "private";

/// El esquema de Cedar del despliegue.
///
/// Declara las dos acciones de la superficie de contenido crudo, porque
/// un bundle que no las declara no puede concederlas, y el servicio no
/// arranca con un bundle que no declara lo que autoriza con.
pub const SCHEMA: &str = r#"{
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

/// La política de Cedar del despliegue.
///
/// Las dos personas pueden publicar y descargar. Lo que **no** decide la
/// política es el aislamiento entre cuentas: ese lo lleva la clave de
/// cada fila del catálogo, y por eso el caso de la cuenta ajena espera
/// un `404` y no un `403`.
pub fn the_policy() -> String {
    let mut policy = String::new();
    for (account, user) in [(FIRST_ACCOUNT, "alice"), (SECOND_ACCOUNT, "bob")] {
        for action in ["UploadAsset", "DownloadAsset"] {
            policy.push_str(&format!(
                "permit(principal == Iam::User::\"hrn:hodei:iam::{account}:User/{user}\", action == Artifacts::Action::\"{action}\", resource);\n"
            ));
        }
    }
    policy
}

/// Lo que un token dice.
#[derive(Serialize)]
struct Claims {
    sub: String,
    account: String,
    exp: usize,
    iss: String,
    aud: String,
}

/// El token de una persona de una cuenta.
///
/// La cuenta del token es la que el servicio verifica: no la que el
/// llamante dice. Un token de otra cuenta es un token de otra cuenta, y
/// por eso el `404` del repositorio ajeno no dice si existe.
pub fn token(account: &str, user: &str) -> String {
    encode(
        &Header::new(Algorithm::HS256),
        &Claims {
            sub: format!("hrn:hodei:iam::{account}:User/{user}"),
            account: account.to_string(),
            exp: 4_000_000_000,
            iss: ISSUER.to_string(),
            aud: AUDIENCE.to_string(),
        },
        &EncodingKey::from_secret(SECRET),
    )
    .expect("the token is signed")
}

/// Un puerto libre del sistema.
///
/// El puerto se abre y se cierra: el servicio lo ocupa después. Es una
/// carrera que casi nunca se pierde, y el arnés la espera y reintenta si
/// el servicio no llega a abrir el puerto.
pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("a free port")
        .local_addr()
        .expect("the address of the port")
        .port()
}

/// El puerto en el que el servicio **dice** que escucha.
///
/// El servicio lo escribe en su log **después** de haber ocupado el
/// puerto, así que esa línea es la prueba de que el proceso que la
/// escribió es el dueño del puerto. Un `connect` que funciona no lo
/// prueba: el otro proceso puede tener el puerto cogido y el nuestro
/// haber muerto al no poder obtenerlo.
fn the_bound_port(log: &Path) -> Option<u16> {
    let content = std::fs::read_to_string(log).ok()?;
    let line = content
        .lines()
        .find(|line| line.contains("Listening on http://"))?;
    let address = without_the_colors(line.split("Listening on http://").nth(1)?);
    address.trim().rsplit(':').next()?.trim().parse().ok()
}

/// Quita las secuencias de escape del log.
///
/// El formato del log puede traer códigos de color, y un puerto con un
/// `\u{1b}[0m` pegado no se puede leer como número.
fn without_the_colors(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character != '\u{1b}' {
            plain.push(character);
            continue;
        }
        // Es una secuencia de escape: se come hasta su letra final.
        for escaped in characters.by_ref() {
            if escaped.is_ascii_alphabetic() {
                break;
            }
        }
    }
    plain
}

/// El nombre que el almacén le pone a una escritura en curso.
///
/// `<pid>-<nanos:x>-<counter:x>.part`: el pid del proceso que escribe, el
/// instante y un contador. El arnés lo copia porque lo que quiere es un
/// staging **de verdad**, con el nombre que el almacén reconoce: un
/// fichero con otro nombre no es una escritura en curso, y el almacén no
/// lo toca.
fn a_staging_name() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    format!("{}-{nanos:x}-{counter:x}.part", std::process::id())
}

/// Un despliegue limpio: un directorio, una base, un almacén, las
/// políticas y el esquema.
///
/// "Limpio" quiere decir **vacío**: la base y el almacén se crean aquí,
/// y lo que hay en ellos es lo que la prueba ha puesto.
pub struct Deployment {
    /// El directorio que lo contiene todo.
    pub directory: tempfile::TempDir,
    /// El directorio de la base.
    pub db: String,
    /// El directorio del almacén de contenido.
    pub cas: String,
    /// El directorio del staging del almacén de contenido.
    pub staging: PathBuf,
    /// Los ficheros de staging que el arnés tiene **bloqueados**.
    ///
    /// El bloqueo es lo que dice que una subida está en curso, y el
    /// fichero vive mientras el descriptor vive: por eso se guardan
    /// aquí y no en una variable suelta.
    lives: Mutex<Vec<std::fs::File>>,
}

impl Deployment {
    /// Un despliegue vacío.
    pub fn new() -> Self {
        let directory = tempfile::tempdir().expect("a temp dir");
        let db = directory.path().join("db").to_string_lossy().into_owned();
        let cas = directory.path().join("cas").to_string_lossy().into_owned();
        let staging = PathBuf::from(&cas).join("staging");
        let policy = directory.path().join("policies.cedar");
        let schema = directory.path().join("schema.json");
        std::fs::write(&policy, the_policy()).expect("the policy is written");
        std::fs::write(&schema, SCHEMA).expect("the schema is written");
        Self {
            directory,
            db,
            cas,
            staging,
            lives: Mutex::new(Vec::new()),
        }
    }

    /// La ruta del fichero de la política.
    fn policy_path(&self) -> PathBuf {
        self.directory.path().join("policies.cedar")
    }

    /// La ruta del fichero del esquema.
    fn schema_path(&self) -> PathBuf {
        self.directory.path().join("schema.json")
    }

    /// Las escrituras en curso que hay en el almacén.
    pub fn staging_entries(&self) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(&self.staging) else {
            return Vec::new();
        };
        let mut found: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        found.sort();
        found
    }

    /// Los bytes que hay escritos en el staging.
    ///
    /// Es lo que separa "el servicio ha abierto el fichero" de "el
    /// servicio está escribiendo el cuerpo": un staging vacío significa
    /// que la subida todavía no ha empezado a llegar, y matar el proceso
    /// ahí no es matar una subida a la mitad.
    pub fn staging_bytes(&self) -> u64 {
        self.staging_entries()
            .iter()
            .filter_map(|path| std::fs::metadata(path).ok())
            .map(|metadata| metadata.len())
            .sum()
    }

    /// Envejece los ficheros de staging.
    ///
    /// Envejecer un fichero es lo que hace el tiempo, y el arnés no
    /// puede esperar dos horas: lo que se quiere es un staging
    /// abandonado, y la caducidad la decide la **fecha de modificación**.
    pub fn age_the_staging(&self, age: Duration) {
        let when = SystemTime::now() - age;
        for path in self.staging_entries() {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .expect("the staging file opens");
            file.set_times(FileTimes::new().set_modified(when))
                .expect("the staging file ages");
        }
    }

    /// Deja un staging **vivo**, como el de una subida de otro proceso.
    ///
    /// Es un fichero recién escrito en el directorio de staging, con el
    /// nombre que el almacén le pone a una escritura, y **bloqueado**:
    /// el bloqueo es lo que dice que la subida está en curso, y lo que
    /// el arranque tiene que respetar. Un fichero sin bloquear sería el
    /// de un proceso que se quedó sin escribir, y ese sí se limpia.
    ///
    /// El descriptor se queda en el despliegue, porque el bloqueo vive en
    /// el descriptor: si se soltara, el staging dejaría de estar vivo.
    pub fn a_live_staging(&self) -> PathBuf {
        let path = self.staging.join(a_staging_name());
        let file = std::fs::File::create(&path).expect("the live staging is written");
        file.try_lock().expect("the live staging is locked");
        self.lives
            .lock()
            .expect("the lives of the harness")
            .push(file);
        path
    }

    /// Siembra el catálogo con un repositorio por cuenta.
    ///
    /// La siembra va en un **proceso aparte**, y no por comodidad: el
    /// motor embebido mantiene el bloqueo del directorio mientras hay una
    /// conexión abierta, y el servicio es otro proceso que tiene que poder
    /// abrir la base.
    pub fn seed(&self) {
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .status()
            .expect("the seeder runs");
        assert!(status.success(), "the catalog could not be seeded");
    }

    /// Arranca el servicio y espera a que abra el puerto.
    ///
    /// El puerto se elige en cada intento, y no antes: un puerto libre
    /// puede ser tomado por otro proceso entre que se elige y que el
    /// servicio lo ocupa, y en ese caso el arnés acabaría hablando con
    /// el servicio de otro despliegue —una suite en verde que no midió
    /// lo que dice medir—.
    ///
    /// Lo que quita esa duda es el **log**: el servicio dice en qué
    /// puerto escuchó, y lo dice **después** de haberlo ocupado. Si otro
    /// proceso tenía el puerto, este servicio no puede escribir esa
    /// línea, así que el arnés no lo toma por suyo y repite con otro.
    ///
    /// El servicio también puede morir al arrancar cuando el motor de
    /// disco todavía tiene el bloqueo del directorio, así que un intento
    /// que muere antes de abrir el puerto se repite: lo que se prueba es
    /// el servicio, no la carrera del bloqueo.
    pub fn start(&self, extra_environment: &[(&str, &str)]) -> Service {
        let log = self.log_path();
        for _attempt in 0..5 {
            let port = free_port();
            let stderr = File::create(&log).expect("the log of the service");
            let stdout = stderr.try_clone().expect("the log of the service, twice");
            let mut command = Command::new(BIN);
            command
                .env_remove("RUN_MODE")
                .env_remove("HODEI_SECURITY__AUTH_JWKS_PATH")
                .env("HODEI_SERVER__HOST", "127.0.0.1")
                .env("HODEI_SERVER__PORT", port.to_string())
                .env("HODEI_ROCKSDB__PATH", &self.db)
                .env("HODEI_CONTENT__CAS_PATH", &self.cas)
                .env("HODEI_REPOSITORY__DURABLE", "true")
                .env("HODEI_SECURITY__RAW_API_ENABLED", "true")
                .env(
                    "HODEI_SECURITY__AUTH_JWT_SECRET",
                    String::from_utf8(SECRET.to_vec()).unwrap(),
                )
                .env("HODEI_SECURITY__AUTH_ISSUER", ISSUER)
                .env("HODEI_SECURITY__AUTH_AUDIENCE", AUDIENCE)
                .env(
                    "HODEI_SECURITY__AUTHORIZATION_POLICY_PATH",
                    self.policy_path(),
                )
                .env(
                    "HODEI_SECURITY__AUTHORIZATION_SCHEMA_PATH",
                    self.schema_path(),
                )
                .env("RUST_LOG", "info")
                .stdin(Stdio::null())
                .stdout(Stdio::from(stdout))
                .stderr(Stdio::from(stderr));
            for (name, value) in extra_environment {
                command.env(name, value);
            }
            let mut child = command.spawn().expect("the service starts");

            let deadline = Instant::now() + Duration::from_secs(30);
            while Instant::now() < deadline {
                // El puerto lo dice el servicio, y solo lo dice si lo
                // ha ocupado: esa es la prueba de que el hijo es el
                // dueño del puerto.
                if the_bound_port(&log) == Some(port)
                    && TcpStream::connect(("127.0.0.1", port)).is_ok()
                    && matches!(child.try_wait(), Ok(None))
                {
                    return Service { port, child };
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
            std::fs::read_to_string(&log).unwrap_or_default()
        );
    }

    /// La ruta del log del servicio.
    pub fn log_path(&self) -> PathBuf {
        self.directory.path().join("service.log")
    }

    /// El log del servicio, para leerlo cuando algo falla.
    pub fn the_log(&self) -> String {
        std::fs::read_to_string(self.log_path()).unwrap_or_default()
    }
}

/// El servicio que está en pie.
pub struct Service {
    /// El puerto donde escucha.
    pub port: u16,
    /// El proceso.
    pub child: Child,
}

impl Drop for Service {
    /// El servicio no se queda vivo si la prueba se cae.
    ///
    /// Sin esto, una prueba que falla antes de parar el servicio deja
    /// un proceso escuchando en su puerto: el siguiente test puede
    /// hablar con un servicio viejo, y la suite se pone verde sin haber
    /// probado lo que dice probar.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Para el servicio.
pub fn stop(service: &mut Service) {
    let _ = service.child.kill();
    let _ = service.child.wait();
}

/// Para el servicio, de golpe.
///
/// Un `SIGKILL` es lo que pasa cuando una máquina se apaga: no hay
/// oportunidad de limpiar nada, y por eso es el que reproduce la
/// interrupción de verdad.
pub fn kill(service: &mut Service) {
    let _ = service.child.kill();
    let _ = service.child.wait();
}

/// El digest de un fichero, dicho por `sha256sum`.
///
/// La comparación no la hace el código bajo prueba: `sha256sum` es un
/// programa que no tiene nada que ver con este servicio, y es el que dice
/// si los bytes son los mismos.
pub fn sha256_of(file: &Path) -> String {
    let output = Command::new("sha256sum")
        .arg(file)
        .output()
        .expect("sha256sum has to be there");
    assert!(output.status.success(), "sha256sum failed on {file:?}");
    String::from_utf8(output.stdout)
        .expect("the digest is text")
        .split_whitespace()
        .next()
        .expect("a digest")
        .to_string()
}

/// La respuesta de un `curl`.
pub struct Answer {
    /// El código de estado.
    pub status: u16,
    /// Los bytes del cuerpo que el `curl` escribió.
    ///
    /// Lo dice el propio `curl`, que sabe cuántos bytes escribió en el
    /// destino: es la forma de medir un `HEAD` sin cuerpo sin suponerlo.
    pub size: u64,
    /// Las cabeceras, en texto.
    pub headers: String,
}

impl Answer {
    /// Una cabecera de la respuesta.
    pub fn header(&self, name: &str) -> Option<String> {
        self.headers.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            if key.trim().eq_ignore_ascii_case(name) {
                Some(value.trim().to_string())
            } else {
                None
            }
        })
    }
}

/// Un `curl` que publica un fichero.
///
/// Con `--limit-rate` el cuerpo llega despacio, que es lo que hace falta
/// para poder matar el servicio **a mitad de la subida**.
pub fn curl_upload(
    port: u16,
    path: &str,
    token: &str,
    file: &Path,
    limit_rate: Option<&str>,
) -> Answer {
    let mut command = a_curl(port, path, token);
    command.arg("--request");
    command.arg("PUT");
    command.arg("--data-binary");
    command.arg(format!("@{}", file.display()));
    if let Some(rate) = limit_rate {
        command.arg("--limit-rate");
        command.arg(rate);
    }
    command.arg("--output");
    command.arg("/dev/null");
    the_answer(&mut command)
}

/// Un `curl` que descarga a un fichero, o a la nada.
pub fn curl_download(port: u16, path: &str, token: &str, into: Option<&Path>) -> Answer {
    let mut command = a_curl(port, path, token);
    match into {
        Some(file) => {
            command.arg("--output");
            command.arg(file.to_string_lossy().into_owned());
        }
        None => {
            command.arg("--output");
            command.arg("/dev/null");
        }
    }
    the_answer(&mut command)
}

/// Un `curl` que solo pide la descripción.
pub fn curl_describe(port: u16, path: &str, token: &str) -> Answer {
    let mut command = a_curl(port, path, token);
    command.arg("--head");
    command.arg("--output");
    command.arg("/dev/null");
    the_answer(&mut command)
}

/// Un `curl` con una cabecera de más, que deja el cuerpo en un fichero.
///
/// Es lo que permite comprobar los **bytes** de un tramo, y no solo su
/// código y su `Content-Range`.
pub fn curl_get_with_header(
    port: u16,
    path: &str,
    token: &str,
    header: (&str, &str),
    into: &Path,
) -> Answer {
    let mut command = a_curl(port, path, token);
    command.arg("--header");
    command.arg(format!("{}: {}", header.0, header.1));
    command.arg("--output");
    command.arg(into.to_string_lossy().into_owned());
    the_answer(&mut command)
}

/// Un `curl` que publica un fichero, en segundo plano.
///
/// La subida se queda en curso mientras el arnés hace otra cosa: es lo
/// que permite matar al servicio **a mitad de una subida**, que es la
/// forma de mirar un corte de verdad.
pub fn curl_upload_in_background(
    port: u16,
    path: &str,
    token: &str,
    file: &Path,
    limit_rate: Option<&str>,
) -> Child {
    let mut command = Command::new("curl");
    command.args(["--silent", "--show-error", "--path-as-is"]);
    command.arg("--request");
    command.arg("PUT");
    command.arg("--header");
    command.arg(format!("Authorization: Bearer {token}"));
    command.arg("--data-binary");
    command.arg(format!("@{}", file.display()));
    if let Some(rate) = limit_rate {
        command.arg("--limit-rate");
        command.arg(rate);
    }
    command.arg("--output");
    command.arg("/dev/null");
    command.arg("--dump-header");
    command.arg("-");
    command.arg("--write-out");
    command.arg("%{http_code} %{size_download}");
    command.arg(format!("http://127.0.0.1:{port}{path}"));
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.spawn().expect("the upload starts")
}

/// La respuesta de un `curl` que se lanzó en segundo plano.
///
/// Un `curl` que no pudo hablar con el servicio no tiene código: eso es
/// un fallo de la prueba, y por eso el código es `0` y no un número
/// inventado.
pub fn wait_for_the_answer(child: Child) -> Answer {
    let output = child.wait_with_output().expect("the curl finishes");
    assert!(
        output.status.success(),
        "the curl failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let (headers, status, size) = the_answer_of(&stdout);
    Answer {
        status,
        size,
        headers,
    }
}

/// Un `curl` con los argumentos que todas las peticiones comparten.
///
/// El camino va tal cual (`--path-as-is`), la credencial va si la hay,
/// las cabeceras se vuelcan y el `curl` cuenta el código y los bytes que
/// escribió: el código y el tamaño los dice el `curl`, no el código bajo
/// prueba.
fn a_curl(port: u16, path: &str, token: &str) -> Command {
    let mut command = Command::new("curl");
    command.args(["--silent", "--show-error", "--path-as-is"]);
    if !token.is_empty() {
        command.arg("--header");
        command.arg(format!("Authorization: Bearer {token}"));
    }
    command.arg("--dump-header");
    command.arg("-");
    command.arg("--write-out");
    command.arg("%{http_code} %{size_download}");
    command.arg(format!("http://127.0.0.1:{port}{path}"));
    command
}

/// La salida de un `curl`, comprobando que el `curl` terminó bien.
///
/// Un `curl` que falla no tiene respuesta: su código es un error, no un
/// estado HTTP. Mirar el estado de salida es lo que separa "el servicio
/// dijo 404" de "no hubo servicio".
fn the_output(command: &mut Command) -> String {
    let output = command.output().expect("curl has to be there");
    assert!(
        output.status.success(),
        "the curl failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// La respuesta de un `curl` que se ha ejecutado.
fn the_answer(command: &mut Command) -> Answer {
    let stdout = the_output(command);
    let (headers, status, size) = the_answer_of(&stdout);
    Answer {
        status,
        size,
        headers,
    }
}

/// Las cabeceras, el código y el tamaño de la salida de un `curl`.
///
/// El `curl` escribe las cabeceras y después, al final, el código y el
/// número de bytes que escribió en el destino.
fn the_answer_of(stdout: &str) -> (String, u16, u64) {
    let mut headers = String::new();
    let mut status = 0;
    let mut size = 0;
    for line in stdout.lines() {
        let trimmed = line.trim();
        let mut parts = trimmed.split_whitespace();
        let (code, bytes) = match (parts.next(), parts.next()) {
            (Some(code), Some(bytes))
                if !code.is_empty()
                    && !bytes.is_empty()
                    && code.chars().all(|character| character.is_ascii_digit())
                    && bytes.chars().all(|character| character.is_ascii_digit()) =>
            {
                (code, bytes)
            }
            _ => {
                headers.push_str(line);
                headers.push('\n');
                continue;
            }
        };
        status = code.parse().unwrap_or(0);
        size = bytes.parse().unwrap_or(0);
    }
    (headers, status, size)
}

/// Un caso del informe.
#[derive(Clone)]
pub struct Case {
    /// El identificador del caso.
    pub id: String,
    /// Lo que se dio.
    pub given: String,
    /// Lo que se hizo.
    pub when: String,
    /// El comando que lo hizo.
    pub command: String,
    /// El resultado real.
    pub outcome: String,
    /// Si el caso pasó.
    pub passed: bool,
}

/// El informe de la conformidad.
///
/// Los casos se escriben en un fichero, no solo en el log: un informe que
/// solo existe en la salida de una CI se pierde en cuanto la CI se
/// limpia.
pub struct Report {
    cases: Vec<Case>,
}

impl Report {
    /// Un informe vacío.
    pub fn new() -> Self {
        Self { cases: Vec::new() }
    }

    /// Anota un caso y escribe el informe.
    ///
    /// Un caso con el mismo identificador **reemplaza** al anterior: un
    /// caso que se anota antes de ejecutarse y se vuelve a anotar con su
    /// resultado deja en el informe un `FALLO` si la prueba se muere
    /// antes de terminar, y no un caso que no aparece. Un informe que
    /// solo sabe decir `PASS` no es un informe: es propaganda.
    pub fn record(&mut self, case: Case) {
        match self
            .cases
            .iter_mut()
            .find(|existing| existing.id == case.id)
        {
            Some(existing) => *existing = case,
            None => self.cases.push(case),
        }
        self.write();
    }

    /// La ruta del fichero del informe.
    pub fn the_path() -> PathBuf {
        let configured = std::env::var_os("HODEI_UAT_REPORT").map(PathBuf::from);
        configured.unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("uat")
                .join("raw-conformity.md")
        })
    }

    /// Escribe el informe.
    pub fn write(&self) {
        let path = Self::the_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("the directory of the report");
        }
        std::fs::write(&path, self.as_markdown()).expect("the report is written");
    }

    /// El informe, en Markdown.
    pub fn as_markdown(&self) -> String {
        let mut text = String::new();
        text.push_str("# Conformidad RAW — informe\n\n");
        text.push_str(&format!("**Servicio:** {}\n\n", the_version()));
        text.push_str(&format!("**Cuando:** {}\n\n", the_moment()));
        text.push_str(&format!("**Herramientas:** {}\n\n", the_tools()));
        text.push_str("| Caso | Given | When | Comando | Resultado |\n");
        text.push_str("|---|---|---|---|---|\n");
        for case in &self.cases {
            text.push_str(&format!(
                "| {} | {} | {} | `{}` | {} {} |\n",
                case.id,
                case.given,
                case.when,
                case.command.replace('|', "\\|"),
                if case.passed { "PASS" } else { "FALLO" },
                case.outcome
            ));
        }
        text.push_str(
            "\n## Lo que este informe **no** certifica\n\n\
             - **Ni compatibilidad OCI.** Nada de lo que se ha ejecutado habla el protocolo de OCI Distribution: no hay manifiestos, ni `HEAD` de tags, ni `GET` de blobs por digest con la semántica de OCI. La superficie es `raw`.\n\
             - **Ni compatibilidad S3.** No hay `ListBuckets`, ni `PutObject` con firma AWS, ni nada que se parezca a la API de S3.\n\
             - **Ni disponibilidad.** No se ha medido el tiempo de respuesta, ni el comportamiento bajo carga, ni que el servicio aguante lo que sea: lo que se ha medido es que los bytes vuelven intactos.\n\
             - **Ni la recolección de blobs sin referencia.** Un blob sin referenciar se puede quedar en el almacén; lo que se ha comprobado es que no se publica solo.\n",
        );
        text
    }
}

/// El informe compartido por los casos de este binario.
static REPORT: OnceLock<Mutex<Report>> = OnceLock::new();

/// El informe compartido.
pub fn the_report() -> &'static Mutex<Report> {
    REPORT.get_or_init(|| Mutex::new(Report::new()))
}

/// Anota un caso en el informe compartido.
pub fn record(case: Case) {
    let mut report = the_report().lock().expect("the report is not poisoned");
    report.record(case);
}

/// La versión del servicio que se ha probado.
pub fn the_version() -> String {
    let commit = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|commit| !commit.is_empty())
        .unwrap_or_else(|| "sin commit".to_string());
    format!(
        "hodei-artifacts {} (commit {})",
        env!("CARGO_PKG_VERSION"),
        commit
    )
}

/// Las versiones de las herramientas que participa en la conformidad.
pub fn the_tools() -> String {
    let curl = the_first_line_of(&["curl", "--version"]);
    let sha = the_first_line_of(&["sha256sum", "--version"]);
    let cargo = the_first_line_of(&["cargo", "--version"]);
    let system = the_first_line_of(&["uname", "-srm"]);
    format!("`{curl}`; `{sha}`; `{cargo}`; sistema `{system}`")
}

/// El momento de la conformidad.
///
/// Un informe sin fecha es un informe que no se puede situar en el
/// tiempo, y un informe que no se puede situar en el tiempo no dice si
/// se ha repetido o si es la primera vez.
pub fn the_moment() -> String {
    let output = Command::new("date").args(["--iso-8601=seconds"]).output();
    match output {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => "sin fecha".to_string(),
    }
}

/// La primera línea de la salida de un comando.
fn the_first_line_of(command: &[&str]) -> String {
    let Ok(output) = Command::new(command[0]).args(&command[1..]).output() else {
        return format!("{}: no disponible", command[0]);
    };
    if !output.status.success() {
        return format!("{}: no disponible", command[0]);
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or("desconocida")
        .trim()
        .to_string()
}

/// Siembra el catálogo en el proceso actual.
///
/// Solo siembra si el proceso es el que otro ha mandado sembrar: el
/// proceso que siembra es otro, y el que se prueba es el servicio.
pub fn seed_if_asked() {
    let Some(db) = std::env::var_os("HODEI_SEED_DB") else {
        return;
    };
    seed_the_catalog(&db.to_string_lossy());
}

/// Siembra el catálogo con un repositorio por cuenta.
pub fn seed_the_catalog(path: &str) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("the runtime");
    runtime.block_on(async {
        let db = surrealdb::Surreal::new::<surrealdb::engine::local::RocksDb>(path)
            .await
            .expect("rocksdb");
        db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");
        let store = hodei_catalog_storage::SurrealCatalogStore::with_authorizer(
            db.clone(),
            std::sync::Arc::new(hodei_catalog::AllowAllAuthorizer),
        );
        store.initialize().await.expect("initialize");
        for (account, key) in [
            (FIRST_ACCOUNT, FIRST_REPOSITORY),
            (FIRST_ACCOUNT, FIRST_ONLY_REPOSITORY),
            (SECOND_ACCOUNT, SECOND_REPOSITORY),
        ] {
            let mut config =
                hodei_catalog::RepositoryConfig::new(hodei_catalog::DigestScope::Global);
            config.config_settings = serde_json::json!({});
            store
                .create_repository(
                    account,
                    hodei_catalog::RepositoryDescriptor {
                        hrn: hodei_catalog::repository_hrn(
                            "hodei",
                            "artifacts",
                            "eu",
                            account,
                            key,
                        ),
                        account: account.to_string(),
                        repo_key: key.to_string(),
                        format: hodei_catalog::RepositoryFormat::Raw,
                        mode: hodei_catalog::RepositoryMode::Hosted,
                        config,
                        created_at: chrono::Utc::now(),
                        updated_at: chrono::Utc::now(),
                    },
                )
                .await
                .expect("the repository is created");
        }
        drop(store);
        drop(db);
    });
}
