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

use std::collections::HashMap;
use std::fs::{File, FileTimes};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use base64::Engine as _;
use hodei_catalog::CatalogWriter;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::Serialize;
use sha2::Digest as _;

/// El laboratorio del registro: el upstream con contador, el resultado de
/// un caso y el informe en JSON de una ejecución.
///
/// Vive aquí y no en el fichero de un caso porque siete tareas más lo
/// van a usar: un upstream con contador que se implemente dos veces es un
/// contador que mide uno de los dos.
pub mod lab;

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
      "Repository": { "memberOfTypes": ["Artifacts::Repository"], "shape": { "type": "Record", "attributes": {} } },
      "Artifact": { "shape": { "type": "Record", "attributes": {} } }
    },
    "actions": {
      "CreateRepository": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
      "ReadRepository": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
      "PromoteAsset": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
      "RevokePromotion": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
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
    // Solo alice promueve y revoca: bob puede leer y escribir, pero la
    // decision de que software es de fiar no es suya.
    for action in ["PromoteAsset", "RevokePromotion"] {
        policy.push_str(&format!(
            "permit(principal == Iam::User::\"hrn:hodei:iam::{FIRST_ACCOUNT}:User/alice\", action == Artifacts::Action::\"{action}\", resource);\n"
        ));
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
///
/// El token se acuña con un nombre, y ese nombre es el que va al
/// informe: el token de verdad no se escribe nunca en un fichero que se
/// archiva. El nombre sale de aquí, de la misma función que acuña el
/// token, así que los dos no pueden desincronizarse.
pub fn token(account: &str, user: &str) -> String {
    let signed = encode(
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
    .expect("the token is signed");
    remember_the_name_of(&signed, user);
    signed
}

/// Los nombres de las credenciales que el arnés ha acuñado.
///
/// El informe dice **quién** actuó, no con qué se autenticó: el token no
/// viaja al informe, y un token en un fichero archivado es una
/// credencial en un fichero archivado.
fn the_names() -> &'static Mutex<HashMap<String, String>> {
    static NAMES: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    NAMES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// El nombre con el que se escribe una credencial.
///
/// Una credencial que el arnés no acuñó no tiene nombre conocido, y se
/// escribe como redactada: el nombre es lo que se puede decir en claro.
pub fn the_name_of(token: &str) -> String {
    if token.is_empty() {
        return "<sin credencial>".to_string();
    }
    the_names()
        .lock()
        .expect("the names of the credentials")
        .get(token)
        .cloned()
        .unwrap_or_else(|| "<redactada>".to_string())
}

/// Anota el nombre de una credencial acuñada.
fn remember_the_name_of(token: &str, user: &str) {
    the_names()
        .lock()
        .expect("the names of the credentials")
        .insert(token.to_string(), format!("<{user}>"));
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
        self.seed_with(&[]);
    }

    /// Siembra el catálogo con un repositorio por cuenta **y** los que se
    /// le pidan.
    ///
    /// Los repositorios de mas son los que necesita una suite que empuja a
    /// un repositorio con nombre: el registro solo publica en repositorios
    /// que existen, y sin sembrarlo la suite mediria un 404 donde queria
    /// medir un push.
    pub fn seed_with(&self, extra: &[&str]) {
        self.seed_trusted(extra, &[]);
    }

    /// Siembra como [`Self::seed_with`] y deja en el canal **trusted** los
    /// repositorios de la primera cuenta que se nombren.
    ///
    /// El canal es un ajuste del repositorio (`config_settings.trust.serve`)
    /// y no hay una ruta HTTP que cree repositorios del catalogo, asi que
    /// se fija al sembrar, como lo haria quien opera el despliegue.
    pub fn seed_trusted(&self, extra: &[&str], trusted: &[&str]) {
        self.seed_proxies(extra, trusted, &[]);
    }

    /// Siembra como [`Self::seed_trusted`] y crea ademas repositorios
    /// **proxy** de la primera cuenta: cada uno con su clave y la
    /// configuracion de su upstream (`config_settings.proxy.upstream`).
    pub fn seed_proxies(
        &self,
        extra: &[&str],
        trusted: &[&str],
        proxies: &[(&str, serde_json::Value)],
    ) {
        let proxies: serde_json::Map<String, serde_json::Value> = proxies
            .iter()
            .map(|(key, upstream)| (key.to_string(), upstream.clone()))
            .collect();
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env("HODEI_SEED_EXTRA", extra.join(","))
            .env("HODEI_SEED_TRUSTED", trusted.join(","))
            .env(
                "HODEI_SEED_PROXY",
                serde_json::Value::Object(proxies).to_string(),
            )
            .status()
            .expect("the seeder runs");
        assert!(status.success(), "the catalog could not be seeded");
    }

    /// Siembra el catálogo declarando **sustituibles** los repositorios
    /// listados: su `config_settings.raw.replace` es `allow`, y publicar
    /// en una dirección ocupada es un reemplazo y no un 409.
    ///
    /// La política no se puede pedir por HTTP: la decide quien opera el
    /// despliegue al sembrar, igual que el canal de confianza.
    pub fn seed_with_replacement(&self, replace: &[&str]) {
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env("HODEI_SEED_REPLACE", replace.join(","))
            .status()
            .expect("the seeder runs");
        assert!(status.success(), "the catalog could not be seeded");
    }

    /// Reconfigura un repositorio proxy ya sembrado: sustituye su
    /// `config_settings.proxy` y sube la version de su configuracion.
    ///
    /// Lo hace un proceso que acaba enseguida y con el servicio parado: la
    /// base la bloquea el proceso que la abre.
    pub fn reconfigure_proxy(&self, key: &str, proxy: serde_json::Value) {
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env("HODEI_RECONFIG_KEY", key)
            .env("HODEI_RECONFIG_PROXY", proxy.to_string())
            .status()
            .expect("the reconfigurer runs");
        assert!(status.success(), "the repository could not be reconfigured");
    }

    /// Reconfigura un repositorio ya sembrado: le pone (o le quita, con
    /// `null`) la politica de namespaces y sube la version de su
    /// configuracion.
    ///
    /// Lo hace un proceso que acaba enseguida y con el servicio parado: la
    /// base la bloquea el proceso que la abre. Es la revision de politica
    /// que una resolucion previa incompatible tiene que respetar.
    pub fn reconfigure_namespaces(&self, key: &str, namespaces: serde_json::Value) {
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env("HODEI_RECONFIG_KEY", key)
            .env("HODEI_RECONFIG_NAMESPACES", namespaces.to_string())
            .status()
            .expect("the reconfigurer runs");
        assert!(status.success(), "the namespaces could not be reconfigured");
    }

    /// Crea repositorios de **grupo** de la primera cuenta, en el orden
    /// dado (un grupo anidado se declara despues de los suyos). Cada uno es
    /// una clave y la lista ordenada de sus miembros.
    ///
    /// Se siembra en un proceso aparte y con el servicio parado, como el
    /// resto: la base la bloquea el proceso que la abre.
    pub fn seed_groups(&self, groups: &[(&str, &[&str])]) {
        let groups: Vec<serde_json::Value> = groups
            .iter()
            .map(|(key, members)| serde_json::json!({ "key": key, "members": members }))
            .collect();
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env(
                "HODEI_SEED_GROUPS",
                serde_json::Value::Array(groups).to_string(),
            )
            .status()
            .expect("the group seeder runs");
        assert!(status.success(), "the groups could not be seeded");
    }

    /// Crea un **grupo go** de la primera cuenta: la clave y la lista
    /// ordenada de sus miembros.
    pub fn seed_go_group(&self, key: &str, members: &[&str]) {
        let groups = vec![serde_json::json!({
            "key": key,
            "members": members,
            "format": "go",
        })];
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env(
                "HODEI_SEED_GROUPS",
                serde_json::Value::Array(groups).to_string(),
            )
            .status()
            .expect("the group seeder runs");
        assert!(status.success(), "the go group could not be seeded");
    }

    /// Crea un **grupo nuget** de la primera cuenta: la clave y la lista
    /// ordenada de sus miembros.
    pub fn seed_nuget_group(&self, key: &str, members: &[&str]) {
        let groups = vec![serde_json::json!({
            "key": key,
            "members": members,
            "format": "nuget",
        })];
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env(
                "HODEI_SEED_GROUPS",
                serde_json::Value::Array(groups).to_string(),
            )
            .status()
            .expect("the group seeder runs");
        assert!(status.success(), "the nuget group could not be seeded");
    }

    /// Crea repositorios **Maven2 hosted** de la primera cuenta. Cada uno
    /// es una clave y su `config_settings.maven` (`null` si no lleva).
    ///
    /// Se siembra en un proceso aparte y con el servicio parado: la base
    /// la bloquea el proceso que la abre.
    pub fn seed_maven(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, maven)| serde_json::json!({ "key": key, "maven": maven }))
            .collect();
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env(
                "HODEI_SEED_MAVEN",
                serde_json::Value::Array(repositories).to_string(),
            )
            .status()
            .expect("the maven seeder runs");
        assert!(
            status.success(),
            "the maven repositories could not be seeded"
        );
    }

    /// Crea repositorios **Maven2 proxy** de la primera cuenta. Cada uno es
    /// una clave, su `config_settings.maven` (`null` si no lleva) y su
    /// `config_settings.proxy` (el upstream y su cache).
    pub fn seed_maven_proxies(
        &self,
        repositories: &[(&str, serde_json::Value, serde_json::Value)],
    ) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, maven, proxy)| {
                serde_json::json!({ "key": key, "maven": maven, "proxy": proxy })
            })
            .collect();
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env(
                "HODEI_SEED_MAVEN",
                serde_json::Value::Array(repositories).to_string(),
            )
            .status()
            .expect("the maven proxy seeder runs");
        assert!(status.success(), "the maven proxies could not be seeded");
    }

    /// Crea grupos **Maven2** de la primera cuenta, en el orden dado.
    pub fn seed_maven_groups(&self, groups: &[(&str, &[&str])]) {
        let groups: Vec<serde_json::Value> = groups
            .iter()
            .map(|(key, members)| {
                serde_json::json!({ "key": key, "members": members, "format": "maven2" })
            })
            .collect();
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env(
                "HODEI_SEED_GROUPS",
                serde_json::Value::Array(groups).to_string(),
            )
            .status()
            .expect("the maven group seeder runs");
        assert!(status.success(), "the maven groups could not be seeded");
    }

    /// Crea repositorios **npm hosted** de la primera cuenta. Cada uno es
    /// una clave y su `config_settings.npm` (`null` si no lleva).
    pub fn seed_npm(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, npm)| serde_json::json!({ "key": key, "npm": npm, "format": "npm" }))
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the npm repositories");
    }

    /// Crea repositorios **cargo hosted** de la primera cuenta. Cada entrada
    /// es una clave y su config (`null` si no tiene).
    pub fn seed_cargo(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(
                |(key, cargo)| serde_json::json!({ "key": key, "cargo": cargo, "format": "cargo" }),
            )
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the cargo repositories");
    }

    /// Crea repositorios **conan hosted** de la primera cuenta. Cada uno
    /// es una clave y su config (`null` si no lleva).
    pub fn seed_conan(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(
                |(key, conan)| serde_json::json!({ "key": key, "conan": conan, "format": "conan" }),
            )
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the conan repositories");
    }

    /// Crea repositorios **galaxy hosted** de la primera cuenta. Cada uno
    /// es una clave y su config (`null` si no lleva).
    /// Crea repositorios **terraform** de la primera cuenta. Cada
    /// entrada es una clave y su config (`null` si no tiene); con la
    /// config se crea hosted (registry de modulos) y con `proxy` proxy
    /// (mirror de providers).
    pub fn seed_terraform(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, terraform)| {
                serde_json::json!({ "key": key, "terraform": terraform, "format": "terraform" })
            })
            .collect();
        self.run_the_seeder(
            "HODEI_SEED_MAVEN",
            repositories,
            "the terraform repositories",
        );
    }

    /// Crea repositorios **terraform proxy** (mirror de providers) de la
    /// primera cuenta: la clave y su `config_settings.proxy`.
    pub fn seed_terraform_proxies(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, proxy)| {
                serde_json::json!({ "key": key, "proxy": proxy, "format": "terraform" })
            })
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the terraform mirrors");
    }

    pub fn seed_galaxy(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(
                |(key, galaxy)| serde_json::json!({ "key": key, "galaxy": galaxy, "format": "galaxy" }),
            )
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the galaxy repositories");
    }

    /// Crea repositorios **hf hosted** de la primera cuenta y siembra sus
    /// modelos. Cada asset es `(ruta de asset, fichero fuente)`: el
    /// contenido se guarda en el CAS y se publica en el catalogo. La
    /// escritura del dialecto no se declara (la superficie hf es de
    /// lectura), y por eso el contenido entra por la siembra del hub.
    pub fn seed_hf(&self, repositories: &[(&str, Vec<(&str, std::path::PathBuf)>)]) {
        let entries: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, assets)| {
                serde_json::json!({
                    "key": key,
                    "assets": assets
                        .iter()
                        .map(|(asset, file)| {
                            serde_json::json!({
                                "asset": asset,
                                "file": file.to_string_lossy(),
                            })
                        })
                        .collect::<Vec<_>>(),
                })
            })
            .collect();
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env("HODEI_SEED_CAS", &self.cas)
            .env(
                "HODEI_SEED_HF",
                serde_json::Value::Array(entries).to_string(),
            )
            .status()
            .expect("the seeder runs");
        assert!(status.success(), "the hf repositories could not be seeded");
    }

    /// Crea repositorios **apt hosted** de la primera cuenta. Cada
    /// entrada es una clave y su config (`null` si no tiene).
    pub fn seed_apt(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, apt)| serde_json::json!({ "key": key, "format": "apt", "apt": apt }))
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the apt repositories");
    }

    /// Crea repositorios **yum hosted** de la primera cuenta. Cada
    /// entrada es una clave y su config (`null` si no tiene).
    pub fn seed_yum(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, yum)| serde_json::json!({ "key": key, "format": "rpm", "yum": yum }))
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the yum repositories");
    }

    /// Crea repositorios **yum proxy** de la primera cuenta. Cada uno es
    /// una clave, su `config_settings.proxy` (el upstream y su cache) y el
    /// canal de confianza en el que se siembra.
    ///
    /// El canal es un ajuste del repositorio (`config_settings.trust.serve`)
    /// y no hay una ruta HTTP que lo fije: se fija al sembrar, como lo
    /// haria quien opera el despliegue. Los que no estan en la lista de
    /// confianza se siembran en el canal abierto.
    pub fn seed_yum_proxies(&self, trusted: &[&str], repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, proxy)| {
                serde_json::json!({
                    "key": key,
                    "proxy": proxy,
                    "format": "rpm",
                    "trusted": trusted.contains(key),
                })
            })
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the yum proxies");
    }

    /// Crea repositorios **go hosted** de la primera cuenta. Cada entrada
    /// es una clave y su config (`null` si no tiene).
    pub fn seed_go(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, go)| serde_json::json!({ "key": key, "format": "go", "go": go }))
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the go repositories");
    }

    /// Crea repositorios **go hosted** de la primera cuenta con el **ambito
    /// del contenido particionado**.
    ///
    /// Existe para el caso que mide la politica de ambito: esta release solo
    /// sirve el ambito global, y un miembro de ambito particionado tiene que
    /// declararse con el diagnostico del ambito, no con un 404 que seria
    /// mentir.
    pub fn seed_go_partitioned(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, go)| {
                serde_json::json!({
                    "key": key,
                    "format": "go",
                    "go": go,
                    "scope": "partitioned",
                })
            })
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the go repositories");
    }

    /// Crea repositorios **go proxy** de la primera cuenta: la clave y su
    /// `config_settings.proxy` (el upstream y su cache).
    pub fn seed_go_proxies(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, proxy)| serde_json::json!({ "key": key, "proxy": proxy, "format": "go" }))
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the go proxies");
    }

    /// Crea repositorios **go proxy** de la primera cuenta, algunos de ellos
    /// en el canal **trusted**.
    ///
    /// El canal es un ajuste del repositorio (`config_settings.trust.serve`)
    /// y no hay una ruta HTTP que lo fije: se fija al sembrar, como lo
    /// haria quien opera el despliegue. Los que no estan en la lista de
    /// confianza se siembran en el canal abierto.
    pub fn seed_go_proxies_trusted(
        &self,
        trusted: &[&str],
        repositories: &[(&str, serde_json::Value)],
    ) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, proxy)| {
                serde_json::json!({
                    "key": key,
                    "proxy": proxy,
                    "format": "go",
                    "trusted": trusted.contains(key),
                })
            })
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the go proxies");
    }

    /// Crea repositorios **nuget hosted** de la primera cuenta. Cada
    /// entrada es una clave y su config (`null` si no tiene).
    pub fn seed_nuget(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(
                |(key, nuget)| serde_json::json!({ "key": key, "format": "nuget", "nuget": nuget }),
            )
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the nuget repositories");
    }

    /// Crea repositorios **nuget proxy** de la primera cuenta: la clave y
    /// su `config_settings.proxy` (el upstream y su cache).
    pub fn seed_nuget_proxies(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(
                |(key, proxy)| serde_json::json!({ "key": key, "proxy": proxy, "format": "nuget" }),
            )
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the nuget proxies");
    }

    /// Crea repositorios **helm** de la primera cuenta, servidos por el
    /// canal OCI. Cada entrada es una clave y su config (`null` si no tiene).
    pub fn seed_helm(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, helm)| serde_json::json!({ "key": key, "format": "helm", "helm": helm }))
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the helm repositories");
    }

    /// Crea repositorios **npm proxy** de la primera cuenta: la clave y su
    /// `config_settings.proxy` (el upstream y su cache).
    pub fn seed_npm_proxies(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, proxy)| serde_json::json!({ "key": key, "proxy": proxy, "format": "npm" }))
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the npm proxies");
    }

    /// Crea repositorios **PyPI hosted** de la primera cuenta. Cada uno es
    /// una clave y su `config_settings.pypi` (`null` si no lleva).
    pub fn seed_pypi(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, pypi)| serde_json::json!({ "key": key, "pypi": pypi, "format": "pypi" }))
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the pypi repositories");
    }

    /// Crea repositorios **PyPI hosted** de la primera cuenta, algunos de
    /// ellos en el canal **trusted**.
    ///
    /// El canal es un ajuste del repositorio (`config_settings.trust.serve`)
    /// y no hay una ruta HTTP que lo fije: se fija al sembrar, como lo
    /// haria quien opera el despliegue.
    pub fn seed_pypi_trusted(&self, trusted: &[&str], repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, pypi)| {
                serde_json::json!({
                    "key": key,
                    "pypi": pypi,
                    "format": "pypi",
                    "trusted": trusted.contains(key),
                })
            })
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the pypi repositories");
    }

    /// Crea repositorios **PyPI proxy** de la primera cuenta: la clave y su
    /// `config_settings.proxy` (el upstream y su cache).
    pub fn seed_pypi_proxies(&self, repositories: &[(&str, serde_json::Value)]) {
        let repositories: Vec<serde_json::Value> = repositories
            .iter()
            .map(|(key, proxy)| serde_json::json!({ "key": key, "proxy": proxy, "format": "pypi" }))
            .collect();
        self.run_the_seeder("HODEI_SEED_MAVEN", repositories, "the pypi proxies");
    }

    /// Crea grupos **PyPI** de la primera cuenta, en el orden dado.
    pub fn seed_pypi_groups(&self, groups: &[(&str, &[&str])]) {
        let groups: Vec<serde_json::Value> = groups
            .iter()
            .map(|(key, members)| {
                serde_json::json!({ "key": key, "members": members, "format": "pypi" })
            })
            .collect();
        self.run_the_seeder("HODEI_SEED_GROUPS", groups, "the pypi groups");
    }

    /// Crea grupos **npm** de la primera cuenta, en el orden dado.
    pub fn seed_npm_groups(&self, groups: &[(&str, &[&str])]) {
        let groups: Vec<serde_json::Value> = groups
            .iter()
            .map(|(key, members)| {
                serde_json::json!({ "key": key, "members": members, "format": "npm" })
            })
            .collect();
        self.run_the_seeder("HODEI_SEED_GROUPS", groups, "the npm groups");
    }

    fn run_the_seeder(&self, variable: &str, entries: Vec<serde_json::Value>, what: &str) {
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env(variable, serde_json::Value::Array(entries).to_string())
            .status()
            .expect("the seeder runs");
        assert!(status.success(), "{what} could not be seeded");
    }

    /// Siembra repositorios de la primera cuenta con ajustes libres.
    ///
    /// Cada entrada es el JSON que `seed_the_maven` entiende: `key`,
    /// `format` (`raw`, `npm`, `pypi`, `maven2`...), `proxy` (si es proxy),
    /// `namespaces` (la politica de reserva, si la declara), `scope` (el
    /// ambito del contenido: `partitioned` si lo declara) y los ajustes
    /// propios del formato (`maven`, `npm`, `pypi`). Es la puerta que usan
    /// las suites que mezclan formato y reserva sin multiplicar sembradores.
    pub fn seed_repositories(&self, entries: &[serde_json::Value]) {
        self.run_the_seeder(
            "HODEI_SEED_MAVEN",
            entries.to_vec(),
            "the free-form repositories",
        );
    }

    /// Siembra grupos de la primera cuenta a partir de entradas libres:
    /// `key`, `members` y la `namespaces` que declaren.
    pub fn seed_groups_raw(&self, groups: &[serde_json::Value]) {
        self.run_the_seeder("HODEI_SEED_GROUPS", groups.to_vec(), "the free-form groups");
    }

    /// Cambia los miembros de un grupo ya sembrado: la version de su
    /// configuracion sube.
    pub fn reconfigure_group(&self, key: &str, members: &[&str]) {
        let status = Command::new(std::env::current_exe().expect("the test binary"))
            .args([
                "--exact",
                "the_catalog_can_be_seeded",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("HODEI_SEED_DB", &self.db)
            .env("HODEI_RECONFIG_GROUP", key)
            .env(
                "HODEI_RECONFIG_MEMBERS",
                serde_json::json!(members).to_string(),
            )
            .status()
            .expect("the group reconfigurer runs");
        assert!(status.success(), "the group could not be reconfigured");
    }

    /// Sustituye la politica de Cedar del despliegue. Hay que hacerlo antes
    /// de arrancar el servicio.
    pub fn write_policy(&self, policy: &str) {
        std::fs::write(self.policy_path(), policy).expect("the policy is written");
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
        for _attempt in 0..5 {
            if let Some(service) = self.try_start(free_port(), extra_environment) {
                return service;
            }
        }
        panic!(
            "the service did not open the port; the log says:\n{}",
            std::fs::read_to_string(self.log_path()).unwrap_or_default()
        );
    }

    /// Arranca el servicio en un puerto concreto.
    ///
    /// El reinicio necesita el **mismo** puerto: el cliente ya sabe a
    /// donde mira, y con otro puerto se mediria otro servicio--uno recien
    /// nacido-- y no lo que se quiere medir--el mismo servicio, otra vez.
    ///
    /// Los intentos se repiten en el mismo puerto, como los de
    /// [`Self::start`]: el motor de disco puede tardar en soltar el
    /// bloqueo del directorio, y un reinicio que se rinde al primer
    /// intento mediria un fallo de arranque por el arranque.
    pub fn start_on(&self, port: u16, extra_environment: &[(&str, &str)]) -> Service {
        for _attempt in 0..5 {
            if let Some(service) = self.try_start(port, extra_environment) {
                return service;
            }
        }
        panic!(
            "the service did not open the port {port}; the log says:\n{}",
            std::fs::read_to_string(self.log_path()).unwrap_or_default()
        )
    }

    /// Un intento de arrancar el servicio en un puerto.
    fn try_start(&self, port: u16, extra_environment: &[(&str, &str)]) -> Option<Service> {
        let log = self.log_path();
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
            .env("HODEI_SECURITY__OCI_API_ENABLED", "true")
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
            // El puerto lo dice el servicio, y solo lo dice si lo ha
            // ocupado: esa es la prueba de que el hijo es el dueño del
            // puerto.
            if the_bound_port(&log) == Some(port)
                && TcpStream::connect(("127.0.0.1", port)).is_ok()
                && matches!(child.try_wait(), Ok(None))
            {
                return Some(Service { port, child });
            }
            if matches!(child.try_wait(), Ok(Some(_))) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let _ = child.kill();
        let _ = child.wait();
        std::thread::sleep(Duration::from_millis(500));
        None
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

/// Los metadatos de una distribucion, como los lee `twine`.
pub fn the_metadata(name: &str, version: &str, requires_python: Option<&str>) -> String {
    let mut metadata = format!(
        "Metadata-Version: 2.1\n\
         Name: {name}\n\
         Version: {version}\n\
         Summary: a distribution for the PyPI suite\n"
    );
    if let Some(requires_python) = requires_python {
        metadata.push_str(&format!("Requires-Python: {requires_python}\n"));
    }
    metadata.push('\n');
    metadata
}

/// El nombre que lleva el fichero de una distribucion.
///
/// PEP 427: en el nombre del fichero, el nombre del proyecto se escribe con
/// `_` en vez de `-`.
pub fn the_escaped_name(name: &str) -> String {
    name.replace(['-', '.'], "_")
}

/// Un `RECORD` de wheel: los ficheros, su hash y su tamano.
fn the_record(entries: &[(&str, &[u8])]) -> String {
    let mut record = String::new();
    for (path, body) in entries {
        let digest = sha2::Sha256::digest(body);
        record.push_str(&format!(
            "{path},sha256={},{}\n",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest),
            body.len()
        ));
    }
    record
}

/// Un wheel: un zip con los metadatos, el `RECORD` y el modulo del paquete.
pub fn a_wheel(name: &str, version: &str, requires_python: Option<&str>) -> Vec<u8> {
    the_wheel(name, version, requires_python, 0)
}

/// Un wheel con un miembro de relleno.
///
/// Una distribucion de verdad pesa mas de dos megas, y el limite que
/// trae el servidor web por defecto son dos: hace falta una que lo pase.
pub fn a_big_wheel(
    name: &str,
    version: &str,
    requires_python: Option<&str>,
    filler: usize,
) -> Vec<u8> {
    the_wheel(name, version, requires_python, filler)
}

/// El wheel de siempre, con un miembro de relleno si se pide.
fn the_wheel(name: &str, version: &str, requires_python: Option<&str>, filler: usize) -> Vec<u8> {
    use std::io::Write;
    let escaped = the_escaped_name(name);
    let dist = format!("{escaped}-{version}.dist-info");
    let metadata = the_metadata(name, version, requires_python);
    let wheel = "Wheel-Version: 1.0\n\
                 Generator: hodei-tests\n\
                 Root-Is-Purelib: true\n\
                 Tag: py3-none-any\n";
    let module = format!("\"\"\"{name}.\"\"\"\n\nVALUE = 42\n");
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut entries: Vec<(String, Vec<u8>)> = vec![
        (format!("{dist}/METADATA"), metadata.into_bytes()),
        (format!("{dist}/WHEEL"), wheel.as_bytes().to_vec()),
        (format!("{name}/__init__.py"), module.into_bytes()),
    ];
    if filler > 0 {
        entries.push((format!("{name}/data.bin"), vec![b'x'; filler]));
    }
    for (path, body) in &entries {
        writer
            .start_file(path.clone(), options)
            .expect("the file of the wheel starts");
        writer
            .write_all(body)
            .expect("the body of the file is written");
    }
    let record = the_record(
        &entries
            .iter()
            .map(|(path, body)| (path.as_str(), body.as_slice()))
            .collect::<Vec<_>>(),
    );
    writer
        .start_file(format!("{dist}/RECORD"), options)
        .expect("the record of the wheel starts");
    writer
        .write_all(record.as_bytes())
        .expect("the record is written");
    writer.finish().expect("the wheel is closed").into_inner()
}

/// Un sdist: un tar con gzip con el `PKG-INFO` y el `pyproject.toml`.
pub fn an_sdist(name: &str, version: &str, requires_python: Option<&str>) -> Vec<u8> {
    use std::io::Write;
    let root = format!("{name}-{version}");
    let metadata = the_metadata(name, version, requires_python);
    let pyproject = format!(
        "[build-system]\n\
         requires = [\"setuptools>=61\"]\n\
         build-backend = \"setuptools.build_meta\"\n\
         \n\
         [project]\n\
         name = \"{name}\"\n\
         version = \"{version}\"\n"
    );
    let module = format!("\"\"\"{name}.\"\"\"\n\nVALUE = 42\n");
    let entries: Vec<(String, &[u8])> = vec![
        (format!("{root}/PKG-INFO"), metadata.as_bytes()),
        (format!("{root}/pyproject.toml"), pyproject.as_bytes()),
        (format!("{root}/{name}/__init__.py"), module.as_bytes()),
    ];
    let mut tar: Vec<u8> = Vec::new();
    for (path, body) in entries {
        tar.extend_from_slice(&the_tar_header(&path, body.len() as u64));
        tar.extend_from_slice(body);
        let padding = (512 - body.len() % 512) % 512;
        tar.extend(std::iter::repeat_n(0u8, padding));
    }
    // Dos bloques de ceros: el fin del archivo.
    tar.extend(std::iter::repeat_n(0u8, 1024));
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&tar).expect("the tar is compressed");
    encoder.finish().expect("the sdist is closed")
}

/// La cabecera de un fichero de tar.
fn the_tar_header(path: &str, size: u64) -> [u8; 512] {
    let mut header = [0u8; 512];
    header[..path.len()].copy_from_slice(path.as_bytes());
    let write = |header: &mut [u8; 512], at: usize, text: &str| {
        header[at..at + text.len()].copy_from_slice(text.as_bytes());
    };
    write(&mut header, 100, "000644 ");
    write(&mut header, 108, "000000 ");
    write(&mut header, 116, "000000 ");
    write(&mut header, 124, &format!("{size:011o} "));
    write(&mut header, 136, &format!("{:011o} ", 0));
    // El checksum se calcula con el campo en blanco.
    header[148..156].fill(b' ');
    header[156] = b'0';
    write(&mut header, 257, "ustar");
    header[263] = 0;
    write(&mut header, 265, "00");
    let checksum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
    write(&mut header, 148, &format!("{checksum:06o} "));
    header[154] = 0;
    header
}

/// El comando `sha256sum` que comprueba el digest de un fichero.
///
/// Es el mismo programa y el mismo fichero que el arnes ejecuta: lo que
/// se escribe en el informe es lo que se corrio, no una reconstruccion.
pub fn sha256_command(file: &Path) -> String {
    let root = file
        .parent()
        .map(|root| root.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = if root.is_empty() {
        file.display().to_string()
    } else {
        file.display().to_string().replace(&root, "<despliegue>")
    };
    format!("sha256sum {name}")
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
    /// El cuerpo que el `curl` escribió, si lo escribió en un fichero.
    ///
    /// Es lo que dice el servidor, byte a byte: sin esto, un test solo
    /// puede afirmar el codigo y no **que** bytes sirvio.
    pub body: Vec<u8>,
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

/// Un `curl` con los argumentos con los que se ejecuta.
///
/// El comando que se ejecuta y el comando que se escribe en el informe
/// salen de la **misma** lista de argumentos. Un comando resumido es una
/// paráfrasis, y una paráfrasis es una opinión: lo que se ejecutó y lo
/// que se escribe son lo mismo.
///
/// La credencial va **redactada** en lo que se escribe: el informe dice
/// con quién se habló, no con qué se autenticó. Un informe se archiva y
/// se comparte, y un token en un fichero es una credencial en un
/// fichero.
pub struct Curl {
    /// Los argumentos, en el orden en que se le pasan al `curl`.
    args: Vec<String>,
    /// La credencial real, que se redacta al escribirla.
    token: String,
    /// El directorio del despliegue, que se abrevia al escribirlo.
    ///
    /// Las rutas del despliegue están en un directorio temporal que
    /// dentro de seis meses no existe. Escribir la ruta entera es
    /// escribir ruido; escribir `<despliegue>/fichero` dice lo mismo y
    /// se puede leer.
    root: Option<PathBuf>,
    /// Donde el `curl` escribe el cuerpo, si lo escribe en un fichero.
    ///
    /// Sin esto, una respuesta solo dice el codigo: no se puede afirmar
    /// **que** bytes sirvio el servidor, solo que dijo algo.
    body: Option<PathBuf>,
}

impl Curl {
    /// Los argumentos que todas las peticiones comparten.
    ///
    /// El camino va tal cual (`--path-as-is`), la credencial va si la
    /// hay, las cabeceras se vuelcan y el `curl` cuenta el código y los
    /// bytes que escribió: el código y el tamaño los dice el `curl`, no
    /// el código bajo prueba.
    fn base(port: u16, path: &str, token: &str) -> Self {
        let mut args = vec![
            "--silent".to_string(),
            "--show-error".to_string(),
            "--path-as-is".to_string(),
        ];
        if !token.is_empty() {
            args.push("--header".to_string());
            args.push(format!("Authorization: Bearer {token}"));
        }
        args.push("--dump-header".to_string());
        args.push("-".to_string());
        args.push("--write-out".to_string());
        args.push("%{http_code} %{size_download}".to_string());
        args.push(format!("http://127.0.0.1:{port}{path}"));
        Self {
            args,
            token: token.to_string(),
            root: None,
            body: None,
        }
    }

    /// Un `curl` que publica un fichero.
    ///
    /// Con `--limit-rate` el cuerpo llega despacio, que es lo que hace
    /// falta para poder matar el servicio **a mitad de la subida**.
    pub fn upload(
        port: u16,
        path: &str,
        token: &str,
        file: &Path,
        limit_rate: Option<&str>,
    ) -> Self {
        let mut curl = Self::base(port, path, token);
        curl.args.push("--request".to_string());
        curl.args.push("PUT".to_string());
        curl.args.push("--data-binary".to_string());
        curl.args.push(format!("@{}", file.display()));
        if let Some(rate) = limit_rate {
            curl.args.push("--limit-rate".to_string());
            curl.args.push(rate.to_string());
        }
        curl.args.push("--output".to_string());
        curl.args.push("/dev/null".to_string());
        curl.root = file.parent().map(Path::to_path_buf);
        curl
    }

    /// Un `curl` que descarga a un fichero, o a la nada.
    pub fn download(port: u16, path: &str, token: &str, into: Option<&Path>) -> Self {
        let mut curl = Self::base(port, path, token);
        curl.args.push("--output".to_string());
        curl.args.push(match into {
            Some(file) => file.display().to_string(),
            None => "/dev/null".to_string(),
        });
        curl.root = into.and_then(|file| file.parent()).map(Path::to_path_buf);
        curl.body = into.map(Path::to_path_buf);
        curl
    }

    /// Un `curl` que solo pide la descripción.
    pub fn describe(port: u16, path: &str, token: &str) -> Self {
        let mut curl = Self::base(port, path, token);
        curl.args.push("--head".to_string());
        curl.args.push("--output".to_string());
        curl.args.push("/dev/null".to_string());
        curl
    }

    /// Un `curl` con una cabecera de más, que deja el cuerpo en un
    /// fichero.
    ///
    /// Es lo que permite comprobar los **bytes** de un tramo, y no solo su
    /// código y su `Content-Range`.
    pub fn with_header(
        port: u16,
        path: &str,
        token: &str,
        header: (&str, &str),
        into: &Path,
    ) -> Self {
        let mut curl = Self::base(port, path, token);
        curl.args.push("--header".to_string());
        curl.args.push(format!("{}: {}", header.0, header.1));
        curl.args.push("--output".to_string());
        curl.args.push(into.display().to_string());
        curl.root = into.parent().map(Path::to_path_buf);
        curl
    }

    /// Un `curl` con el metodo, el cuerpo y el tipo de contenido que
    /// hagan falta.
    ///
    /// El protocolo de push es el que mas metodos usa: `POST` abre la
    /// sesion, `PATCH` mete el trozo, `PUT` la cierra y `DELETE` la
    /// cancela. Cada uno con su propio cuerpo, y el manifiesto con su
    /// propio tipo de contenido.
    pub fn call(
        port: u16,
        method: &str,
        path: &str,
        token: &str,
        body: Option<&Path>,
        content_type: Option<&str>,
        into: Option<&Path>,
    ) -> Self {
        match content_type {
            Some(content_type) => Self::call_with_headers(
                port,
                method,
                path,
                token,
                body,
                &[("content-type", content_type)],
                into,
            ),
            None => Self::call_with_headers(port, method, path, token, body, &[], into),
        }
    }

    /// Un `curl` con el metodo, el cuerpo y las cabeceras de mas que
    /// hagan falta.
    ///
    /// El `Content-Range` del protocolo de push es una cabecera mas, y
    /// sin ella no se puede probar que un trozo que no encaja recibe el
    /// error que dice el protocolo. El tipo de contenido tambien es una
    /// cabecera, y por eso viaja en la misma lista.
    pub fn call_with_headers(
        port: u16,
        method: &str,
        path: &str,
        token: &str,
        body: Option<&Path>,
        extra: &[(&str, &str)],
        into: Option<&Path>,
    ) -> Self {
        let mut curl = Self::base(port, path, token);
        if method != "GET" {
            // `--request HEAD` hace que curl espere un cuerpo que HEAD
            // nunca trae: el modo descripcion es `--head`.
            if method == "HEAD" {
                curl.args.push("--head".to_string());
            } else {
                curl.args.push("--request".to_string());
                curl.args.push(method.to_string());
            }
        }
        if let Some(file) = body {
            curl.args.push("--data-binary".to_string());
            curl.args.push(format!("@{}", file.display()));
        }
        for (name, value) in extra {
            curl.args.push("--header".to_string());
            curl.args.push(format!("{name}: {value}"));
        }
        curl.args.push("--output".to_string());
        curl.args.push(match into {
            // Con `--head` curl escribe las cabeceras en el destino:
            // HEAD no trae cuerpo, y el arnes lo exige vacio.
            Some(_) if method == "HEAD" => "/dev/null".to_string(),
            Some(file) => file.display().to_string(),
            None => "/dev/null".to_string(),
        });
        curl.root = into
            .or(body)
            .and_then(|file| file.parent())
            .map(Path::to_path_buf);
        // HEAD no escribe cuerpo: si senalaramos el destino, la lectura
        // encontraria los restos del numero de fichero de otro run.
        curl.body = if method == "HEAD" {
            None
        } else {
            into.map(Path::to_path_buf)
        };
        curl
    }

    /// El proceso que se ejecuta.
    fn command(&self) -> Command {
        let mut command = Command::new("curl");
        command.args(&self.args);
        command
    }

    /// El comando tal y como se escribe en el informe.
    pub fn shown(&self) -> String {
        let root = self
            .root
            .as_ref()
            .map(|root| root.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut text = String::from("curl");
        for argument in &self.args {
            text.push(' ');
            let argument = if root.is_empty() {
                argument.clone()
            } else {
                argument.replace(&root, "<despliegue>")
            };
            if argument.contains(' ') {
                text.push('\'');
                text.push_str(&argument);
                text.push('\'');
            } else {
                text.push_str(&argument);
            }
        }
        if !self.token.is_empty() {
            text = text.replace(&self.token, &the_name_of(&self.token));
        }
        text
    }

    /// Ejecuta el `curl` y devuelve su respuesta.
    pub fn run(&self) -> Answer {
        let mut command = self.command();
        the_answer(&mut command, self.body.as_deref())
    }

    /// Lanza el `curl` en segundo plano.
    ///
    /// La subida se queda en curso mientras el arnés hace otra cosa: es
    /// lo que permite matar al servicio **a mitad de una subida**, que
    /// es la forma de mirar un corte de verdad.
    pub fn run_in_background(&self) -> Child {
        let mut command = self.command();
        command.stdin(Stdio::null());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        command.spawn().expect("the upload starts")
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
    Curl::upload(port, path, token, file, limit_rate).run()
}

/// Un `curl` que descarga a un fichero, o a la nada.
pub fn curl_download(port: u16, path: &str, token: &str, into: Option<&Path>) -> Answer {
    Curl::download(port, path, token, into).run()
}

/// Un `curl` que solo pide la descripción.
pub fn curl_describe(port: u16, path: &str, token: &str) -> Answer {
    Curl::describe(port, path, token).run()
}

/// Un `curl` con una cabecera de más, que deja el cuerpo en un fichero.
pub fn curl_get_with_header(
    port: u16,
    path: &str,
    token: &str,
    header: (&str, &str),
    into: &Path,
) -> Answer {
    Curl::with_header(port, path, token, header, into).run()
}

/// Un `curl` que publica un fichero, en segundo plano.
pub fn curl_upload_in_background(
    port: u16,
    path: &str,
    token: &str,
    file: &Path,
    limit_rate: Option<&str>,
) -> Child {
    Curl::upload(port, path, token, file, limit_rate).run_in_background()
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
        body: Vec::new(),
    }
}

/// Los recuentos que el arranque dice de los stagings.
///
/// El arranque limpia lo caducado y respeta lo vivo, y lo dice: el
/// número de los dos. Un recuento que solo existe en el código no es
/// evidencia de nada; el log del servicio sí lo es.
pub fn the_boot_cleanup(log: &str) -> (Option<u64>, Option<u64>) {
    let plain = without_the_colors(log);
    let mut removed = None;
    let mut alive = None;
    for line in plain.lines() {
        let Some((_, fields)) = line.split_once("at the boot") else {
            continue;
        };
        if removed.is_none() {
            removed = a_number_after(fields, "removed");
        }
        if alive.is_none() {
            alive = a_number_after(fields, "alive");
        }
    }
    (removed, alive)
}

/// El número que un campo dice en una línea del log.
///
/// El formato del log es del servicio, y cambia; lo que no cambia es
/// que el número va después del nombre del campo.
fn a_number_after(text: &str, field: &str) -> Option<u64> {
    let mut rest = text;
    while let Some(position) = rest.find(field) {
        let after = &rest[position + field.len()..];
        let digits: String = after
            .trim_start()
            .trim_start_matches(['=', ':'])
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if !digits.is_empty() {
            return digits.parse().ok();
        }
        rest = &rest[position + field.len()..];
    }
    None
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
fn the_answer(command: &mut Command, body: Option<&Path>) -> Answer {
    let stdout = the_output(command);
    let (headers, status, size) = the_answer_of(&stdout);
    Answer {
        status,
        size,
        headers,
        body: body
            .and_then(|path| std::fs::read(path).ok())
            .unwrap_or_default(),
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
///
/// Cada superficie tiene **su** informe, con su titulo y su aviso de lo
/// que no certifica: un informe que se titula "conformidad RAW" y
/// certify casos de OCI es un informe que no dice la verdad, y el que lo
/// lee no puede saber que mide.
pub struct Report {
    /// El titulo del informe.
    title: &'static str,
    /// El aviso de lo que este informe **no** certifica.
    disclaimer: &'static str,
    /// Donde se escribe el informe.
    path: PathBuf,
    /// Las herramientas que han hablado con el servicio.
    tools: String,
    cases: Vec<Case>,
}

/// Lo que el informe de la superficie cruda **no** certifica.
const RAW_DISCLAIMER: &str = "\
- **Ni compatibilidad OCI.** Nada de lo que se ha ejecutado habla el protocolo de OCI Distribution: no hay manifiestos, ni `HEAD` de tags, ni `GET` de blobs por digest con la semántica de OCI. La superficie es `raw`.\n\
- **Ni compatibilidad S3.** No hay `ListBuckets`, ni `PutObject` con firma AWS, ni nada que se parezca a la API de S3.\n\
- **Ni disponibilidad.** No se ha medido el tiempo de respuesta, ni el comportamiento bajo carga, ni que el servicio aguante lo que sea: lo que se ha medido es que los bytes vuelven intactos.\n\
- **Ni la recolección de blobs sin referencia.** Un blob sin referenciar se puede quedar en el almacén; lo que se ha comprobado es que no se publica solo.";

/// Lo que el informe del push OCI **no** certifica.
const OCI_PUSH_DISCLAIMER: &str = "\
- **Ni compatibilidad OCI completa.** Lo que se ha ejecutado es el corte de **push**: las sesiones de subida y el manifiesto. El indice multi-plataforma, los referrers y las firmas son de otras tareas, y no se pueden certificar con lo que aqui se ha medido.\n\
- **Ni `oras` ni el `docker` de la CLI.** Los clientes reales que han hablado con el registro son `skopeo` y `podman`: son los que estan instalados en este entorno.\n\
- **Ni la promoción del canal de confianza.** Lo que se ha comprobado es que el artefacto admitido queda en **cuarentena**; promoverlo es de otra tarea.\n\
- **Ni disponibilidad.** No se ha medido el tiempo de respuesta, ni el comportamiento bajo carga, ni que el servicio aguante lo que sea: lo que se ha medido es que los bytes vuelven intactos.\n\
- **Ni la recolección de blobs sin referencia.** Un blob sin referenciar se puede quedar en el almacén; lo que se ha comprobado es que no se publica solo.";

/// Lo que el informe del indice y los referrers **no** certifica.
const OCI_INDEX_DISCLAIMER: &str = "\
- **Ni la validacion criptografica de las firmas.** Un referrer de firma se registra como lo que es: un manifiesto que dice que cuelga de otro. El registro **no** comprueba firmas, y la presencia de una no concede nada: la decision la toma el motor de confianza.\n\
- **Ni la promocion de nada.** Lo que se ha comprobado es que un referrer **no** cambia el canal de confianza de aquello a lo que cuelga: sigue en cuarentena, y promoverla es de otra tarea.\n\
- **Ni compatibilidad OCI completa.** Lo que se ha ejecutado es el corte de **consulta y adjuntos**: la lista de etiquetas, los referrers, el indice multi-plataforma y el manifiesto de artefacto. Las sesiones de subida son de otra tarea.\n\
- **Ni `docker` de la CLI.** Los clientes reales que han hablado con el registro son `oras`.\n\
- **Ni la suite oficial de conformidad.** Este informe dice lo que se ha ejecutado aqui, no lo que dice la suite del `distribution-spec`.\n\
- **Ni disponibilidad.** No se ha medido el tiempo de respuesta, ni el comportamiento bajo carga, ni que el servicio aguante lo que sea.";

/// Lo que el informe de la compatibilidad con clientes reales **no**
/// certifica.
///
/// Este informe es el que mide **que clientes hablan el protocolo**, y su
/// valor esta en que sean los de verdad. Su limite esta en lo que los
/// clientes de verdad no hacen, y en lo que el registro no hace.
const OCI_E2E_DISCLAIMER: &str = "\
- **Ni la conformidad de las categorias que no se han ejecutado.** El informe nombra las categorias de la suite oficial que se han ejecutado, con su resultado. De las demas no se dice nada: una categoria que no se ha ejecutado no es una categoria conforme, y decir lo contrario seria propaganda.\n\
- **Ni validacion criptografica de las firmas.** Un referrer de firma se registra como lo que es: un manifiesto que dice que cuelga de otro. El registro **no** comprueba firmas, y la presencia de una no concede nada: la decision la toma el motor de confianza.\n\
- **Ni la promocion de nada.** Lo que se ha comprobado es que un referrer **no** cambia el canal de confianza de aquello a lo que cuelga: sigue en cuarentena, y promoverla es de otra tarea.\n\
- **Ni que el cliente de Docker y el motor sean la misma cosa.** El `docker` de la CLI es el cliente real y el motor que hay detras es el `podman` exponiendo la API de Docker: la CLI de Docker no habla con un registro, habla con un demonio.\n\
- **Ni el borrado de contenido.** El protocolo lo tiene (borrar un blob, un manifiesto, una etiqueta) y este registro no lo implementa.\n\
- **Ni disponibilidad.** No se ha medido el tiempo de respuesta, ni el comportamiento bajo carga, ni que el servicio aguante lo que sea.";

impl Report {
    /// Un informe vacío, el de la superficie cruda.
    pub fn new() -> Self {
        Self {
            title: "Conformidad RAW — informe",
            disclaimer: RAW_DISCLAIMER,
            path: Self::the_path(),
            tools: the_tools(),
            cases: Vec::new(),
        }
    }

    /// Un informe vacío, el del push OCI.
    ///
    /// Es un informe aparte, con su titulo y su aviso: el informe de la
    /// superficie cruda lo leen los gates de la conformacion RAW, y un
    /// caso de OCI en el no dice nada de lo que el gate cree.
    pub fn for_oci_push() -> Self {
        Self {
            title: "Conformidad OCI push — informe",
            disclaimer: OCI_PUSH_DISCLAIMER,
            path: Self::the_oci_push_path(),
            // Las versiones de los clientes van en el informe: un cliente
            // sin version es un cliente del que no se puede decir que
            // habla el protocolo.
            tools: format!(
                "{}; `{}`; `{}`",
                the_tools(),
                the_first_line_of(&["skopeo", "--version"]),
                the_first_line_of(&["podman", "--version"])
            ),
            cases: Vec::new(),
        }
    }

    /// Un informe vacio, el del indice y los referrers.
    ///
    /// Es un informe aparte, con su titulo y su aviso: uno que certify
    /// el push y el indice en el mismo titulo diria que el registro
    /// cumple el protocolo entero, y lo que se ha medido es una parte.
    pub fn for_oci_index() -> Self {
        Self {
            title: "Conformidad OCI indice, etiquetas y referrers — informe",
            disclaimer: OCI_INDEX_DISCLAIMER,
            path: Self::the_oci_index_path(),
            // Las versiones de los clientes van en el informe: un cliente
            // sin version es un cliente del que no se puede decir que
            // habla el protocolo.
            tools: format!(
                "{}; `oras {}`",
                the_tools(),
                the_version_of(&[&the_oras(), "version"])
            ),
            cases: Vec::new(),
        }
    }

    /// Un informe vacio, el de la compatibilidad con los clientes reales.
    ///
    /// Es un informe **aparte**, con su titulo y su aviso: el del indice
    /// certify las consultas y los adjuntos, y lo que se mide aqui es otra
    /// cosa--que clientes de verdad hablan el protocolo-- y un informe que
    /// lo mezclara diria que el registro lo cumple entero.
    pub fn for_oci_e2e() -> Self {
        Self {
            title: "Compatibilidad OCI con clientes reales — informe",
            disclaimer: OCI_E2E_DISCLAIMER,
            path: Self::the_oci_e2e_path(),
            // Las versiones de los clientes van en el informe: una
            // compatibilidad sin la version del cliente no es una
            // compatibilidad, es una intencion.
            tools: format!(
                "{}; `oras {}`; `podman {}`; `docker {}`",
                the_tools(),
                the_version_of(&[&the_oras(), "version"]),
                the_first_line_of(&[&the_podman(), "--version"]),
                the_first_line_of(&[&the_docker(), "--version"]),
            ),
            cases: Vec::new(),
        }
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

    /// La ruta del fichero del informe del push OCI.
    ///
    /// La del informe de la superficie cruda la decide la pipeline, y no
    /// se mezcla con la de otra superficie.
    pub fn the_oci_push_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("uat")
            .join("oci-push-conformity.md")
    }

    /// La ruta del fichero del informe del indice.
    pub fn the_oci_index_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("uat")
            .join("oci-index-conformity.md")
    }

    /// La ruta del fichero del informe de la compatibilidad con los
    /// clientes reales.
    pub fn the_oci_e2e_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("uat")
            .join("oci-compatibility.md")
    }

    /// Escribe el informe.
    ///
    /// La escritura es **atomica**: el informe se escribe en un fichero
    /// al lado y se renombra encima. Un informe a medias--por un corte o
    /// por un lector que llega mientras se escribe-- es un informe
    /// corrupto, y un informe corrupto que existe pasa el guard de la
    /// pipeline como si fuera bueno.
    pub fn write(&self) {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).expect("the directory of the report");
        }
        let temporary = self.path.with_extension("md.tmp");
        std::fs::write(&temporary, self.as_markdown()).expect("the report is written");
        std::fs::rename(&temporary, &self.path).expect("the report is put in its place");
    }

    /// El informe, en Markdown.
    pub fn as_markdown(&self) -> String {
        let mut text = String::new();
        text.push_str(&format!("# {}\n\n", self.title));
        text.push_str(&format!("**Servicio:** {}\n\n", the_version()));
        text.push_str(&format!("**Cuando:** {}\n\n", the_moment()));
        text.push_str(&format!("**Herramientas:** {}\n\n", self.tools));
        text.push_str(
            "\n**Credenciales:** van redactadas. Donde el `curl` llevaba el token, aqui \
             dice `<alice>` o `<bob>`: el informe dice con quien se hablo, no con que se \
             autentico. Un informe se archiva y se comparte, y un token en un fichero es \
             una credencial en un fichero.\n\n",
        );
        text.push_str("| Caso | Given | When | Comando | Resultado |\n");
        text.push_str("|---|---|---|---|---|\n");
        for case in &self.cases {
            text.push_str(&format!(
                "| {} | {} | {} | `{}` | {} {} |\n",
                the_cell(&case.id),
                the_cell(&case.given),
                the_cell(&case.when),
                the_cell(&case.command),
                if case.passed { "PASS" } else { "FALLO" },
                the_cell(&case.outcome)
            ));
        }
        text.push_str(&format!(
            "\n## Lo que este informe **no** certifica\n\n{}\n",
            self.disclaimer
        ));
        text
    }
}

/// El texto de una celda de la tabla del informe.
///
/// La tabla es de Markdown, asi que una barra vertical rompe la fila y
/// un salto de linea la convierte en dos: el texto de un caso--que puede
/// ser la salida de un cliente-- se escapa para que la tabla siga siendo
/// una tabla.
///
/// Y se **redacta**: el informe dice con quien se hablo, no con que se
/// autentico. Un texto que llega al informe sin pasar por aqui puede
/// llevar un token, y un informe se archiva y se comparte.
fn the_cell(text: &str) -> String {
    let mut cell = String::new();
    for line in text.lines() {
        if !cell.is_empty() {
            cell.push_str("<br>");
        }
        cell.push_str(&line.replace('|', "\\|"));
    }
    the_names()
        .lock()
        .expect("the names of the credentials")
        .iter()
        .filter(|(token, _)| !token.is_empty())
        .fold(cell, |cell, (token, name)| {
            cell.replace(token.as_str(), name)
        })
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

/// El informe del push OCI, compartido por los casos de su binario.
static OCI_PUSH_REPORT: OnceLock<Mutex<Report>> = OnceLock::new();

/// El informe del push OCI.
pub fn the_oci_push_report() -> &'static Mutex<Report> {
    OCI_PUSH_REPORT.get_or_init(|| Mutex::new(Report::for_oci_push()))
}

/// Anota un caso en el informe del push OCI.
pub fn record_oci_push(case: Case) {
    let mut report = the_oci_push_report()
        .lock()
        .expect("the report is not poisoned");
    report.record(case);
}

/// El informe del indice y los referrers, compartido por los casos de su
/// binario.
static OCI_INDEX_REPORT: OnceLock<Mutex<Report>> = OnceLock::new();

/// El informe del indice y los referrers.
pub fn the_oci_index_report() -> &'static Mutex<Report> {
    OCI_INDEX_REPORT.get_or_init(|| Mutex::new(Report::for_oci_index()))
}

/// Anota un caso en el informe del indice y los referrers.
pub fn record_oci_index(case: Case) {
    let mut report = the_oci_index_report()
        .lock()
        .expect("the report is not poisoned");
    report.record(case);
}

/// El informe de la compatibilidad con los clientes reales, compartido por
/// los casos de su binario.
static OCI_E2E_REPORT: OnceLock<Mutex<Report>> = OnceLock::new();

/// El informe de la compatibilidad con los clientes reales.
pub fn the_oci_e2e_report() -> &'static Mutex<Report> {
    OCI_E2E_REPORT.get_or_init(|| Mutex::new(Report::for_oci_e2e()))
}

/// Anota un caso en el informe de la compatibilidad con los clientes
/// reales.
pub fn record_oci_e2e(case: Case) {
    let mut report = the_oci_e2e_report()
        .lock()
        .expect("the report is not poisoned");
    report.record(case);
}

/// Donde esta el `oras`.
///
/// No siempre esta en el `PATH` de quien lanza las pruebas, y una
/// herramienta que no se encuentra es una herramienta que no se puede
/// probar: se dice donde se busca y se deja el sitio.
pub fn the_oras() -> String {
    if let Some(binary) = std::env::var_os("HODEI_ORAS_BIN") {
        return binary.to_string_lossy().into_owned();
    }
    THE_ORAS
        .get_or_init(|| {
            for candidate in [
                "oras".to_string(),
                format!(
                    "{}/.local/bin/oras",
                    std::env::var("HOME").unwrap_or_default()
                ),
            ] {
                if Command::new(&candidate)
                    .arg("version")
                    .output()
                    .is_ok_and(|output| output.status.success())
                {
                    return candidate;
                }
            }
            "oras".to_string()
        })
        .clone()
}

/// Donde esta el `oras`: se busca **una vez**.
///
/// La busqueda lanza un proceso por cada comando que usa la herramienta,
/// y una prueba que llama a `oras` veinte veces lanza veinte procesos
/// para preguntar lo mismo.
static THE_ORAS: OnceLock<String> = OnceLock::new();

/// Donde esta el `docker` de la CLI.
///
/// La CLI de Docker no viene con el sistema: se instala aparte, y el sitio
/// donde se ha puesto se dice, para que el informe pueda nombrar la version
/// con la que se ha hablado.
pub fn the_docker() -> String {
    if let Some(binary) = std::env::var_os("HODEI_DOCKER_BIN") {
        return binary.to_string_lossy().into_owned();
    }
    THE_DOCKER
        .get_or_init(|| {
            for candidate in [
                "docker".to_string(),
                format!(
                    "{}/.local/bin/docker",
                    std::env::var("HOME").unwrap_or_default()
                ),
            ] {
                if Command::new(&candidate)
                    .arg("--version")
                    .output()
                    .is_ok_and(|output| output.status.success())
                {
                    return candidate;
                }
            }
            "docker".to_string()
        })
        .clone()
}

/// Donde esta el `docker` de la CLI: se busca **una vez**.
static THE_DOCKER: OnceLock<String> = OnceLock::new();

/// Donde esta el `podman`.
///
/// El `podman` es el motor de imagen, y el que hace de demonio para el
/// `docker` de la CLI: la CLI de Docker no habla con un registro, habla con
/// un demonio.
pub fn the_podman() -> String {
    if let Some(binary) = std::env::var_os("HODEI_PODMAN_BIN") {
        return binary.to_string_lossy().into_owned();
    }
    THE_PODMAN
        .get_or_init(|| {
            for candidate in ["podman".to_string(), "/usr/bin/podman".to_string()] {
                if Command::new(&candidate)
                    .arg("--version")
                    .output()
                    .is_ok_and(|output| output.status.success())
                {
                    return candidate;
                }
            }
            "podman".to_string()
        })
        .clone()
}

/// Donde esta el `podman`: se busca **una vez**.
static THE_PODMAN: OnceLock<String> = OnceLock::new();

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

/// La version de una herramienta, sin el ruido que la envuelve.
///
/// El `oras` dice su version en una linea con la forma
/// `Version:        1.2.0`, y en el informe lo que interesa es el
/// numero.
fn the_version_of(command: &[&str]) -> String {
    let line = the_first_line_of(command);
    match line.split_once(':') {
        Some((_, version)) => version.trim().to_string(),
        None => line,
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
    if let Ok(repositories) = std::env::var("HODEI_SEED_MAVEN") {
        let repositories: Vec<serde_json::Value> =
            serde_json::from_str(&repositories).expect("the maven repositories parse");
        seed_the_maven(&db.to_string_lossy(), &repositories);
        return;
    }
    if let Ok(hf) = std::env::var("HODEI_SEED_HF") {
        let cas = std::env::var("HODEI_SEED_CAS").expect("the cas of the hf seeding");
        let repositories: Vec<serde_json::Value> =
            serde_json::from_str(&hf).expect("the hf repositories parse");
        seed_the_hf(&db.to_string_lossy(), &cas, &repositories);
        return;
    }
    if let Ok(groups) = std::env::var("HODEI_SEED_GROUPS") {
        let groups: Vec<serde_json::Value> =
            serde_json::from_str(&groups).expect("the groups parse");
        seed_the_groups(&db.to_string_lossy(), &groups);
        return;
    }
    if let Ok(key) = std::env::var("HODEI_RECONFIG_GROUP") {
        let members = std::env::var("HODEI_RECONFIG_MEMBERS").expect("the new members");
        let members: Vec<String> = serde_json::from_str(&members).expect("the members parse");
        reconfigure_the_group(&db.to_string_lossy(), &key, &members);
        return;
    }
    if let Ok(key) = std::env::var("HODEI_RECONFIG_KEY") {
        if let Ok(namespaces) = std::env::var("HODEI_RECONFIG_NAMESPACES") {
            let namespaces = serde_json::from_str(&namespaces).expect("the new namespaces parse");
            reconfigure_the_namespaces(&db.to_string_lossy(), &key, namespaces);
            return;
        }
        let proxy = std::env::var("HODEI_RECONFIG_PROXY").expect("the new proxy settings");
        let proxy = serde_json::from_str(&proxy).expect("the new proxy settings parse");
        reconfigure_the_proxy(&db.to_string_lossy(), &key, proxy);
        return;
    }
    let extra = std::env::var("HODEI_SEED_EXTRA").unwrap_or_default();
    let extra: Vec<&str> = extra.split(',').filter(|one| !one.is_empty()).collect();
    let trusted = std::env::var("HODEI_SEED_TRUSTED").unwrap_or_default();
    let trusted: Vec<&str> = trusted.split(',').filter(|one| !one.is_empty()).collect();
    let replace = std::env::var("HODEI_SEED_REPLACE").unwrap_or_default();
    let replace: Vec<&str> = replace.split(',').filter(|one| !one.is_empty()).collect();
    let proxies = std::env::var("HODEI_SEED_PROXY")
        .ok()
        .and_then(|raw| {
            serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&raw).ok()
        })
        .unwrap_or_default();
    seed_the_catalog_with(&db.to_string_lossy(), &extra, &trusted, &proxies, &replace);
}

/// Siembra el catálogo con un repositorio por cuenta.
pub fn seed_the_catalog(path: &str, extra: &[&str], trusted: &[&str]) {
    seed_the_catalog_with(path, extra, trusted, &serde_json::Map::new(), &[]);
}

/// Siembra el catálogo y, ademas, un repositorio proxy por cada upstream.
pub fn seed_the_catalog_with(
    path: &str,
    extra: &[&str],
    trusted: &[&str],
    proxies: &serde_json::Map<String, serde_json::Value>,
    replace: &[&str],
) {
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
        let seeded = [
            (FIRST_ACCOUNT, FIRST_REPOSITORY),
            (FIRST_ACCOUNT, FIRST_ONLY_REPOSITORY),
            (SECOND_ACCOUNT, SECOND_REPOSITORY),
        ]
        .into_iter()
        .chain(extra.iter().map(|key| (FIRST_ACCOUNT, *key)))
        .chain(proxies.keys().map(|key| (FIRST_ACCOUNT, key.as_str())));
        for (account, key) in seeded {
            let mut config =
                hodei_catalog::RepositoryConfig::new(hodei_catalog::DigestScope::Global);
            let mut settings = serde_json::Map::new();
            if account == FIRST_ACCOUNT && trusted.contains(&key) {
                settings.insert("trust".into(), serde_json::json!({ "serve": "trusted" }));
            }
            if account == FIRST_ACCOUNT && replace.contains(&key) {
                settings.insert("raw".into(), serde_json::json!({ "replace": "allow" }));
            }
            let mode = match proxies.get(key) {
                Some(upstream) if account == FIRST_ACCOUNT => {
                    // Un objeto con `upstream` ya es el `proxy` completo
                    // (con `cache`); si no, es solo el upstream.
                    let proxy = if upstream.get("upstream").is_some() {
                        upstream.clone()
                    } else {
                        serde_json::json!({ "upstream": upstream })
                    };
                    settings.insert("proxy".into(), proxy);
                    hodei_catalog::RepositoryMode::Proxy
                }
                _ => hodei_catalog::RepositoryMode::Hosted,
            };
            config.config_settings = serde_json::Value::Object(settings);
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
                        mode,
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

/// Cambia la configuracion proxy de un repositorio sembrado: la version de
/// su configuracion sube.
fn reconfigure_the_proxy(path: &str, key: &str, proxy: serde_json::Value) {
    use hodei_catalog::{CatalogReader, CatalogWriter};
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
        let current = store
            .repository(FIRST_ACCOUNT, key)
            .await
            .expect("the repository exists");
        let mut config = current.config;
        let mut settings = match config.config_settings {
            serde_json::Value::Object(settings) => settings,
            _ => serde_json::Map::new(),
        };
        settings.insert("proxy".into(), proxy);
        config.config_settings = serde_json::Value::Object(settings);
        store
            .update_config(FIRST_ACCOUNT, key, config)
            .await
            .expect("the configuration is updated");
        drop(store);
        drop(db);
    });
}

/// Cambia la politica de namespaces de un repositorio sembrado: la version
/// de su configuracion sube, y una resolucion previa incompatible deja de
/// valer.
fn reconfigure_the_namespaces(path: &str, key: &str, namespaces: serde_json::Value) {
    use hodei_catalog::{CatalogReader, CatalogWriter};
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
        let current = store
            .repository(FIRST_ACCOUNT, key)
            .await
            .expect("the repository exists");
        let mut config = current.config;
        let mut settings = match config.config_settings {
            serde_json::Value::Object(settings) => settings,
            _ => serde_json::Map::new(),
        };
        // `null` quita la politica: reservar deja de estar reservado.
        if namespaces.is_null() {
            settings.remove("namespaces");
        } else {
            settings.insert("namespaces".into(), namespaces);
        }
        config.config_settings = serde_json::Value::Object(settings);
        store
            .update_config(FIRST_ACCOUNT, key, config)
            .await
            .expect("the configuration is updated");
        drop(store);
        drop(db);
    });
}

