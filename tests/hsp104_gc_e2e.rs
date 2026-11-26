//! Garbage collection against the real binary (HSP-104 / HAR-10).
//!
//! The GC service is already tested from the inside, with an in-memory ledger and
//! injected cuts. This tests what that test cannot: the real
//! binary, with the real catalog and store, the administrative surface
//! behind Cedar, and the restart of a real process.
//!
//! The cases follow the operations UAT (`uat/UAT-OPERATIONS.md`):
//! GC-01 (what is shared is not deleted), GC-02 (what is retained is not deleted and the
//! plan says why), GC-03 (the orphan is deleted exactly once, and only with
//! explicit permission) and GC-04 (after a restart there is no double deletion nor
//! loss).
//!
//! Honest limit: the cut **between** the mark and the sweep cannot be
//! provoked from outside a process without putting a test hook in the
//! production code. Those cuts are injected in
//! `hsp104_gc_service.rs`; here the real process is killed and it is checked
//! that what was already done is not repeated and what was retained stays retained.

mod support;

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;
use support::{Deployment, FIRST_ACCOUNT, FIRST_ONLY_REPOSITORY, FIRST_REPOSITORY, SECOND_ACCOUNT};

/// The person who operates the collection: can do everything.
const ALICE: &str = "alice";

/// The person who can only see the plan.
const BOB: &str = "bob";

/// The grace period of the test deployment.
const GRACE_SECS: u64 = 2;

const OCI_MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";

/// The catalog seeder (repositories per account).
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// The deployment schema, with the administrative surface.
fn the_schema() -> String {
    let mut schema: Value = serde_json::from_str(support::SCHEMA).expect("the schema parses");
    schema["Admin"] = serde_json::json!({
        "entityTypes": { "AdminApi": { "shape": { "type": "Record", "attributes": {} } } },
        "actions": {
            "AccessAdminApi": {
                "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Admin::AdminApi"] }
            }
        }
    });
    schema.to_string()
}

/// The policy: alice operates everything; bob can only request the plan.
///
/// Each administrative route is a distinct resource, and that is why bob's
/// policy names the plan and nothing else.
fn the_policy() -> String {
    format!(
        "{}\npermit(principal == Iam::User::\"hrn:hodei:iam::{FIRST_ACCOUNT}:User/{ALICE}\", action == Admin::Action::\"AccessAdminApi\", resource);\n\
         permit(principal == Iam::User::\"hrn:hodei:iam::{SECOND_ACCOUNT}:User/{BOB}\", action == Admin::Action::\"AccessAdminApi\", resource == Admin::AdminApi::\"hrn:hodei:admin:global:{SECOND_ACCOUNT}:AdminApi/get-gc-plan\");\n",
        support::the_policy()
    )
}

/// A deployment with the catalog seeded and administration open.
fn a_deployment() -> Deployment {
    let deployment = Deployment::new();
    std::fs::write(
        deployment.directory.path().join("schema.json"),
        the_schema(),
    )
    .expect("the schema is written");
    std::fs::write(
        deployment.directory.path().join("policies.cedar"),
        the_policy(),
    )
    .expect("the policy is written");
    deployment.seed();
    deployment
}

/// The environment the service starts with.
fn the_environment() -> Vec<(&'static str, String)> {
    vec![
        ("HODEI_SECURITY__ADMIN_API_ENABLED", "true".to_string()),
        ("HODEI_CONTENT__GC_GRACE_SECS", GRACE_SECS.to_string()),
    ]
}

fn start(deployment: &Deployment, port: Option<u16>) -> support::Service {
    let environment = the_environment();
    let pairs: Vec<(&str, &str)> = environment
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    match port {
        Some(port) => deployment.start_on(port, &pairs),
        None => deployment.start(&pairs),
    }
}

/// The digest of a content, without the algorithm prefix.
fn the_digest(content: &[u8]) -> String {
    hodei_evidence::ContentDigest::of_bytes(content).value
}

