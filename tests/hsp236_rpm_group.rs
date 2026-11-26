//! HSP-236: YUM/DNF `RepositoryMode::Group` aggregated `repodata` and coherent
//! Hodei signature (`repomd.xml.asc`).
//!
//! Verifies:
//! - AC1: Real `dnf` installs two RPMs from different members (`hosted` + `proxy`)
//!   through the group with `repomd.xml.asc` verified by Hodei's distribution key
//!   (`repo_gpgcheck=1`) and each package verified by checksum.
//! - AC2: NEVRA collisions and concurrent revisions produce deterministic selection
//!   by group member precedence and a coherent index without mixing generations.
//! - AC3: On signing failure, member revocation/removal, or unavailable origin,
//!   only a still-valid complete snapshot is served per policy, or it fails closed.

mod support;

use std::io::{Read as _, Write as _};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

use hodei_artifacts_api::openpgp::MetadataSigner;
use hodei_catalog::yum;
use serde_json::{Value, json};
use sha2::Digest as _;
use support::{Answer, Curl, Deployment, FIRST_ACCOUNT};

#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

static CALLS: AtomicUsize = AtomicUsize::new(0);

fn next(name: &str) -> std::path::PathBuf {
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("hsp236-{name}-{n}"))
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
        "podman is not installed: the yum group client suite needs a fedora container"
    );
    let image = Command::new("podman")
        .args(["image", "exists", IMAGE])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    assert!(
        image,
        "the image {IMAGE} is not available: pull it before running the yum group suite"
    );
}

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

fn sha256_of(bytes: &[u8]) -> String {
    let digest = sha2::Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).expect("gzip writes");
    encoder.finish().expect("gzip finishes")
}

fn gunzip(bytes: &[u8]) -> String {
    let mut decoder = flate2::read::GzDecoder::new(bytes);
    let mut out = String::new();
    decoder.read_to_string(&mut out).expect("gunzip reads");
    out
}

fn build_real_rpm_spec(name: &str, version: &str, release: &str, greeting: &str) -> String {
    format!(
        concat!(
            "Name: {name}\n",
            "Version: {version}\n",
            "Release: {release}\n",
            "Summary: Real test RPM {name}\n",
            "License: MIT\n",
            "BuildRoot: %{{_tmppath}}/{name}-root\n",
            "%description\n",
            "Installed through the real dnf client against Hodei RPM group.\n",
            "%install\n",
            "mkdir -p %{{buildroot}}/usr/share/{name}\n",
            "printf '%s\\n' '{greeting}' > %{{buildroot}}/usr/share/{name}/greeting.txt\n",
            "%files\n",
            "/usr/share/{name}/greeting.txt\n",
        ),
        name = name,
        version = version,
        release = release,
        greeting = greeting,
    )
}

fn proxy_config(upstream: &support::lab::Upstream, key_armor: String) -> Value {
    json!({
        "upstream": {
            "id": "rpm-upstream",
            "url": upstream.base(),
            "scopes": ["repodata/", "Packages/"],
            "gpg_key_armor": key_armor,
        },
        "cache": {},
    })
}

fn publish_upstream_repo(
    upstream: &support::lab::Upstream,
    upstream_signer: &MetadataSigner,
    pkg_path: &str,
    rpm_bytes: &[u8],
    revision: u64,
) {
    let metadata = yum::parse_rpm(rpm_bytes).expect("parse real rpm");
    let sha = sha256_of(rpm_bytes);
    let record = yum::package_record(&metadata, pkg_path, rpm_bytes.len() as u64, &sha);
    let primary_xml = yum::primary_document(&[&record]).into_bytes();
    let primary_gz = gzip(&primary_xml);
    let gz_sha = sha256_of(&primary_gz);
    let xml_sha = sha256_of(&primary_xml);
    let primary_loc = yum::primary_location(&gz_sha);
    let repomd = yum::repomd_document(
        revision,
        &[yum::RepodataEntry {
            kind: "primary",
            location: &primary_loc,
            checksum: &gz_sha,
            size: primary_gz.len() as u64,
            open_checksum: &xml_sha,
            open_size: primary_xml.len() as u64,
            timestamp: revision,
        }],
    )
    .into_bytes();
    let sig = upstream_signer
        .sign_detached(&repomd)
        .expect("upstream signs repomd");

    upstream.put("repodata/repomd.xml", &repomd, Some("\"up-repomd\""));
    upstream.put("repodata/repomd.xml.asc", &sig, Some("\"up-sig\""));
    upstream.put(&primary_loc, &primary_gz, Some("\"up-primary\""));
    upstream.put(pkg_path, rpm_bytes, Some("\"up-rpm\""));
}