/// Crea grupos de la primera cuenta, en orden.
fn seed_the_groups(path: &str, groups: &[serde_json::Value]) {
    use hodei_catalog::CatalogWriter;
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
        for group in groups {
            let key = group["key"].as_str().expect("the key");
            let mut config =
                hodei_catalog::RepositoryConfig::new(hodei_catalog::DigestScope::Global);
            let mut settings = serde_json::json!({ "group": { "members": group["members"] } });
            // La reserva de namespaces de un grupo, si la suite la declara.
            if !group["namespaces"].is_null() {
                settings["namespaces"] = group["namespaces"].clone();
            }
            config.config_settings = settings;
            store
                .create_repository(
                    FIRST_ACCOUNT,
                    hodei_catalog::RepositoryDescriptor {
                        hrn: hodei_catalog::repository_hrn(
                            "hodei",
                            "artifacts",
                            "eu",
                            FIRST_ACCOUNT,
                            key,
                        ),
                        account: FIRST_ACCOUNT.to_string(),
                        repo_key: key.to_string(),
                        format: if group["format"].as_str() == Some("npm") {
                            hodei_catalog::RepositoryFormat::Npm
                        } else if group["format"].as_str() == Some("maven2") {
                            hodei_catalog::RepositoryFormat::Maven2
                        } else if group["format"].as_str() == Some("pypi") {
                            hodei_catalog::RepositoryFormat::PyPi
                        } else if group["format"].as_str() == Some("go") {
                            hodei_catalog::RepositoryFormat::Go
                        } else if group["format"].as_str() == Some("nuget") {
                            hodei_catalog::RepositoryFormat::NuGet
                        } else if group["format"].as_str() == Some("oci") {
                            hodei_catalog::RepositoryFormat::Oci
                        } else {
                            hodei_catalog::RepositoryFormat::Raw
                        },
                        mode: hodei_catalog::RepositoryMode::Group,
                        config,
                        created_at: chrono::Utc::now(),
                        updated_at: chrono::Utc::now(),
                    },
                )
                .await
                .expect("the group is created");
        }
        drop(store);
        drop(db);
    });
}

