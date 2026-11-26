//! HSP-124's per-account quotas, **black box**, over the real hub.
//!
//! What is looked at here is not the ledger (the pure test already covers that),
//! but what a real client sees:
//!
//! - That the `413` arrives **before** the rejected content exists:
//!   the rejected path returns `404`, and the content that did get in is
//!   downloaded intact.
//! - That two simultaneous uploads that do not both fit **never both
//!   fit**: one `201` and one `413`, and the rejected one's reservation is
//!   released -- afterwards new content fits.
//! - That saturation answers `429` with `Retry-After`, and that cancelling
//!   the saturating session **releases** the slot.
//! - That `GET /api/v1/account/usage` reports the own account with its own
//!   numbers: the other account's are zeros, and without a credential there is no
//!   response.
//! - That content the account already had is not charged twice: `usage`
//!   reports it with the same number before and after.

mod support;

use std::path::{Path, PathBuf};

use support::{Answer, Deployment, FIRST_ACCOUNT, FIRST_REPOSITORY, SECOND_ACCOUNT};

#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// A path of the seeded repository's raw surface.
fn the_raw_path(target: &str) -> String {
    format!("/repository/{FIRST_REPOSITORY}/{target}")
}

/// A deployment with the catalog seeded.
fn a_deployment() -> Deployment {
    let deployment = Deployment::new();
    deployment.seed();
    deployment
}

/// Content of the requested size, different per seed.
fn the_content(size: usize, seed: u8) -> Vec<u8> {
    (0..size)
        .map(|index| (index as u8).wrapping_add(seed))
        .collect()
}

/// Writes an upload file and returns its path.
fn the_file_in(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("the body is written");
    path
}

/// A raw `PUT` of the given file, at the requested rate.
fn publish(port: u16, target: &str, token: &str, file: &Path, limit_rate: Option<&str>) -> Answer {
    let path = the_raw_path(target);
    support::Curl::upload(port, &path, token, file, limit_rate).run()
}

/// A raw `GET`.
fn download(port: u16, target: &str, token: &str) -> Answer {
    let path = the_raw_path(target);
    let into = Path::new(env!("CARGO_TARGET_TMPDIR")).join("downloaded.bin");
    support::Curl::download(port, &path, token, Some(&into)).run()
}

/// A token's `usage`, as JSON.
fn usage(port: u16, token: &str) -> serde_json::Value {
    let into = Path::new(env!("CARGO_TARGET_TMPDIR")).join("usage.json");
    let answer = support::Curl::call(
        port,
        "GET",
        "/api/v1/account/usage",
        token,
        None,
        None,
        Some(&into),
    )
    .run();
    assert_eq!(answer.status, 200, "the usage answer:\n{}", answer.headers);
    serde_json::from_slice(&answer.body).expect("the usage is json")
}

// ---------------------------------------------------------------------------
// The storage quota: before saving, and without cheating on delete.
// ---------------------------------------------------------------------------

