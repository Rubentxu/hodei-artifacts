//! The conformity of the raw content surface, **black box**.
//!
//! There are no mocks here: there is a real process, which is the service's
//! binary, started on a real port, and there are system tools
//! talking to it: `curl` and `sha256sum`.
//!
//! What is looked at is not a Rust function, but what a client
//! sees:
//!
//! - That what is published comes back **intact**: the hash of the file that
//!   comes out of `curl` is the hash of the file that went in, and it is stated by a
//!   program that has nothing to do with this service.
//! - That a process that **dies in the middle of an upload** does not leave a
//!   half-published path, and that on restarting the service picks up
//!   what was left and respects what is alive.
//! - That two simultaneous uploads **do not get mixed**: what is downloaded
//!   has the hash of what was uploaded, always.
//! - That two accounts with real tokens **do not see** each other, and that the
//!   account that does not have the repository cannot know whether it exists.
//!
//! And the result is written to a report, which is what remains six
//! months from now.

mod support;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use support::{
    Case, Deployment, FIRST_ACCOUNT, FIRST_ONLY_REPOSITORY, FIRST_REPOSITORY, SECOND_ACCOUNT,
    SECOND_REPOSITORY,
};

/// The content of the first person: bytes that are not text, so that
/// the comparison is of bytes and not of lines.
fn the_content() -> Vec<u8> {
    (0u16..4096).map(|value| (value % 251) as u8).collect()
}

/// A different content for each writer, so that a mix
/// is noticed: each one has its own hash.
fn the_content_of(seed: u8) -> Vec<u8> {
    (0u16..8192)
        .map(|value| ((value as u8).wrapping_add(seed)) % 251)
        .collect()
}

/// A large content, so that the uploads really overlap.
///
/// A four-kilobyte body is uploaded before the next
/// `curl` connects, and then what is measured is not concurrency but
/// the idempotence of republishing.
fn a_big_content(seed: u8) -> Vec<u8> {
    the_content_of(seed).repeat(512)
}

/// The catalog seeder.
///
/// The embedded engine holds the lock on the database directory
/// while a connection is open, and the service is another process: the
/// seeding has to be in a process that ends. This test is the
/// one that runs in that process, and the rest of the time it does
/// nothing.
#[test]
fn the_catalog_can_be_seeded() {
    support::seed_if_asked();
}

/// A clean deployment with the catalog seeded.
fn a_deployment() -> Deployment {
    let deployment = Deployment::new();
    deployment.seed();
    deployment
}

/// The content's path inside the repository.
fn the_path(repository: &str, path: &str) -> String {
    format!("/repository/{repository}/{path}")
}

/// The report case, before running it.
///
/// It is recorded twice: at the start, with what is expected, and at the end,
/// with what was measured. If the test dies in between, the
/// report keeps the first record, which says `FAILURE`. A
/// report that only knows how to say `PASS` is not a report.
fn the_case(id: &str, given: &str, when: &str, command: &str) -> Case {
    Case {
        id: id.to_string(),
        given: given.to_string(),
        when: when.to_string(),
        command: command.to_string(),
        outcome: "en curso".to_string(),
        passed: false,
    }
}

/// A case's commands, as they were executed.
///
/// The report column carries the **real** command: what was executed,
/// with its flags. A summarized command is a paraphrase, and a
/// paraphrase is an opinion.
///
/// The credential is **redacted**: the report says `<alice>` where the
/// `curl` carried the token. A report is archived and shared, and a
/// token in a file is a credential in a file. What is
/// written is who acted, not what they authenticated with.
fn the_commands(curls: &[&support::Curl]) -> String {
    curls
        .iter()
        .map(|curl| curl.shown())
        .collect::<Vec<_>>()
        .join(" ; ")
}

// ---------------------------------------------------------------------------
// RAW-01 and RAW-02 — publish, download, describe and request a range.
// ---------------------------------------------------------------------------

