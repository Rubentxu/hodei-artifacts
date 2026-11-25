//! Conan 2 hosted de HSP-159 contra el cliente real (`conan`).
//!
//! El `conan` del venv publica un recipe con dos perfiles contra el
//! servicio de verdad, lo sube con `conan upload`, lo lista desde el
//! remote y lo resuelve con `conan install` en caches limpias: las
//! revisiones (rrev/prev), el `package_id` y los checksums los hace el
//! propio cliente, no este test. Hace falta `conan` 2.x: en el `CONAN_BIN`
//! (el venv de la UAT) o en el `PATH`; sin él, la suite **falla**
//! diciendo que falta. Cada ejecución usa su propio `CONAN_HOME`: nada se
//! lee ni se escribe fuera del directorio del despliegue.
//!
//! El remote de referencia del spike fue `conan_server` 1.66 detrás de un
//! proxy de registro (`target/uat/hsp159/spike-ref/`); el dialecto
//! observado ahí es el que este test demanda del servicio.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::Digest as _;
use support::{Deployment, FIRST_ACCOUNT, SECOND_ACCOUNT};

/// Exige el `conan` 2.x del venv o del PATH, y devuelve su ruta.
fn the_conan_binary() -> String {
    if let Ok(path) = std::env::var("CONAN_BIN") {
        return path;
    }
    "conan".to_string()
}

fn require_conan() -> String {
    let binary = the_conan_binary();
    let found = Command::new(&binary)
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "the conan CLI is not installed: the Conan suite needs `conan` in CONAN_BIN (the UAT venv) or in the PATH"
    );
    let version = String::from_utf8_lossy(
        &Command::new(&binary)
            .arg("--version")
            .output()
            .expect("the version")
            .stdout,
    )
    .trim()
    .to_string();
    assert!(
        version.starts_with("Conan version 2."),
        "the Conan suite pins the client major version 2.x, found: {version}"
    );
    binary
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

/// Un `conan` aislado: su `CONAN_HOME`, su remote y su credencial.
struct Client {
    /// La ruta del ejecutable.
    binary: String,
    /// El directorio del despliegue.
    root: PathBuf,
    /// El `CONAN_HOME` de esta cache.
    home: PathBuf,
}

impl Client {
    /// Una cache limpia con el remote de Hodei y su credencial.
    fn new(root: &Path, name: &str, port: u16, repository: &str, token: &str) -> Self {
        let home = root.join(format!("conan-home-{name}"));
        std::fs::create_dir_all(&home).expect("the conan home");
        let client = Self {
            binary: the_conan_binary(),
            root: root.to_path_buf(),
            home,
        };
        client.run(&["profile", "detect", "-f"]);
        let url = format!("http://127.0.0.1:{port}/conan/{repository}");
        client.run(&["remote", "add", "hodei", &url]);
        // La credencial del remote es la credencial bearer del hub: el
        // cliente la cambia por el token en el authenticate del dialecto.
        client
            .run(&["remote", "login", "hodei", "alice", "-p", token])
            .expect("the login");
        client
    }

    /// Ejecuta el `conan` con su home aislado.
    fn run(&self, args: &[&str]) -> Run {
        let output = Command::new(&self.binary)
            .args(args)
            .env("CONAN_HOME", &self.home)
            .env("HOME", &self.root)
            .env("CONAN_NON_INTERACTIVE", "1")
            // `conan create` writes its generator scripts (`conanbuild.sh`,
            // `conanrun.sh`, ...) into the working directory: the deployment
            // directory, never the checkout the suite runs from.
            .current_dir(&self.root)
            .output()
            .expect("conan runs");
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Run {
            ok: output.status.success(),
            output: text,
        }
    }

    /// Los ficheros de package en la cache, con su flavor para identificar
    /// el `package_id` al que sirven.
    ///
    /// La cache guarda el `conan_package.tgz` en `{package}/d/` y los
    /// ficheros servidos en `{package}/p/`, con `{package}` bajo `p/b/`
    /// cuando el binario lo creó este cliente y bajo `p/` cuando lo bajó.
    /// La relación entre los dos es la de directorios hermanos, y no
    /// depende del nombre: un cliente no impone el suyo al otro.
    fn package_files(&self) -> Vec<(String, String)> {
        let mut found = Vec::new();
        let packages = self.home.join("p");
        the_walk_for_packages(&packages, &mut found);
        found.sort();
        found
    }
}

