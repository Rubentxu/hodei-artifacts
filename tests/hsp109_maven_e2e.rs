//! Maven 2 hosted for HSP-109 / HAR-15, against the wire.
//!
//! The service is a real process with a real catalog, byte store and
//! Cedar. What is measured is what a Maven client does: publish
//! POM, jar and checksums under the standard path, read them back, see the derived
//! `maven-metadata.xml`, and the rejections (credential, permission,
//! coordinates, checksum, version, immutability) also after a restart.

mod support;

use std::path::PathBuf;

use sha1::{Digest, Sha1};
use support::{Curl, Deployment, FIRST_ACCOUNT, SECOND_ACCOUNT};

const MIXED: &str = "mvn-mixed";
const RELEASES: &str = "mvn-release";
const SNAPSHOTS: &str = "mvn-snapshot";
const REDEPLOY: &str = "mvn-redeploy";

const BASE: &str = "com/acme/lib";
const JAR: &str = "com/acme/lib/1.0/lib-1.0.jar";
const POM: &str = "com/acme/lib/1.0/lib-1.0.pom";

/// The catalog seeder.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

fn pom(group: &str, artifact: &str, version: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<project>\n  <modelVersion>4.0.0</modelVersion>\n  <groupId>{group}</groupId>\n  <artifactId>{artifact}</artifactId>\n  <version>{version}</version>\n</project>\n"
    )
    .into_bytes()
}