/// Publishing and downloading give the same content, and HEAD describes it without
/// fetching it.
///
/// The hash is stated by `sha256sum`, not by the code under test.
#[test]
fn the_content_that_is_published_comes_back_intact() {
    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let port = service.port;
    let token = support::token(FIRST_ACCOUNT, "alice");
    let source = deployment.directory.path().join("source.bin");
    std::fs::write(&source, the_content()).expect("the source is written");
    let expected = support::sha256_of(&source);
    let path = the_path(FIRST_REPOSITORY, "dir/file.bin");

    // The `curl`s are built first: the report and the execution come
    // from the same construction, so the report cannot lie.
    let download_path = deployment.directory.path().join("downloaded.bin");
    let range_path = deployment.directory.path().join("ranged.bin");
    let upload = support::Curl::upload(port, &path, &token, &source, None);
    let download = support::Curl::download(port, &path, &token, Some(&download_path));
    let describe = support::Curl::describe(port, &path, &token);
    let range =
        support::Curl::with_header(port, &path, &token, ("range", "bytes=0-9"), &range_path);

    let mut case = the_case(
        "RAW-01, RAW-02",
        "un repositorio hosted limpio y un token válido",
        "se publica, se descarga, se describe y se pide un tramo con curl",
        &format!(
            "{} ; {}",
            the_commands(&[&upload, &download, &describe, &range]),
            support::sha256_command(&download_path)
        ),
    );
    support::record(case.clone());

    // The report's command has to be the one that is executed, with its
    // flags: a `HEAD` that is not in the command cannot be repeated.
    assert!(
        case.command.contains("--head"),
        "the command of the report does not have the HEAD: {}",
        case.command
    );
    assert!(
        case.command.contains("range: bytes=0-9"),
        "the command of the report does not have the range: {}",
        case.command
    );
    assert!(
        !case.command.contains(token.as_str()),
        "the real token must never reach the report: {}",
        case.command
    );

    let published = upload.run();
    assert_eq!(
        published.status,
        201,
        "the publication failed:\n{}",
        deployment.the_log()
    );

    let answer = download.run();
    assert_eq!(answer.status, 200);
    let measured = support::sha256_of(&download_path);
    assert_eq!(
        measured, expected,
        "the bytes that came back are not the bytes that went in"
    );

    // And the header says what the content measures, with the digest inside.
    let etag = answer.header("etag");
    assert_eq!(
        etag,
        Some(format!("\"sha256:{expected}\"")),
        "the ETag has to be the digest of the content"
    );

    // HEAD describes without fetching: same status, same size, and **zero**
    // body bytes. The zero is counted by `curl` itself.
    let described = describe.run();
    assert_eq!(described.status, 200);
    assert_eq!(
        described.header("content-length"),
        Some((the_content().len()).to_string()),
        "the HEAD has to say the size of the content"
    );
    assert_eq!(
        described.size, 0,
        "the HEAD has to come without a body, and the curl counted {} bytes",
        described.size
    );

    // And the range is a 206 with its Content-Range, and with **its bytes**: a
    // 206 with the right Content-Range and another asset's content
    // would be a range that lies.
    let ranged = range.run();
    assert_eq!(ranged.status, 206);
    assert_eq!(
        ranged.header("content-range"),
        Some(format!("bytes 0-9/{}", the_content().len()))
    );
    let ranged_bytes = std::fs::read(&range_path).expect("the range is written");
    assert_eq!(
        ranged_bytes,
        &the_content()[0..10],
        "the bytes of the range are not the bytes of the content"
    );
    assert_eq!(ranged.size, 10, "the range is ten bytes long");

    case.outcome = format!(
        "sha256sum idéntico ({measured}), ETag {}, HEAD {} con {} bytes de cuerpo, Range {} con {} y sus 10 bytes",
        etag.clone().unwrap_or_default(),
        described.status,
        described.size,
        ranged.status,
        ranged.header("content-range").unwrap_or_default()
    );
    case.passed = true;
    support::record(case);

    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// RAW-06 — the process dies in the middle of an upload.
// ---------------------------------------------------------------------------

/// A process that dies in the middle of an upload does not leave a
/// half-published path, and on restarting the service it picks up what was left.
///
/// The `curl` is rate-limited so that the upload lasts long enough
/// to be able to kill the service with the body half sent, and the harness waits
/// for the staging to **have bytes** before killing: an empty staging
/// means the upload had not started arriving, and killing then is not
/// killing an upload halfway.
#[test]
fn an_upload_that_is_interrupted_leaves_nothing_published() {
    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let port = service.port;
    let token = support::token(FIRST_ACCOUNT, "alice");

    let cut_path = the_path(FIRST_REPOSITORY, "cut/file.bin");
    let keep_path = the_path(FIRST_REPOSITORY, "keep/file.bin");
    let published_path = deployment.directory.path().join("published.bin");
    let big_path = deployment.directory.path().join("big.bin");
    let after_path = deployment.directory.path().join("after.bin");

    // The cut shows in the command: the limited `curl` that stays
    // in progress, the `kill` and the real startup.
    let cut_upload = support::Curl::upload(port, &cut_path, &token, &big_path, Some("256K"));
    let keep_upload = support::Curl::upload(port, &keep_path, &token, &published_path, None);

    let mut case = the_case(
        "RAW-06",
        "un contenido publicado y una subida en curso",
        "el proceso se muere a mitad de la subida y se vuelve a arrancar",
        &format!(
            "{} ; {} ; kill -9 <servicio> ; <servicio> (arranque normal, con la caducidad por defecto)",
            keep_upload.shown(),
            cut_upload.shown()
        ),
    );
    support::record(case.clone());

    // A content that was published, so as to be able to check that it remains
    // intact after the cut.
    std::fs::write(&published_path, the_content()).expect("the source is written");
    let expected = support::sha256_of(&published_path);
    let answer = keep_upload.run();
    assert_eq!(answer.status, 201);

    // And now an upload that never gets to finish.
    std::fs::write(&big_path, the_content_of(7).repeat(128)).expect("the source is written");
    let mut upload = cut_upload.run_in_background();

    // Wait until the upload is **in progress**: the staging exists and
    // has bytes, meaning the service is writing the body.
    let deadline = Instant::now() + Duration::from_secs(30);
    while deployment.staging_bytes() == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        deployment.staging_bytes() > 0,
        "the upload never reached the store:\n{}",
        deployment.the_log()
    );

    // And now the service is killed, with no chance to clean anything up.
    support::kill(&mut service);
    let _ = upload.wait();

    // The dead process's staging is still on disk: nobody cleaned it up.
    assert!(
        !deployment.staging_entries().is_empty(),
        "the staging of the interrupted upload was expected to be there"
    );

    // That staging is an abandoned file: it has been there for two hours, which is
    // more than the default expiry (one hour). And next to it a **live**
    // staging is left, like that of another process's upload, which is what
    // startup must not touch.
    //
    // The live one is also two hours old: if only its date protected it,
    // startup would take it away. What protects it is the **lock**,
    // which is what says that someone is writing it.
    let live = deployment.a_live_staging();
    deployment.age_the_staging(Duration::from_secs(2 * 3600));

    // On restart, the service cleans up what it finds expired and
    // respects what is alive. And it is restarted with the real
    // configuration: the default expiry, without touching anything.
    let mut service = deployment.start(&[]);
    let port = service.port;

    // The restart opens another port, so the later `curl`s are
    // built here: the report's command is the one that was executed, with
    // the port that was executed.
    let missing_command = support::Curl::download(port, &cut_path, &token, None);
    let again_command = support::Curl::upload(port, &cut_path, &token, &published_path, None);
    let after_command = support::Curl::download(port, &keep_path, &token, Some(&after_path));

    assert!(
        deployment.staging_entries().contains(&live),
        "the staging of a live upload was removed at the boot"
    );
    assert!(
        live.exists(),
        "the staging of a live upload was removed at the boot"
    );
    assert_eq!(
        deployment.staging_entries().len(),
        1,
        "the expired staging was not cleaned up: {:?}",
        deployment.staging_entries()
    );

    // The cut upload's path does not exist: it was not published halfway.
    let missing = missing_command.run();
    assert_eq!(
        missing.status, 404,
        "an interrupted upload must not publish a path"
    );

    // And publishing it again **works**: a `404` and a `409` together
    // would be a dangling asset, a path nobody can use. With the
    // `409` alone, all that is checked is that the address is free.
    let again = again_command.run();
    assert_eq!(
        again.status,
        201,
        "the interrupted path is not free:\n{}",
        deployment.the_log()
    );

    // And the content that was published is still the same.
    let answer = after_command.run();
    assert_eq!(answer.status, 200);
    assert_eq!(
        support::sha256_of(&after_path),
        expected,
        "the content that was published did not survive the interruption"
    );

    // Startup says how many stagings it took away and how many it left, and the
    // report says so: it is the service's tally, not the harness's.
    let (removed, alive) = support::the_boot_cleanup(&deployment.the_log());
    assert_eq!(
        removed,
        Some(1),
        "the boot did not say that it removed the expired staging:\n{}",
        deployment.the_log()
    );
    assert_eq!(
        alive,
        Some(1),
        "the boot did not say that it left the live staging alone:\n{}",
        deployment.the_log()
    );

    case.command = format!(
        "{} ; {} ; kill -9 <servicio> ; <servicio> (arranque normal, con la caducidad por defecto) ; {} ; {} ; {}",
        keep_upload.shown(),
        cut_upload.shown(),
        missing_command.shown(),
        again_command.shown(),
        after_command.shown()
    );
    case.outcome = format!(
        "la ruta cortada no existe ({}), republicar en ella da {} (no 409), el staging caducado se limpió al arrancar y el vivo sigue —caducado y bloqueado, que es lo que lo salva—, y el contenido publicado sigue con el mismo sha256 ({expected})",
        missing.status, again.status
    );
    // The tallies are written as the service's log states them, with
    // the names of its fields: that way the report and the log can be
    // compared without translating.
    case.outcome.push_str(&format!(
        " (removed: {removed}, alive: {alive})",
        removed = removed.unwrap_or(0),
        alive = alive.unwrap_or(0)
    ));
    case.passed = true;
    support::record(case);

    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// CAS-02 and CAT-02 — simultaneous uploads.
// ---------------------------------------------------------------------------

/// Several uploads at once do not get mixed.
///
/// Three situations, because each fails in a different way:
///
/// - The same content to the same path: the effect is **one**, and what
///   is downloaded is that content.
/// - Different contents to different paths: each path has its own.
/// - Different contents to the same path: the address publishes once,
///   so one wins and the others receive a `409`.
///
/// What cannot happen is that what is downloaded is a mix: the
/// hash of what is downloaded has to be the hash of what was uploaded.
#[test]
fn simultaneous_uploads_do_not_mix_the_content() {
    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let port = service.port;
    let token = support::token(FIRST_ACCOUNT, "alice");
    let directory = deployment.directory.path().to_path_buf();

    let same_path = the_path(FIRST_REPOSITORY, "same/file.bin");
    let same_download_path = directory.join("same-downloaded.bin");

    // This case's `curl`s are built before being executed, and the
    // report carries them as they are: the five competing for the same
    // path are the same command with the same limit flag.
    let same_upload = support::Curl::upload(
        port,
        &same_path,
        &token,
        &directory.join("same.bin"),
        Some("1M"),
    );
    let same_download =
        support::Curl::download(port, &same_path, &token, Some(&same_download_path));

    // The three groups of uploads and the three downloads: each group is a
    // command, and the number of times it is repeated is stated by the prose.
    let distinct_upload = support::Curl::upload(
        port,
        &the_path(FIRST_REPOSITORY, "distinct/0.bin"),
        &token,
        &directory.join("distinct-0.bin"),
        Some("1M"),
    );
    let distinct_download = support::Curl::download(
        port,
        &the_path(FIRST_REPOSITORY, "distinct/0.bin"),
        &token,
        Some(&directory.join("distinct-0-downloaded.bin")),
    );
    let contested_upload = support::Curl::upload(
        port,
        &the_path(FIRST_REPOSITORY, "contested/file.bin"),
        &token,
        &directory.join("contested-0.bin"),
        Some("1M"),
    );
    let contested_download = support::Curl::download(
        port,
        &the_path(FIRST_REPOSITORY, "contested/file.bin"),
        &token,
        Some(&directory.join("contested-downloaded.bin")),
    );

    let mut case = the_case(
        "CAS-02, CAT-02",
        "cinco peticiones con el mismo contenido, cuatro con contenidos distintos y cuatro disputando la misma ruta",
        "se publican todos a la vez y se descarga lo que hay",
        &format!(
            "{} (cinco en paralelo) ; {} (cuatro en paralelo) ; {} (cuatro en paralelo) ; {} ; {} ; {} ; {}",
            same_upload.shown(),
            distinct_upload.shown(),
            contested_upload.shown(),
            same_download.shown(),
            distinct_download.shown(),
            contested_download.shown(),
            support::sha256_command(&same_download_path)
        ),
    );
    support::record(case.clone());

    // --- The same content, to the same path, by five requests.
    let same = directory.join("same.bin");
    std::fs::write(&same, a_big_content(1)).expect("the source is written");
    let same_digest = support::sha256_of(&same);
    let mut uploads = Vec::new();
    for _ in 0..5 {
        uploads.push(same_upload.run_in_background());
    }

    // And the uploads **overlap**: there is more than one staging in progress, which
    // is what says the requests were in flight at the same time and that
    // each had its own staging. Without this check, what is
    // measured is a republish in a row.
    let deadline = Instant::now() + Duration::from_secs(30);
    while deployment.staging_entries().len() < 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        deployment.staging_entries().len() >= 2,
        "the uploads did not overlap: {:?}",
        deployment.staging_entries()
    );

    let mut same_statuses = Vec::new();
    for upload in uploads {
        same_statuses.push(support::wait_for_the_answer(upload).status);
    }
    assert!(
        same_statuses
            .iter()
            .all(|status| *status == 200 || *status == 201),
        "the same content published at once has to be idempotent: {same_statuses:?}\n{}",
        deployment.the_log()
    );

    let answer = same_download.run();
    assert_eq!(answer.status, 200);
    assert_eq!(
        support::sha256_of(&same_download_path),
        same_digest,
        "the content of a simultaneous publication is not the content that was published"
    );

    // --- Different contents, to different paths.
    let mut uploads = Vec::new();
    let mut digests = Vec::new();
    for writer in 0..4u8 {
        let file = directory.join(format!("distinct-{writer}.bin"));
        std::fs::write(&file, a_big_content(writer)).expect("the source is written");
        digests.push((writer, support::sha256_of(&file)));
        uploads.push(
            support::Curl::upload(
                port,
                &the_path(FIRST_REPOSITORY, &format!("distinct/{writer}.bin")),
                &token,
                &file,
                Some("1M"),
            )
            .run_in_background(),
        );
    }
    let mut distinct_statuses = Vec::new();
    for upload in uploads {
        distinct_statuses.push(support::wait_for_the_answer(upload).status);
    }
    assert!(
        distinct_statuses
            .iter()
            .all(|status| *status == 200 || *status == 201),
        "distinct contents at distinct paths have to be published: {distinct_statuses:?}\n{}",
        deployment.the_log()
    );

    for (writer, digest) in &digests {
        let file = directory.join(format!("distinct-{writer}-downloaded.bin"));
        let answer = support::curl_download(
            port,
            &the_path(FIRST_REPOSITORY, &format!("distinct/{writer}.bin")),
            &token,
            Some(&file),
        );
        assert_eq!(answer.status, 200);
        assert_eq!(
            support::sha256_of(&file),
            *digest,
            "the content of the path {writer} is not the content that was published there"
        );
    }

    // --- Different contents, to the same path.
    let mut uploads = Vec::new();
    let mut contenders = Vec::new();
    for writer in 0..4u8 {
        let file = directory.join(format!("contested-{writer}.bin"));
        std::fs::write(&file, a_big_content(100 + writer)).expect("the source is written");
        contenders.push(support::sha256_of(&file));
        uploads.push(
            support::Curl::upload(
                port,
                &the_path(FIRST_REPOSITORY, "contested/file.bin"),
                &token,
                &file,
                Some("1M"),
            )
            .run_in_background(),
        );
    }
    let mut contested_statuses = Vec::new();
    for upload in uploads {
        contested_statuses.push(support::wait_for_the_answer(upload).status);
    }
    let created = contested_statuses
        .iter()
        .filter(|status| **status == 201)
        .count();
    let refused = contested_statuses
        .iter()
        .filter(|status| **status == 409)
        .count();
    assert_eq!(
        created,
        1,
        "a path that publishes once has to have exactly one winner: {contested_statuses:?}\n{}",
        deployment.the_log()
    );
    assert_eq!(
        refused, 3,
        "the publications that lost have to be refused: {contested_statuses:?}"
    );

    // And what remains is **one** of those that were published, whole.
    let file = directory.join("contested-downloaded.bin");
    let answer = support::curl_download(
        port,
        &the_path(FIRST_REPOSITORY, "contested/file.bin"),
        &token,
        Some(&file),
    );
    assert_eq!(answer.status, 200);
    let final_digest = support::sha256_of(&file);
    assert!(
        contenders.contains(&final_digest),
        "the content that survived is not any of the contents that were published"
    );

    // And no half-finished write remains.
    assert!(
        deployment.staging_entries().is_empty(),
        "a finished publication must not leave a staging behind: {:?}",
        deployment.staging_entries()
    );

    case.outcome = format!(
        "el mismo contenido es idempotente ({same_statuses:?}), las rutas distintas guardan lo suyo ({distinct_statuses:?}), la ruta disputada tiene {created} ganador y {refused} 409 ({contested_statuses:?}), y cada hash descargado es el hash de lo subido"
    );
    case.passed = true;
    support::record(case);

    support::stop(&mut service);
}