#[test]
fn ac1_real_dnf_installs_hosted_and_proxy_packages_through_group_with_hodei_signature() {
    require_podman();

    let work = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        work.path().join("hello-hosted.spec"),
        build_real_rpm_spec("hello-hosted", "1.0.0", "1", "from-hosted-member"),
    )
    .expect("write hosted spec");
    std::fs::write(
        work.path().join("hello-proxy.spec"),
        build_real_rpm_spec("hello-proxy", "1.0.0", "1", "from-proxy-member"),
    )
    .expect("write proxy spec");

    container_run(
        work.path(),
        "set -e; \
         rpm -q rpm-build >/dev/null 2>&1 || dnf -y -q install rpm-build; \
         mkdir -p /root/rpmbuild/SPECS /root/rpmbuild/RPMS; \
         rpmbuild -bb /work/hello-hosted.spec; \
         rpmbuild -bb /work/hello-proxy.spec; \
         cp /root/rpmbuild/RPMS/*/*.rpm /work/",
    );

    let hosted_rpm =
        std::fs::read(work.path().join("hello-hosted-1.0.0-1.x86_64.rpm")).expect("hosted rpm");
    let proxy_rpm =
        std::fs::read(work.path().join("hello-proxy-1.0.0-1.x86_64.rpm")).expect("proxy rpm");

    let upstream = support::lab::Upstream::start();
    let upstream_signer =
        MetadataSigner::generate("Upstream <upstream@example.local>").expect("upstream signer");
    let hub_signer = MetadataSigner::generate("Hodei Hub <hub@hodei.local>").expect("hub signer");
    let hub_armor = hub_signer.to_armor().expect("hub armor");

    publish_upstream_repo(
        &upstream,
        &upstream_signer,
        "Packages/hello-proxy-1.0.0-1.x86_64.rpm",
        &proxy_rpm,
        1_700_000_100,
    );

    let deployment = Deployment::new();
    deployment.seed_yum(&[("rpm-hosted", Value::Null)]);
    deployment.seed_yum_proxies(
        &[],
        &[(
            "rpm-proxy",
            proxy_config(
                &upstream,
                upstream_signer.key_asc().expect("upstream key_asc"),
            ),
        )],
    );
    deployment.seed_yum_group("rpm-group", &["rpm-hosted", "rpm-proxy"]);

    let token = support::token(FIRST_ACCOUNT, "alice");
    let allowed = format!("127.0.0.1:{};private;http", upstream.port());
    let mut service = deployment.start(&[
        ("HODEI_PROXY__ALLOWED_HOSTS", &allowed),
        ("HODEI_METADATA_SIGNER_ARMOR", &hub_armor),
    ]);
    let port = service.port;

    let put_ans = call(
        port,
        "PUT",
        "/yum/rpm-hosted/pool/hello-hosted-1.0.0-1.x86_64.rpm",
        &token,
        Some(&hosted_rpm),
    );
    assert_eq!(put_ans.status, 200, "hosted upload failed");

    // Group key.asc must be Hodei's key, never the upstream's key.
    let key_ans = call(port, "GET", "/yum/rpm-group/key.asc", &token, None);
    assert_eq!(key_ans.status, 200);
    assert_eq!(
        String::from_utf8_lossy(&key_ans.body),
        hub_signer.key_asc().expect("hub key_asc")
    );
    assert_ne!(
        String::from_utf8_lossy(&key_ans.body),
        upstream_signer.key_asc().expect("upstream key_asc")
    );

    // Group repomd.xml.asc must verify with Hodei's key and fail with upstream's key.
    let repomd_ans = call(
        port,
        "GET",
        "/yum/rpm-group/repodata/repomd.xml",
        &token,
        None,
    );
    assert_eq!(repomd_ans.status, 200);
    let primary_decl = yum::declared_primary(&String::from_utf8_lossy(&repomd_ans.body))
        .expect("group repomd declares primary metadata");
    let primary_ans = call(
        port,
        "GET",
        &format!("/yum/rpm-group/{}", primary_decl.location),
        &token,
        None,
    );
    assert_eq!(primary_ans.status, 200);
    let primary_xml = gunzip(&primary_ans.body);
    assert!(
        primary_xml.contains(&sha256_of(&hosted_rpm)),
        "primary metadata must declare the exact hosted RPM checksum"
    );
    assert!(
        primary_xml.contains(&sha256_of(&proxy_rpm)),
        "primary metadata must declare the exact proxied RPM checksum"
    );
    let sig_ans = call(
        port,
        "GET",
        "/yum/rpm-group/repodata/repomd.xml.asc",
        &token,
        None,
    );
    assert_eq!(sig_ans.status, 200);
    let hub_key_asc = hub_signer.key_asc().expect("hub key_asc");
    let upstream_key_asc = upstream_signer.key_asc().expect("upstream key_asc");
    MetadataSigner::verify_detached(&repomd_ans.body, &sig_ans.body, &hub_key_asc)
        .expect("group signature verifies with Hodei distribution key");
    assert!(
        MetadataSigner::verify_detached(&repomd_ans.body, &sig_ans.body, &upstream_key_asc)
            .is_err(),
        "upstream key must not verify merged group repomd.xml"
    );

    let repo_file = format!(
        concat!(
            "[hodei-group]\n",
            "name=Hodei RPM Group\n",
            "baseurl=http://alice:{token}@127.0.0.1:{port}/yum/rpm-group/\n",
            "enabled=1\n",
            "gpgcheck=0\n",
            "repo_gpgcheck=1\n",
            "gpgkey=http://alice:{token}@127.0.0.1:{port}/yum/rpm-group/key.asc\n",
        ),
        token = token,
        port = port,
    );
    std::fs::write(work.path().join("hodei-group.repo"), repo_file).expect("write .repo");

    let out = container_run(
        work.path(),
        "set -e; \
         cp /work/hodei-group.repo /etc/yum.repos.d/hodei-group.repo; \
         dnf -y --disablerepo='*' --enablerepo='hodei-group' makecache; \
         dnf -y --disablerepo='*' --enablerepo='hodei-group' install hello-hosted hello-proxy; \
         cat /usr/share/hello-hosted/greeting.txt; \
         cat /usr/share/hello-proxy/greeting.txt",
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("from-hosted-member"), "stdout: {stdout}");
    assert!(stdout.contains("from-proxy-member"), "stdout: {stdout}");

    support::stop(&mut service);
}

