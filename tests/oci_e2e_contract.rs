//! OCI compatibility with real clients (HSP-103 / HAR-09).
//!
//! Everything before has spoken the protocol with a `curl` and with `oras`.
//! This speaks the protocol with **the real clients**: `podman`, the
//! `docker` CLI and `oras`, against the real service, on a
//! real port.
//!
//! And it is not the same. A `curl` that abides by the specification shows that
//! the registry *can* speak the protocol; a `podman push` that ends
//! in `exit 0` shows that the registry *speaks* the protocol, because the
//! client does things a `curl` does not: it asks for a token, negotiates the media
//! type, checks the digest of what it downloads, and notices if the
//! registry lies.
//!
//! The real clients are the ones that say whether the registry works. A
//! registry that has only been tested with `curl` may have the happy path
//! and no other, and the happy path is the one nobody uses.

mod support;

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use support::{Deployment, FIRST_ACCOUNT, FIRST_ONLY_REPOSITORY, FIRST_REPOSITORY, SECOND_ACCOUNT};

/// The first person of the first account.
const ALICE: &str = "alice";

/// The first repository used by the official conformance suite.
const CONFORMANCE_REPOSITORY: &str = "conformance/repo1";

/// The second repository used by the official conformance suite.
///
/// The suite uses two because some tests push to one and read from the other:
/// cross-repository mounting, for example.
const CONFORMANCE_REPOSITORY_TWO: &str = "conformance/repo2";

/// The repository **with levels** that is published in the
/// hierarchy case.
///
/// A name with levels is what makes the authorization engine
/// declare a parent--`team`-- and the one the schema has to allow.
const THE_NESTED_REPOSITORY: &str = "team/demo";

/// The media type of the SBOM that hangs from the artifact.
const SBOM_TYPE: &str = "application/vnd.hodei.sbom.v1";

/// The content of the file that goes inside the image.
///
/// It is small on purpose: what is tested is the protocol, not the
/// size of the image.
const THE_FILE: &str = "hodei e2e\n";

/// A clean deployment with the catalog seeded.
fn a_deployment() -> Deployment {
    let deployment = Deployment::new();
    deployment.seed();
    deployment
}

/// The report case, before running it.
///
/// It is recorded twice: at the start, with what is expected, and at the end,
/// with what was measured. If the test dies in between, the
/// report keeps the first record, which says `FAILURE`.
fn the_case(id: &str, given: &str, when: &str, command: &str) -> support::Case {
    support::Case {
        id: id.to_string(),
        given: given.to_string(),
        when: when.to_string(),
        command: command.to_string(),
        outcome: "en curso".to_string(),
        passed: false,
    }
}

/// What a client command returns.
struct Outcome {
    /// The exit code.
    status: i32,
    /// What it said on standard output.
    stdout: String,
    /// What it said on standard error.
    stderr: String,
}

impl Outcome {
    /// What it said, from both outputs.
    fn said(&self) -> String {
        let mut text = self.stdout.clone();
        if !self.stderr.trim().is_empty() {
            if !text.trim().is_empty() {
                text.push('\n');
            }
            text.push_str(&self.stderr);
        }
        text
    }
}

/// A client's home: where its configuration, its credentials and
/// its store live.
///
/// It is a **temporary** directory, and that is why the test leaves nothing on the
/// machine of whoever runs it. `oras login` writes a credentials
/// file where the client keeps its configuration, and a `login` that
/// leaves the test's credential in the person's home is a
/// credential in a file of the person's. And `HOME` also decides where
/// `podman`'s image store lives: with the temporary home, the
/// images the test builds do not stay in the store of whoever
/// runs it.
struct Home {
    directory: tempfile::TempDir,
}

impl Home {
    /// An empty home.
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("a temp dir");
        std::fs::create_dir_all(directory.path().join("config")).expect("the config directory");
        Self { directory }
    }

    /// The home's path.
    fn path(&self) -> &std::path::Path {
        self.directory.path()
    }
}