/// Crea repositorios Maven2 hosted de la primera cuenta.
fn seed_the_maven(path: &str, repositories: &[serde_json::Value]) {
    use hodei_catalog::CatalogWriter;
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
        for repository in repositories {
            let key = repository["key"].as_str().expect("the key");
            // El ambito del contenido: por defecto el global, que es el
            // unico que sirve esta release. Una suite que mide la politica
            // de ambito siembra un miembro **particionado**, y lo declara
            // aqui: no hay otra puerta para ello.
            let scope = if repository["scope"].as_str() == Some("partitioned") {
                hodei_catalog::DigestScope::AccountPartitioned
            } else {
                hodei_catalog::DigestScope::Global
            };
            let mut config = hodei_catalog::RepositoryConfig::new(scope);
            let mut settings = serde_json::Map::new();
            if !repository["maven"].is_null() {
                settings.insert("maven".into(), repository["maven"].clone());
            }
            if !repository["terraform"].is_null() {
                settings.insert("terraform".into(), repository["terraform"].clone());
            }
            if !repository["npm"].is_null() {
                settings.insert("npm".into(), repository["npm"].clone());
            }
            if !repository["pypi"].is_null() {
                settings.insert("pypi".into(), repository["pypi"].clone());
            }
            if repository["trusted"].as_bool() == Some(true) {
                // El canal de confianza es un ajuste del repositorio.
                settings.insert("trust".into(), serde_json::json!({ "serve": "trusted" }));
            }
            // La reserva de namespaces es un ajuste del repositorio: la
            // suite que la declara la escribe al sembrar, como el resto de
            // ajustes que no tienen ruta HTTP.
            if !repository["namespaces"].is_null() {
                settings.insert("namespaces".into(), repository["namespaces"].clone());
            }
            let format = if repository["format"].as_str() == Some("raw") {
                hodei_catalog::RepositoryFormat::Raw
            } else if repository["format"].as_str() == Some("npm") {
                hodei_catalog::RepositoryFormat::Npm
            } else if repository["format"].as_str() == Some("pypi") {
                hodei_catalog::RepositoryFormat::PyPi
            } else if repository["format"].as_str() == Some("cargo") {
                hodei_catalog::RepositoryFormat::Cargo
            } else if repository["format"].as_str() == Some("go") {
                hodei_catalog::RepositoryFormat::Go
            } else if repository["format"].as_str() == Some("nuget") {
                hodei_catalog::RepositoryFormat::NuGet
            } else if repository["format"].as_str() == Some("oci") {
                hodei_catalog::RepositoryFormat::Oci
            } else if repository["format"].as_str() == Some("helm") {
                hodei_catalog::RepositoryFormat::Helm
            } else if repository["format"].as_str() == Some("apt") {
                hodei_catalog::RepositoryFormat::Apt
            } else if repository["format"].as_str() == Some("rpm") {
                hodei_catalog::RepositoryFormat::Rpm
            } else if repository["format"].as_str() == Some("conan") {
                hodei_catalog::RepositoryFormat::Conan
            } else if repository["format"].as_str() == Some("galaxy") {
                hodei_catalog::RepositoryFormat::Galaxy
            } else if repository["format"].as_str() == Some("terraform") {
                hodei_catalog::RepositoryFormat::Terraform
            } else {
                hodei_catalog::RepositoryFormat::Maven2
            };
            let mode = if repository["proxy"].is_null() {
                hodei_catalog::RepositoryMode::Hosted
            } else {
                settings.insert("proxy".into(), repository["proxy"].clone());
                hodei_catalog::RepositoryMode::Proxy
            };
            config.config_settings = serde_json::Value::Object(settings);
            store
                .create_repository(
                    FIRST_ACCOUNT,
                    hodei_catalog::RepositoryDescriptor {
                        hrn: hodei_catalog::repository_hrn(
                            "hodei",
                            "artifacts",
                            "eu",
                            FIRST_ACCOUNT,
                            key,
                        ),
                        account: FIRST_ACCOUNT.to_string(),
                        repo_key: key.to_string(),
                        format,
                        mode,
                        config,
                        created_at: chrono::Utc::now(),
                        updated_at: chrono::Utc::now(),
                    },
                )
                .await
                .expect("the maven repository is created");
        }
        drop(store);
        drop(db);
    });
}

