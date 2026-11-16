//! PyPI hosted de HSP-112 contra los clientes reales (`twine` y `pip`).
//!
//! El `twine` publica un wheel y un sdist contra un servicio de verdad, y el
//! `pip` los instala: primero el wheel, y despues el sdist, obligando al
//! cliente a construirlo. El indice que ve el `pip` es el de este servicio,
//! y el modulo que instala es el que se publico.
//!
//! Hace falta `twine` y `pip` en el PATH: sin ellos, la suite **falla**
//! diciendo que falta. La credencial va en un `netrc` temporal y en las
//! variables de entorno de `twine`, nunca en argv, y cada ejecucion usa su
//! propio `HOME`: nada se lee ni se escribe fuera del directorio del
//! despliegue.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use support::{Deployment, FIRST_ACCOUNT};

/// El seeder del catalogo.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// Exige el `twine` de linea de comandos.
fn require_twine() {
    let found = Command::new("twine")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "the twine CLI is not installed: the PyPI suite needs `twine` in the PATH"
    );
}

/// Exige el `pip` de linea de comandos.
fn require_pip() {
    let found = Command::new("pip")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "the pip CLI is not installed: the PyPI suite needs `pip` in the PATH"
    );
}

/// El resultado de una ejecucion de un cliente.
struct Run {
    /// Si el cliente termino bien.
    ok: bool,
    /// Lo que dijo, por stdout y por stderr.
    output: String,
}

/// Un cliente de PyPI aislado: su `HOME`, su `netrc` y su cache.
struct Client {
    /// El directorio del cliente.
    root: PathBuf,
    /// El `netrc` con la credencial.
    netrc: PathBuf,
    /// El puerto del servicio.
    port: u16,
    /// El repositorio del servicio.
    repository: String,
    /// La credencial.
    token: String,
}

impl Client {
    /// Un cliente que habla con `/pypi/{repository}/` del servicio.
    ///
    /// Cada cliente tiene su `HOME` y su `netrc`: dos clientes con
    /// credenciales distintas no pueden pisarse.
    fn new(root: &Path, name: &str, port: u16, repository: &str, token: &str) -> Self {
        let home = root.join(format!("home-{name}"));
        std::fs::create_dir_all(&home).expect("the home");
        let netrc = root.join(format!("netrc-{name}"));
        // La credencial viaja en la **contrasena**: la superficie PyPI
        // acepta la autenticacion basica de `pip` y `twine`, y lo que
        // importa es la credencial de la plataforma.
        std::fs::write(
            &netrc,
            format!("machine 127.0.0.1 login alice password {token}\n"),
        )
        .expect("the netrc is written");
        Self {
            root: root.to_path_buf(),
            netrc,
            port,
            repository: repository.to_string(),
            token: token.to_string(),
        }
    }

    /// La URL del repositorio, como la ve el cliente.
    fn the_repository_url(&self) -> String {
        format!("http://127.0.0.1:{}/pypi/{}/", self.port, self.repository)
    }

    /// La URL del indice simple, como la ve el cliente.
    fn the_index_url(&self) -> String {
        format!(
            "http://127.0.0.1:{}/pypi/{}/simple/",
            self.port, self.repository
        )
    }

    /// Publica una distribucion con `twine`.
    fn publish(&self, file: &Path) -> Run {
        let mut command = Command::new("twine");
        command
            .arg("upload")
            .arg("--repository-url")
            .arg(self.the_repository_url())
            .arg("--non-interactive")
            .arg("--disable-progress-bar")
            .arg(file)
            // La credencial va en el entorno, nunca en la linea de ordenes.
            .env("TWINE_USERNAME", "alice")
            .env("TWINE_PASSWORD", &self.token)
            .env("HOME", self.root.join("home"))
            .env("XDG_CONFIG_HOME", self.root.join("home"));
        if let Some(path) = the_python_path() {
            command.env("PYTHONPATH", path);
        }
        let output = command.output().expect("twine runs");
        Run {
            ok: output.status.success(),
            output: the_text(&output.stdout, &output.stderr),
        }
    }

    /// Instala con `pip`.
    fn install(&self, target: &Path, extra: &[&str], package: &str) -> Run {
        let output = Command::new("pip")
            .arg("install")
            .arg("--index-url")
            .arg(self.the_index_url())
            .arg("--target")
            .arg(target)
            .arg("--no-cache-dir")
            .arg("--no-deps")
            .arg("--disable-pip-version-check")
            .args(extra)
            .arg(package)
            // El `netrc` dice quien es: el `pip` no interactua.
            .env("NETRC", &self.netrc)
            .env("HOME", self.root.join("home"))
            .env("PIP_CONFIG_FILE", "/dev/null")
            .output()
            .expect("pip runs");
        Run {
            ok: output.status.success(),
            output: the_text(&output.stdout, &output.stderr),
        }
    }
}