/// La búsqueda de los packages de una cache conan: cada directorio que
/// tiene `d/conan_package.tgz` y `p/flavor.txt` es un binario con su
/// contenido.
fn the_walk_for_packages(directory: &Path, found: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries {
        let entry = entry.expect("an entry");
        let path = entry.path();
        if path.is_dir() {
            the_walk_for_packages(&path, found);
        } else if path.file_name().and_then(|name| name.to_str()) == Some("conan_package.tgz") {
            let parent = path
                .parent()
                .expect("the d directory")
                .parent()
                .expect("the package");
            let flavor =
                std::fs::read_to_string(parent.join("p").join("flavor.txt")).unwrap_or_default();
            found.push((flavor, path.display().to_string()));
        }
    }
}

/// El sha256 de un fichero, como lo verifican las dos caches.
fn sha256_of(path: &str) -> String {
    let bytes = std::fs::read(path).expect("the file");
    format!("{:x}", sha2::Sha256::digest(&bytes))
}

/// Los identificadores versionados que imprime `conan list`: los
/// token hexadecimales del listado (rrev y prev).
fn the_revisions_of_output(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for line in text.lines() {
        for token in line.split_whitespace() {
            let token = token.trim_end_matches('#');
            let token = token
                .split('(')
                .next()
                .expect("a token has no parenthesis")
                .trim();
            let hex = token.chars().all(|c| c.is_ascii_hexdigit());
            if hex && token.len() >= 16 && token.len() <= 64 {
                found.push(token.to_string());
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// El recipe con un `package_id` por flavor: el mismo recipe (misma rrev)
/// produce dos binarios que no se mezclan.
fn the_recipe(root: &Path) -> PathBuf {
    let directory = root.join("recipes/hello");
    std::fs::create_dir_all(&directory).expect("the recipe directory");
    let recipe = directory.join("conanfile.py");
    std::fs::write(
        &recipe,
        r#"
from conan import ConanFile
import os, shutil

class HelloConan(ConanFile):
    name = "hello"
    version = "1.0.0"
    settings = "os"
    options = {"flavor": [1, 2]}
    default_options = {"flavor": 1}

    def package(self):
        shutil.copy(os.path.join(self.recipe_folder, "conanfile.py"),
                    os.path.join(self.package_folder, "conanfile.py"))
        with open(os.path.join(self.package_folder, "flavor.txt"), "w") as f:
            f.write("flavor=%s" % self.options.flavor)
"#,
    )
    .expect("the recipe");
    directory
}

fn the_profile(root: &Path, name: &str, flavor: &str) -> PathBuf {
    let directory = root.join("profiles");
    std::fs::create_dir_all(&directory).expect("the profiles directory");
    let profile = directory.join(name);
    std::fs::write(
        &profile,
        format!("[settings]\nos=Linux\narch=x86_64\n\n[options]\nhello*:flavor={flavor}\n"),
    )
    .expect("the profile");
    profile
}

/// El seeder del catalogo: cada suite lo trae porque el arnés reinicia
/// el binario de la suite para sembrar antes de arrancar el servicio.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

#[test]
fn the_conan_client_publishes_and_resolves_by_package_id() {
    require_conan();
    let deployment = Deployment::new();
    deployment.seed_conan(&[("conan-hosted", serde_json::Value::Null)]);
    let mut service = deployment.start(&[]);
    let port = service.port;
    let root = deployment.directory.path().to_path_buf();

    let token = support::token(FIRST_ACCOUNT, "alice");
    let author = Client::new(&root, "author", port, "conan-hosted", &token);
    let recipe = the_recipe(&root);
    let flavor_one = the_profile(&root, "flavor1", "1");
    let flavor_two = the_profile(&root, "flavor2", "2");

    // AC1: crear con DOS perfiles. El mismo recipe produce dos package_id
    // con sus prev; la revision del recipe no cambia.
    author
        .run(&[
            "create",
            recipe.to_str().unwrap(),
            "-pr:a",
            flavor_one.to_str().unwrap(),
            "--user=hodei",
            "--channel=stable",
        ])
        .expect("the create of flavor 1");
    author
        .run(&[
            "create",
            recipe.to_str().unwrap(),
            "-pr:a",
            flavor_two.to_str().unwrap(),
            "--user=hodei",
            "--channel=stable",
        ])
        .expect("the create of flavor 2");

    // AC2: la subida real contra el remote. Al subirla, el cliente
    // empaqueta cada binario en la cache del autor: ahí quedan los
    // digests ANTES de que nadie los baje, que es lo que la otra cache
    // tiene que recibir byte a byte.
    author
        .run(&[
            "upload",
            "hello/1.0.0@hodei/stable",
            "-r",
            "hodei",
            "--confirm",
        ])
        .expect("the upload");
    let packages = author.package_files();
    assert_eq!(
        packages.len(),
        2,
        "two profiles produce two packages, found: {:?}",
        packages
    );
    let flavors: Vec<&str> = packages.iter().map(|(flavor, _)| flavor.as_str()).collect();
    assert!(
        flavors.contains(&"flavor=1") && flavors.contains(&"flavor=2"),
        "the two packages carry their own flavor: {:?}",
        flavors
    );
    let uploaded_digests: Vec<(String, String)> = packages
        .iter()
        .map(|(flavor, path)| (flavor.clone(), sha256_of(path)))
        .collect();

    // AC2: el listing del remote es consistente y las revisiones se
    // conservan: el remote dice las mismas revisiones que la cache local.
    // El cliente solo lista revisiones cuando el patron las pide (`#*`).
    let local = author
        .run(&["list", "hello/1.0.0@hodei/stable#*"])
        .expect("the local list");
    let remote = author
        .run(&["list", "hello/1.0.0@hodei/stable#*", "-r", "hodei"])
        .expect("the remote list");
    assert!(
        remote.contains("hello/1.0.0@hodei/stable"),
        "the remote lists the recipe: {}",
        remote
    );
    let remote_revisions = the_revisions_of_output(&remote);
    let local_revisions = the_revisions_of_output(&local);
    assert!(
        !remote_revisions.is_empty(),
        "the remote lists revisions (rrev and prev): {}",
        remote
    );
    assert_eq!(
        remote_revisions, local_revisions,
        "the remote says the same revisions the local cache says:\nremote:\n{}\nlocal:\n{}",
        remote, local
    );

    // AC2/AC4: el digest es verificable y el scoping es por cuenta. Otro
    // principal no ve lo que no le toca: su listing del mismo remote no
    // encuentra nada, porque el repositorio vive en otra cuenta.
    let other = support::token(SECOND_ACCOUNT, "bob");
    let stranger = Client::new(&root, "stranger", port, "conan-hosted", &other);
    let stranger_list = stranger.run(&["list", "hello/*", "-r", "hodei"]);
    assert!(
        stranger_list.ok,
        "the listing of another principal answers, not an error: {}",
        stranger_list.output
    );
    assert!(
        !stranger_list.output.contains("hello/1.0.0"),
        "another principal does not see the recipe of the first account: {}",
        stranger_list.output
    );

    // AC1/AC3: resolver en una cache limpia, los DOS perfiles. Cada
    // package_id recibe SUS bytes: el flavor que baja es el que el perfil
    // pide, y el digest coincide con el que se subió.
    let consumer = Client::new(&root, "consumer", port, "conan-hosted", &token);
    consumer
        .run(&[
            "install",
            "--requires=hello/1.0.0@hodei/stable",
            "-pr:a",
            flavor_one.to_str().unwrap(),
            "-r",
            "hodei",
        ])
        .expect("the install of flavor 1");
    consumer
        .run(&[
            "install",
            "--requires=hello/1.0.0@hodei/stable",
            "-pr:a",
            flavor_two.to_str().unwrap(),
            "-r",
            "hodei",
        ])
        .expect("the install of flavor 2");
    let downloaded = consumer.package_files();
    assert_eq!(downloaded.len(), 2, "both packages land in the cache");
    for (flavor, path) in &downloaded {
        let digest = sha256_of(path);
        let expected = uploaded_digests
            .iter()
            .find(|(uploaded, _)| uploaded == flavor)
            .map(|(_, digest)| digest)
            .unwrap_or_else(|| panic!("the flavor {flavor} was uploaded"));
        assert_eq!(
            &digest, expected,
            "the bytes served for {flavor} are the bytes uploaded"
        );
    }

    // AC3: offline. El remote muere: la cache caliente resuelve sin red y
    // el binario que falta falla, sin caer a nada.
    support::stop(&mut service);
    let warm = consumer.run(&[
        "install",
        "--requires=hello/1.0.0@hodei/stable",
        "-pr:a",
        flavor_one.to_str().unwrap(),
        "-r",
        "hodei",
    ]);
    assert!(
        warm.ok,
        "the offline install of a warm cache works with the remote dead: {}",
        warm.output
    );
    // Falta un asset: la cache no tiene el binario de flavor 2 tras
    // quitarlo, y el remote muerto no da alternativa: el install falla.
    consumer
        .run(&["remove", "hello/1.0.0@hodei/stable:*", "-c"])
        .expect("the removal of the cached binaries");
    let missing = consumer.run(&[
        "install",
        "--requires=hello/1.0.0@hodei/stable",
        "-pr:a",
        flavor_two.to_str().unwrap(),
        "-r",
        "hodei",
    ]);
    assert!(
        !missing.ok,
        "the offline install without the binary fails instead of falling to anything: {}",
        missing.output
    );
    assert!(
        missing.output.contains("missing binary") || missing.output.contains("ERROR"),
        "the failure says why: {}",
        missing.output
    );
}