/// Siembra los repositorios **hf hosted** y sus modelos: el contenido de
/// cada asset entra en el CAS y en el catalogo por la puerta del propio
/// catalogo (el `FileBlobStore` y el writer), la misma combinacion que
/// usa la superficie cuando un cliente publica. El repo_id del paquete es
/// el que la ruta del asset lleva: `artifacts/{owner}/{model}/{file}`.
fn seed_the_hf(path: &str, cas: &str, repositories: &[serde_json::Value]) {
    use hodei_catalog::{CatalogWriter, PackageCoordinates, Publication};
    use hodei_hub::{BlobStore as _, FileBlobStore};
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
        let blobs = FileBlobStore::new(cas).expect("the blob store");
        for repository in repositories {
            let key = repository["key"].as_str().expect("the key");
            let mut config =
                hodei_catalog::RepositoryConfig::new(hodei_catalog::DigestScope::Global);
            config.config_settings = serde_json::Value::Object(serde_json::Map::new());
            store
                .create_repository(
                    FIRST_ACCOUNT,
                    hodei_catalog::RepositoryDescriptor {
                        hrn: hodei_catalog::repository_hrn(
                            "hodei",
                            "artifacts",
                            "eu",
                            FIRST_ACCOUNT,
                            key,
                        ),
                        account: FIRST_ACCOUNT.to_string(),
                        repo_key: key.to_string(),
                        format: hodei_catalog::RepositoryFormat::Hf,
                        mode: hodei_catalog::RepositoryMode::Hosted,
                        config,
                        created_at: chrono::Utc::now(),
                        updated_at: chrono::Utc::now(),
                    },
                )
                .await
                .expect("the hf repository is created");
            for asset in repository["assets"].as_array().expect("the assets") {
                let asset_path = asset["asset"].as_str().expect("the asset path");
                let file = asset["file"].as_str().expect("the source file");
                let bytes = std::fs::read(file).expect("the model file");
                let digest = blobs.put(&bytes).await.expect("the blob is stored");
                // El repo_id es el que la ruta del asset lleva: entre
                // `artifacts/` y la ultima barra. Un fichero del modelo es
                // su propia version del paquete, como los ficheros de un
                // recipe conan.
                let rest = asset_path
                    .strip_prefix("artifacts/")
                    .expect("the asset of a model");
                let (repo_id, filename) = rest.rsplit_once('/').expect("the file of a model");
                let publication = Publication {
                    repository: key.to_string(),
                    asset_path: asset_path.to_string(),
                    coordinates: PackageCoordinates {
                        package_name: repo_id.to_string(),
                        package_version: filename.to_string(),
                    },
                    digest: digest.clone(),
                    size_bytes: bytes.len() as u64,
                    tags: Vec::new(),
                    actor: "seeder".to_string(),
                    correlation_id: "seed-hf".to_string(),
                    overwrite: false,
                    precondition: None,
                    subject: None,
                };
                store
                    .publish(FIRST_ACCOUNT, publication)
                    .await
                    .expect("the model file is published");
            }
        }
        drop(store);
        drop(db);
    });
}

/// Cambia los miembros de un grupo sembrado: la version de su
/// configuracion sube.
fn reconfigure_the_group(path: &str, key: &str, members: &[String]) {
    use hodei_catalog::{CatalogReader, CatalogWriter};
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
        let current = store
            .repository(FIRST_ACCOUNT, key)
            .await
            .expect("the group exists");
        let mut config = current.config;
        config.config_settings = serde_json::json!({ "group": { "members": members } });
        store
            .update_config(FIRST_ACCOUNT, key, config)
            .await
            .expect("the configuration is updated");
        drop(store);
        drop(db);
    });
}