fn sha1_hex(bytes: &[u8]) -> String {
    Sha1::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

struct Reply {
    status: u16,
    body: Vec<u8>,
    headers: support::Answer,
}

impl Reply {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    fn header(&self, name: &str) -> Option<String> {
        self.headers.header(name)
    }
}

struct Lab {
    deployment: Deployment,
}

impl Lab {
    fn new() -> Self {
        let deployment = Deployment::new();
        deployment.seed();
        deployment.seed_maven(&[
            (MIXED, serde_json::Value::Null),
            (RELEASES, serde_json::json!({ "version_policy": "release" })),
            (
                SNAPSHOTS,
                serde_json::json!({ "version_policy": "snapshot" }),
            ),
            (REDEPLOY, serde_json::json!({ "release_redeploy": true })),
        ]);
        Self { deployment }
    }

    fn file(&self, name: &str, body: &[u8]) -> PathBuf {
        let path = self.deployment.directory.path().join(name);
        std::fs::write(&path, body).expect("the body is written");
        path
    }

    fn call(
        &self,
        port: u16,
        method: &str,
        repository: &str,
        path: &str,
        token: &str,
        body: Option<&[u8]>,
    ) -> Reply {
        let upload = body.map(|bytes| self.file("upload.bin", bytes));
        let into = self.deployment.directory.path().join("response.bin");
        let _ = std::fs::remove_file(&into);
        let answer = Curl::call(
            port,
            method,
            &format!("/repository/{repository}/{path}"),
            token,
            upload.as_deref(),
            None,
            Some(&into),
        )
        .run();
        Reply {
            status: answer.status,
            body: std::fs::read(&into).unwrap_or_default(),
            headers: answer,
        }
    }

    fn put(&self, port: u16, repository: &str, path: &str, body: &[u8]) -> Reply {
        let alice = support::token(FIRST_ACCOUNT, "alice");
        self.call(port, "PUT", repository, path, &alice, Some(body))
    }

    fn get(&self, port: u16, repository: &str, path: &str) -> Reply {
        let alice = support::token(FIRST_ACCOUNT, "alice");
        self.call(port, "GET", repository, path, &alice, None)
    }

    /// Publishes the way `mvn deploy` does: the jar, its checksum, the POM and its
    /// own.
    fn deploy(&self, port: u16, repository: &str, version: &str, jar: &[u8]) {
        let dir = format!("{BASE}/{version}");
        let jar_path = format!("{dir}/lib-{version}.jar");
        let pom_path = format!("{dir}/lib-{version}.pom");
        let pom_bytes = pom("com.acme", "lib", version);
        assert_eq!(self.put(port, repository, &jar_path, jar).status, 201);
        assert_eq!(
            self.put(
                port,
                repository,
                &format!("{jar_path}.sha1"),
                sha1_hex(jar).as_bytes()
            )
            .status,
            200
        );
        assert_eq!(
            self.put(port, repository, &pom_path, &pom_bytes).status,
            201
        );
        assert_eq!(
            self.put(
                port,
                repository,
                &format!("{pom_path}.sha1"),
                sha1_hex(&pom_bytes).as_bytes()
            )
            .status,
            200
        );
    }
}

// ---------------------------------------------------------------------------
// Deployment and reading
// ---------------------------------------------------------------------------

#[test]
fn a_deployed_artifact_is_resolved_with_its_checksums_and_metadata() {
    let lab = Lab::new();
    let service = lab.deployment.start(&[]);
    let port = service.port;
    let jar = b"PK-not-really-a-jar-but-bytes".to_vec();
    lab.deploy(port, MIXED, "1.0", &jar);

    let served = lab.get(port, MIXED, JAR);
    assert_eq!(served.status, 200);
    assert_eq!(served.body, jar);
    assert_eq!(
        lab.get(port, MIXED, POM).body,
        pom("com.acme", "lib", "1.0")
    );

    // The checksum comes from the content, not from what was uploaded.
    let sidecar = lab.get(port, MIXED, &format!("{JAR}.sha1"));
    assert_eq!(sidecar.status, 200);
    assert_eq!(sidecar.text().trim(), sha1_hex(&jar));

    // The artifact metadata is derived by the service.
    let metadata = lab.get(port, MIXED, &format!("{BASE}/maven-metadata.xml"));
    assert_eq!(metadata.status, 200);
    let xml = metadata.text();
    assert!(xml.contains("<version>1.0</version>"), "{xml}");
    assert!(xml.contains("<release>1.0</release>"), "{xml}");
    assert!(
        metadata
            .header("content-type")
            .is_some_and(|value| value.starts_with("application/xml")),
    );
    let metadata_sum = lab.get(port, MIXED, &format!("{BASE}/maven-metadata.xml.sha1"));
    assert_eq!(metadata_sum.status, 200);
    assert_eq!(metadata_sum.text().trim(), sha1_hex(xml.as_bytes()));

    // HEAD describes without a body.
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let head = Curl::describe(port, &format!("/repository/{MIXED}/{JAR}"), &alice).run();
    assert_eq!(head.status, 200);
    assert_eq!(head.header("content-length"), Some(jar.len().to_string()));
    assert_eq!(head.size, 0);

    // A new version shows up in the metadata, sorted.
    lab.deploy(port, MIXED, "1.1", b"second");
    let xml = lab
        .get(port, MIXED, &format!("{BASE}/maven-metadata.xml"))
        .text();
    assert!(xml.contains("<latest>1.1</latest>"), "{xml}");
    assert!(xml.find("<version>1.0<").unwrap() < xml.find("<version>1.1<").unwrap());
}

#[test]
fn the_deployed_content_survives_a_restart() {
    let lab = Lab::new();
    let mut service = lab.deployment.start(&[]);
    let port = service.port;
    lab.deploy(port, MIXED, "1.0", b"jar-before-restart");
    support::kill(&mut service);

    let service = lab.deployment.start_on(port, &[]);
    assert_eq!(service.port, port);
    let served = lab.get(port, MIXED, JAR);
    assert_eq!(served.status, 200);
    assert_eq!(served.body, b"jar-before-restart");
    let xml = lab
        .get(port, MIXED, &format!("{BASE}/maven-metadata.xml"))
        .text();
    assert!(xml.contains("<version>1.0</version>"), "{xml}");
}

// ---------------------------------------------------------------------------
// Access
// ---------------------------------------------------------------------------

#[test]
fn a_request_without_credential_is_401_and_without_permission_is_403() {
    let lab = Lab::new();
    let service = lab.deployment.start(&[]);
    let port = service.port;

    let anonymous = lab.call(port, "PUT", MIXED, JAR, "", Some(b"x"));
    assert_eq!(anonymous.status, 401);
    assert_eq!(lab.call(port, "GET", MIXED, JAR, "", None).status, 401);

    // Carol is in the account, but the policy gives her nothing.
    let carol = support::token(FIRST_ACCOUNT, "carol");
    assert_eq!(
        lab.call(port, "PUT", MIXED, JAR, &carol, Some(b"x")).status,
        403
    );
    assert_eq!(lab.call(port, "GET", MIXED, JAR, &carol, None).status, 403);

    // A token from another account does not see the repository.
    let bob = support::token(SECOND_ACCOUNT, "bob");
    assert_eq!(lab.call(port, "GET", MIXED, JAR, &bob, None).status, 404);

    // None of the above left anything published.
    assert_eq!(lab.get(port, MIXED, JAR).status, 404);
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

#[test]
fn the_coordinates_of_the_pom_must_match_the_path() {
    let lab = Lab::new();
    let service = lab.deployment.start(&[]);
    let port = service.port;

    for wrong in [
        pom("com.other", "lib", "1.0"),
        pom("com.acme", "other", "1.0"),
        pom("com.acme", "lib", "2.0"),
    ] {
        let reply = lab.put(port, MIXED, POM, &wrong);
        assert_eq!(reply.status, 400, "{}", reply.text());
    }
    assert_eq!(
        lab.put(port, MIXED, POM, b"this is not xml <<<").status,
        400
    );
    assert_eq!(lab.get(port, MIXED, POM).status, 404);
    assert_eq!(
        lab.put(port, MIXED, POM, &pom("com.acme", "lib", "1.0"))
            .status,
        201
    );
}

#[test]
fn a_bad_checksum_or_an_invalid_layout_is_rejected() {
    let lab = Lab::new();
    let service = lab.deployment.start(&[]);
    let port = service.port;

    // A checksum before its file cannot be verified.
    let early = lab.put(
        port,
        MIXED,
        &format!("{JAR}.sha1"),
        sha1_hex(b"jar").as_bytes(),
    );
    assert_eq!(early.status, 409, "{}", early.text());

    assert_eq!(lab.put(port, MIXED, JAR, b"jar").status, 201);

    // Valid hex but of other content: it is a checksum mismatch.
    let mismatch = lab.put(
        port,
        MIXED,
        &format!("{JAR}.sha1"),
        sha1_hex(b"another content").as_bytes(),
    );
    assert_eq!(mismatch.status, 400, "{}", mismatch.text());
    // What is not hex is not a checksum.
    assert_eq!(
        lab.put(port, MIXED, &format!("{JAR}.sha1"), b"zzzz-not-hex")
            .status,
        400
    );
    // The correct one is accepted.
    assert_eq!(
        lab.put(
            port,
            MIXED,
            &format!("{JAR}.sha1"),
            sha1_hex(b"jar").as_bytes()
        )
        .status,
        200
    );

    // Invalid versions and paths.
    for path in [
        "com/acme/lib/1.0/other-1.0.jar",
        "com/acme/lib/bad%20version/lib-bad%20version.jar",
        "com/acme/lib/1.0/lib-2.0.jar",
        "com/acme/lib/1.0/.hidden",
        "lib-1.0.jar",
    ] {
        let reply = lab.put(port, MIXED, path, b"x");
        assert_eq!(reply.status, 400, "{path}: {}", reply.text());
    }
}

#[test]
fn metadata_uploaded_by_a_client_is_validated_and_never_stored() {
    let lab = Lab::new();
    let service = lab.deployment.start(&[]);
    let port = service.port;
    lab.deploy(port, MIXED, "1.0", b"jar");
    let path = format!("{BASE}/maven-metadata.xml");

    let derived = lab.get(port, MIXED, &path).text();
    let client = "<metadata><groupId>com.acme</groupId><artifactId>lib</artifactId>\
                  <versioning><versions><version>9.9</version></versions></versioning></metadata>";
    let accepted = lab.put(port, MIXED, &path, client.as_bytes());
    assert_eq!(accepted.status, 200, "{}", accepted.text());

    // What is read is what is derived from the catalog, not what the client said.
    let after = lab.get(port, MIXED, &path).text();
    assert_eq!(after, derived);
    assert!(!after.contains("9.9"));

    let foreign = "<metadata><groupId>com.other</groupId><artifactId>lib</artifactId></metadata>";
    assert_eq!(lab.put(port, MIXED, &path, foreign.as_bytes()).status, 400);
    assert_eq!(lab.put(port, MIXED, &path, b"not xml <<").status, 400);
}

// ---------------------------------------------------------------------------
// Version policy and immutability
// ---------------------------------------------------------------------------

#[test]
fn a_release_is_immutable_and_an_identical_redeploy_is_idempotent() {
    let lab = Lab::new();
    let service = lab.deployment.start(&[]);
    let port = service.port;

    assert_eq!(lab.put(port, MIXED, JAR, b"first").status, 201);
    let same = lab.put(port, MIXED, JAR, b"first");
    assert_eq!(same.status, 200);
    let different = lab.put(port, MIXED, JAR, b"second");
    assert_eq!(different.status, 409, "{}", different.text());
    assert_eq!(lab.get(port, MIXED, JAR).body, b"first");
}

#[test]
fn a_repository_can_allow_release_redeploys() {
    let lab = Lab::new();
    let service = lab.deployment.start(&[]);
    let port = service.port;

    assert_eq!(lab.put(port, REDEPLOY, JAR, b"first").status, 201);
    assert_eq!(lab.put(port, REDEPLOY, JAR, b"second").status, 200);
    assert_eq!(lab.get(port, REDEPLOY, JAR).body, b"second");
}

#[test]
fn snapshots_are_replaceable_and_timestamped_files_are_not() {
    let lab = Lab::new();
    let service = lab.deployment.start(&[]);
    let port = service.port;
    let floating = "com/acme/lib/1.0-SNAPSHOT/lib-1.0-SNAPSHOT.jar";
    let stamped = "com/acme/lib/1.0-SNAPSHOT/lib-1.0-20240101.120000-1.jar";
    let stamped_pom = "com/acme/lib/1.0-SNAPSHOT/lib-1.0-20240101.120000-1.pom";

    assert_eq!(lab.put(port, MIXED, floating, b"one").status, 201);
    assert_eq!(lab.put(port, MIXED, floating, b"two").status, 200);
    assert_eq!(lab.get(port, MIXED, floating).body, b"two");

    assert_eq!(lab.put(port, MIXED, stamped, b"build-1").status, 201);
    assert_eq!(lab.put(port, MIXED, stamped, b"other").status, 409);
    assert_eq!(
        lab.put(
            port,
            MIXED,
            stamped_pom,
            &pom("com.acme", "lib", "1.0-SNAPSHOT")
        )
        .status,
        201
    );

    // The version metadata describes the latest build.
    let xml = lab
        .get(port, MIXED, "com/acme/lib/1.0-SNAPSHOT/maven-metadata.xml")
        .text();
    assert!(
        xml.contains("<timestamp>20240101.120000</timestamp>"),
        "{xml}"
    );
    assert!(xml.contains("<buildNumber>1</buildNumber>"), "{xml}");
}

#[test]
fn the_version_policy_of_the_repository_is_enforced() {
    let lab = Lab::new();
    let service = lab.deployment.start(&[]);
    let port = service.port;
    let snapshot = "com/acme/lib/1.0-SNAPSHOT/lib-1.0-SNAPSHOT.jar";

    assert_eq!(lab.put(port, RELEASES, snapshot, b"x").status, 400);
    assert_eq!(lab.put(port, RELEASES, JAR, b"x").status, 201);
    assert_eq!(lab.put(port, SNAPSHOTS, JAR, b"x").status, 400);
    assert_eq!(lab.put(port, SNAPSHOTS, snapshot, b"x").status, 201);
}

#[test]
fn a_failed_deploy_leaves_recoverable_metadata() {
    let lab = Lab::new();
    let service = lab.deployment.start(&[]);
    let port = service.port;
    let metadata = format!("{BASE}/maven-metadata.xml");

    // A jar without a POM is not a resolvable version: it does not appear.
    assert_eq!(lab.put(port, MIXED, JAR, b"jar").status, 201);
    assert_eq!(lab.get(port, MIXED, &metadata).status, 404);

    // When the deployment completes (the POM arrives later) the metadata
    // appears on its own: there is no state to repair.
    assert_eq!(
        lab.put(port, MIXED, POM, &pom("com.acme", "lib", "1.0"))
            .status,
        201
    );
    let xml = lab.get(port, MIXED, &metadata).text();
    assert!(xml.contains("<version>1.0</version>"), "{xml}");
}

#[test]
fn concurrent_deploys_leave_consistent_metadata() {
    let lab = Lab::new();
    let service = lab.deployment.start(&[]);
    let port = service.port;
    let alice = support::token(FIRST_ACCOUNT, "alice");

    let mut children = Vec::new();
    for index in 0..6 {
        let version = format!("1.{index}");
        let body = lab.file(
            &format!("pom-{index}.xml"),
            &pom("com.acme", "lib", &version),
        );
        let path = format!("/repository/{MIXED}/{BASE}/{version}/lib-{version}.pom");
        children.push(
            Curl::call(port, "PUT", &path, &alice, Some(&body), None, None).run_in_background(),
        );
    }
    for child in children {
        assert_eq!(support::wait_for_the_answer(child).status, 201);
    }
    let xml = lab
        .get(port, MIXED, &format!("{BASE}/maven-metadata.xml"))
        .text();
    for index in 0..6 {
        assert!(
            xml.contains(&format!("<version>1.{index}</version>")),
            "{xml}"
        );
    }
    assert!(xml.contains("<latest>1.5</latest>"), "{xml}");
}

#[test]
fn derived_metadata_answers_head_and_conditional_requests() {
    let lab = Lab::new();
    let service = lab.deployment.start(&[]);
    let port = service.port;
    let alice = support::token(FIRST_ACCOUNT, "alice");
    lab.deploy(port, MIXED, "1.0", b"jar");
    let path = format!("/repository/{MIXED}/{BASE}/maven-metadata.xml");

    // HEAD of the metadata and its checksum: real size, no body.
    let body = lab
        .get(port, MIXED, &format!("{BASE}/maven-metadata.xml"))
        .body;
    let head = Curl::describe(port, &path, &alice).run();
    assert_eq!(head.status, 200);
    assert_eq!(head.header("content-length"), Some(body.len().to_string()));
    assert_eq!(head.size, 0);
    let head = Curl::describe(port, &format!("{path}.sha1"), &alice).run();
    assert_eq!(head.status, 200);
    assert_eq!(head.size, 0);

    // With the ETag the service gave, the client receives a 304.
    let etag = Curl::describe(port, &path, &alice)
        .run()
        .header("etag")
        .expect("the metadata carries an ETag");
    let same = Curl::call_with_headers(
        port,
        "GET",
        &path,
        &alice,
        None,
        &[("If-None-Match", &etag)],
        None,
    )
    .run();
    assert_eq!(same.status, 304);
    assert_eq!(same.size, 0);

    // An If-Match that is not the current ETag: 412.
    let stale = Curl::call_with_headers(
        port,
        "GET",
        &path,
        &alice,
        None,
        &[("If-Match", "\"stale\"")],
        None,
    )
    .run();
    assert_eq!(stale.status, 412);

    // After a deployment the content changes and the old ETag is no longer valid.
    lab.deploy(port, MIXED, "1.1", b"jar-two");
    let changed = Curl::call_with_headers(
        port,
        "GET",
        &path,
        &alice,
        None,
        &[("If-None-Match", &etag)],
        None,
    )
    .run();
    assert_eq!(changed.status, 200);
}