#[test]
fn ac2_nevra_collision_precedence_and_generation_coherence() {
    require_podman();

    let work = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        work.path().join("collide-hosted.spec"),
        build_real_rpm_spec("collide-pkg", "1.0.0", "1", "winner-hosted"),
    )
    .expect("spec 1");
    std::fs::write(
        work.path().join("collide-proxy.spec"),
        build_real_rpm_spec("collide-pkg", "1.0.0", "1", "winner-proxy"),
    )
    .expect("spec 2");
    std::fs::write(
        work.path().join("extra.spec"),
        build_real_rpm_spec("extra-pkg", "2.0.0", "1", "extra-v2"),
    )
    .expect("spec 3");

    container_run(
        work.path(),
        "set -e; \
         rpm -q rpm-build >/dev/null 2>&1 || dnf -y -q install rpm-build; \
         mkdir -p /root/rpmbuild/RPMS; \
         rpmbuild -bb /work/collide-hosted.spec; \
         cp /root/rpmbuild/RPMS/x86_64/collide-pkg-1.0.0-1.x86_64.rpm /work/collide-hosted.rpm; \
         rm -f /root/rpmbuild/RPMS/x86_64/collide-pkg-1.0.0-1.x86_64.rpm; \
         rpmbuild -bb /work/collide-proxy.spec; \
         cp /root/rpmbuild/RPMS/x86_64/collide-pkg-1.0.0-1.x86_64.rpm /work/collide-proxy.rpm; \
         rpmbuild -bb /work/extra.spec; \
         cp /root/rpmbuild/RPMS/x86_64/extra-pkg-2.0.0-1.x86_64.rpm /work/extra.rpm",
    );

    let hosted_rpm = std::fs::read(work.path().join("collide-hosted.rpm")).expect("hosted rpm");
    let proxy_rpm = std::fs::read(work.path().join("collide-proxy.rpm")).expect("proxy rpm");
    let extra_rpm = std::fs::read(work.path().join("extra.rpm")).expect("extra rpm");
    assert_ne!(sha256_of(&hosted_rpm), sha256_of(&proxy_rpm));

    let upstream = support::lab::Upstream::start();
    let upstream_signer =
        MetadataSigner::generate("Upstream <upstream@example.local>").expect("upstream signer");
    let hub_signer = MetadataSigner::generate("Hodei Hub <hub@hodei.local>").expect("hub signer");
    let hub_armor = hub_signer.to_armor().expect("hub armor");

    publish_upstream_repo(
        &upstream,
        &upstream_signer,
        "Packages/collide-pkg-1.0.0-1.x86_64.rpm",
        &proxy_rpm,
        1_700_000_200,
    );

    let deployment = Deployment::new();
    deployment.seed_yum(&[("rpm-hosted", Value::Null)]);
    deployment.seed_yum_proxies(
        &[],
        &[(
            "rpm-proxy",
            proxy_config(
                &upstream,
                upstream_signer.key_asc().expect("upstream key_asc"),
            ),
        )],
    );
    deployment.seed_yum_group("group-hosted-first", &["rpm-hosted", "rpm-proxy"]);
    deployment.seed_yum_group("group-proxy-first", &["rpm-proxy", "rpm-hosted"]);

    let token = support::token(FIRST_ACCOUNT, "alice");
    let allowed = format!("127.0.0.1:{};private;http", upstream.port());
    let mut service = deployment.start(&[
        ("HODEI_PROXY__ALLOWED_HOSTS", &allowed),
        ("HODEI_METADATA_SIGNER_ARMOR", &hub_armor),
    ]);
    let port = service.port;

    assert_eq!(
        call(
            port,
            "PUT",
            "/yum/rpm-hosted/pool/collide-pkg-1.0.0-1.x86_64.rpm",
            &token,
            Some(&hosted_rpm),
        )
        .status,
        200
    );

    // 1. Hosted-first group selects hosted's NEVRA entry and bytes.
    let repomd1 = call(
        port,
        "GET",
        "/yum/group-hosted-first/repodata/repomd.xml",
        &token,
        None,
    );
    assert_eq!(repomd1.status, 200);
    let repomd1_repeat = call(
        port,
        "GET",
        "/yum/group-hosted-first/repodata/repomd.xml",
        &token,
        None,
    );
    assert_eq!(repomd1_repeat.status, 200);
    assert_eq!(
        repomd1_repeat.body, repomd1.body,
        "an unchanged group state has a deterministic metadata snapshot"
    );
    let primary1_decl =
        yum::declared_primary(&String::from_utf8_lossy(&repomd1.body)).expect("primary1");
    let primary1 = call(
        port,
        "GET",
        &format!("/yum/group-hosted-first/{}", primary1_decl.location),
        &token,
        None,
    );
    assert_eq!(primary1.status, 200);
    let xml1 = gunzip(&primary1.body);
    let entries1 = yum::parse_package_entries(&xml1).expect("entries1");
    assert_eq!(entries1.len(), 1, "NEVRA collision must deduplicate to 1");
    assert_eq!(entries1[0].location, "pool/collide-pkg-1.0.0-1.x86_64.rpm");
    assert!(xml1.contains(&sha256_of(&hosted_rpm)));
    assert!(!xml1.contains(&sha256_of(&proxy_rpm)));

    // 2. Proxy-first group selects proxy's NEVRA entry and bytes.
    let repomd_pf = call(
        port,
        "GET",
        "/yum/group-proxy-first/repodata/repomd.xml",
        &token,
        None,
    );
    assert_eq!(repomd_pf.status, 200);
    let primary_pf_decl =
        yum::declared_primary(&String::from_utf8_lossy(&repomd_pf.body)).expect("primary_pf");
    let primary_pf = call(
        port,
        "GET",
        &format!("/yum/group-proxy-first/{}", primary_pf_decl.location),
        &token,
        None,
    );
    assert_eq!(primary_pf.status, 200);
    let xml_pf = gunzip(&primary_pf.body);
    let entries_pf = yum::parse_package_entries(&xml_pf).expect("entries_pf");
    assert_eq!(entries_pf.len(), 1);
    assert_eq!(
        entries_pf[0].location,
        "Packages/collide-pkg-1.0.0-1.x86_64.rpm"
    );
    assert!(xml_pf.contains(&sha256_of(&proxy_rpm)));
    assert!(!xml_pf.contains(&sha256_of(&hosted_rpm)));

    // A concurrent publication cannot replace the immutable primary that
    // this already-issued repomd references.
    let primary1_path = format!("/yum/group-hosted-first/{}", primary1_decl.location);
    let start = Arc::new(Barrier::new(5));
    let mut readers = Vec::new();
    for _ in 0..4 {
        let start = Arc::clone(&start);
        let path = primary1_path.clone();
        let token = token.clone();
        let expected = primary1_decl.checksum.clone();
        readers.push(std::thread::spawn(move || {
            start.wait();
            for _ in 0..4 {
                let answer = call(port, "GET", &path, &token, None);
                assert_eq!(answer.status, 200);
                assert_eq!(sha256_of(&answer.body), expected);
            }
        }));
    }
    let publish_start = Arc::clone(&start);
    let publish_token = token.clone();
    let publish_body = extra_rpm.clone();
    let publisher = std::thread::spawn(move || {
        publish_start.wait();
        call(
            port,
            "PUT",
            "/yum/rpm-hosted/pool/extra-pkg-2.0.0-1.x86_64.rpm",
            &publish_token,
            Some(&publish_body),
        )
        .status
    });
    for reader in readers {
        reader.join().expect("a concurrent reader finishes");
    }
    assert_eq!(
        publisher.join().expect("the concurrent publisher finishes"),
        200
    );

    let hub_key_asc = hub_signer.key_asc().expect("hub key_asc");
    let sig1 = call(
        port,
        "GET",
        "/yum/group-hosted-first/repodata/repomd.xml.asc",
        &token,
        None,
    );
    assert_eq!(sig1.status, 200);
    MetadataSigner::verify_detached(&repomd1.body, &sig1.body, &hub_key_asc)
        .expect("the old signature remains paired with the downloaded repomd after publication");

    let gen1_again = call(
        port,
        "GET",
        &format!("/yum/group-hosted-first/{}", primary1_decl.location),
        &token,
        None,
    );
    assert_eq!(gen1_again.status, 200);
    assert_eq!(sha256_of(&gen1_again.body), primary1_decl.checksum);

    // New repomd.xml + repomd.xml.asc reflect generation 2 coherently.
    let repomd2 = call(
        port,
        "GET",
        "/yum/group-hosted-first/repodata/repomd.xml",
        &token,
        None,
    );
    assert_eq!(repomd2.status, 200);
    let sig2 = call(
        port,
        "GET",
        "/yum/group-hosted-first/repodata/repomd.xml.asc",
        &token,
        None,
    );
    assert_eq!(sig2.status, 200);
    MetadataSigner::verify_detached(&repomd2.body, &sig2.body, &hub_key_asc)
        .expect("generation 2 signature verifies");
    let primary2_decl =
        yum::declared_primary(&String::from_utf8_lossy(&repomd2.body)).expect("primary2");
    assert_ne!(primary2_decl.checksum, primary1_decl.checksum);
    let primary2 = call(
        port,
        "GET",
        &format!("/yum/group-hosted-first/{}", primary2_decl.location),
        &token,
        None,
    );
    assert_eq!(primary2.status, 200);
    let entries2 = yum::parse_package_entries(&gunzip(&primary2.body)).expect("entries2");
    assert_eq!(entries2.len(), 2);

    support::stop(&mut service);
}