/// La salida de un proceso, por stdout y por stderr.
fn the_text(stdout: &[u8], stderr: &[u8]) -> String {
    let mut text = String::from_utf8_lossy(stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(stderr));
    text
}

/// El sitio donde el interprete tiene sus paquetes de usuario.
///
/// El cliente usa su propio `HOME`, y el sitio donde el interprete busca
/// sus paquetes depende del `HOME` de quien lo ejecuta: sin este camino,
/// el `twine` instalado para el usuario real no se ve desde el despliegue
/// aislado.
fn the_python_path() -> Option<String> {
    let output = Command::new("python3")
        .args(["-c", "import site; print(site.getusersitepackages())"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        return None;
    }
    Some(path)
}

#[test]
fn twine_publishes_and_pip_installs_a_wheel_and_an_sdist() {
    require_twine();
    require_pip();
    let deployment = Deployment::new();
    deployment.seed_pypi(&[("pypi-hosted", serde_json::Value::Null)]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let root = deployment.directory.path();
    let client = Client::new(root, "client", service.port, "pypi-hosted", &token);

    // Las dos distribuciones, con el nombre que llevan en el indice.
    let dist = root.join("dist");
    std::fs::create_dir_all(&dist).expect("the dist directory");
    let wheel = dist.join("labpkg-1.0-py3-none-any.whl");
    std::fs::write(&wheel, support::a_wheel("labpkg", "1.0", Some(">=3.9")))
        .expect("the wheel is written");
    let sdist = dist.join("labpkg-1.0.tar.gz");
    std::fs::write(&sdist, support::an_sdist("labpkg", "1.0", Some(">=3.9")))
        .expect("the sdist is written");

    // El `twine` publica las dos.
    for file in [&wheel, &sdist] {
        let published = client.publish(file);
        assert!(
            published.ok,
            "twine upload of {} failed:\n{}",
            file.display(),
            published.output
        );
    }

    // El indice que ve el `pip` tiene las dos distribuciones.
    let index = support::Curl::call_with_headers(
        service.port,
        "GET",
        "/pypi/pypi-hosted/simple/labpkg/",
        &token,
        None,
        &[("accept", "application/vnd.pypi.simple.v1+json")],
        Some(&root.join("index.json")),
    )
    .run();
    assert_eq!(index.status, 200);
    let document: serde_json::Value =
        serde_json::from_slice(&index.body).expect("the index is JSON");
    let files: Vec<&str> = document["files"]
        .as_array()
        .expect("the files")
        .iter()
        .filter_map(|file| file["filename"].as_str())
        .collect();
    assert!(files.contains(&"labpkg-1.0-py3-none-any.whl"), "{files:?}");
    assert!(files.contains(&"labpkg-1.0.tar.gz"), "{files:?}");

    // El `pip` instala el wheel.
    let target = root.join("installed-wheel");
    let installed = client.install(&target, &[], "labpkg");
    assert!(installed.ok, "pip install failed:\n{}", installed.output);
    let module = target.join("labpkg/__init__.py");
    assert!(
        module.exists(),
        "the wheel did not install the module:\n{}",
        installed.output
    );
    assert!(
        std::fs::read_to_string(&module)
            .expect("the module")
            .contains("VALUE = 42"),
        "the installed module is not the one that was published"
    );

    // Y con `--no-binary` instala el sdist, que tiene que construirse.
    let target = root.join("installed-sdist");
    let installed = client.install(
        &target,
        &["--no-binary", ":all:", "--no-build-isolation"],
        "labpkg==1.0",
    );
    assert!(
        installed.ok,
        "pip install of the sdist failed:\n{}",
        installed.output
    );
    let module = target.join("labpkg/__init__.py");
    assert!(
        module.exists(),
        "the sdist did not install the module:\n{}",
        installed.output
    );

    // Una credencial que no vale no publica: el servicio contesta 401 y el
    // cliente lo dice.
    let other = dist.join("labpkg-2.0-py3-none-any.whl");
    std::fs::write(&other, support::a_wheel("labpkg", "2.0", None))
        .expect("the second wheel is written");
    let wrong = Client::new(
        root,
        "wrong",
        service.port,
        "pypi-hosted",
        "not-a-real-credential",
    );
    let refused = wrong.publish(&other);
    assert!(!refused.ok, "twine published with a wrong credential");
    assert!(
        refused.output.contains("401"),
        "the refusal is not a 401:\n{}",
        refused.output
    );
}