/// A client command, executed **in a home**.
///
/// `HOME` and `DOCKER_CONFIG` point at the home: whatever the client
/// writes --its configuration, its credentials, its store-- stays
/// inside the home and disappears with it.
fn run_in(home: &Home, program: &str, args: &[&str]) -> Outcome {
    let output = Command::new(program)
        .args(args)
        .env("HOME", home.path())
        .env("DOCKER_CONFIG", home.path().join("config"))
        .output()
        .unwrap_or_else(|error| panic!("{program} could not be run: {error}"));
    Outcome {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// A client command, executed **in a home** and **in a directory**.
///
/// The working directory is not a detail: `oras` takes the file name from the
/// argument **as is**, and that name is the artifact's title. A title with an
/// absolute path cannot be downloaded inside a directory--the client does not
/// let itself out of it, and it is right--, so an artifact is published
/// with the name it would have inside the working
/// directory.
///
/// It is the client's own path traversal protection, not a
/// limitation of the registry.
fn run_in_dir(home: &Home, program: &str, args: &[&str], directory: &std::path::Path) -> Outcome {
    let output = Command::new(program)
        .args(args)
        .env("HOME", home.path())
        .env("DOCKER_CONFIG", home.path().join("config"))
        .current_dir(directory)
        .output()
        .unwrap_or_else(|error| panic!("{program} could not be run: {error}"));
    Outcome {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// `oras`'s `login`, with the credential on standard input.
///
/// A credential on the command line is visible in the machine's whole `ps`,
/// and the file where `oras` stores it--the client's
/// configuration-- is in the case's temporary home: the credential goes away with
/// the test.
fn oras_login(home: &Home, oras: &str, registry: &str, token: &str) -> Outcome {
    let mut command = Command::new(oras);
    command.args([
        "login",
        "--plain-http",
        "--username",
        ALICE,
        "--password-stdin",
        registry,
    ]);
    command
        .env("HOME", home.path())
        .env("DOCKER_CONFIG", home.path().join("config"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("the login starts");
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().expect("the stdin of the login");
        stdin
            .write_all(token.as_bytes())
            .expect("the credential is written");
    }
    let output = child.wait_with_output().expect("the login finishes");
    Outcome {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// The registry's address, as the clients see it.
///
/// The name is `localhost` and not an address: the daemon behind
/// the Docker CLI only relaxes TLS verification when the
/// registry is called `localhost`, and the test registry speaks HTTP.
/// With a `127.0.0.1` address the daemon talks to it over HTTPS, and
/// the client is left without an image.
fn the_registry(port: u16) -> String {
    format!("localhost:{port}")
}

/// The client's credentials file.
///
/// The token travels in the **file** and not on the command line: a
/// command line is visible in the machine's whole `ps`, and a
/// credential on the command line is a credential on the machine.
/// The file lives in the client's home, which is temporary, so the
/// credential goes away with the test.
///
/// The format is that of `containers/image`: the `user:password` pair
/// in base64, per repository.
fn the_auth_file(home: &Home, registry: &str, user: &str, token: &str) -> PathBuf {
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{token}"));
    let path = home.path().join(format!("auth-{user}.json"));
    std::fs::write(
        &path,
        format!("{{\"auths\":{{\"{registry}\":{{\"auth\":\"{encoded}\"}}}}}}"),
    )
    .expect("the auth file is written");
    path
}

/// The catalog seeder (repositories per account).
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

// ---------------------------------------------------------------------------
// The podman: push, tag and pull of an image.
// ---------------------------------------------------------------------------

/// `podman` pushes an image, tags it with another tag, and pulls it
/// back down.
///
/// What is measured is that the client ends in `exit 0` and that what comes back
/// is **the same content**: the digest of the manifest that is pushed and the
/// digest of the manifest that is pulled are the same. A client that pulls something
/// else has not pulled the image: it has pulled what the registry gave it, and
/// that is exactly what must be looked at.
#[test]
fn podman_pushes_an_image_tags_it_and_pulls_it_back() {
    let mut case = the_case(
        "OCI-PUSH-03 (podman)",
        "una imagen construida sin red y un repositorio hosted limpio",
        "se empuja la imagen, se etiqueta con otra etiqueta, se borra la local y se vuelve a bajar",
        "podman push --authfile <alice> --tls-verify=false localhost:<port>/releases:v1 <local>",
    );
    support::record_oci_e2e(case.clone());

    // The client's home: temporary, so that neither the credential nor the
    // test's images stay on the machine of whoever runs it.
    let home = Home::new();
    let deployment = a_deployment();
    let service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, ALICE);
    let registry = the_registry(service.port);
    let local = format!("localhost/hsp103-e2e-{port}:v1", port = service.port);
    let auth = the_auth_file(&home, &registry, ALICE, &token);
    let auth = auth
        .to_str()
        .expect("the path of the auth file")
        .to_string();

    let context = a_build_context();
    let built = the_built_image(&home, &local, None, context.path());
    assert_eq!(
        built.status,
        0,
        "the image could not be built:\n{}",
        built.said()
    );

    // The digest of what is local, before touching the registry: it is the
    // reference against which what comes back is checked.
    let before = the_digest_of(&home, &local);
    assert!(
        before.starts_with("sha256:"),
        "the local image has no digest: {before}"
    );

    // The digest of what is pushed: the client writes it to a file,
    // and it is the digest of the manifest as it travels to the registry.
    let digests = tempfile::tempdir().expect("a temp dir");
    let digest_file = digests.path().join("digest");
    let pushed = run_in(
        &home,
        &support::the_podman(),
        &[
            "push",
            "--tls-verify=false",
            "--digestfile",
            digest_file.to_str().expect("the path of the digest file"),
            "--authfile",
            auth.as_str(),
            &local,
            &format!("{registry}/{FIRST_REPOSITORY}:v1"),
        ],
    );
    assert_eq!(
        pushed.status,
        0,
        "the image could not be pushed:\n{}",
        pushed.said()
    );
    let pushed_digest = std::fs::read_to_string(&digest_file)
        .expect("the digest of the push")
        .trim()
        .to_string();
    assert!(
        pushed_digest.starts_with("sha256:"),
        "the pushed image has no digest: {pushed_digest}"
    );

    // The tag is the client's, not the registry's: `podman tag` names
    // the image locally and the `push` of that name is what decides which
    // tag is published.
    let tagged = run_in(
        &home,
        &support::the_podman(),
        &["tag", &local, &format!("{registry}/{FIRST_REPOSITORY}:v2")],
    );
    assert_eq!(
        tagged.status,
        0,
        "the image could not be tagged:\n{}",
        tagged.said()
    );

    let pushed_v2 = run_in(
        &home,
        &support::the_podman(),
        &[
            "push",
            "--tls-verify=false",
            "--authfile",
            auth.as_str(),
            &format!("{registry}/{FIRST_REPOSITORY}:v2"),
            &format!("{registry}/{FIRST_REPOSITORY}:v2"),
        ],
    );
    assert_eq!(
        pushed_v2.status,
        0,
        "the tagged image could not be pushed:\n{}",
        pushed_v2.said()
    );

    // The local image is deleted: what is pulled has to come from the
    // registry, not from what was already on the machine.
    let removed = run_in(&home, &support::the_podman(), &["rmi", &local]);
    assert_eq!(
        removed.status,
        0,
        "the image could not be removed:\n{}",
        removed.said()
    );

    let pulled = run_in(
        &home,
        &support::the_podman(),
        &[
            "pull",
            "--tls-verify=false",
            "--authfile",
            auth.as_str(),
            &format!("{registry}/{FIRST_REPOSITORY}:v1"),
        ],
    );
    assert_eq!(
        pulled.status,
        0,
        "the image could not be pulled:\n{}",
        pulled.said()
    );

    // What comes back has to be **the same** as what went up: the
    // digest the client computed when pushing and the one it computes when
    // pulling are the digest of the same manifest.
    let after = the_digest_of(&home, &format!("{registry}/{FIRST_REPOSITORY}:v1"));
    assert_eq!(
        after, pushed_digest,
        "what came back is not what went up: {after} != {pushed_digest}"
    );

    // And the registry says the same: what it publishes and what the client
    // received is the same manifest.
    let published = the_published_digest(service.port, FIRST_REPOSITORY, "v1", &token);
    assert_eq!(
        published, pushed_digest,
        "what the registry published is not what went up: {published} != {pushed_digest}"
    );

    // And the second tag is pulled too, which is what it means for
    // both tags to exist in the registry.
    let pulled_v2 = run_in(
        &home,
        &support::the_podman(),
        &[
            "pull",
            "--tls-verify=false",
            "--authfile",
            auth.as_str(),
            &format!("{registry}/{FIRST_REPOSITORY}:v2"),
        ],
    );
    assert_eq!(
        pulled_v2.status,
        0,
        "the tagged image could not be pulled:\n{}",
        pulled_v2.said()
    );

    // The machine is left as it was: the images made for the
    // test do not stay in the person's store.
    let _ = run_in(
        &home,
        &support::the_podman(),
        &["rmi", &format!("{registry}/{FIRST_REPOSITORY}:v1")],
    );
    let _ = run_in(
        &home,
        &support::the_podman(),
        &["rmi", &format!("{registry}/{FIRST_REPOSITORY}:v2")],
    );

    // What is written to the report is the **published** digest, which is the
    // one that comes back intact. And the local digest is stated **as it is**: if
    // they match, it says they match; if not, it says that the local manifest
    // and the published one are two distinct manifests with the same
    // content--the client adds the creation annotation when pushing--.
    // A report that always says the same thing says nothing.
    case.outcome = if before == after {
        format!(
            "push y pull en exit 0, y el digest vuelve intacto: {after} \
             (el digest local y el publicado coinciden)"
        )
    } else {
        format!(
            "push y pull en exit 0, y el digest publicado vuelve intacto: {after} \
             (el digest local era {before}: el cliente anade la anotacion de creacion al empujar, \
             y el manifiesto local y el publicado son dos manifiestos distintos con el mismo contenido)",
            after = after,
            before = before
        )
    };
    case.passed = true;
    support::record_oci_e2e(case);
}

/// The digest of a local image.
///
/// The client says it in its own format, and the format it uses is the
/// digest of the manifest: the same one the registry publishes.
fn the_digest_of(home: &Home, reference: &str) -> String {
    let outcome = run_in(
        home,
        &support::the_podman(),
        &["image", "inspect", "--format", "{{.Digest}}", reference],
    );
    assert_eq!(
        outcome.status,
        0,
        "the image {reference} could not be inspected:\n{}",
        outcome.said()
    );
    outcome.stdout.trim().to_string()
}

/// The build context of the test image.
///
/// It is a minimal `Containerfile` over `scratch`: the image is built
/// **without network**, and what is tested is the registry's protocol, not
/// the machine's ability to download a base image.
fn a_build_context() -> tempfile::TempDir {
    let context = tempfile::tempdir().expect("a temp dir");
    std::fs::write(context.path().join("file.txt"), THE_FILE).expect("the file is written");
    std::fs::write(
        context.path().join("Containerfile"),
        "FROM scratch\nCOPY file.txt /file.txt\n",
    )
    .expect("the containerfile is written");
    context
}

/// Builds the test image, with the **lock** of the
/// builds.
///
/// Two `podman build`s at once on the same machine fight over
/// `fuse-overlayfs`: the unmount waits for the write to finish,
/// the process **does not terminate**, and the machine is left with
/// gigabytes of dirty pages and processes that cannot be killed. It is an
/// environment problem and not the registry's, but it hangs the suite, and a
/// suite that hangs the machine cannot be run in parallel.
///
/// The lock is a `flock` on a file in `target`: it serializes the
/// builds **between cases and between processes**, not just between threads,
/// and the system releases it when the process dies.
fn the_built_image(
    home: &Home,
    name: &str,
    architecture: Option<&str>,
    context: &std::path::Path,
) -> Outcome {
    let _lock = the_build_lock();
    let mut arguments = vec!["build".to_string(), "--tls-verify=false".to_string()];
    if let Some(architecture) = architecture {
        arguments.push("--arch".to_string());
        arguments.push(architecture.to_string());
    }
    arguments.push("-t".to_string());
    arguments.push(name.to_string());
    arguments.push(context.to_string_lossy().into_owned());
    let borrowed: Vec<&str> = arguments.iter().map(|argument| argument.as_str()).collect();
    run_in(home, &support::the_podman(), &borrowed)
}

/// The image builds' lock.
///
/// The file is in `target`: it is execution state, not the
/// repository's, and it is not versioned.
fn the_build_lock() -> std::fs::File {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("oci-e2e-build.lock");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("the directory of the lock");
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&path)
        .expect("the lock of the builds");
    file.lock().expect("the lock of the builds is taken");
    file
}

/// The local image, in a tar the daemon can read.
///
/// The Docker CLI pushes what the **daemon** has, and the daemon does not
/// see the store of the client that built it: the image travels in a tar,
/// which is the format both understand. The tar stays in
/// the directory that is returned, and that directory lives as long as the
/// case does.
fn the_saved_image(home: &Home, image: &str) -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().expect("a temp dir");
    let tar = directory.path().join("image.tar");
    let saved = run_in(
        home,
        &support::the_podman(),
        &[
            "save",
            "--output",
            tar.to_str().expect("the path of the tar"),
            image,
        ],
    );
    assert_eq!(
        saved.status,
        0,
        "the image {image} could not be saved:\n{}",
        saved.said()
    );
    (directory, tar)
}

// ---------------------------------------------------------------------------
// The CLI's docker: push and pull of an image and of an index.
// ---------------------------------------------------------------------------

/// The daemon behind the CLI's `docker`.
///
/// The Docker CLI does not talk to a registry: it talks to a **daemon**, and
/// it is the daemon that talks to the registry. The daemon on this
/// machine is `podman`, which exposes the Docker API over a unix socket.
///
/// And the socket is started with its own configuration, which is the one that
/// says the test registry is one of those spoken to **without TLS**:
/// the test registry speaks HTTP, and an address can be marked
/// as insecure in the client's configuration.
struct Engine {
    /// The daemon's process.
    child: Child,
    /// The home where the CLI's things live (the configuration and the
    /// credentials): it is temporary, and the credential does not stay on the
    /// machine of whoever runs the test.
    home: Home,
    /// The daemon's socket.
    socket: PathBuf,
}

impl Drop for Engine {
    /// The daemon shuts down with the test.
    ///
    /// A daemon that is left listening on a system socket is a
    /// daemon that stays forever, and the machine of whoever runs
    /// the test is not the place to leave it.
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Engine {
    /// The environment the client's commands need.
    ///
    /// `HOME` is the daemon's: it is where the clients' configuration is,
    /// which says the test registry is spoken to without TLS,
    /// and without it `docker` neither pulls nor pushes anything.
    fn environment(&self) -> Vec<(String, String)> {
        vec![
            (
                "DOCKER_HOST".to_string(),
                format!("unix://{}", self.socket.display()),
            ),
            (
                "DOCKER_CONFIG".to_string(),
                self.home
                    .path()
                    .join("config")
                    .to_string_lossy()
                    .into_owned(),
            ),
            (
                "HOME".to_string(),
                self.home.path().to_string_lossy().into_owned(),
            ),
        ]
    }
}

/// Starts the daemon and waits for its socket to exist.
///
/// The socket is the proof that the daemon is listening: it appears
/// when it is, and not before.
///
/// The clients' configuration goes in the daemon's **HOME**, and not
/// in a variable: the clients library reads
/// `$HOME/.config/containers/registries.conf`, and that file takes
/// priority over the system's. Without it, the daemon tries to talk
/// **HTTPS** to a registry that speaks HTTP, and the client is left without an
/// image.
fn an_engine(port: u16) -> Engine {
    let home = Home::new();
    let socket = home.path().join("engine.sock");
    let registries = home
        .path()
        .join(".config")
        .join("containers")
        .join("registries.conf");
    std::fs::create_dir_all(registries.parent().expect("the config directory"))
        .expect("the config directory");
    std::fs::write(
        &registries,
        format!(
            "[[registry]]\nlocation = \"localhost:{port}\"\ninsecure = true\n",
            port = port
        ),
    )
    .expect("the registries config is written");

    let child = Command::new(support::the_podman())
        .args([
            "system",
            "service",
            "--time=0",
            &format!("unix://{}", socket.display()),
        ])
        .env("HOME", home.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("the engine starts");

    // The daemon goes into the `Engine` before waiting: that way its shutdown is
    // the business of its `Drop`, which kills it and waits for it, and not of this path. A
    // daemon the test abandons stays listening.
    let engine = Engine {
        child,
        home,
        socket,
    };

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        // The **connected** socket is the proof that the daemon serves:
        // the file may exist before the daemon is serving, and a
        // daemon that dies afterwards leaves the file there. With the connection,
        // what is measured is what the client is going to find.
        if engine.socket.exists() && std::os::unix::net::UnixStream::connect(&engine.socket).is_ok()
        {
            return engine;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    drop(engine);
    panic!("the engine did not open its socket");
}

/// A command of the CLI's `docker`.
///
/// The client reads its credentials from its configuration, which is in a
/// temporary directory: the test's credential is not written to the
/// configuration of the machine of whoever runs it.
fn docker(engine: &Engine, args: &[&str]) -> Outcome {
    let mut command = Command::new(support::the_docker());
    command.args(args);
    for (name, value) in engine.environment() {
        command.env(name, value);
    }
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("docker could not be run: {error}"));
    Outcome {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// The test's `docker login`.
///
/// The credential goes on standard input, which is the only path that does not
/// leave it on the command line: a command line is visible in the
/// history and in the report.
fn docker_login(engine: &Engine, registry: &str, token: &str) -> Outcome {
    let mut child = Command::new(support::the_docker());
    child.args(["login", "--username", ALICE, "--password-stdin", registry]);
    for (name, value) in engine.environment() {
        command_env(&mut child, name, value);
    }
    child
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = child.spawn().expect("the login starts");
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().expect("the stdin of the login");
        stdin
            .write_all(token.as_bytes())
            .expect("the credential is written");
    }
    let output = child.wait_with_output().expect("the login finishes");
    Outcome {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// A command's environment, one entry.
fn command_env(command: &mut Command, name: String, value: String) {
    command.env(name, value);
}

/// The CLI's `docker` pushes and pulls a single-architecture image.
///
/// What is measured is that the client ends in `exit 0` and that what
/// comes back is the same content: the digest the registry publishes is the
/// digest the daemon leaves recorded.
#[test]
fn the_docker_cli_pushes_and_pulls_a_single_architecture_image() {
    let mut case = the_case(
        "OCI-PUSH-03 (docker, una plataforma)",
        "una imagen de una sola plataforma y un repositorio hosted limpio",
        "el docker de la CLI la empuja, se borra del demonio y se vuelve a bajar",
        "docker push localhost:<port>/releases:v1",
    );
    support::record_oci_e2e(case.clone());

    // The client's home: temporary, so that neither the credential nor the
    // test's images stay on the machine of whoever runs it.
    let home = Home::new();
    let deployment = a_deployment();
    let service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, ALICE);
    let registry = the_registry(service.port);
    let engine = an_engine(service.port);
    let reference = format!("{registry}/{FIRST_REPOSITORY}:v1");

    let login = docker_login(&engine, &registry, &token);
    assert_eq!(login.status, 0, "the login failed:\n{}", login.said());

    // The image has to be in the daemon's store under the destination's
    // name: the client asks the daemon to push "this", and "this"
    // is looked up by name.
    let context = a_build_context();
    let local = format!("localhost/hsp103-e2e-{port}:v1", port = service.port);
    let built = the_built_image(&home, &local, None, context.path());
    assert_eq!(
        built.status,
        0,
        "the image could not be built:\n{}",
        built.said()
    );

    // The image goes to the daemon in a tar: it is what the daemon knows.
    let (_tar_directory, tar) = the_saved_image(&home, &local);
    let loaded = docker(
        &engine,
        &[
            "load",
            "--input",
            tar.to_str().expect("the path of the tar"),
        ],
    );
    assert_eq!(
        loaded.status,
        0,
        "the image could not be loaded:\n{}",
        loaded.said()
    );

    let tagged = docker(&engine, &["tag", &local, &reference]);
    assert_eq!(
        tagged.status,
        0,
        "the image could not be tagged:\n{}",
        tagged.said()
    );

    let pushed = docker(&engine, &["push", &reference]);
    assert_eq!(
        pushed.status,
        0,
        "the image could not be pushed:\n{}",
        pushed.said()
    );

    // The digest the registry publishes: that of the manifest it has, not
    // the one the daemon computes on its machine.
    let published = the_published_digest(service.port, FIRST_REPOSITORY, "v1", &token);
    assert!(
        published.starts_with("sha256:"),
        "the pushed image has no digest: {published}"
    );

    // It is deleted from the daemon: what is pulled has to come from the registry.
    let removed = docker(&engine, &["rmi", &reference]);
    assert_eq!(
        removed.status,
        0,
        "the image could not be removed:\n{}",
        removed.said()
    );

    let pulled = docker(&engine, &["pull", &reference]);
    assert_eq!(
        pulled.status,
        0,
        "the image could not be pulled:\n{}",
        pulled.said()
    );

    // What comes back has to be **the same content**, and content
    // is identified by its digest. Three things are compared: the digest that
    // the registry publishes before and after the round trip, and the digest
    // the client records.
    //
    // And here the **registry's** digest has to be looked at, not the first one
    // the daemon has recorded: the image enters the daemon through a tar,
    // with the manifest of its local format, and leaves through another, with the
    // manifest the registry publishes. Both are real and neither is
    // the other, and the order in which the daemon lists them is not a contract.
    let after = the_published_digest(service.port, FIRST_REPOSITORY, "v1", &token);
    assert_eq!(
        after, published,
        "what came back is not what went up: {after} != {published}"
    );
    let digests = the_repo_digests(&engine, &reference);
    let repository = the_repository_of(&reference);
    assert!(
        digests
            .iter()
            .any(|entry| entry == &format!("{repository}@{published}")),
        "the client did not record the digest the registry published: {digests:?}"
    );

    let _ = run_in(&home, &support::the_podman(), &["rmi", &local]);
    let _ = docker(&engine, &["rmi", &reference]);

    case.outcome = format!("push y pull en exit 0, y el digest vuelve intacto: {published}");
    case.passed = true;
    support::record_oci_e2e(case);
}

/// The digest the registry publishes for a tag.
///
/// It is stated by the `Docker-Content-Digest` header of the response: it is the
/// digest of the manifest the registry has, not the one the client
/// computed on its machine.
fn the_published_digest(port: u16, name: &str, tag: &str, token: &str) -> String {
    let answer = support::curl_describe(port, &format!("/v2/{name}/manifests/{tag}"), token);
    assert_eq!(
        answer.status,
        200,
        "the manifest {name}:{tag} could not be described:\n{}",
        String::from_utf8_lossy(&answer.body)
    );
    answer
        .header("Docker-Content-Digest")
        .expect("the digest of the manifest")
}

/// The digests under which the daemon has the image.
///
/// The daemon may have **more than one**: the image enters through a tar--with
/// the manifest of its local format-- and leaves through another--with the manifest
/// the registry publishes--. Both are real and the order in which the
/// daemon lists them is not a contract, so **all** are returned and
/// whoever compares looks for the one that matters to them.
fn the_repo_digests(engine: &Engine, reference: &str) -> Vec<String> {
    let outcome = docker(
        engine,
        &[
            "image",
            "inspect",
            "--format",
            "{{json .RepoDigests}}",
            reference,
        ],
    );
    assert_eq!(
        outcome.status,
        0,
        "the image {reference} could not be inspected:\n{}",
        outcome.said()
    );
    outcome
        .stdout
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|entry| entry.trim().trim_matches('"').to_string())
        .filter(|entry| !entry.is_empty())
        .collect()
}

/// The repository name of a reference.
///
/// The reference is `name:tag`, and the name may have ports and
/// levels: the separator is the **last** colon, and only if what
/// follows has no slashes.
fn the_repository_of(reference: &str) -> String {
    match reference.rsplit_once(':') {
        Some((name, tag)) if !tag.contains('/') => name.to_string(),
        _ => reference.to_string(),
    }
}

/// The CLI's `docker` pushes an index of two platforms.
///
/// The index is assembled **by the CLI**: `docker manifest create` and `docker manifest
/// add` are client code, and `docker manifest push` pushes the index and
/// its children to the registry without going through the daemon. It is the path a
/// real client uses to publish a multi-platform image, and it is
/// the one that says whether the registry knows how to receive an index.
#[test]
fn the_docker_cli_pushes_and_pulls_a_multi_architecture_image() {
    let mut case = the_case(
        "OCI-INDEX-01 (docker, multi-plataforma)",
        "una imagen por plataforma, en el demonio y en el registro",
        "la CLI arma un indice de dos plataformas, lo empuja y lo vuelve a bajar",
        "docker manifest push localhost:<port>/releases:multi",
    );
    support::record_oci_e2e(case.clone());

    // The client's home: temporary, so that neither the credential nor the
    // test's images stay on the machine of whoever runs it.
    let home = Home::new();
    let deployment = a_deployment();
    let service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, ALICE);
    let registry = the_registry(service.port);
    let engine = an_engine(service.port);
    let index = format!("{registry}/{FIRST_REPOSITORY}:multi");

    let login = docker_login(&engine, &registry, &token);
    assert_eq!(login.status, 0, "the login failed:\n{}", login.said());

    // One image per platform: the index has to carry different
    // images, and one image is distinguished from another by its architecture.
    let context = a_build_context();
    let mut children = Vec::new();
    for architecture in ["amd64", "arm64"] {
        let local = format!(
            "localhost/hsp103-e2e-{port}:{architecture}",
            port = service.port
        );
        let built = the_built_image(&home, &local, Some(architecture), context.path());
        assert_eq!(
            built.status,
            0,
            "the image for {architecture} could not be built:\n{}",
            built.said()
        );

        let (_tar_directory, tar) = the_saved_image(&home, &local);
        let loaded = docker(
            &engine,
            &[
                "load",
                "--input",
                tar.to_str().expect("the path of the tar"),
            ],
        );
        assert_eq!(
            loaded.status,
            0,
            "the image for {architecture} could not be loaded:\n{}",
            loaded.said()
        );

        // Each image is pushed with its own tag: the index names
        // them, and the registry has to have both.
        let reference = format!("{registry}/{FIRST_REPOSITORY}:v1-{architecture}");
        let tagged = docker(&engine, &["tag", &local, &reference]);
        assert_eq!(
            tagged.status,
            0,
            "the image for {architecture} could not be tagged:\n{}",
            tagged.said()
        );
        let pushed = docker(&engine, &["push", &reference]);
        assert_eq!(
            pushed.status,
            0,
            "the image for {architecture} could not be pushed:\n{}",
            pushed.said()
        );
        children.push(reference);
    }

    // The index is assembled **by the CLI**: `docker manifest create` and `docker
    // manifest push` are client code, and they push the index and its
    // children to the registry without going through the daemon. It is the path a
    // real client uses to publish a multi-platform image.
    //
    // The CLI talks to the registry **on its own**, and does so with TLS
    // unless told otherwise: `--insecure` is what tells it that the
    // test registry speaks HTTP.
    let created = docker(
        &engine,
        &[
            "manifest",
            "create",
            "--insecure",
            &index,
            &children[0],
            &children[1],
        ],
    );
    assert_eq!(
        created.status,
        0,
        "the manifest list could not be created:\n{}",
        created.said()
    );

    let pushed = docker(&engine, &["manifest", "push", "--insecure", &index]);
    assert_eq!(
        pushed.status,
        0,
        "the manifest list could not be pushed:\n{}",
        pushed.said()
    );

    // The digest the registry publishes for the index: it is what is
    // compared after the round trip.
    let published = the_published_digest(service.port, FIRST_REPOSITORY, "multi", &token);
    assert!(
        published.starts_with("sha256:"),
        "the pushed index has no digest: {published}"
    );

    // The index is looked at **from the client**, without a daemon: it is the CLI that
    // talks to the registry, and what it sees is what the registry published.
    let inspected = docker(&engine, &["manifest", "inspect", "--insecure", &index]);
    assert_eq!(
        inspected.status,
        0,
        "the manifest list could not be inspected:\n{}",
        inspected.said()
    );
    assert!(
        inspected.stdout.contains("arm64") && inspected.stdout.contains("amd64"),
        "the manifest list does not carry both platforms: {}",
        inspected.stdout
    );

    // And the daemon pulls it: the index resolves to a platform and the
    // image comes back.
    let pulled = docker(&engine, &["pull", &index]);
    assert_eq!(
        pulled.status,
        0,
        "the manifest list could not be pulled:\n{}",
        pulled.said()
    );

    // And what is pulled is **the same** as what was published: the digest of the
    // index, before and after the round trip.
    let after = the_published_digest(service.port, FIRST_REPOSITORY, "multi", &token);
    assert_eq!(
        after, published,
        "what came back is not what went up: {after} != {published}"
    );

    for child in &children {
        let _ = docker(&engine, &["rmi", child]);
    }

    case.outcome = format!(
        "el indice de dos plataformas se empuja y se baja en exit 0, con su plataforma dentro, y el digest vuelve intacto: {published}"
    );
    case.passed = true;
    support::record_oci_e2e(case);
}

// ---------------------------------------------------------------------------
// The `oras`: an artifact, a hanging SBOM and the referrers.
// ---------------------------------------------------------------------------

/// `oras` pushes an artifact, hangs an SBOM from it and discovers the referrers.
///
/// What is measured is the path used by supply chain
/// tools: the artifact is published with its type, the SBOM is published
/// **hanging** from the artifact--with the `subject` that links it-- and the
/// referrers are discovered through the referrers API. An SBOM that is published and
/// cannot be discovered is an SBOM that is of no use.
#[test]
fn the_oras_pushes_an_artifact_attaches_a_sbom_and_discovers_the_referrers() {
    let mut case = the_case(
        "OCI-REFERRERS-01 (oras)",
        "un artefacto con su tipo y un SBOM con el suyo",
        "el oras empuja el artefacto, lo vuelve a bajar, cuelga el SBOM y descubre los referrers",
        "oras discover --plain-http localhost:<port>/releases:v1",
    );
    support::record_oci_e2e(case.clone());

    // The client's home: temporary, so that neither the credential nor the
    // test's images stay on the machine of whoever runs it.
    let home = Home::new();
    let deployment = a_deployment();
    let service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, ALICE);
    let registry = the_registry(service.port);
    let reference = format!("{registry}/{FIRST_REPOSITORY}:v1");
    let oras = support::the_oras();

    // The credential goes on standard input, which is the only path that
    // does not leave it on the command line: a command line is visible in
    // the machine's whole `ps`.
    let logged = oras_login(&home, &oras, &registry, &token);
    assert_eq!(
        logged.status,
        0,
        "the oras login failed:\n{}",
        logged.said()
    );

    // The artifact: a file with its media type. It is published under the
    // name it has **inside the working directory**, because that
    // name is the artifact's title and a title with an absolute
    // path cannot be downloaded inside a directory.
    let files = tempfile::tempdir().expect("a temp dir");
    let artifact = files.path().join("artifact.txt");
    std::fs::write(&artifact, THE_FILE).expect("the artifact is written");
    let pushed = run_in_dir(
        &home,
        &oras,
        &[
            "push",
            "--plain-http",
            "--disable-path-validation",
            &reference,
            "artifact.txt:application/vnd.hodei.artifact.v1",
        ],
        files.path(),
    );
    assert_eq!(
        pushed.status,
        0,
        "the artifact could not be pushed:\n{}",
        pushed.said()
    );

    // And the artifact is pulled again: what was published has to be able to be
    // pulled with its bytes and its type. An artifact that can only be
    // published is an artifact that cannot be used.
    let pulled = tempfile::tempdir().expect("a temp dir");
    let pulled_artifact = run_in(
        &home,
        &oras,
        &[
            "pull",
            "--plain-http",
            "--output",
            pulled.path().to_str().expect("the output directory"),
            &reference,
        ],
    );
    assert_eq!(
        pulled_artifact.status,
        0,
        "the artifact could not be pulled:\n{}",
        pulled_artifact.said()
    );
    let bytes = std::fs::read(pulled.path().join("artifact.txt")).expect("the artifact is there");
    assert_eq!(
        bytes,
        THE_FILE.as_bytes(),
        "what came back is not what went up"
    );

    // The SBOM: hangs from the artifact with its `subject`.
    let sbom = files.path().join("sbom.json");
    std::fs::write(
        &sbom,
        "{\"bomFormat\":\"CycloneDX\",\"specVersion\":\"1.5\"}\n",
    )
    .expect("the sbom is written");
    let attached = run_in_dir(
        &home,
        &oras,
        &[
            "attach",
            "--plain-http",
            "--disable-path-validation",
            "--artifact-type",
            SBOM_TYPE,
            &reference,
            &format!("sbom.json:{SBOM_TYPE}"),
        ],
        files.path(),
    );
    assert_eq!(
        attached.status,
        0,
        "the sbom could not be attached:\n{}",
        attached.said()
    );

    // And the referrers are discovered: the hanging SBOM has to show up.
    let discovered = run_in(
        &home,
        &oras,
        &[
            "discover",
            "--plain-http",
            "--distribution-spec",
            "v1.1-referrers-api",
            &reference,
        ],
    );
    assert_eq!(
        discovered.status,
        0,
        "the referrers could not be discovered:\n{}",
        discovered.said()
    );
    assert!(
        discovered.stdout.contains(SBOM_TYPE),
        "the sbom is not among the referrers:\n{}",
        discovered.said()
    );

    case.outcome = format!(
        "el artefacto se empuja y se baja con sus bytes, el SBOM se cuelga y se descubre: \
         {SBOM_TYPE} aparece entre los referrers",
        SBOM_TYPE = SBOM_TYPE
    );
    case.passed = true;
    support::record_oci_e2e(case);
}

// ---------------------------------------------------------------------------
// The failures, with real clients.
// ---------------------------------------------------------------------------

/// Another account's `podman` cannot publish to the first
/// account's repository.
///
/// What is measured is that the client **fails**: another account's repository
/// does not exist for this one, and the client is left without an image. And that
/// the account that can, still can: isolation is not a door
/// that closes for everyone.
#[test]
fn the_tenant_of_another_account_cannot_publish_in_the_repository_of_the_first() {
    let mut case = the_case(
        "OCI-ISOLATION-01 (podman, otra cuenta)",
        "una imagen local y un repositorio que es de la primera cuenta",
        "el podman de la segunda cuenta intenta publicar y no puede",
        "podman push --authfile <bob> localhost:<port>/private:v1 <local>",
    );
    support::record_oci_e2e(case.clone());

    // The client's home: temporary, so that neither the credential nor the
    // test's images stay on the machine of whoever runs it.
    let home = Home::new();
    let deployment = a_deployment();
    let service = deployment.start(&[]);
    let registry = the_registry(service.port);
    let local = format!("localhost/hsp103-e2e-{port}:v1", port = service.port);

    let context = a_build_context();
    let built = the_built_image(&home, &local, None, context.path());
    assert_eq!(
        built.status,
        0,
        "the image could not be built:\n{}",
        built.said()
    );

    // Each account has its credential, and each in its own file.
    let alice_token = support::token(FIRST_ACCOUNT, ALICE);
    let bob = support::token(SECOND_ACCOUNT, "bob");
    let auth_alice = the_auth_file(&home, &registry, ALICE, &alice_token);
    let auth_alice = auth_alice
        .to_str()
        .expect("the path of the auth file")
        .to_string();
    let auth_bob = the_auth_file(&home, &registry, "bob", &bob);
    let auth_bob = auth_bob
        .to_str()
        .expect("the path of the auth file")
        .to_string();
    let refused = run_in(
        &home,
        &support::the_podman(),
        &[
            "push",
            "--tls-verify=false",
            "--authfile",
            auth_bob.as_str(),
            &local,
            &format!("{registry}/{FIRST_ONLY_REPOSITORY}:v1"),
        ],
    );
    assert_ne!(
        refused.status,
        0,
        "the other tenant could publish in the repository of the first:\n{}",
        refused.said()
    );

    // And the first account can still: isolation is per account,
    // not a door that closes for everyone.
    let allowed = run_in(
        &home,
        &support::the_podman(),
        &[
            "push",
            "--tls-verify=false",
            "--authfile",
            auth_alice.as_str(),
            &local,
            &format!("{registry}/{FIRST_ONLY_REPOSITORY}:v1"),
        ],
    );
    assert_eq!(
        allowed.status,
        0,
        "the first tenant could not publish in its own repository:\n{}",
        allowed.said()
    );

    case.outcome = "la otra cuenta no publica y la primera si".to_string();
    case.passed = true;
    support::record_oci_e2e(case);
}

/// Publishing twice is publishing once.
///
/// What is measured is that the client can **retry**: the second
/// `push` of the same content ends green and the registry keeps
/// publishing the same digest. A client that retries--because of a network
/// outage, a timeout--cannot leave the repository halfway or
/// change what had been published.
#[test]
fn the_publication_is_idempotent_when_the_client_retries() {
    let mut case = the_case(
        "OCI-RETRY-01 (podman, reintento)",
        "una imagen local ya publicada",
        "el cliente la publica otra vez y el registro publica el mismo digest",
        "podman push --authfile <alice> localhost:<port>/releases:v1 <local>",
    );
    support::record_oci_e2e(case.clone());

    // The client's home: temporary, so that neither the credential nor the
    // test's images stay on the machine of whoever runs it.
    let home = Home::new();
    let deployment = a_deployment();
    let service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, ALICE);
    let registry = the_registry(service.port);
    let auth = the_auth_file(&home, &registry, ALICE, &token);
    let auth = auth
        .to_str()
        .expect("the path of the auth file")
        .to_string();
    let local = format!("localhost/hsp103-e2e-{port}:v1", port = service.port);
    let published = format!("{registry}/{FIRST_REPOSITORY}:v1");

    let context = a_build_context();
    let built = the_built_image(&home, &local, None, context.path());
    assert_eq!(
        built.status,
        0,
        "the image could not be built:\n{}",
        built.said()
    );

    let digests = tempfile::tempdir().expect("a temp dir");
    let digest_file = digests.path().join("digest");
    let first = run_in(
        &home,
        &support::the_podman(),
        &[
            "push",
            "--tls-verify=false",
            "--digestfile",
            digest_file.to_str().expect("the path of the digest file"),
            "--authfile",
            auth.as_str(),
            &local,
            &published,
        ],
    );
    assert_eq!(
        first.status,
        0,
        "the image could not be pushed:\n{}",
        first.said()
    );
    let first_digest = std::fs::read_to_string(&digest_file)
        .expect("the digest of the push")
        .trim()
        .to_string();

    // The retry: the same content, the same tag.
    let second = run_in(
        &home,
        &support::the_podman(),
        &[
            "push",
            "--tls-verify=false",
            "--authfile",
            auth.as_str(),
            &local,
            &published,
        ],
    );
    assert_eq!(
        second.status,
        0,
        "the retry could not be pushed:\n{}",
        second.said()
    );

    let published_digest = the_published_digest(service.port, FIRST_REPOSITORY, "v1", &token);
    assert_eq!(
        published_digest, first_digest,
        "the retry published something else: {published_digest} != {first_digest}"
    );

    case.outcome = format!("el reintento deja el mismo digest: {published_digest}");
    case.passed = true;
    support::record_oci_e2e(case);
}

/// Published content survives a restart of the service.
///
/// What is measured is that the content is still there **after** the
/// service dies abruptly and comes back up on the **same** port:
/// the client that looks at the same address again finds what it
/// had published. A restart that loses the catalog or the store
/// turns publication into a tall tale.
#[test]
fn the_published_content_survives_a_restart_of_the_service_on_the_same_port() {
    let mut case = the_case(
        "OCI-DURABILITY-01 (podman, reinicio)",
        "una imagen publicada en un servicio que se apaga de golpe",
        "el servicio vuelve en el mismo puerto y el cliente baja lo mismo",
        "podman pull --authfile <alice> localhost:<port>/releases:v1",
    );
    support::record_oci_e2e(case.clone());

    let home = Home::new();
    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let port = service.port;
    let registry = the_registry(port);
    let token = support::token(FIRST_ACCOUNT, ALICE);
    let auth = the_auth_file(&home, &registry, ALICE, &token);
    let auth = auth
        .to_str()
        .expect("the path of the auth file")
        .to_string();
    let local = format!("localhost/hsp103-e2e-{port}:v1", port = port);

    let context = a_build_context();
    let built = the_built_image(&home, &local, None, context.path());
    assert_eq!(
        built.status,
        0,
        "the image could not be built:\n{}",
        built.said()
    );

    let published = format!("{registry}/{FIRST_REPOSITORY}:v1");
    let pushed = run_in(
        &home,
        &support::the_podman(),
        &[
            "push",
            "--tls-verify=false",
            "--authfile",
            auth.as_str(),
            &local,
            &published,
        ],
    );
    assert_eq!(
        pushed.status,
        0,
        "the image could not be pushed:\n{}",
        pushed.said()
    );
    // What the registry publishes: it is pulled and the digest that
    // comes back is looked at. That digest is the reference against which what
    // comes back **after** the restart is measured.
    let first = run_in(
        &home,
        &support::the_podman(),
        &[
            "pull",
            "--tls-verify=false",
            "--authfile",
            auth.as_str(),
            &published,
        ],
    );
    assert_eq!(
        first.status,
        0,
        "the image could not be pulled before the restart:\n{}",
        first.said()
    );
    let digest = the_digest_of(&home, &published);

    // The service is shut down **abruptly**: a `SIGKILL` is what happens
    // when the machine is switched off, and it leaves no time to clean up anything.
    support::kill(&mut service);

    // And it comes back on the **same** port: the client already knows where it is looking.
    let _service = deployment.start_on(port, &[]);

    // The local image is deleted: what is pulled has to come from the
    // registry, not from what was already on the machine.
    let removed = run_in(&home, &support::the_podman(), &["rmi", &local]);
    assert_eq!(
        removed.status,
        0,
        "the image could not be removed:\n{}",
        removed.said()
    );

    let pulled = run_in(
        &home,
        &support::the_podman(),
        &[
            "pull",
            "--tls-verify=false",
            "--authfile",
            auth.as_str(),
            &published,
        ],
    );
    assert_eq!(
        pulled.status,
        0,
        "the image could not be pulled after the restart:\n{}",
        pulled.said()
    );
    let after = the_digest_of(&home, &published);
    assert_eq!(
        after, digest,
        "what came back after the restart is not what went up: {after} != {digest}"
    );

    case.outcome = format!("el mismo digest despues del reinicio: {after}");
    case.passed = true;
    support::record_oci_e2e(case);
}

// ---------------------------------------------------------------------------
// The repository with levels.
// ---------------------------------------------------------------------------

/// A repository **with levels** is published with its client.
///
/// The name with levels--`team/demo`-- is the case the authorization
/// schema did not understand: the authorization engine declared the repository's
/// parent--`team`-- and the schema did not allow a
/// `Repository` to be a member of another `Repository`, so the engine
/// failed closed and the repository was answered with a `403 DENIED`.
///
/// What is measured is that the **real** client publishes to it and that what it
/// publishes can be pulled again: without this case, nobody checks the
/// schema fix with a real client.
#[test]
fn the_repository_with_levels_is_published_by_its_own_account() {
    let mut case = the_case(
        "OCI-HIERARCHY-01 (podman, niveles)",
        "un repositorio con niveles y la credencial de su cuenta",
        "el podman publica en el y lo vuelve a bajar",
        "podman push --authfile <alice> --tls-verify=false localhost:<port>/team/demo:v1 <local>",
    );
    support::record_oci_e2e(case.clone());

    let home = Home::new();
    let deployment = Deployment::new();
    deployment.seed_with(&[THE_NESTED_REPOSITORY]);
    let service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, ALICE);
    let registry = the_registry(service.port);
    let auth = the_auth_file(&home, &registry, ALICE, &token);
    let auth = auth
        .to_str()
        .expect("the path of the auth file")
        .to_string();
    let local = format!("localhost/hsp103-e2e-{port}:v1", port = service.port);

    let context = a_build_context();
    let built = the_built_image(&home, &local, None, context.path());
    assert_eq!(
        built.status,
        0,
        "the image could not be built:\n{}",
        built.said()
    );

    let published = format!("{registry}/{THE_NESTED_REPOSITORY}:v1");
    let pushed = run_in(
        &home,
        &support::the_podman(),
        &[
            "push",
            "--tls-verify=false",
            "--authfile",
            auth.as_str(),
            &local,
            &published,
        ],
    );
    assert_eq!(
        pushed.status,
        0,
        "the image could not be pushed in the repository with levels:\n{}",
        pushed.said()
    );

    // And what was published can be pulled again: the `403` this
    // repository suffered was an authorization one, not a protocol one.
    let removed = run_in(&home, &support::the_podman(), &["rmi", &local]);
    assert_eq!(
        removed.status,
        0,
        "the image could not be removed:\n{}",
        removed.said()
    );

    let pulled = run_in(
        &home,
        &support::the_podman(),
        &[
            "pull",
            "--tls-verify=false",
            "--authfile",
            auth.as_str(),
            &published,
        ],
    );
    assert_eq!(
        pulled.status,
        0,
        "the image could not be pulled from the repository with levels:\n{}",
        pulled.said()
    );

    let digest = the_digest_of(&home, &published);
    assert!(
        digest.starts_with("sha256:"),
        "the pulled image has no digest: {digest}"
    );

    let _ = run_in(&home, &support::the_podman(), &["rmi", &published]);

    case.outcome =
        format!("el repositorio con niveles se publica y se baja con su digest: {digest}");
    case.passed = true;
    support::record_oci_e2e(case);
}

// ---------------------------------------------------------------------------
// The official conformance suite.
// ---------------------------------------------------------------------------
/// The official conformance suite's binary.
///
/// It is compiled by `scripts/oci-conformance.sh`, which leaves it in `target/`. If it is not
/// there, the official suite has **not** been run: and a suite that has not been
/// run is not a suite that passes, it is a suite that has not been looked at.
fn the_conformance_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("oci-conformance")
        .join("oci-conformance")
}

/// Where the suite leaves its results.
///
/// It is not a temporary directory: the official suite's results are the
/// **evidence** of the compatibility declaration, and evidence that is
/// deleted when the test ends cannot be archived or reviewed.
fn the_conformance_results() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("uat")
        .join("oci-conformance")
}

/// The official suite's categories, with their status.
///
/// The suite prints two lists--an **API** one and a **data** one-- and each
/// line is the category's name and its status, with filler dots
/// in between.
fn the_categories(stdout: &str, section: &str) -> Vec<(String, String)> {
    let mut categories = Vec::new();
    let mut inside = false;
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed == section {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, status)) = trimmed.rsplit_once(':') {
            let name = name.trim().trim_end_matches('.').trim().to_string();
            categories.push((name, status.trim().to_string()));
        }
    }
    categories
}