#[test]
fn ac3_unavailable_origin_fails_closed_never_exposing_partial_index() {
    let upstream = support::lab::Upstream::start();
    upstream.fail(true);
    let upstream_signer =
        MetadataSigner::generate("Upstream <upstream@example.local>").expect("upstream signer");

    let deployment = Deployment::new();
    deployment.seed_yum(&[("rpm-hosted", Value::Null)]);
    deployment.seed_yum_proxies(
        &[],
        &[(
            "rpm-proxy-down",
            proxy_config(
                &upstream,
                upstream_signer.key_asc().expect("upstream key_asc"),
            ),
        )],
    );
    deployment.seed_yum_group("rpm-group-down", &["rpm-hosted", "rpm-proxy-down"]);

    let token = support::token(FIRST_ACCOUNT, "alice");
    let allowed = format!("127.0.0.1:{};private;http", upstream.port());
    let mut service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &allowed)]);
    let port = service.port;

    let repomd = call(
        port,
        "GET",
        "/yum/rpm-group-down/repodata/repomd.xml",
        &token,
        None,
    );
    assert_eq!(
        repomd.status, 502,
        "an unavailable origin must return BAD_GATEWAY, not a partial index"
    );
    let sig = call(
        port,
        "GET",
        "/yum/rpm-group-down/repodata/repomd.xml.asc",
        &token,
        None,
    );
    assert_eq!(
        sig.status, 502,
        "an unavailable origin signature must return BAD_GATEWAY"
    );

    support::stop(&mut service);
}

