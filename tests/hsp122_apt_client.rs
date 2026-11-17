//! El `apt-get install` de HSP-122 contra el **cliente real**: un
//! `debian:bookworm` en podman cuyo `sources.list` apunta al hub.
//!
//! La distro esta **fijada**, que es lo que pide el criterio de
//! aceptacion: `apt 2.6.1`, `dpkg 1.21.x`, `gpgv` incluido. El flujo es
//! el de cualquier sysadmin:
//!
//! 1. dentro del contenedor se construye un `.deb` real con
//!    `dpkg-deb --build` (los nuestros, sinteticos, no instalarian);
//! 2. el hub lo publica por `PUT /apt/{repo}/pool/...`;
//! 3. la clave del hub (`key.asc`) se dearma y se instala como
//!    `signed-by`, la ruta documentada por Debian;
//! 4. `apt-get update` tiene que **verificar la firma del Release** con
//!    gpgv para no marcar el origen como no firmado, y
//!    `apt-get install -y hello-hodei` tiene que bajar el paquete del
//!    pool, comprobar su SHA256 contra el indice e instalarlo.
//!
//! Sin podman o sin la imagen, la suite falla diciendo que falta, como
//! las demas suites de cliente real.

mod support;

use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use base64::Engine as _;
use serde_json::Value;
use support::{Answer, Curl, Deployment};

#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

static CALLS: AtomicUsize = AtomicUsize::new(0);

fn next(name: &str) -> std::path::PathBuf {
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{n}"))
}

const IMAGE: &str = "docker.io/library/debian:bookworm";

fn require_podman() {
    let found = Command::new("podman")
        .args(["--version"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "podman is not installed: the apt client suite needs a debian container"
    );
    let image = Command::new("podman")
        .args(["image", "exists", IMAGE])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    assert!(
        image,
        "the image {IMAGE} is not available: pull it before running the apt client suite"
    );
}

/// Ejecuta un script dentro del contenedor con `work` montado en `/work`
/// y red de host para alcanzar al hub en el 127.0.0.1.
fn container_run(work: &Path, script: &str) -> std::process::Output {
    let output = Command::new("podman")
        .arg("run")
        .arg("--rm")
        .arg("--network=host")
        .arg("-v")
        .arg(format!("{}:/work:z", work.display()))
        .arg(IMAGE)
        .arg("sh")
        .arg("-c")
        .arg(script)
        .output()
        .expect("the container runs");
    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        panic!("the container failed ({script}):\n{stdout}\n{stderr}");
    }
    output
}

fn call(port: u16, method: &str, path: &str, token: &str, body: Option<&[u8]>) -> Answer {
    let sent = body.map(|bytes| {
        let target = next("sent");
        std::fs::write(&target, bytes).expect("the body is written");
        target
    });
    let into = next("answer");
    let headers: &[(&str, &str)] = if sent.is_some() {
        &[("content-type", "application/octet-stream")]
    } else {
        &[]
    };
    Curl::call_with_headers(
        port,
        method,
        path,
        token,
        sent.as_deref(),
        headers,
        Some(&into),
    )
    .run()
}

