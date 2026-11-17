//! Cargo hosted de HSP-119 contra el cliente real (`cargo`).
//!
//! El `cargo` publica un crate contra un servicio de verdad, lo anade a
//! un proyecto consumidor con `cargo add` y lo compila con `cargo build`:
//! el indice sparse, la descarga y la comprobacion del `cksum` las hace
//! el propio cliente, no este test. Hace falta `cargo` en el PATH: sin
//! el, la suite **falla** diciendo que falta. La credencial va en el
//! entorno (`CARGO_REGISTRIES_HODEI_TOKEN`), nunca en argv, y cada
//! ejecucion usa su propio `CARGO_HOME`: nada se lee ni se escribe fuera
//! del directorio del despliegue.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use support::{Deployment, FIRST_ACCOUNT};

/// El seeder del catalogo.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// Exige el `cargo` de linea de ordenes.
fn require_cargo() {
    let found = Command::new("cargo")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "the cargo CLI is not installed: the Cargo suite needs `cargo` in the PATH"
    );
}

/// El resultado de una ejecucion del cliente.
struct Run {
    /// Si el cliente termino bien.
    ok: bool,
    /// Lo que dijo, por stdout y por stderr.
    output: String,
}

/// Un `cargo` aislado: su `CARGO_HOME`, su registro y su credencial.
struct Client {
    /// El directorio del despliegue, donde vive el `CARGO_HOME`.
    root: PathBuf,
    /// El `CARGO_HOME` del cliente, con la config del registro.
    cargo_home: PathBuf,
    /// La credencial.
    token: String,
}

impl Client {
    /// Un cliente cuyo registro `hodei` apunta al indice sparse del servicio.
    ///
    /// Cada cliente tiene su `CARGO_HOME`: dos clientes con credenciales
    /// distintas no pueden pisarse, y nada toca el registro del usuario real.
    fn new(root: &Path, name: &str, port: u16, repository: &str, token: &str) -> Self {
        let cargo_home = root.join(format!("cargo-home-{name}"));
        std::fs::create_dir_all(&cargo_home).expect("the cargo home");
        std::fs::write(
            cargo_home.join("config.toml"),
            format!(
                "[registries.hodei]\nindex = \"sparse+http://127.0.0.1:{port}/cargo/{repository}/\"\n\n[registry]\nglobal-credential-providers = [\"cargo:token\"]\n"
            ),
        )
        .expect("the registry config is written");
        // La credencial vive donde la pone `cargo login`: el indice sparse
        // tambien se pide con ella, y el proveedor `cargo:token` es lo que
        // la lee (los registros alternativos autenticados no la buscan sin
        // proveedor configurado).
        std::fs::write(
            cargo_home.join("credentials.toml"),
            format!("[registry.hodei]\ntoken = \"{token}\"\n"),
        )
        .expect("the credentials are written");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(
                cargo_home.join("credentials.toml"),
                std::fs::Permissions::from_mode(0o600),
            )
            .expect("the credentials are private");
        }
        Self {
            root: root.to_path_buf(),
            cargo_home,
            token: token.to_string(),
        }
    }

    /// Ejecuta el `cargo` con su home y su credencial en el entorno.
    fn run(&self, directory: &Path, args: &[&str]) -> Run {
        let output = Command::new("cargo")
            .args(args)
            .current_dir(directory)
            .env("CARGO_HOME", &self.cargo_home)
            .env("HOME", &self.root)
            // El proveedor `cargo:token` acepta la credencial por entorno:
            // nada de tokens en argv.
            .env("CARGO_REGISTRIES_HODEI_TOKEN", &self.token)
            .env("CARGO_NET_OFFLINE", "false")
            .output()
            .expect("cargo runs");
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Run {
            ok: output.status.success(),
            output: text,
        }
    }
}

/// El proyecto de un crate publicable, escrito en disco.
fn a_crate(root: &Path, name: &str, body: &str) -> PathBuf {
    let directory = root.join(name);
    std::fs::create_dir_all(directory.join("src")).expect("the crate skeleton");
    std::fs::write(
        directory.join("Cargo.toml"),
        format!(
            "[package]\nname = \"{name}\"\nversion = \"1.0.0\"\nedition = \"2021\"\ndescription = \"a crate published through the real cargo client\"\nlicense = \"MIT\"\n"
        ),
    )
    .expect("the manifest is written");
    std::fs::write(directory.join("src/lib.rs"), body).expect("the library is written");
    directory
}