#[test]
fn ac3_upstream_key_rotation_invalidates_old_snapshot_fail_closed() {
    let upstream = support::lab::Upstream::start();
    let old_signer =
        MetadataSigner::generate("Old upstream <old-upstream@example.local>").expect("old signer");
    let replacement_signer = MetadataSigner::generate("Replacement <replacement@example.local>")
        .expect("replacement signer");
    let hub_signer = MetadataSigner::generate("Hodei Hub <hub@hodei.local>")
        .expect("hub signer");
    let hub_armor = hub_signer.to_armor().expect("hub armor");
    let primary_xml = yum::primary_document(&[]).into_bytes();
    let primary_gz = gzip(&primary_xml);
    let gz_sha = sha256_of(&primary_gz);
    let xml_sha = sha256_of(&primary_xml);
    let primary_loc = yum::primary_location(&gz_sha);
    let repomd = yum::repomd_document(
        1_700_000_301,
        &[yum::RepodataEntry {
            kind: "primary",
            location: &primary_loc,
            checksum: &gz_sha,
            size: primary_gz.len() as u64,
            open_checksum: &xml_sha,
            open_size: primary_xml.len() as u64,
            timestamp: 1_700_000_301,
        }],
    )
    .into_bytes();
    let signature = old_signer
        .sign_detached(&repomd)
        .expect("old upstream key signs repomd");
    upstream.put("repodata/repomd.xml", &repomd, Some("\"up-repomd\""));
    upstream.put(
        "repodata/repomd.xml.asc",
        &signature,
        Some("\"up-signature\""),
    );
    upstream.put(&primary_loc, &primary_gz, Some("\"up-primary\""));

    let deployment = Deployment::new();
    deployment.seed_yum(&[("rpm-hosted", Value::Null)]);
    deployment.seed_yum_proxies(
        &[],
        &[(
            "rpm-proxy-rotating-key",
            proxy_config(&upstream, old_signer.key_asc().expect("old key")),
        )],
    );
    deployment.seed_yum_group(
        "rpm-group-rotating-key",
        &["rpm-hosted", "rpm-proxy-rotating-key"],
    );

    let token = support::token(FIRST_ACCOUNT, "alice");
    let allowed = format!("127.0.0.1:{};private;http", upstream.port());
    let mut service = deployment.start(&[
        ("HODEI_PROXY__ALLOWED_HOSTS", &allowed),
        ("HODEI_METADATA_SIGNER_ARMOR", &hub_armor),
    ]);
    let first = call(
        service.port,
        "GET",
        "/yum/rpm-group-rotating-key/repodata/repomd.xml",
        &token,
        None,
    );
    assert_eq!(first.status, 200, "the original upstream key is accepted");
    let first_signature = call(
        service.port,
        "GET",
        "/yum/rpm-group-rotating-key/repodata/repomd.xml.asc",
        &token,
        None,
    );
    assert_eq!(first_signature.status, 200);
    MetadataSigner::verify_detached(&first.body, &first_signature.body, &hub_signer.key_asc().unwrap())
        .expect("the initial generation is signed by Hodei");
    support::stop(&mut service);

    deployment.reconfigure_proxy(
        "rpm-proxy-rotating-key",
        proxy_config(&upstream, replacement_signer.key_asc().expect("replacement key")),
    );
    let mut service = deployment.start(&[
        ("HODEI_PROXY__ALLOWED_HOSTS", &allowed),
        ("HODEI_METADATA_SIGNER_ARMOR", &hub_armor),
    ]);
    let repomd_answer = call(
        service.port,
        "GET",
        "/yum/rpm-group-rotating-key/repodata/repomd.xml",
        &token,
        None,
    );
    assert_eq!(
        repomd_answer.status, 502,
        "a snapshot signed by a replaced upstream key must not be served"
    );
    let signature_answer = call(
        service.port,
        "GET",
        "/yum/rpm-group-rotating-key/repodata/repomd.xml.asc",
        &token,
        None,
    );
    assert_eq!(
        signature_answer.status, 502,
        "the signature route must fail closed after upstream key rotation"
    );
    support::stop(&mut service);
}

