//! The validity of trusted-channel grants (HSP-135), against the
//! wire.
//!
//! Promoting is not granting forever: the grant is born with a horizon
//! and when it closes the content returns to quarantine without anyone
//! revoking it. And the horizon is a property **written** alongside the
//! grant: it survives a service restart, and a new promotion reopens it
//! with a new decision.
//!
//! The tests talk to a real process and a real catalog. The
//! horizon is shortened through configuration (`GRANT_MAX_VALIDITY_SECS=1`) and
//! really waited out: what is measured is the wire, not a clock injected into a
//! layer nobody asks.

mod support;

use support::{Deployment, FIRST_ACCOUNT, FIRST_REPOSITORY};

/// The repository that stays in the open channel.
const OPEN_REPOSITORY: &str = "lab";

/// The digest of a content, without the algorithm prefix.
fn hex_of(content: &[u8]) -> String {
    hodei_evidence::ContentDigest::of_bytes(content).value
}

/// Any request, with the response body in a file.
fn call(
    deployment: &Deployment,
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    body: Option<(&[u8], &str)>,
) -> (u16, String) {
    let directory = deployment.directory.path();
    let into = directory.join("response.bin");
    let _ = std::fs::remove_file(&into);
    let sent = directory.join("request.bin");
    let (file, content_type) = match body {
        Some((bytes, content_type)) => {
            std::fs::write(&sent, bytes).expect("the body is written");
            (Some(sent.as_path()), Some(content_type))
        }
        None => (None, None),
    };
    let answer = if method == "HEAD" {
        support::Curl::describe(port, path, token).run()
    } else {
        support::Curl::call(port, method, path, token, file, content_type, Some(&into)).run()
    };
    let text = String::from_utf8_lossy(&std::fs::read(&into).unwrap_or_default()).into_owned();
    (answer.status, text)
}

/// Uploads a raw file.
fn put_raw(deployment: &Deployment, port: u16, token: &str, path: &str, bytes: &[u8]) -> u16 {
    call(
        deployment,
        port,
        "PUT",
        path,
        token,
        Some((bytes, "application/octet-stream")),
    )
    .0
}

/// Requests promotion and returns the code and the body.
fn promote(
    deployment: &Deployment,
    port: u16,
    token: &str,
    path: &str,
    digest_hex: &str,
) -> (u16, String) {
    let body = serde_json::json!({
        "repository": FIRST_REPOSITORY,
        "path": path,
        "digest": format!("sha256:{digest_hex}"),
    })
    .to_string();
    call(
        deployment,
        port,
        "POST",
        "/api/v1/promotions",
        token,
        Some((body.as_bytes(), "application/json")),
    )
}

/// Writes a trust policy that all software meets.
fn permissive(deployment: &Deployment) -> String {
    let policy = hodei_trust_policy::TrustPolicy::new("default", "1", Vec::new());
    let path = deployment.directory.path().join("trust-policy.json");
    std::fs::write(&path, serde_json::to_vec(&policy).expect("the policy")).expect("written");
    path.to_string_lossy().into_owned()
}

/// A deployment with `releases` in the trusted channel and `lab` open.
fn a_deployment() -> Deployment {
    let deployment = Deployment::new();
    deployment.seed_trusted(&[OPEN_REPOSITORY], &[FIRST_REPOSITORY]);
    deployment
}

fn raw(path: &str) -> String {
    format!("/repository/{FIRST_REPOSITORY}/{path}")
}

/// The catalog seeder.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

// ---------------------------------------------------------------------------
// AC2: validity runs out, and the exhaustion shows on the wire.
// ---------------------------------------------------------------------------