/// Where a blob lives in the byte store.
fn blob_path(deployment: &Deployment, digest: &str) -> PathBuf {
    Path::new(&deployment.cas)
        .join(&digest[..2])
        .join(&digest[2..4])
        .join(digest)
}

/// Leaves bytes in the store that no catalog names: what remains if the
/// process dies between writing the content and confirming the catalog. The
/// registry has no deletion, so this is the only way to have a
/// real orphan.
fn plant_orphan(deployment: &Deployment, content: &[u8]) -> String {
    let digest = the_digest(content);
    let path = blob_path(deployment, &digest);
    std::fs::create_dir_all(path.parent().expect("a blob has a directory"))
        .expect("the directory is created");
    std::fs::write(&path, content).expect("the orphan is written");
    digest
}

/// An administrative request. Returns the code and the body as JSON.
fn admin(
    deployment: &Deployment,
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    body: Option<&Value>,
) -> (u16, Value) {
    let request = deployment.directory.path().join("admin-request.json");
    let response = deployment.directory.path().join("admin-response.json");
    let _ = std::fs::remove_file(&response);
    let body_file = body.map(|value| {
        std::fs::write(&request, value.to_string()).expect("the request is written");
        request.as_path()
    });
    let answer = support::Curl::call(
        port,
        method,
        &format!("/api/v1{path}"),
        token,
        body_file,
        body.map(|_| "application/json"),
        Some(&response),
    )
    .run();
    let bytes = std::fs::read(&response).unwrap_or_default();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (answer.status, json)
}

/// Publishes a blob through the OCI distribution, with no manifest naming it.
fn push_blob(deployment: &Deployment, port: u16, token: &str, content: &[u8]) -> String {
    let digest = the_digest(content);
    let file = deployment.directory.path().join("blob.bin");
    std::fs::write(&file, content).expect("the blob is written");
    let answer = support::Curl::call(
        port,
        "POST",
        &format!("/v2/{FIRST_REPOSITORY}/blobs/uploads/?digest=sha256:{digest}"),
        token,
        Some(&file),
        Some("application/octet-stream"),
        None,
    )
    .run();
    assert_eq!(
        answer.status,
        201,
        "the blob was not published:\n{}",
        deployment.the_log()
    );
    digest
}

/// Publishes an image manifest that names a config and a layer.
fn push_image(
    deployment: &Deployment,
    port: u16,
    token: &str,
    tag: &str,
    config: &str,
    layer: &str,
) -> String {
    let manifest = serde_json::json!({
        "schemaVersion": 2,
        "mediaType": OCI_MANIFEST,
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": format!("sha256:{config}"),
            "size": 1
        },
        "layers": [{
            "mediaType": "application/vnd.oci.image.layer.v1.tar",
            "digest": format!("sha256:{layer}"),
            "size": 1
        }]
    })
    .to_string()
    .into_bytes();
    let file = deployment.directory.path().join("manifest.json");
    std::fs::write(&file, &manifest).expect("the manifest is written");
    let answer = support::Curl::call(
        port,
        "PUT",
        &format!("/v2/{FIRST_REPOSITORY}/manifests/{tag}"),
        token,
        Some(&file),
        Some(OCI_MANIFEST),
        None,
    )
    .run();
    assert!(
        answer.status == 201 || answer.status == 200,
        "the manifest was not published ({}):\n{}",
        answer.status,
        deployment.the_log()
    );
    the_digest(&manifest)
}

/// Publishes a file to a raw repository.
fn push_raw(deployment: &Deployment, port: u16, token: &str, repository: &str, content: &[u8]) {
    let file = deployment.directory.path().join("raw.bin");
    std::fs::write(&file, content).expect("the file is written");
    let answer = support::curl_upload(
        port,
        &format!("/repository/{repository}/dir/shared.bin"),
        token,
        &file,
        None,
    );
    assert!(
        answer.status == 201 || answer.status == 200,
        "the raw publication failed ({}):\n{}",
        answer.status,
        deployment.the_log()
    );
}