/// El proyecto consumidor, con un main que usa la dependencia.
fn a_consumer(root: &Path) -> PathBuf {
    let directory = root.join("consumer");
    std::fs::create_dir_all(directory.join("src")).expect("the consumer skeleton");
    std::fs::write(
        directory.join("Cargo.toml"),
        "[package]\nname = \"consumer\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\n",
    )
    .expect("the manifest is written");
    std::fs::write(
        directory.join("src/main.rs"),
        "fn main() {\n    println!(\"{}\", hodei_cli_pkg::the_answer());\n}\n",
    )
    .expect("the main is written");
    directory
}

#[test]
fn cargo_publishes_adds_and_builds_against_the_registry() {
    require_cargo();
    let deployment = Deployment::new();
    deployment.seed_cargo(&[("cargo-hosted", serde_json::Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let root = deployment.directory.path();
    let client = Client::new(root, "client", service.port, "cargo-hosted", &token);

    // El `cargo` publica: empaqueta, sube y el servicio acepta la version.
    let producer = a_crate(
        root,
        "hodei-cli-pkg",
        "pub fn the_answer() -> i32 {\n    42\n}\n",
    );
    let published = client.run(&producer, &["publish", "--registry", "hodei"]);
    assert!(published.ok, "cargo publish failed:\n{}", published.output);

    // El `cargo add` lo encuentra por su nombre en el indice sparse.
    let consumer = a_consumer(root);
    let added = client.run(&consumer, &["add", "hodei-cli-pkg", "--registry", "hodei"]);
    assert!(added.ok, "cargo add failed:\n{}", added.output);
    let manifest = std::fs::read_to_string(consumer.join("Cargo.toml")).expect("the manifest");
    assert!(
        manifest.contains("hodei-cli-pkg"),
        "cargo add did not write the dependency:\n{manifest}"
    );

    // El `cargo build` descarga el .crate y el cliente **verifica el
    // cksum contra el indice**: si el digest no cuadrase, la descarga
    // se rechaza antes de construir. La marca en el output es la descarga real.
    let built = client.run(&consumer, &["build"]);
    assert!(built.ok, "cargo build failed:\n{}", built.output);
    assert!(
        built.output.contains("hodei-cli-pkg"),
        "the consumer did not fetch the published crate:\n{}",
        built.output
    );

    // Y el codigo que compila es el publicado: `cargo run` ejecuta el 42.
    let ran = client.run(&consumer, &["run", "--quiet"]);
    assert!(ran.ok, "cargo run failed:\n{}", ran.output);
    assert!(
        ran.output.contains("42"),
        "the built program is not the published crate:\n{}",
        ran.output
    );

    // Una segunda version: el indice la anade sin borrar la primera.
    std::fs::write(
        producer.join("Cargo.toml"),
        "[package]\nname = \"hodei-cli-pkg\"\nversion = \"1.1.0\"\nedition = \"2021\"\ndescription = \"a crate published through the real cargo client\"\nlicense = \"MIT\"\n",
    )
    .expect("the second manifest is written");
    let republished = client.run(&producer, &["publish", "--registry", "hodei"]);
    assert!(
        republished.ok,
        "the second publish failed:\n{}",
        republished.output
    );
    let latest = client.run(&consumer, &["update", "-p", "hodei-cli-pkg"]);
    assert!(latest.ok, "cargo update failed:\n{}", latest.output);
    let lock = std::fs::read_to_string(consumer.join("Cargo.lock")).expect("the lock");
    assert!(
        lock.contains("version = \"1.1.0\""),
        "the index does not offer the new version:\n{lock}"
    );

    // Una credencial que no vale no publica: el servicio contesta 401 y
    // el cliente lo dice.
    let wrong = Client::new(
        root,
        "wrong",
        service.port,
        "cargo-hosted",
        "not-a-real-credential",
    );
    let refused = wrong.run(&producer, &["publish", "--registry", "hodei"]);
    assert!(!refused.ok, "cargo published with a wrong credential");
    assert!(
        refused.output.contains("401"),
        "the refusal is not a 401:\n{}",
        refused.output
    );
}