#[test]
fn ac3_hodei_signing_key_revocation_fails_closed_after_rotation() {
    let gnupg = tempfile::tempdir().expect("gnupg home");
    let keygen = Command::new("gpg")
        .env("GNUPGHOME", gnupg.path())
        .args([
            "--batch",
            "--pinentry-mode",
            "loopback",
            "--passphrase",
            "locked-test-passphrase",
            "--quick-generate-key",
            "Revoked Signer <revoked@hodei.local>",
            "ed25519",
            "sign",
            "never",
        ])
        .output()
        .expect("gpg keygen");
    assert!(
        keygen.status.success(),
        "gpg keygen failed: {}",
        String::from_utf8_lossy(&keygen.stderr)
    );
    let export = Command::new("gpg")
        .env("GNUPGHOME", gnupg.path())
        .args([
            "--batch",
            "--pinentry-mode",
            "loopback",
            "--passphrase",
            "locked-test-passphrase",
            "--export-secret-keys",
            "--armor",
            "revoked@hodei.local",
        ])
        .output()
        .expect("gpg export");
    assert!(
        export.status.success(),
        "gpg export failed: {}",
        String::from_utf8_lossy(&export.stderr)
    );
    let revoked_armor = String::from_utf8(export.stdout).expect("utf8 armor");

    let deployment = Deployment::new();
    deployment.seed_yum(&[("rpm-hosted", Value::Null)]);
    deployment.seed_yum_group("rpm-group-hodei-key-rotation", &["rpm-hosted"]);
    let token = support::token(FIRST_ACCOUNT, "alice");
    let healthy_signer = MetadataSigner::generate("Hodei Hub <hub@hodei.local>")
        .expect("healthy Hodei signer");
    let healthy_armor = healthy_signer.to_armor().expect("healthy signer armor");
    let mut service = deployment.start(&[("HODEI_METADATA_SIGNER_ARMOR", &healthy_armor)]);
    let first = call(
        service.port,
        "GET",
        "/yum/rpm-group-hodei-key-rotation/repodata/repomd.xml",
        &token,
        None,
    );
    assert_eq!(first.status, 200);
    let first_signature = call(
        service.port,
        "GET",
        "/yum/rpm-group-hodei-key-rotation/repodata/repomd.xml.asc",
        &token,
        None,
    );
    assert_eq!(first_signature.status, 200);
    MetadataSigner::verify_detached(
        &first.body,
        &first_signature.body,
        &healthy_signer.key_asc().expect("healthy public key"),
    )
    .expect("the initial snapshot has a valid Hodei signature");
    support::stop(&mut service);

    let mut service = deployment.start(&[("HODEI_METADATA_SIGNER_ARMOR", &revoked_armor)]);
    let repomd = call(
        service.port,
        "GET",
        "/yum/rpm-group-hodei-key-rotation/repodata/repomd.xml",
        &token,
        None,
    );
    assert_eq!(
        repomd.status, 500,
        "a new generation must not be served after the Hodei signing key is revoked"
    );
    let signature = call(
        service.port,
        "GET",
        "/yum/rpm-group-hodei-key-rotation/repodata/repomd.xml.asc",
        &token,
        None,
    );
    assert_eq!(
        signature.status, 500,
        "the old snapshot signature must not be served after key rotation"
    );
    support::stop(&mut service);
}

