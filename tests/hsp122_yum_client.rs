//! El `dnf install` de HSP-122 contra el **cliente real**: un
//! `quay.io/fedora/fedora:latest` en podman cuyo `.repo` apunta al hub.
//!
//! La distro esta **fijada** en una imagen concreta, que es lo que pide
//! el criterio de aceptacion: el `dnf` y el `rpm` de Fedora, con
//! `repo_gpgcheck=1`. El flujo es el de cualquier sysadmin:
//!
//! 1. dentro del contenedor se construye un `.rpm` real con
//!    `rpmbuild` (los nuestros, sinteticos, no instalarian);
//! 2. el hub lo publica por `PUT /yum/{repo}/pool/...`, cuya ruta
//!    dicta el header del propio binario;
//! 3. `dnf -y install hello-hodei` tiene que descargar el `repomd.xml`,
//!    **verificar su firma** contra la `key.asc` del hub, bajar el
//!    `primary.xml.gz`, comprobar el SHA256 del paquete contra el
//!    indice e instalarlo;
//! 4. con la clave de un tercero, la instalacion tiene que negarse.
//!
//! Sin podman o sin la imagen, la suite falla diciendo que falta, como
//! las demas suites de cliente real.

mod support;

use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

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

const IMAGE: &str = "quay.io/fedora/fedora:latest";

