//! HSP-122 hosted APT, at the HTTP level.
//!
//! Tested: the upload of a `.deb` with the pool grammar, the
//! assembly of the per-architecture `Packages` indexes, the reproducible aggregate
//! `Release`, its `Release.gpg` signature verified against the served
//! `key.asc` with the **`gpgv` oracle** (the same verifier that
//! apt invokes), the ETag of conditional reads and the immutability
//! of the version.
//!
//! The real client (`apt-get`) lives in `hsp122_apt_client.rs`.

mod support;

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use base64::Engine as _;
use sha2::{Digest, Sha256};
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

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn call(
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    body: Option<&[u8]>,
    extra: &[(&str, &str)],
) -> Answer {
    if method == "HEAD" {
        return Curl::describe(port, path, token).run();
    }
    let sent = body.map(|bytes| {
        let target = next("sent");
        std::fs::write(&target, bytes).expect("the body is written");
        target
    });
    let into = next("answer");
    let mut headers: Vec<(&str, &str)> = extra.to_vec();
    if sent.is_some() {
        headers.push(("content-type", "application/octet-stream"));
    }
    Curl::call_with_headers(
        port,
        method,
        path,
        token,
        sent.as_deref(),
        &headers,
        Some(&into),
    )
    .run()
}

fn text_of(answer: &Answer) -> String {
    String::from_utf8_lossy(&answer.body).into_owned()
}

// ---------------------------------------------------------------------------
// A minimal .deb faithful to the format: `ar` container with debian-binary,
// control.tar (ustar tar with ./control) and an empty data.tar.
// ---------------------------------------------------------------------------

fn ar_header(name: &str, size: usize) -> Vec<u8> {
    let mut header = format!("{name:<16}0           0     0     644      {size:<10}`\n");
    if header.len() != 60 {
        header.truncate(60);
    }
    header.into_bytes()
}

fn tar_block(name: &str, data: &[u8]) -> Vec<u8> {
    let mut block = vec![0u8; 512];
    block[0..name.len()].copy_from_slice(name.as_bytes());
    let size = format!("{:07o}\0", data.len());
    block[124..132].copy_from_slice(size.as_bytes());
    block[156] = b'0';
    block.extend_from_slice(data);
    let pad = (512 - data.len() % 512) % 512;
    block.extend(std::iter::repeat_n(0, pad));
    block
}

fn build_deb(control_text: &str) -> Vec<u8> {
    let mut control_tar = tar_block("./control", control_text.as_bytes());
    control_tar.extend(vec![0u8; 1024]);
    let data_tar = vec![0u8; 1024];

    let mut deb = Vec::new();
    deb.extend_from_slice(b"!<arch>\n");
    for (name, body) in [
        ("debian-binary", b"2.0\n".to_vec()),
        ("control.tar", control_tar),
        ("data.tar", data_tar),
    ] {
        deb.extend_from_slice(&ar_header(name, body.len()));
        deb.extend_from_slice(&body);
        if body.len() % 2 == 1 {
            deb.push(b'\n');
        }
    }
    deb
}

fn hello_deb() -> Vec<u8> {
    build_deb(concat!(
        "Package: hello-hodei\n",
        "Version: 1.0.0\n",
        "Architecture: amd64\n",
        "Maintainer: Hodei <hodei@example.local>\n",
        "Depends: libc6 (>= 2.34)\n",
        "Section: utils\n",
        "Priority: optional\n",
        "Description: A tiny hello package\n",
        " Installed through the real apt client.\n",
    ))
}

fn tools_deb() -> Vec<u8> {
    build_deb(concat!(
        "Package: hodei-tools\n",
        "Version: 0.2.0-1\n",
        "Architecture: all\n",
        "Maintainer: Hodei <hodei@example.local>\n",
        "Description: Architecture independent tools\n",
        " No binaries, no architecture.\n",
    ))
}

const HELLO_PATH: &str = "pool/main/hello-hodei/hello-hodei_1.0.0_amd64.deb";
const TOOLS_PATH: &str = "pool/main/hodei-tools/hodei-tools_0.2.0-1_all.deb";