#[test]
fn a_promotion_serves_until_its_horizon_closes_and_a_new_one_reopens_it() {
    let deployment = a_deployment();
    let policy = permissive(&deployment);
    let mut service = deployment.start(&[
        ("HODEI_TRUST__POLICY_FILE", &policy),
        ("HODEI_TRUST__GRANT_MAX_VALIDITY_SECS", "1"),
    ]);
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let path = raw("app/aging.bin");
    let bytes = b"release with a horizon".to_vec();
    let digest = hex_of(&bytes);
    assert_eq!(
        put_raw(&deployment, service.port, &alice, &path, &bytes),
        201
    );

    let (status, body) = promote(&deployment, service.port, &alice, "app/aging.bin", &digest);
    assert_eq!(status, 200, "{body}");
    let (status, served) = call(&deployment, service.port, "GET", &path, &alice, None);
    assert_eq!(status, 200, "the fresh promotion does not serve");
    assert_eq!(served.as_bytes(), bytes.as_slice());

    // The horizon closes: with no revocation in between, the content
    // goes back to being quarantine. Nobody said "no": simply nobody says it anymore.
    std::thread::sleep(std::time::Duration::from_millis(1500));
    let (status, _) = call(&deployment, service.port, "GET", &path, &alice, None);
    assert_eq!(status, 404, "an expired grant was served");

    // A new promotion reopens the horizon with a new decision.
    let (status, body) = promote(&deployment, service.port, &alice, "app/aging.bin", &digest);
    assert_eq!(status, 200, "{body}");
    let (status, _) = call(&deployment, service.port, "GET", &path, &alice, None);
    assert_eq!(status, 200, "the re-promotion does not serve");
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// AC4: validity is persisted state, not process memory.
// ---------------------------------------------------------------------------

#[test]
fn a_promotion_survives_a_restart_with_its_validity() {
    let deployment = a_deployment();
    let policy = permissive(&deployment);
    let environment = [("HODEI_TRUST__POLICY_FILE", policy.as_str())];
    let mut service = deployment.start(&environment);
    let port = service.port;
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let path = raw("app/durable.bin");
    let bytes = b"promised to last".to_vec();
    let digest = hex_of(&bytes);
    assert_eq!(put_raw(&deployment, port, &alice, &path, &bytes), 201);
    let (status, body) = promote(&deployment, port, &alice, "app/durable.bin", &digest);
    assert_eq!(status, 200, "{body}");
    support::stop(&mut service);

    // Another process, the same database: the grant still admits because its
    // validity travels written with it, not in the memory of whoever issued it.
    let mut service = deployment.start_on(port, &environment);
    let (status, served) = call(&deployment, service.port, "GET", &path, &alice, None);
    assert_eq!(status, 200, "the grant did not survive the restart");
    assert_eq!(served.as_bytes(), bytes.as_slice());
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// AC1: validity is not negotiated through the request body.
// ---------------------------------------------------------------------------

#[test]
fn the_client_cannot_ask_for_a_longer_leash() {
    let deployment = a_deployment();
    let policy = permissive(&deployment);
    let mut service = deployment.start(&[
        ("HODEI_TRUST__POLICY_FILE", &policy),
        ("HODEI_TRUST__GRANT_MAX_VALIDITY_SECS", "1"),
    ]);
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let path = raw("app/impudent.bin");
    let bytes = b"who decides how long".to_vec();
    let digest = hex_of(&bytes);
    assert_eq!(
        put_raw(&deployment, service.port, &alice, &path, &bytes),
        201
    );

    // An extra field in the body does not open the promotion nor extend anything:
    // it is either ignored and promoted with the server's horizon, or
    // rejected. What cannot happen is the client setting the validity.
    let body = serde_json::json!({
        "repository": FIRST_REPOSITORY,
        "path": "app/impudent.bin",
        "digest": format!("sha256:{digest}"),
        "valid_until": "2999-01-01T00:00:00Z",
    })
    .to_string();
    let (status, _) = call(
        &deployment,
        service.port,
        "POST",
        "/api/v1/promotions",
        &alice,
        Some((body.as_bytes(), "application/json")),
    );
    assert!(
        status == 200 || status == 400,
        "the client negotiated its horizon: {status}"
    );
    if status == 200 {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        let (status, _) = call(&deployment, service.port, "GET", &path, &alice, None);
        assert_eq!(status, 404, "the client's horizon outlived the server's");
    }
    support::stop(&mut service);
}