fn require_podman() {
    let found = Command::new("podman")
        .args(["--version"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    assert!(
        found,
        "podman is not installed: the yum client suite needs a fedora container"
    );
    let image = Command::new("podman")
        .args(["image", "exists", IMAGE])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    assert!(
        image,
        "the image {IMAGE} is not available: pull it before running the yum client suite"
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
        .arg("bash")
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

/// El spec del paquete real: `rpmbuild` lo exige bien formado. La ruta
/// del `.repo` del hub se construye aqui con credenciales en la
/// userinfo, que es como dnf espera la autenticacion basica.
const RPM_SPEC: &str = concat!(
    "Name: hello-hodei\n",
    "Version: 1.0.0\n",
    "Release: 1\n",
    "Summary: A tiny hello package\n",
    "License: MIT\n",
    "BuildRoot: %{_tmppath}/hello-hodei-root\n",
    "%description\n",
    "Installed through the real dnf client against Hodei.\n",
    "%install\n",
    "mkdir -p %{buildroot}/usr/share/hello-hodei\n",
    "cp /work/greeting.txt %{buildroot}/usr/share/hello-hodei/greeting.txt\n",
    "%files\n",
    "/usr/share/hello-hodei/greeting.txt\n",
);

const POOL_PATH: &str = "pool/hello-hodei-1.0.0-1.x86_64.rpm";

fn repo_file(port: u16, token: &str, key_url: &str) -> String {
    format!(
        concat!(
            "[hodei]\n",
            "name=Hodei fedora hosted\n",
            "baseurl=http://alice:{token}@127.0.0.1:{port}/yum/fedora-hosted/\n",
            "enabled=1\n",
            "gpgcheck=0\n",
            "repo_gpgcheck=1\n",
            "gpgkey={key_url}\n",
        ),
        token = token,
        port = port,
        key_url = key_url,
    )
}

#[test]
fn the_real_dnf_client_installs_and_verifies_the_signed_metadata() {
    require_podman();
    let deployment = Deployment::new();
    deployment.seed_yum(&[("fedora-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    let work = next("stage");
    std::fs::create_dir_all(&work).expect("the scratch dir");
    std::fs::write(work.join("hello.spec"), RPM_SPEC).expect("the spec");
    std::fs::write(work.join("greeting.txt"), "hola desde hodei\n").expect("the payload");

    // 1. Un `.rpm` de verdad, construido por el rpm de la distro fijada.
    container_run(
        &work,
        concat!(
            "set -euo pipefail\n",
            "dnf -y -q install rpm-build gnupg2 >/dev/null\n",
            "mkdir -p /work/rpmbuild/{BUILD,RPMS,SOURCES,SPECS,SRPMS}\n",
            "rpmbuild --define '_topdir /work/rpmbuild' -bb /work/hello.spec >/dev/null\n",
            "chmod 644 /work/rpmbuild/RPMS/x86_64/*.rpm\n",
        ),
    );
    let rpm = std::fs::read(work.join("rpmbuild/RPMS/x86_64/hello-hodei-1.0.0-1.x86_64.rpm"))
        .expect("the built rpm");
    assert!(!rpm.is_empty(), "rpmbuild produced a package");

    // 2. Se publica por la superficie yum del hub: la ruta del pool es
    //    la que dicta el header del binario construido fuera de aqui.
    let published = call(
        port,
        "PUT",
        &format!("/yum/fedora-hosted/{POOL_PATH}"),
        &token,
        Some(&rpm),
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));

    // 3. La clave publica del hub, en armadura, para el gpgkey del repo.
    let key = call(port, "GET", "/yum/fedora-hosted/key.asc", &token, None);
    assert_eq!(key.status, 200, "{}", text_of(&key));
    std::fs::write(work.join("key.asc"), key.body.as_slice()).expect("the key");

    // 4. El gestor real de la distro fijada. `dnf` (dnf5) instala el
    //    paquete comprobando el sha256 del indice, y `microdnf`, el otro
    //    cliente oficial de Fedora, es quien **aprieta** la firma del
    //    repomd: dnf5 5.4.2 avisa "repomd.xml GPG signature verification
    //    error" y continua, microdnf se niega. Ambos flujos se prueban.
    let script = format!(
        concat!(
            "set -euo pipefail\n",
            "dnf -y -q install gnupg2 microdnf >/dev/null\n",
            "cat > /etc/yum.repos.d/hodei.repo <<'EOF'\n",
            "{repo}\n",
            "EOF\n",
            "dnf -y -q --disablerepo='*' --enablerepo=hodei install hello-hodei\n",
            "rpm -q hello-hodei\n",
            "cat /usr/share/hello-hodei/greeting.txt\n",
            "rpm -e hello-hodei\n",
            "microdnf --disablerepo='*' --enablerepo=hodei --refresh -y install hello-hodei\n",
            "rpm -q hello-hodei\n",
            "cat /usr/share/hello-hodei/greeting.txt\n",
        ),
        repo = repo_file(port, &token, "file:///work/key.asc"),
    );
    let installed = container_run(&work, &script);
    let stdout = String::from_utf8_lossy(&installed.stdout);
    let occurrences = stdout.matches("hello-hodei-1.0.0-1.x86_64").count();
    assert!(
        occurrences >= 2,
        "dnf and microdnf must each report the package installed: {stdout}"
    );
    assert!(
        stdout.matches("hola desde hodei").count() >= 2,
        "both clients must land the published payload: {stdout}"
    );
}

#[test]
fn dnf_refuses_the_metadata_signed_by_a_stranger_key() {
    require_podman();
    let deployment = Deployment::new();
    deployment.seed_yum(&[("fedora-hosted", Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    let work = next("stage-stranger");
    std::fs::create_dir_all(&work).expect("the scratch dir");
    std::fs::write(work.join("hello.spec"), RPM_SPEC).expect("the spec");
    std::fs::write(work.join("greeting.txt"), "hola desde hodei\n").expect("the payload");
    container_run(
        &work,
        concat!(
            "set -euo pipefail\n",
            "dnf -y -q install rpm-build >/dev/null\n",
            "mkdir -p /work/rpmbuild/{BUILD,RPMS,SOURCES,SPECS,SRPMS}\n",
            "rpmbuild --define '_topdir /work/rpmbuild' -bb /work/hello.spec >/dev/null\n",
            "chmod 644 /work/rpmbuild/RPMS/x86_64/*.rpm\n",
        ),
    );
    let rpm = std::fs::read(work.join("rpmbuild/RPMS/x86_64/hello-hodei-1.0.0-1.x86_64.rpm"))
        .expect("the built rpm");
    let published = call(
        port,
        "PUT",
        &format!("/yum/fedora-hosted/{POOL_PATH}"),
        &token,
        Some(&rpm),
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));

    // Un tercero que no es el firmante del hub. Con su clave fijada en
    // el repo, la metadata del hub no abre: microdnf —el cliente que
    // aprieta `repo_gpgcheck`— tiene que negarse, no instalar a ciegas.
    let stranger =
        hodei_artifacts_api::openpgp::MetadataSigner::generate("Stranger <x@example.local>")
            .expect("a second key");
    std::fs::write(
        work.join("stranger.asc"),
        stranger.key_asc().expect("an armored key"),
    )
    .expect("the stranger key");

    let script = format!(
        concat!(
            "dnf -y -q install gnupg2 microdnf >/dev/null\n",
            "cat > /etc/yum.repos.d/hodei.repo <<'EOF'\n",
            "{repo}\n",
            "EOF\n",
            "if microdnf --disablerepo='*' --enablerepo=hodei --refresh -y install hello-hodei ",
            ">/tmp/dnf.log 2>&1; then echo ACCEPTED; exit 1; fi\n",
            "cat /tmp/dnf.log; echo refused\n",
            "if rpm -q hello-hodei >/dev/null 2>&1; then echo INSTANYWAY; exit 1; fi\n",
        ),
        repo = repo_file(port, &token, "file:///work/stranger.asc"),
    );
    let refused = container_run(&work, &script);
    let stdout = String::from_utf8_lossy(&refused.stdout);
    assert!(
        stdout.contains("refused"),
        "microdnf must reject a repomd whose signature does not open with the pinned key"
    );
    assert!(
        stdout
            .to_lowercase()
            .contains("signature verification error"),
        "the refusal must name the metadata signature, not a missing package: {stdout}"
    );
}