// ---------------------------------------------------------------------------
// RAW-05 and CAT-03 — two accounts with real tokens.
// ---------------------------------------------------------------------------

/// Two accounts do not see each other, and the one that does not have the repository cannot
/// know whether it exists.
///
/// Both accounts have a repository with the **same key**, so
/// the same address with different tokens are two different contents:
/// it is isolation per account, measured with both URLs identical.
#[test]
fn two_accounts_do_not_see_each_other() {
    let deployment = a_deployment();
    let mut service = deployment.start(&[]);
    let port = service.port;
    let first = support::token(FIRST_ACCOUNT, "alice");
    let second = support::token(SECOND_ACCOUNT, "bob");
    let directory = deployment.directory.path().to_path_buf();

    let shared_first = the_path(FIRST_REPOSITORY, "shared/file.bin");
    let shared_second = the_path(SECOND_REPOSITORY, "shared/file.bin");
    let only_first = the_path(FIRST_ONLY_REPOSITORY, "secret/file.bin");
    let carol = support::token(FIRST_ACCOUNT, "carol");

    // This case's `curl`s are built beforehand: the report carries the
    // two accounts and the request without credentials, each with its
    // redacted credential.
    let first_upload = support::Curl::upload(
        port,
        &shared_first,
        &first,
        &directory.join("first.bin"),
        None,
    );
    let second_upload = support::Curl::upload(
        port,
        &shared_second,
        &second,
        &directory.join("second.bin"),
        None,
    );
    let only_first_upload = support::Curl::upload(
        port,
        &only_first,
        &first,
        &directory.join("only-first.bin"),
        None,
    );
    let first_download = support::Curl::download(
        port,
        &shared_first,
        &first,
        Some(&directory.join("by-first.bin")),
    );
    let second_download = support::Curl::download(
        port,
        &shared_second,
        &second,
        Some(&directory.join("by-second.bin")),
    );
    let other_account = support::Curl::download(port, &only_first, &second, None);
    let uncovered = support::Curl::download(port, &shared_first, &carol, None);
    let anonymous = support::Curl::download(port, &only_first, "", None);

    let mut case = the_case(
        "RAW-05, CAT-03",
        "dos cuentas con tokens reales y repositorios con la misma clave",
        "las dos publican en la misma dirección y cada una descarga, y la otra busca un repositorio que no tiene",
        &the_commands(&[
            &first_upload,
            &second_upload,
            &only_first_upload,
            &first_download,
            &second_download,
            &other_account,
            &uncovered,
            &anonymous,
        ]),
    );
    support::record(case.clone());

    // Both accounts publish to the same address.
    let first_file = directory.join("first.bin");
    std::fs::write(&first_file, the_content_of(1)).expect("the source is written");
    let first_digest = support::sha256_of(&first_file);
    let answer = first_upload.run();
    assert_eq!(answer.status, 201, "{}", deployment.the_log());

    let second_file = directory.join("second.bin");
    std::fs::write(&second_file, the_content_of(2)).expect("the source is written");
    let second_digest = support::sha256_of(&second_file);
    let answer = second_upload.run();
    assert_eq!(answer.status, 201, "{}", deployment.the_log());

    // And the first account also publishes to a repository that the
    // second does not have. Without this, the second's `404` would not distinguish
    // "you cannot see it" from "nothing is published": someone would have
    // had to publish it.
    let only_first_file = directory.join("only-first.bin");
    std::fs::write(&only_first_file, the_content_of(3)).expect("the source is written");
    let answer = only_first_upload.run();
    assert_eq!(answer.status, 201, "{}", deployment.the_log());

    // And each downloads its own from the same address.
    let answer = first_download.run();
    assert_eq!(answer.status, 200);
    assert_eq!(
        support::sha256_of(&directory.join("by-first.bin")),
        first_digest,
        "the first account did not get its own content"
    );

    let answer = second_download.run();
    assert_eq!(answer.status, 200);
    assert_eq!(
        support::sha256_of(&directory.join("by-second.bin")),
        second_digest,
        "the second account did not get its own content"
    );

    // And what is at the other account's address cannot be read:
    // with the second's token, the first's address does not exist.
    let answer = other_account.run();
    assert_eq!(
        answer.status, 404,
        "a repository of another account must not exist for this one"
    );

    // And the denial is **coherent**: the same account, the same
    // repository and another person: the Cedar bundle does not cover it, so
    // the response is a 403 —which does say the repository exists— instead of
    // a 404 that would lie.
    let answer = uncovered.run();
    assert_eq!(
        answer.status,
        403,
        "a principal that the bundle does not cover has to be refused:\n{}",
        deployment.the_log()
    );

    // And without credentials nothing is said either.
    let answer = anonymous.run();
    assert_eq!(answer.status, 401);

    case.outcome = format!(
        "cada cuenta descarga su propio contenido (hash distinto en la misma dirección), el repositorio de la otra cuenta es {}, el principal que el bundle no cubre es {} y sin credencial es {}",
        404, 403, 401
    );
    case.passed = true;
    support::record(case);

    support::stop(&mut service);
}

/// The report is written where the CI can read it.
#[test]
fn the_report_is_written_where_the_ci_can_read_it() {
    let path: PathBuf = support::Report::the_path();
    assert!(
        path.ends_with("raw-conformity.md"),
        "the report has a known name: {path:?}"
    );
    let parent = path.parent().expect("the report has a directory");
    std::fs::create_dir_all(parent).expect("the directory of the report");
    // And the report says what it does not certify, because a report that only
    // says what it does is propaganda.
    let report = support::Report::new();
    let markdown = report.as_markdown();
    assert!(markdown.contains("no** certifica"));
    assert!(markdown.contains("OCI"));
    assert!(markdown.contains("S3"));
    // And it says the credential is redacted, so that nobody reads the
    // column as if they were commands that can be copied as they are.
    assert!(
        markdown.contains("redactad"),
        "the report does not say that the credentials are redacted:\n{markdown}"
    );
}