/// The official conformance suite is run against the registry.
///
/// What is declared is what has been run: the categories the
/// suite marks as conformant, by name. Those that fail are noted with
/// their reason and their action, and are **not** declared: declaring them would be
/// propaganda.
#[test]
fn the_official_conformance_suite_runs_the_declared_categories() {
    let mut case = the_case(
        "OCI-CONFORM-01 (suite oficial)",
        "la suite oficial del distribution-spec compilada y dos repositorios sembrados",
        "la suite empuja, lista, baja y pregunta por los referrers",
        "oci-conformance (OCI_REGISTRY=localhost:<port>)",
    );
    support::record_oci_e2e(case.clone());

    let binary = the_conformance_binary();
    assert!(
        binary.exists(),
        "the official conformance suite is not built: run scripts/oci-conformance.sh"
    );

    let deployment = Deployment::new();
    deployment.seed_with(&[CONFORMANCE_REPOSITORY, CONFORMANCE_REPOSITORY_TWO]);
    let service = deployment.start(&[]);
    let token = support::token(FIRST_ACCOUNT, ALICE);
    let results = the_conformance_results();
    std::fs::create_dir_all(&results).expect("the results directory");

    let output = Command::new(&binary)
        .env("OCI_REGISTRY", the_registry(service.port))
        .env("OCI_TLS", "disabled")
        .env("OCI_REPO1", CONFORMANCE_REPOSITORY)
        .env("OCI_REPO2", CONFORMANCE_REPOSITORY_TWO)
        .env("OCI_USERNAME", ALICE)
        .env("OCI_PASSWORD", &token)
        .env("OCI_VERSION", "1.1")
        .env("OCI_DATA_SHA512", "false")
        .env("OCI_API_BLOBS_UPLOAD_CANCEL", "true")
        .env("OCI_RESULTS_DIR", &results)
        .env("OCI_LOG", "warn")
        .output()
        .expect("the conformance suite runs");

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    println!("{stdout}");
    println!("{stderr}");

    // The suite's exit code is the **whole** of the run: the
    // suite exits with 1 as soon as a category--declared or not-- fails or
    // errors, because its overall status is the worst of its categories. Without
    // this check, a failure in a category that is not declared
    // left the pipeline green with a "0 red" that nobody looked at.
    assert!(
        output.status.success(),
        "the conformance suite reported a failure:\n{stdout}\n{stderr}"
    );

    let api = the_categories(&stdout, "API conformance:");
    let data = the_categories(&stdout, "Data conformance:");
    assert!(
        !api.is_empty(),
        "the suite did not report its API categories:\n{stdout}"
    );
    assert!(
        !data.is_empty(),
        "the suite did not report its data categories:\n{stdout}"
    );

    for (name, status) in api.iter().chain(data.iter()) {
        println!("{name}: {status}");
    }

    // What is declared is checked: each declared category has to
    // be green, under the name the declaration gives.
    for (declared, reported) in DECLARED_API
        .iter()
        .map(|name| (name, api.iter().find(|(reported, _)| reported == name)))
        .chain(
            DECLARED_DATA
                .iter()
                .map(|name| (name, data.iter().find(|(reported, _)| reported == name))),
        )
    {
        let (reported, status) = reported.unwrap_or_else(|| {
            panic!("the suite did not report the declared category {declared}:\n{stdout}")
        });
        assert_eq!(
            status, "Pass",
            "the declared category {declared} is not conformant: {reported} is {status}"
        );
    }

    // And what is not declared is not claimed: the other categories are
    // noted with their status, so that the report also says what the
    // registry is **not**.
    let others: Vec<String> = api
        .iter()
        .chain(data.iter())
        .filter(|(name, _)| {
            !DECLARED_API.contains(&name.as_str()) && !DECLARED_DATA.contains(&name.as_str())
        })
        .map(|(name, status)| format!("{name}: {status}"))
        .collect();

    // The numbers go to the report **measured**, not written: the
    // suite counts them in its JUnit report, which is what is archived.
    let totals = the_totals(&results.join("junit.xml"));
    case.outcome = format!(
        "{} categorias declaradas en verde; {} pruebas en total: {} en verde, {} omitidas, \
         {} en rojo; el resto: {}",
        DECLARED_API.len() + DECLARED_DATA.len(),
        totals.0,
        totals.1,
        totals.2,
        totals.3 + totals.4,
        others.join(", ")
    );
    case.passed = true;
    support::record_oci_e2e(case);
}

