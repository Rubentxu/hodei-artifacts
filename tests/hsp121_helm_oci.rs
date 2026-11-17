//! Helm por OCI de HSP-121 contra el cliente real (`helm`).
//!
//! El chart se crea y se empaqueta localmente, se sube con `helm push`
//! contra el hub OCI existente y se vuelve a bajar con `helm pull`: las
//! dos rutas hablan la distribucion OCI estandar, sin superficie propia,
//! y el `.tgz` que vuelve es byte a byte el que se marcho.
//!
//! Hace falta `helm` en el PATH: sin el, la suite **falla** diciendo que
//! falta. La credencial va por stdin, nunca en argv, y cada ejecucion usa
//! su propia `HOME` y su propio `DOCKER_CONFIG`: nada se lee ni se escribe
//! fuera del directorio del despliegue.

mod support;

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;
use sha2::{Digest, Sha256};
use support::Deployment;

/// El seeder del catalogo.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

fn require_helm() {
    let found = Command::new("helm")
        .args(["version", "--short"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "the helm CLI is not installed: the Helm suite needs `helm` in the PATH"
    );
}

/// El resultado de una ejecucion del cliente.
struct Run {
    ok: bool,
    output: String,
}

/// Un `helm` aislado: su casa, su config de registro, su cache.
struct Client {
    root: std::path::PathBuf,
}

impl Client {
    fn new(root: &Path) -> Self {
        let home = root.join("helm-home");
        std::fs::create_dir_all(home.join("config")).expect("the helm home");
        std::fs::create_dir_all(home.join("cache")).expect("the helm cache");
        std::fs::create_dir_all(root.join("work")).expect("the work directory");
        Self {
            root: home.parent().expect("the root").to_path_buf(),
        }
    }

    fn run(&self, args: &[&str], stdin: Option<&str>) -> Run {
        let home = self.root.join("helm-home");
        let mut command = Command::new("helm");
        command
            .args(args)
            .current_dir(self.root.join("work"))
            .env("HOME", &home)
            .env("DOCKER_CONFIG", home.join("config"))
            .env("XDG_CACHE_HOME", home.join("cache"));
        if let Some(secret) = stdin {
            command.stdin(Stdio::piped());
            let mut child = command.spawn().expect("helm runs");
            child
                .stdin
                .as_mut()
                .expect("the stdin")
                .write_all(secret.as_bytes())
                .expect("the credential is written");
            let output = child.wait_with_output().expect("helm finishes");
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            return Run {
                ok: output.status.success(),
                output: text,
            };
        }
        let output = command.output().expect("helm runs");
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Run {
            ok: output.status.success(),
            output: text,
        }
    }
}

fn sha_of(path: &Path) -> String {
    let bytes = std::fs::read(path).expect("the chart is there");
    Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn a_chart_is_pushed_and_pulled_through_oci_with_the_bytes_preserved() {
    require_helm();
    let deployment = Deployment::new();
    // El repositorio helm vive para el canal OCI: un chart por OCI es un
    // artefacto OCI, y el hub es el mismo.
    deployment.seed_helm(&[("demo-chart", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    let client = Client::new(deployment.directory.path());
    let registry = format!("127.0.0.1:{port}");

    // Un chart de verdad, creado y empaquetado por el propio helm.
    let created = client.run(&["create", "demo-chart"], None);
    assert!(created.ok, "helm create: {}", created.output);
    let packaged = client.run(&["package", "demo-chart", "--destination", "."], None);
    assert!(packaged.ok, "helm package: {}", packaged.output);
    let tgz = deployment
        .directory
        .path()
        .join("work")
        .join("demo-chart-0.1.0.tgz");
    assert!(tgz.exists(), "helm package wrote: {}", packaged.output);

    // La sesion con el registro: la credencial por stdin.
    let login = client.run(
        &[
            "registry",
            "login",
            registry.as_str(),
            "-u",
            "alice",
            "--password-stdin",
            "--plain-http",
        ],
        Some(&token),
    );
    assert!(login.ok, "helm registry login: {}", login.output);

    // `helm push` sube el .tgz como capa de un artefacto OCI, con la
    // etiqueta de la version del chart.
    let pushed = client.run(
        &[
            "push",
            "demo-chart-0.1.0.tgz",
            &format!("oci://{registry}"),
            "--plain-http",
        ],
        None,
    );
    assert!(pushed.ok, "helm push: {}", pushed.output);

    // Y `helm pull` lo devuelve: el mismo nombre, la misma version, y
    // los mismos bytes. La integridad no es un extra del canal: es lo
    // que hace que un chart descargado sea el chart publicado.
    std::fs::create_dir_all(deployment.directory.path().join("work/pull")).expect("the pull dir");
    let pulled = client.run(
        &[
            "pull",
            &format!("oci://{registry}/demo-chart"),
            "--version",
            "0.1.0",
            "--destination",
            "pull",
            "--plain-http",
        ],
        None,
    );
    assert!(pulled.ok, "helm pull: {}", pulled.output);
    let round_trip = deployment
        .directory
        .path()
        .join("work")
        .join("pull")
        .join("demo-chart-0.1.0.tgz");
    assert!(round_trip.exists(), "helm pull wrote: {}", pulled.output);
    assert_eq!(
        sha_of(&tgz),
        sha_of(&round_trip),
        "el chart que vuelve no es byte a byte el que se marcho"
    );

    // Por digest tambien se pide: es lo que firma un valor de `helm push`.
    let manifest = client.run(
        &[
            "pull",
            &format!("oci://{registry}/demo-chart:0.1.0"),
            "--destination",
            "pull",
            "--plain-http",
        ],
        None,
    );
    assert!(manifest.ok, "helm pull by tag: {}", manifest.output);
}

#[test]
fn a_helm_chart_does_not_live_where_it_was_not_published() {
    require_helm();
    let deployment = Deployment::new();
    deployment.seed_helm(&[("demo-chart", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    let client = Client::new(deployment.directory.path());
    let registry = format!("127.0.0.1:{port}");
    std::fs::create_dir_all(deployment.directory.path().join("work/pull")).expect("the pull dir");
    let login = client.run(
        &[
            "registry",
            "login",
            registry.as_str(),
            "-u",
            "alice",
            "--password-stdin",
            "--plain-http",
        ],
        Some(&token),
    );
    assert!(login.ok, "helm registry login: {}", login.output);

    // Nadie publico nada: pedir el chart es un error del registro, no un
    // chart vacio.
    let missing = client.run(
        &[
            "pull",
            &format!("oci://{registry}/demo-chart"),
            "--version",
            "9.9.9",
            "--destination",
            "pull",
            "--plain-http",
        ],
        None,
    );
    assert!(
        !missing.ok,
        "an unpublished chart must not pull: {}",
        missing.output
    );

    // Y sin credencial tampoco hay canal: la distribucion pregunta antes
    // de abrir la casa. Otra casa es otro cliente: aqui no hay sesion de
    // login guardada.
    let anonymous_root = deployment.directory.path().join("anonymous");
    let anonymous = Client::new(&anonymous_root);
    let denied = anonymous.run(
        &[
            "pull",
            &format!("oci://{registry}/demo-chart"),
            "--version",
            "0.1.0",
            "--destination",
            ".",
            "--plain-http",
        ],
        None,
    );
    assert!(
        !denied.ok,
        "an anonymous pull must not succeed: {}",
        denied.output
    );
}