#[test]
fn ac3_signing_failure_fails_closed() {
    let gnupg = tempfile::tempdir().expect("gnupg home");
    let keygen = Command::new("gpg")
        .env("GNUPGHOME", gnupg.path())
        .args([
            "--batch",
            "--pinentry-mode",
            "loopback",
            "--passphrase",
            "locked-test-passphrase",
            "--quick-generate-key",
            "Locked Signer <locked@hodei.local>",
            "ed25519",
            "sign",
            "never",
        ])
        .output()
        .expect("gpg keygen");
    assert!(
        keygen.status.success(),
        "gpg keygen failed: {}",
        String::from_utf8_lossy(&keygen.stderr)
    );
    let export = Command::new("gpg")
        .env("GNUPGHOME", gnupg.path())
        .args([
            "--batch",
            "--pinentry-mode",
            "loopback",
            "--passphrase",
            "locked-test-passphrase",
            "--export-secret-keys",
            "--armor",
            "locked@hodei.local",
        ])
        .output()
        .expect("gpg export");
    assert!(
        export.status.success(),
        "gpg export failed: {}",
        String::from_utf8_lossy(&export.stderr)
    );
    let locked_armor = String::from_utf8(export.stdout).expect("utf8 armor");

    let deployment = Deployment::new();
    deployment.seed_yum(&[("rpm-hosted", Value::Null)]);
    deployment.seed_yum_group("rpm-group-locked", &["rpm-hosted"]);

    let token = support::token(FIRST_ACCOUNT, "alice");
    let mut service = deployment.start(&[("HODEI_METADATA_SIGNER_ARMOR", &locked_armor)]);
    let port = service.port;

    let repomd = call(
        port,
        "GET",
        "/yum/rpm-group-locked/repodata/repomd.xml",
        &token,
        None,
    );
    assert_eq!(
        repomd.status, 500,
        "signing failure must fail closed and never serve unsigned repomd.xml"
    );
    let sig = call(
        port,
        "GET",
        "/yum/rpm-group-locked/repodata/repomd.xml.asc",
        &token,
        None,
    );
    assert_eq!(sig.status, 500);

    support::stop(&mut service);
}