/// The plan's verdict for a digest.
fn verdict_of<'a>(plan: &'a Value, digest: &str) -> &'a Value {
    plan["entries"]
        .as_array()
        .expect("the plan has entries")
        .iter()
        .find(|entry| entry["digest"] == format!("sha256:{digest}"))
        .unwrap_or_else(|| panic!("the plan does not list sha256:{digest}:\n{plan:#}"))
}

/// What the registry serves for a manifest by digest.
fn manifest_status(port: u16, token: &str, digest: &str) -> u16 {
    support::Curl::call(
        port,
        "GET",
        &format!("/v2/{FIRST_REPOSITORY}/manifests/sha256:{digest}"),
        token,
        None,
        None,
        None,
    )
    .run()
    .status
}

/// GC-01, GC-02 and GC-03: what is shared and what is retained are kept, the plan
/// says why, and the orphan is deleted exactly once with explicit permission.
#[test]
fn shared_and_held_content_survive_and_the_orphan_is_swept_once() {
    let deployment = a_deployment();
    let alice = support::token(FIRST_ACCOUNT, ALICE);
    let bob = support::token(SECOND_ACCOUNT, BOB);
    let mut service = start(&deployment, None);
    let port = service.port;

    // The same content is a file in two repositories and the layer of an
    // image: a single blob with three roots.
    let shared: Vec<u8> = (0u16..2048).map(|value| (value % 241) as u8).collect();
    let shared_digest = the_digest(&shared);
    push_raw(&deployment, port, &alice, FIRST_REPOSITORY, &shared);
    push_raw(&deployment, port, &alice, FIRST_ONLY_REPOSITORY, &shared);
    let config = push_blob(&deployment, port, &alice, br#"{"os":"linux"}"#);
    let layer = push_blob(&deployment, port, &alice, &shared);
    assert_eq!(layer, shared_digest, "the layer is the shared content");
    let manifest = push_image(&deployment, port, &alice, "v1", &config, &layer);

    // A real orphan, and another that has a retention.
    let orphan_bytes = b"orphan: nobody names this blob".to_vec();
    let orphan = plant_orphan(&deployment, &orphan_bytes);
    let held = plant_orphan(&deployment, b"held: legal hold on this blob");

    let placed = admin(
        &deployment,
        port,
        "POST",
        "/gc/holds",
        &alice,
        Some(&serde_json::json!({
            "scope": { "kind": "digest", "digest": format!("sha256:{held}") },
            "reason": "legal hold for case 4711"
        })),
    );
    assert_eq!(placed.0, 201, "the hold was not placed: {:#}", placed.1);
    let hold_id = placed.1["id"]
        .as_str()
        .expect("the hold has an id")
        .to_string();

    // The grace passes and a new one comes in: that one is still within it.
    std::thread::sleep(Duration::from_secs(GRACE_SECS + 1));
    let fresh = plant_orphan(&deployment, b"fresh: uploaded a moment ago");

    // --- The plan explains each verdict.
    let (status, plan) = admin(&deployment, port, "GET", "/gc/plan", &alice, None);
    assert_eq!(status, 200, "the plan failed:\n{}", deployment.the_log());
    assert_eq!(plan["grace_secs"], GRACE_SECS);
    for kept in [&shared_digest, &config, &manifest] {
        assert_eq!(
            verdict_of(&plan, kept)["verdict"],
            "retained",
            "{kept} is referenced and must be retained"
        );
    }
    let shared_entry = verdict_of(&plan, &shared_digest);
    let roots = shared_entry["reasons"]
        .as_array()
        .expect("a retained blob lists why");
    assert!(
        roots.len() >= 3,
        "the shared blob is kept by two assets and a manifest: {shared_entry:#}"
    );
    let held_entry = verdict_of(&plan, &held);
    assert_eq!(held_entry["verdict"], "retained");
    assert_eq!(
        held_entry["reasons"][0]["kind"], "hold",
        "the plan names the hold as the cause: {held_entry:#}"
    );
    assert_eq!(held_entry["reasons"][0]["id"], hold_id.as_str());
    assert_eq!(verdict_of(&plan, &fresh)["verdict"], "in_grace");
    let orphan_entry = verdict_of(&plan, &orphan);
    assert_eq!(orphan_entry["verdict"], "reclaimable");
    assert_eq!(orphan_entry["length"], orphan_bytes.len());
    assert_eq!(plan["summary"]["reclaimable"], 1);
    assert_eq!(
        plan["summary"]["reclaimable_bytes"],
        orphan_bytes.len() as u64
    );

    // --- Whoever can only see the plan cannot sweep.
    let (status, _) = admin(&deployment, port, "GET", "/gc/plan", &bob, None);
    assert_eq!(status, 200, "bob may read the plan");
    let (status, _) = admin(
        &deployment,
        port,
        "POST",
        "/gc/sweep",
        &bob,
        Some(&serde_json::json!({ "execute": true })),
    );
    assert_eq!(status, 403, "bob may not sweep");
    let (status, _) = admin(&deployment, port, "GET", "/gc/plan", "", None);
    assert_eq!(status, 401, "no credential, no plan");
    assert!(blob_path(&deployment, &orphan).exists());

    // --- Without explicit permission (`execute`), the sweep is the dry-run.
    let (status, dry) = admin(
        &deployment,
        port,
        "POST",
        "/gc/sweep",
        &alice,
        Some(&serde_json::json!({})),
    );
    assert_eq!(status, 200);
    assert_eq!(dry["dry_run"], true);
    assert!(
        blob_path(&deployment, &orphan).exists(),
        "a dry-run deletes nothing"
    );

    // --- With explicit permission, it deletes the orphan and only the orphan.
    let (status, swept) = admin(
        &deployment,
        port,
        "POST",
        "/gc/sweep",
        &alice,
        Some(&serde_json::json!({ "execute": true })),
    );
    assert_eq!(
        status,
        200,
        "the sweep failed: {swept:#}\n{}",
        deployment.the_log()
    );
    assert_eq!(swept["dry_run"], false);
    assert_eq!(swept["report"]["deleted"], 1);
    assert_eq!(swept["report"]["bytes_freed"], orphan_bytes.len());
    assert!(
        !blob_path(&deployment, &orphan).exists(),
        "the orphan is gone"
    );
    for alive in [&shared_digest, &config, &manifest, &held, &fresh] {
        assert!(
            blob_path(&deployment, alive).exists(),
            "{alive} must survive the sweep"
        );
    }
    assert_eq!(
        manifest_status(port, &alice, &manifest),
        200,
        "the manifest still serves after the sweep"
    );

    // --- A second pass finds nothing to delete.
    let (status, again) = admin(
        &deployment,
        port,
        "POST",
        "/gc/sweep",
        &alice,
        Some(&serde_json::json!({ "execute": true })),
    );
    assert_eq!(status, 200);
    assert_eq!(again["report"]["deleted"], 0, "nothing is deleted twice");

    support::stop(&mut service);
}

/// GC-04: after killing the process, what was done is not repeated, the retention remains
/// and releasing it makes reclaimable what it protected.
#[test]
fn a_killed_service_keeps_its_holds_and_does_not_sweep_twice() {
    let deployment = a_deployment();
    let alice = support::token(FIRST_ACCOUNT, ALICE);
    let mut service = start(&deployment, None);
    let port = service.port;

    let kept = plant_orphan(&deployment, b"kept by a hold across restarts");
    let gone = plant_orphan(&deployment, b"orphan that the sweep removes");
    let (status, placed) = admin(
        &deployment,
        port,
        "POST",
        "/gc/holds",
        &alice,
        Some(&serde_json::json!({
            "scope": { "kind": "digest", "digest": format!("sha256:{kept}") },
            "reason": "keep until the audit closes"
        })),
    );
    assert_eq!(status, 201, "{placed:#}");
    let hold_id = placed["id"].as_str().expect("an id").to_string();
    std::thread::sleep(Duration::from_secs(GRACE_SECS + 1));

    let (status, swept) = admin(
        &deployment,
        port,
        "POST",
        "/gc/sweep",
        &alice,
        Some(&serde_json::json!({ "execute": true })),
    );
    assert_eq!(status, 200, "{swept:#}\n{}", deployment.the_log());
    assert_eq!(swept["report"]["deleted"], 1);
    assert!(!blob_path(&deployment, &gone).exists());

    // The process dies abruptly, like a machine that is switched off.
    support::kill(&mut service);
    let mut service = start(&deployment, Some(port));

    let (status, holds) = admin(&deployment, port, "GET", "/gc/holds", &alice, None);
    assert_eq!(status, 200);
    assert!(
        holds["holds"]
            .as_array()
            .expect("a list")
            .iter()
            .any(|hold| hold["id"] == hold_id.as_str()),
        "the hold survived the kill: {holds:#}"
    );
    let (_, plan) = admin(&deployment, port, "GET", "/gc/plan", &alice, None);
    assert_eq!(verdict_of(&plan, &kept)["verdict"], "retained");
    assert!(
        plan["entries"]
            .as_array()
            .expect("entries")
            .iter()
            .all(|entry| entry["digest"] != format!("sha256:{gone}")),
        "what was swept does not come back"
    );
    let (status, again) = admin(
        &deployment,
        port,
        "POST",
        "/gc/sweep",
        &alice,
        Some(&serde_json::json!({ "execute": true })),
    );
    assert_eq!(status, 200);
    assert_eq!(again["report"]["deleted"], 0, "no double deletion");
    assert!(
        blob_path(&deployment, &kept).exists(),
        "the hold still holds"
    );

    // Releasing the retention makes reclaimable what it protected, and the next
    // sweep deletes it.
    let (status, _) = admin(
        &deployment,
        port,
        "POST",
        "/gc/holds/release",
        &alice,
        Some(&serde_json::json!({ "id": hold_id })),
    );
    assert_eq!(status, 200, "the release failed");
    let (status, _) = admin(
        &deployment,
        port,
        "POST",
        "/gc/holds/release",
        &alice,
        Some(&serde_json::json!({ "id": hold_id })),
    );
    assert_eq!(status, 404, "releasing twice is not a second release");
    let (_, plan) = admin(&deployment, port, "GET", "/gc/plan", &alice, None);
    assert_eq!(verdict_of(&plan, &kept)["verdict"], "reclaimable");
    let (_, swept) = admin(
        &deployment,
        port,
        "POST",
        "/gc/sweep",
        &alice,
        Some(&serde_json::json!({ "execute": true })),
    );
    assert_eq!(swept["report"]["deleted"], 1);
    assert!(!blob_path(&deployment, &kept).exists());

    support::stop(&mut service);
}

/// An invalid request deletes nothing and says why.
#[test]
fn an_invalid_hold_is_refused() {
    let deployment = a_deployment();
    let alice = support::token(FIRST_ACCOUNT, ALICE);
    let mut service = start(&deployment, None);
    let port = service.port;

    let (status, body) = admin(
        &deployment,
        port,
        "POST",
        "/gc/holds",
        &alice,
        Some(&serde_json::json!({
            "scope": { "kind": "account", "account": FIRST_ACCOUNT },
            "reason": "   "
        })),
    );
    assert_eq!(status, 400, "a hold needs a reason: {body:#}");
    let (status, _) = admin(
        &deployment,
        port,
        "POST",
        "/gc/holds/release",
        &alice,
        Some(&serde_json::json!({ "id": "hold-does-not-exist" })),
    );
    assert_eq!(status, 404);

    support::stop(&mut service);
}