/// The numbers the suite counts in its JUnit report.
///
/// They are `(tests, green, skipped, failures, errors)`, and they are the
/// numbers the compatibility contract declares: reading them from the
/// run's report is what keeps them from being a hand-written
/// number.
fn the_totals(report: &std::path::Path) -> (usize, usize, usize, usize, usize) {
    let text = std::fs::read_to_string(report).expect("the junit report is there");
    let number = |name: &str| -> usize {
        let needle = format!("{name}=\"");
        let Some(position) = text.find(&needle) else {
            return 0;
        };
        let rest = &text[position + needle.len()..];
        let end = rest.find('"').expect("the attribute closes");
        rest[..end].parse().unwrap_or(0)
    };
    (
        number("tests"),
        number("tests") - number("skipped") - number("failures") - number("errors"),
        number("skipped"),
        number("failures"),
        number("errors"),
    )
}

/// The **API** categories the registry declares.
///
/// What is declared is what has been run and came out green. A
/// category that is not here is not declared: declaring it would be
/// propaganda, and the compatibility contract also says what the
/// registry is **not**.
const DECLARED_API: [&str; 19] = [
    "Ping",
    "Tag listing",
    "Blob push",
    "Blob post only",
    "Blob post put",
    "Blob chunked",
    "Blob streaming",
    "Blob mount",
    "Blob upload cancel",
    "Blob get",
    "Blob head",
    "Manifest put by digest",
    "Manifest put by tag",
    "Manifest put with subject",
    "Manifest get by digest",
    "Manifest get by tag",
    "Manifest head by digest",
    "Manifest head by tag",
    "Referrers",
];

/// The **data** categories the registry declares.
///
/// The `sha512` data set is left **out**: the content digest
/// is `sha256` and only `sha256`, and the suite is not run with
/// an algorithm this registry does not speak. The reason and the action are
/// in the compatibility contract.
const DECLARED_DATA: [&str; 19] = [
    "Image",
    "Image Uncompressed",
    "Image with Large Manifest",
    "Index",
    "Nested Index",
    "Index with Subject",
    "Empty Index",
    "Artifact",
    "Artifact Index",
    "Artifact without Layers",
    "Artifacts with Subject",
    "Missing Subject",
    "Bad Digest Image",
    "Invalid Manifest Digest",
    "Custom Fields",
    "Data Field",
    "No Layers",
    "Non-distributable Layers",
    "Blobs sha256",
];