/// The `413` says no **before** the rejected content exists.
///
/// That is the difference between a quota and waste with an apology: if
/// the rejected content were left published, the `no` would be cosmetic.
#[test]
fn the_stored_quota_refuses_before_anything_lands() {
    let deployment = a_deployment();
    let mut service = deployment.start(&[("HODEI_QUOTA__MAX_STORED_BYTES_PER_ACCOUNT", "1000")]);
    let port = service.port;
    let token = support::token(FIRST_ACCOUNT, "alice");
    let dir = deployment.directory.path();

    let first = the_file_in(dir, "first.bin", &the_content(600, 1));
    assert_eq!(
        publish(port, "one/first.bin", &token, &first, None).status,
        201,
        "the first upload must fit:\n{}",
        deployment.the_log()
    );

    let second = the_file_in(dir, "second.bin", &the_content(600, 2));
    let refused = publish(port, "two/second.bin", &token, &second, None);
    assert_eq!(
        refused.status,
        413,
        "six hundred more must not fit in a thousand:\n{}",
        deployment.the_log()
    );
    assert_eq!(
        refused.header("retry-after"),
        None,
        "a stored quota does not come back: sending later changes nothing"
    );

    // What was rejected does not exist. If it existed, the quota would be a warning.
    assert_eq!(
        download(port, "two/second.bin", &token).status,
        404,
        "the refused content must not have landed"
    );
    assert_eq!(
        download(port, "one/first.bin", &token).status,
        200,
        "the content that fit must still be served"
    );

    // And there is room left for what does fit: the rejection punishes nothing.
    let small = the_file_in(dir, "small.bin", &the_content(50, 3));
    assert_eq!(
        publish(port, "three/small.bin", &token, &small, None).status,
        201,
        "fifty bytes still fit under a thousand"
    );
    let over = the_file_in(dir, "over.bin", &the_content(500, 4));
    assert_eq!(
        publish(port, "four/over.bin", &token, &over, None).status,
        413,
        "six hundred and fifty would pass the thousand"
    );
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// The race: two uploads that do not both fit.
// ---------------------------------------------------------------------------

/// Two simultaneous uploads that do not both fit **never** both fit.
///
/// The body is deliberately slow (`--limit-rate`) so that the two
/// really run overlapped: what is tested is check-and-reserve
/// under a single lock, and with four-kilobyte bodies there would be no
/// overlap to test.
#[test]
fn two_concurrent_uploads_that_do_not_both_fit_never_both_land() {
    let deployment = a_deployment();
    let mut service = deployment.start(&[("HODEI_QUOTA__MAX_STORED_BYTES_PER_ACCOUNT", "1000")]);
    let port = service.port;
    let token = support::token(FIRST_ACCOUNT, "alice");
    let dir = deployment.directory.path();

    let one = the_file_in(dir, "one.bin", &the_content(800, 10));
    let two = the_file_in(dir, "two.bin", &the_content(800, 20));

    let first_token = token.clone();
    let one_path = one.clone();
    let first = std::thread::spawn(move || {
        publish(port, "race/one.bin", &first_token, &one_path, Some("100"))
    });
    std::thread::sleep(std::time::Duration::from_secs(2));
    let second_token = token.clone();
    let two_path = two.clone();
    let second = std::thread::spawn(move || {
        publish(port, "race/two.bin", &second_token, &two_path, Some("100"))
    });

    let statuses = [first.join().unwrap().status, second.join().unwrap().status];
    let sorted = {
        let mut v = statuses.to_vec();
        v.sort_unstable();
        v
    };
    assert_eq!(
        sorted,
        [201, 413],
        "two eight-hundreds never fit in a thousand: exactly one may land (got {statuses:?})\n{}",
        deployment.the_log()
    );

    // The rejected one's reservation was released on abort: the margin that
    // remains is that of the content that landed, not of both.
    let room = the_file_in(dir, "room.bin", &the_content(100, 30));
    assert_eq!(
        publish(port, "after/room.bin", &token, &room, None).status,
        201,
        "the refused upload must have released its reservation"
    );
    let more = the_file_in(dir, "more.bin", &the_content(200, 40));
    assert_eq!(
        publish(port, "after/more.bin", &token, &more, None).status,
        413,
        "eight hundred landed: two hundred more must not fit"
    );
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// Saturation: honest advice and release on cancel.
// ---------------------------------------------------------------------------

/// The third concurrent upload receives `429` with `Retry-After`, and the
/// slot is released on **cancelling** the saturating session.
///
/// The OCI session lives between requests -- it is the real case of sustained
/// saturation -- and its ticket travels inside the session: cancelling it is
/// releasing the reservation.
#[test]
fn the_saturation_advises_when_to_come_back_and_cancelling_releases() {
    let deployment = a_deployment();
    let mut service = deployment.start(&[
        ("HODEI_QUOTA__MAX_CONCURRENT_UPLOADS_PER_ACCOUNT", "1"),
        ("HODEI_QUOTA__RETRY_AFTER_SECS", "42"),
    ]);
    let port = service.port;
    let token = support::token(FIRST_ACCOUNT, "alice");

    let opened = support::Curl::call(
        port,
        "POST",
        &format!("/v2/{FIRST_REPOSITORY}/blobs/uploads/"),
        &token,
        None,
        None,
        None,
    )
    .run();
    assert_eq!(
        opened.status,
        202,
        "the session must open (it holds the only slot):\n{}",
        deployment.the_log()
    );
    let location = opened
        .header("location")
        .expect("the opened session has a location");

    let small = the_file_in(
        deployment.directory.path(),
        "small.bin",
        &the_content(10, 5),
    );
    let refused = publish(port, "busy/small.bin", &token, &small, None);
    assert_eq!(
        refused.status,
        429,
        "the slot is taken by the open session:\n{}",
        deployment.the_log()
    );
    assert_eq!(
        refused.header("retry-after").as_deref(),
        Some("42"),
        "the refusal must say when to come back"
    );

    // Cancelling is releasing: the reservation travels with the session.
    let cancelled = support::Curl::call(port, "DELETE", &location, &token, None, None, None).run();
    assert!(
        (200..300).contains(&cancelled.status),
        "cancelling the session must succeed (got {}):\n{}",
        cancelled.status,
        deployment.the_log()
    );

    let answer = publish(port, "free/small.bin", &token, &small, None);
    assert_eq!(
        answer.status,
        201,
        "the freed slot must admit the upload that was refused:\n{}",
        deployment.the_log()
    );
    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// Observability: the own account, and only the own account.
// ---------------------------------------------------------------------------

/// `usage` says what the quota says, for the ledger's owner.
///
/// The other account's numbers are not visible through any hole: no
/// account parameter and with zeros where the other has bytes. And
/// content that was already there is not charged twice -- that is what
/// `usage` can demonstrate and a `201` cannot.
#[test]
fn the_usage_says_the_own_account_and_nobody_elses() {
    let deployment = a_deployment();
    let mut service = deployment.start(&[
        ("HODEI_QUOTA__MAX_STORED_BYTES_PER_ACCOUNT", "2000"),
        ("HODEI_QUOTA__MAX_CONCURRENT_UPLOADS_PER_ACCOUNT", "3"),
    ]);
    let port = service.port;
    let alice = support::token(FIRST_ACCOUNT, "alice");
    let bob = support::token(SECOND_ACCOUNT, "bob");

    let unsigned =
        support::Curl::call(port, "GET", "/api/v1/account/usage", "", None, None, None).run();
    assert_eq!(unsigned.status, 401, "the usage asks for the credential");
    assert_eq!(
        unsigned.header("www-authenticate").as_deref(),
        Some("Bearer"),
        "the `401` has to name the scheme"
    );

    let content = the_content(600, 7);
    let file = the_file_in(deployment.directory.path(), "seven.bin", &content);
    assert_eq!(
        publish(port, "usage/seven.bin", &alice, &file, None).status,
        201,
        "the upload that fills the ledger must land:\n{}",
        deployment.the_log()
    );

    let mine = usage(port, &alice);
    assert_eq!(mine["account"], FIRST_ACCOUNT);
    assert_eq!(mine["stored_bytes"], 600);
    assert_eq!(mine["in_flight_bytes"], 0);
    assert_eq!(mine["active_uploads"], 0);
    assert_eq!(mine["limits"]["max_stored_bytes_per_account"], 2000);
    assert_eq!(mine["limits"]["max_concurrent_uploads_per_account"], 3);

    // The same sum, on another path: the `CAS` already had it and the ledger can
    // say so. A `usage` that went up would be an accounting lie.
    let again = the_file_in(deployment.directory.path(), "seven-again.bin", &content);
    assert_eq!(
        publish(port, "usage/seven-again.bin", &alice, &again, None).status,
        201,
        "re-publishing owned content must not be refused"
    );
    assert_eq!(usage(port, &alice)["stored_bytes"], 600);

    // Bob does not see anyone's numbers: his are zeros.
    let theirs = usage(port, &bob);
    assert_eq!(theirs["account"], SECOND_ACCOUNT);
    assert_eq!(theirs["stored_bytes"], 0);
    assert_eq!(theirs["in_flight_bytes"], 0);
    assert_eq!(theirs["active_uploads"], 0);
    support::stop(&mut service);
}