#[test]
fn ac3_trusted_member_without_grant_fails_closed() {
    let upstream = support::lab::Upstream::start();
    let upstream_signer =
        MetadataSigner::generate("Upstream <upstream@example.local>").expect("upstream signer");
    let primary_xml = yum::primary_document(&[]).into_bytes();
    let primary_gz = gzip(&primary_xml);
    let gz_sha = sha256_of(&primary_gz);
    let xml_sha = sha256_of(&primary_xml);
    let primary_loc = yum::primary_location(&gz_sha);
    let repomd = yum::repomd_document(
        1_700_000_300,
        &[yum::RepodataEntry {
            kind: "primary",
            location: &primary_loc,
            checksum: &gz_sha,
            size: primary_gz.len() as u64,
            open_checksum: &xml_sha,
            open_size: primary_xml.len() as u64,
            timestamp: 1_700_000_300,
        }],
    )
    .into_bytes();
    let sig = upstream_signer
        .sign_detached(&repomd)
        .expect("upstream signs repomd");
    upstream.put("repodata/repomd.xml", &repomd, Some("\"up-repomd\""));
    upstream.put("repodata/repomd.xml.asc", &sig, Some("\"up-sig\""));
    upstream.put(&primary_loc, &primary_gz, Some("\"up-primary\""));

    let deployment = Deployment::new();
    deployment.seed_yum(&[("rpm-hosted", Value::Null)]);
    deployment.seed_yum_proxies(
        &["rpm-proxy-trusted"],
        &[(
            "rpm-proxy-trusted",
            proxy_config(
                &upstream,
                upstream_signer.key_asc().expect("upstream key_asc"),
            ),
        )],
    );
    deployment.seed_yum_group("rpm-group-trusted", &["rpm-hosted", "rpm-proxy-trusted"]);

    let token = support::token(FIRST_ACCOUNT, "alice");
    let allowed = format!("127.0.0.1:{};private;http", upstream.port());
    let mut service = deployment.start(&[("HODEI_PROXY__ALLOWED_HOSTS", &allowed)]);
    let port = service.port;

    let repomd_ans = call(
        port,
        "GET",
        "/yum/rpm-group-trusted/repodata/repomd.xml",
        &token,
        None,
    );
    assert_eq!(
        repomd_ans.status, 404,
        "trusted member without grant must fail closed"
    );

    support::stop(&mut service);
}
