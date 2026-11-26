//! HSP-112 hosted PyPI against the real clients (`twine` and `pip`).
//!
//! `twine` publishes a wheel and an sdist against a real service, and
//! `pip` installs them: first the wheel, and then the sdist, forcing the
//! client to build it. The index `pip` sees is this service's,
//! and the module it installs is the one that was published.
//!
//! `twine` and `pip` are needed on the PATH: without them, the suite **fails**
//! saying so. The credential goes in a temporary `netrc` and in `twine`'s
//! environment variables, never in argv, and each run uses its
//! own `HOME`: nothing is read or written outside the deployment
//! directory.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;

use support::{Deployment, FIRST_ACCOUNT};

/// The catalog seeder.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// Requires the command-line `twine`.
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

/// Requires the command-line `pip`.
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

/// The result of a client run.
struct Run {
    /// Whether the client finished successfully.
    ok: bool,
    /// What it said, through stdout and stderr.
    output: String,
}

/// An isolated PyPI client: its `HOME`, its `netrc` and its cache.
struct Client {
    /// The client directory.
    root: PathBuf,
    /// The `netrc` with the credential.
    netrc: PathBuf,
    /// The service port.
    port: u16,
    /// The service repository.
    repository: String,
    /// The credential.
    token: String,
}

impl Client {
    /// A client that talks to the service's `/pypi/{repository}/`.
    ///
    /// Each client has its own `HOME` and `netrc`: two clients with
    /// different credentials cannot step on each other.
    fn new(root: &Path, name: &str, port: u16, repository: &str, token: &str) -> Self {
        let home = root.join(format!("home-{name}"));
        std::fs::create_dir_all(&home).expect("the home");
        let netrc = root.join(format!("netrc-{name}"));
        // The credential travels in the **password**: the PyPI surface
        // accepts the basic authentication of `pip` and `twine`, and what
        // matters is the platform credential.
        std::fs::write(
            &netrc,
            format!("machine 127.0.0.1 login alice password {token}\n"),
        )
        .expect("the netrc is written");
        // A file with a credential is only read by whoever writes it.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&netrc, std::fs::Permissions::from_mode(0o600))
                .expect("the netrc is private");
        }
        Self {
            root: root.to_path_buf(),
            netrc,
            port,
            repository: repository.to_string(),
            token: token.to_string(),
        }
    }

    /// The repository URL, as the client sees it.
    fn the_repository_url(&self) -> String {
        format!("http://127.0.0.1:{}/pypi/{}/", self.port, self.repository)
    }

    /// The simple index URL, as the client sees it.
    fn the_index_url(&self) -> String {
        format!(
            "http://127.0.0.1:{}/pypi/{}/simple/",
            self.port, self.repository
        )
    }

    /// Publishes a distribution with `twine`.
    fn publish(&self, file: &Path) -> Run {
        let mut command = Command::new("twine");
        command
            .arg("upload")
            .arg("--repository-url")
            .arg(self.the_repository_url())
            .arg("--non-interactive")
            .arg("--disable-progress-bar")
            .arg(file)
            // The credential goes in the environment, never on the command line.
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

    /// Installs with `pip`.
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
            // The `netrc` says who it is: `pip` does not interact.
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

/// The output of a process, through stdout and stderr.
fn the_text(stdout: &[u8], stderr: &[u8]) -> String {
    let mut text = String::from_utf8_lossy(stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(stderr));
    text
}

/// The place where the interpreter has its user packages.
///
/// The client uses its own `HOME`, and the place where the interpreter looks for
/// its packages depends on the `HOME` of whoever runs it: without this path,
/// the `twine` installed for the real user is not visible from the isolated
/// deployment.
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

    // The two distributions, with the name they carry in the index.
    let dist = root.join("dist");
    std::fs::create_dir_all(&dist).expect("the dist directory");
    let wheel = dist.join("labpkg-1.0-py3-none-any.whl");
    std::fs::write(&wheel, support::a_wheel("labpkg", "1.0", Some(">=3.9")))
        .expect("the wheel is written");
    let sdist = dist.join("labpkg-1.0.tar.gz");
    std::fs::write(&sdist, support::an_sdist("labpkg", "1.0", Some(">=3.9")))
        .expect("the sdist is written");

    // `twine` publishes both.
    for file in [&wheel, &sdist] {
        let published = client.publish(file);
        assert!(
            published.ok,
            "twine upload of {} failed:\n{}",
            file.display(),
            published.output
        );
    }

    // The index `pip` sees has both distributions.
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

    // `pip` installs the wheel.
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

    // And with `--no-binary` it installs the sdist, which has to be built.
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

    // A credential that is not valid does not publish: the service answers 401 and the
    // client says so.
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