#[test]
fn a_deb_is_published_indexed_and_its_metadata_signature_verifies() {
    let deployment = Deployment::new();
    deployment.seed_apt(&[("debian-hosted", serde_json::Value::Null)]);
    let token = support::token(support::FIRST_ACCOUNT, "alice");
    let service = deployment.start(&[]);
    let port = service.port;

    let hello = hello_deb();
    let hello_sha = sha256_hex(&hello);

    // The path is dictated by the control file: uploading to whatever place one likes is a no.
    let misplaced = call(
        port,
        "PUT",
        "/apt/debian-hosted/pool/main/otra-cosa.deb",
        &token,
        Some(&hello),
        &[],
    );
    assert_eq!(misplaced.status, 409, "{}", text_of(&misplaced));
    assert!(
        text_of(&misplaced).contains(HELLO_PATH),
        "the conflict must name the path the grammar demands: {}",
        text_of(&misplaced)
    );

    // A body that is not an `ar` container does not enter the pool.
    let junk = call(
        port,
        "PUT",
        &format!("/apt/debian-hosted/{HELLO_PATH}"),
        &token,
        Some(b"this is not an ar archive"),
        &[],
    );
    assert_eq!(junk.status, 400, "{}", text_of(&junk));

    let published = call(
        port,
        "PUT",
        &format!("/apt/debian-hosted/{HELLO_PATH}"),
        &token,
        Some(&hello),
        &[],
    );
    assert_eq!(published.status, 200, "{}", text_of(&published));

    // The binary is served byte for byte through the index path.
    let download = call(
        port,
        "GET",
        &format!("/apt/debian-hosted/{HELLO_PATH}"),
        &token,
        None,
        &[],
    );
    assert_eq!(download.status, 200, "{}", text_of(&download));
    assert_eq!(download.body, hello);

    // The amd64 index describes the package with the sum and size
    // that apt will verify after the download.
    let packages = call(
        port,
        "GET",
        "/apt/debian-hosted/dists/stable/binary-amd64/Packages",
        &token,
        None,
        &[],
    );
    assert_eq!(packages.status, 200, "{}", text_of(&packages));
    let index = text_of(&packages);
    assert!(index.starts_with("Package: hello-hodei\nVersion: 1.0.0\nArchitecture: amd64\n"));
    assert!(index.contains(&format!("Filename: {HELLO_PATH}\n")));
    assert!(index.contains(&format!("SHA256: {hello_sha}\n")));
    assert!(index.contains(&format!("Size: {}\n", hello.len())));
    assert!(index.contains("Depends: libc6 (>= 2.34)\n"));
    // Records end in a blank line: apt separation.
    assert!(index.ends_with("\n\n"));

    // arm64 does not see the amd64 package.
    let arm = call(
        port,
        "GET",
        "/apt/debian-hosted/dists/stable/binary-arm64/Packages",
        &token,
        None,
        &[],
    );
    assert_eq!(arm.status, 200, "{}", text_of(&arm));
    assert!(
        text_of(&arm).is_empty(),
        "arm64 must not list amd64 packages"
    );

    // The Release mentions both indexes with the exact sum of the
    // body that is served, and its date is from the state, not from the clock.
    let release = call(
        port,
        "GET",
        "/apt/debian-hosted/dists/stable/Release",
        &token,
        None,
        &[],
    );
    assert_eq!(release.status, 200, "{}", text_of(&release));
    let release_text = text_of(&release);
    assert!(release_text.starts_with("Origin: Hodei\nSuite: stable\nCodename: stable\nDate: "));
    assert!(release_text.contains(" UTC\n"));
    assert!(release_text.contains("Architectures: amd64 arm64\n"));
    assert!(release_text.contains("Components: main\nSHA256:\n"));
    let index_sha = sha256_hex(&packages.body);
    assert!(
        release_text.contains(&format!(
            " {index_sha} {} main/binary-amd64/Packages\n",
            packages.body.len()
        )),
        "Release must pin the exact bytes of the served index:\n{release_text}"
    );
    assert!(release_text.contains("main/binary-arm64/Packages\n"));

    // Reproducible: the same metadata, two reads, same etag.
    let again = call(
        port,
        "GET",
        "/apt/debian-hosted/dists/stable/Release",
        &token,
        None,
        &[],
    );
    assert_eq!(
        again.body, release.body,
        "Release must not drift between reads"
    );
    let etag = release.header("etag").expect("the Release carries an etag");

    // And conditional: if it did not change, 304.
    let not_modified = call(
        port,
        "GET",
        "/apt/debian-hosted/dists/stable/Release",
        &token,
        None,
        &[("if-none-match", etag.as_str())],
    );
    assert_eq!(not_modified.status, 304, "the second read must answer 304");

    // The public key and the signature: `gpgv`, the verifier apt uses
    // behind the scenes, opens Release.gpg against the served key.asc.
    if which("gpgv").is_none() {
        eprintln!("skipping the signature check: gpgv is not installed");
    } else {
        let key = call(port, "GET", "/apt/debian-hosted/key.asc", &token, None, &[]);
        assert_eq!(key.status, 200, "{}", text_of(&key));
        assert!(text_of(&key).contains("BEGIN PGP PUBLIC KEY BLOCK"));

        let signature = call(
            port,
            "GET",
            "/apt/debian-hosted/dists/stable/Release.gpg",
            &token,
            None,
            &[],
        );
        assert_eq!(signature.status, 200, "{}", text_of(&signature));

        let dir = next("gpgv");
        std::fs::create_dir_all(&dir).expect("a scratch dir");
        std::fs::write(dir.join("Release"), &release.body).expect("write release");
        std::fs::write(dir.join("Release.gpg"), &signature.body).expect("write signature");
        std::fs::write(dir.join("key.asc"), &key.body).expect("write key");
        // `gpgv --keyring` speaks binary: apt's armor is dearmored
        // here the same way the sysadmin's `gpg --dearmor` dearmors it.
        std::fs::write(dir.join("keyring.bin"), dearmor(&text_of(&key))).expect("write keyring");
        let output = std::process::Command::new("gpgv")
            .arg("--keyring")
            .arg(dir.join("keyring.bin"))
            .arg(dir.join("Release.gpg"))
            .arg(dir.join("Release"))
            .env("LC_ALL", "C")
            .output()
            .expect("gpgv runs");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success() && stderr.contains("Good signature"),
            "gpgv said: {stderr}"
        );

        // The signature does not open a tampered Release: it is a signature of these bytes.
        std::fs::write(dir.join("tampered"), b"Origin: Nobody\n").expect("write tampered");
        let tampered = std::process::Command::new("gpgv")
            .arg("--keyring")
            .arg(dir.join("keyring.bin"))
            .arg(dir.join("Release.gpg"))
            .arg(dir.join("tampered"))
            .env("LC_ALL", "C")
            .output()
            .expect("gpgv runs");
        assert!(!tampered.status.success(), "a forged Release must not open");
    }

    // Immutability: other bytes at the same path are a no with a reason.
    let other = build_deb(concat!(
        "Package: hello-hodei\n",
        "Version: 1.0.0\n",
        "Architecture: amd64\n",
        "Description: a different build of the same version\n",
    ));
    let conflict = call(
        port,
        "PUT",
        &format!("/apt/debian-hosted/{HELLO_PATH}"),
        &token,
        Some(&other),
        &[],
    );
    assert_eq!(conflict.status, 409, "{}", text_of(&conflict));

    // An identical republication is the echo: 200 without touching anything.
    let echo = call(
        port,
        "PUT",
        &format!("/apt/debian-hosted/{HELLO_PATH}"),
        &token,
        Some(&hello),
        &[],
    );
    assert_eq!(echo.status, 200, "{}", text_of(&echo));

    // A new publication moves the index and, with it, the date of the
    // Release: the `all` records enter both architectures.
    let tools = tools_deb();
    let published_tools = call(
        port,
        "PUT",
        &format!("/apt/debian-hosted/{TOOLS_PATH}"),
        &token,
        Some(&tools),
        &[],
    );
    assert_eq!(published_tools.status, 200, "{}", text_of(&published_tools));
    let arm_after = call(
        port,
        "GET",
        "/apt/debian-hosted/dists/stable/binary-arm64/Packages",
        &token,
        None,
        &[],
    );
    assert!(text_of(&arm_after).contains("Package: hodei-tools\n"));
    let release_after = call(
        port,
        "GET",
        "/apt/debian-hosted/dists/stable/Release",
        &token,
        None,
        &[],
    );
    assert_ne!(
        release_after.header("etag").as_deref(),
        Some(etag.as_str()),
        "the Release must move when the state moves"
    );
}

/// The binary content of an OpenPGP armor block: the base64 lines
/// between the markers, without the CRC line that `gpg` writes and nobody reads.
fn dearmor(armored: &str) -> Vec<u8> {
    let body: String = armored
        .lines()
        .skip_while(|line| !line.starts_with("-----BEGIN"))
        .skip(1)
        .take_while(|line| !line.starts_with("-----END"))
        .filter(|line| !(line.trim().is_empty() || line.starts_with('=')))
        // The blank that separates armor headers from the body is also out.
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect();
    base64::engine::general_purpose::STANDARD
        .decode(&body)
        .expect("the armored key is base64")
}

fn which(program: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let candidate = dir.join(program);
            candidate.is_file().then_some(candidate)
        })
    })
}