fn text_of(answer: &Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

/// El contenido binario de un bloque armor OpenPGP.
fn dearmor(armored: &str) -> Vec<u8> {
    let body: String = armored
        .lines()
        .skip_while(|line| !line.starts_with("-----BEGIN"))
        .skip(1)
        .take_while(|line| !line.starts_with("-----END"))
        .filter(|line| !(line.trim().is_empty() || line.starts_with('=')))
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect();
    base64::engine::general_purpose::STANDARD
        .decode(&body)
        .expect("the armored key is base64")
}

const POOL_PATH: &str = "pool/main/hello-hodei/hello-hodei_1.0.0_amd64.deb";

/// El control del paquete real: `dpkg-deb` lo exige bien formado.
const DEBIAN_CONTROL: &str = concat!(
    "Package: hello-hodei\n",
    "Version: 1.0.0\n",
    "Architecture: amd64\n",
    "Maintainer: Hodei <hodei@example.local>\n",
    "Priority: optional\n",
    "Section: utils\n",
    "Description: A tiny hello package\n",
    " Installed through the real apt client against Hodei.\n",
);

#[test]
fn the_real_apt_client_installs_and_verifies_the_signed_metadata() {
    require_podman();
    let deployment = Deployment::new();
    deployment.seed_apt(&[("debian-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    let work = next("stage");
    std::fs::create_dir_all(work.join("pkg/DEBIAN")).expect("the package tree");
    std::fs::write(work.join("pkg/DEBIAN/control"), DEBIAN_CONTROL).expect("the control");
    let payload = work.join("pkg/usr/share/hello-hodei");
    std::fs::create_dir_all(&payload).expect("the payload dir");
    std::fs::write(payload.join("greeting.txt"), "hola desde hodei\n").expect("the payload");

    // 1. Un `.deb` de verdad, construido por el dpkg de la distro fijada.
    container_run(
        &work,
        "cd /work && dpkg-deb --build --root-owner-group pkg hello.deb && chmod 644 hello.deb",
    );
    let deb = std::fs::read(work.join("hello.deb")).expect("the built deb");
    assert!(!deb.is_empty(), "dpkg-deb produced a package");

    // 2. Se publica por la superficie apt del hub.
    let published = call(
        port,
        "PUT",
        &format!("/apt/debian-hosted/{POOL_PATH}"),
        &token,
        Some(&deb),
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));

    // 3. La clave del hub, dearma como manda la documentacion de Debian.
    let key = call(port, "GET", "/apt/debian-hosted/key.asc", &token, None);
    assert_eq!(key.status, 200, "{}", text_of(&key));
    std::fs::write(work.join("key.gpg"), dearmor(&text_of(&key))).expect("the keyring");

    // 4. El apt real: update verifica la firma del Release con gpgv e
    // install baja el paquete del pool comprobando su SHA256.
    let script = format!(
        concat!(
            "set -e\n",
            "rm -f /etc/apt/sources.list.d/debian.sources\n",
            "printf 'deb [signed-by=/work/key.gpg] http://alice:{token}@127.0.0.1:{port}/apt/debian-hosted stable main\\n' ",
            "> /etc/apt/sources.list.d/hodei.list\n",
            "apt-get update -qq\n",
            "apt-get install -y -qq hello-hodei\n",
            "dpkg-query -W -f='${{Status}} ${{Package}}\\n' hello-hodei\n",
            "cat /usr/share/hello-hodei/greeting.txt\n",
        ),
        token = token,
        port = port,
    );
    let installed = container_run(&work, &script);
    let stdout = String::from_utf8_lossy(&installed.stdout);
    assert!(
        stdout.contains("install ok installed hello-hodei"),
        "dpkg must report the package installed: {stdout}"
    );
    assert!(
        stdout.contains("hola desde hodei"),
        "the installed payload must be the published one: {stdout}"
    );
}

#[test]
fn apt_refuses_the_metadata_signed_by_an_stranger_key() {
    require_podman();
    let deployment = Deployment::new();
    deployment.seed_apt(&[("debian-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    let work = next("stage-stranger");
    std::fs::create_dir_all(&work).expect("the scratch dir");
    // Un anillo de un tercero que no es el firmante del hub: con el,
    // `apt-get update` tiene que negarse, no instalar a ciegas.
    let stranger =
        hodei_artifacts_api::openpgp::MetadataSigner::generate("Stranger <x@example.local>")
            .expect("a second key");
    std::fs::write(work.join("key.gpg"), stranger.keyring().expect("a keyring"))
        .expect("the stranger keyring");

    let script = format!(
        concat!(
            "rm -f /etc/apt/sources.list.d/debian.sources\n",
            "printf 'deb [signed-by=/work/key.gpg] http://alice:{token}@127.0.0.1:{port}/apt/debian-hosted stable main\\n' ",
            "> /etc/apt/sources.list.d/hodei.list\n",
            "if apt-get update >/tmp/update.log 2>&1; then echo ACCEPTED; exit 1; fi\n",
            "cat /tmp/update.log; echo refused\n",
        ),
        token = token,
        port = port,
    );
    let refused = container_run(&work, &script);
    let stdout = String::from_utf8_lossy(&refused.stdout);
    assert!(
        stdout.contains("refused"),
        "apt must reject a Release whose signature does not open with the pinned key"
    );
}
