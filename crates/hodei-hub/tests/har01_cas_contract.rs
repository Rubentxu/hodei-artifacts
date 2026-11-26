//! The contract of the production CAS: HAR-01.
//!
//! The M9 CAS was correct in its central guarantee — the same content
//! is not stored twice and what comes out by the digest is what went in —
//! but it was not operable with real client bytes. These five criteria
//! are the ones that make it so:
//!
//! 1. The upload **streams**: the content does not fit entirely in memory.
//! 2. Two producers of the same content **do not step on each other**.
//! 3. The commit **validates** what it publishes, and if it does not add up, it does not publish.
//! 4. The read **distinguishes** what is missing from what is broken, and
//!    expired staging is cleaned up.
//! 5. The store **withstands** a disk error and the death of a process.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use hodei_evidence::{ContentDigest, DigestAlgorithm};
use hodei_hub::blob::{
    BlobError, BlobStore, CommitExpectation, FileBlobStore, InMemoryBlobStore, ReadPolicy,
};
use sha2::{Digest, Sha256};

// ───────────────────────────── utilities ─────────────────────────────

/// A temporary directory for the tests.
///
/// Large content goes to the target directory, which is on disk: in
/// `/tmp` it would go to a tmpfs, take up system memory and falsify the
/// process memory measurement.
fn a_temp_dir(prefix: &str) -> tempfile::TempDir {
    match option_env!("CARGO_TARGET_TMPDIR") {
        Some(path) => tempfile::Builder::new()
            .prefix(prefix)
            .tempdir_in(path)
            .expect("un directorio de usar y tirar"),
        None => tempfile::Builder::new()
            .prefix(prefix)
            .tempdir()
            .expect("un directorio de usar y tirar"),
    }
}

/// The resident memory of the process, in bytes.
///
/// It is the measure that says whether the process **keeps** the content or
/// **lets it pass through**. It only makes sense on Linux.
#[cfg(target_os = "linux")]
fn rss_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let kilobytes: u64 = rest.trim().trim_end_matches("kB").trim().parse().ok()?;
            return Some(kilobytes * 1024);
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn rss_bytes() -> Option<u64> {
    None
}

/// The hexadecimal of the bytes.
fn hex_of(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// The base block: generated once and copied for each part.
///
/// Generating it for each part cost more than the whole test, and there is
/// nothing to debug here.
fn base_block() -> &'static Vec<u8> {
    static BASE: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    BASE.get_or_init(|| {
        let mut block = vec![0u8; 1024 * 1024];
        let mut state: u32 = 0x9E37_79B9;
        for byte in block.iter_mut() {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *byte = (state >> 24) as u8;
        }
        block
    })
}

/// A deterministic block, different at each index.
///
/// The content is always the same for the same index, so that the
/// expected digest can be computed, and different between indices, so that
/// a blob is a blob and not a thousand copies of the same one.
fn a_block(index: u64, size: usize) -> Vec<u8> {
    let base = base_block();
    let mut block = Vec::with_capacity(size);
    while block.len() < size {
        let remaining = size - block.len();
        block.extend_from_slice(&base[..remaining.min(base.len())]);
    }
    // And the index makes it different in each block.
    let key = (index as u32).wrapping_mul(0x9E37_79B9);
    for (position, byte) in block[..size.min(256)].iter_mut().enumerate() {
        *byte ^= (key >> ((position % 4) * 8)) as u8 ^ (position as u8);
    }
    block
}

/// The digest of a file, read in parts.
///
/// It verifies the content **without** loading it whole in memory: if the
/// verification needed a buffer the size of the blob, it would not serve
/// to prove that the store does not buffer.
fn digest_of_file(path: &Path) -> String {
    use std::io::Read;
    let mut file = std::fs::File::open(path).expect("el blob se abre");
    let mut reader = std::io::BufReader::with_capacity(1024 * 1024, &mut file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = reader.read(&mut buffer).expect("el blob se lee");
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    hex_of(&hasher.finalize())
}

/// The modification date of a file, to simulate an old staging.
fn set_modified(path: &Path, when: SystemTime) {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("el staging se abre");
    file.set_times(std::fs::FileTimes::new().set_modified(when))
        .expect("la fecha se pone");
}

/// The files in a directory, sorted.
fn the_files_in(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("el directorio se lee")
        .map(|entry| entry.expect("la entrada se lee").path())
        .collect();
    files.sort();
    files
}

// ───────────────────────────── AC1 ─────────────────────────────

/// AC1 The upload streams: the content does not fit entirely in memory.
///
/// Given a 1 GiB content, when it is uploaded in parts, then the process
/// memory does not grow with the size of the artifact and what is published
/// is exactly what was uploaded.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn har01_ac1_the_upload_does_not_buffer_the_whole_content() {
    const TOTAL: u64 = 1024 * 1024 * 1024;
    const BLOCK: usize = 1024 * 1024;

    let tmp = a_temp_dir("har01-ac1-");
    let store = FileBlobStore::new(tmp.path()).expect("el almacen abre");

    let baseline = rss_bytes();
    let mut peak = baseline.unwrap_or(0);

    let mut session = store.begin().await.expect("la sesion abre");
    let mut hasher = Sha256::new();
    let mut written: u64 = 0;
    let mut index: u64 = 0;
    while written < TOTAL {
        let block = a_block(index, BLOCK);
        hasher.update(&block);
        session.append(&block).await.expect("el bloque entra");
        written += block.len() as u64;
        index += 1;
        if written.is_multiple_of(64 * 1024 * 1024)
            && let Some(current) = rss_bytes()
        {
            peak = peak.max(current);
        }
    }
    let expected = ContentDigest::sha256(hex_of(&hasher.finalize()));

    let digest = session
        .commit(CommitExpectation {
            digest: expected.clone(),
            length: TOTAL,
        })
        .await
        .expect("el commit publica");
    assert_eq!(digest, expected, "el digest publicado es el del contenido");

    if let Some(baseline) = baseline {
        let growth = peak.saturating_sub(baseline);
        // The limit is an eighth of the blob: buffering the whole
        // content would blow far past it, and the margin leaves
        // room for the rest of the tests running in parallel.
        let limit = TOTAL / 8;
        println!(
            "AC1: la memoria residente crecio {growth} bytes ({} MiB) para un blob de {} MiB; el limite es {} MiB",
            growth / (1024 * 1024),
            TOTAL / (1024 * 1024),
            limit / (1024 * 1024)
        );
        assert!(
            growth < limit,
            "la memoria residente crecio {growth} bytes para un blob de 1 GiB: el contenido se esta guardando"
        );
    }

    // And what is on disk is what was uploaded, without loading it whole.
    let path = store.the_path_of(&digest);
    assert_eq!(
        std::fs::metadata(&path).expect("el blob esta").len(),
        TOTAL,
        "el blob publicado mide lo que se subio"
    );
    assert_eq!(
        digest_of_file(&path),
        expected.value,
        "y su contenido es exactamente el que se subio"
    );
}

// ───────────────────────────── AC2 ─────────────────────────────

/// AC2 Two producers of the same content do not step on each other.
///
/// Given several concurrent producers of the same content, when
/// they write at once, then each has its own staging, only one blob remains
/// and its content is the correct one.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn har01_ac2_concurrent_producers_never_share_the_staging() {
    const WRITERS: usize = 16;
    const SIZE: usize = 512 * 1024;

    let tmp = a_temp_dir("har01-ac2-");
    let store = std::sync::Arc::new(FileBlobStore::new(tmp.path()).expect("el almacen abre"));
    let content = std::sync::Arc::new(a_block(1, SIZE));
    let expected = ContentDigest::of_bytes(&content);

    // All write the same content, at once, in parts: so that
    // the writes overlap in time and not just in code.
    let mut handles = Vec::new();
    for _ in 0..WRITERS {
        let store = std::sync::Arc::clone(&store);
        let content = std::sync::Arc::clone(&content);
        let declared = expected.clone();
        handles.push(tokio::spawn(async move {
            let mut session = store.begin().await.expect("la sesion abre");
            let staging = session.the_staging_path().map(Path::to_path_buf);
            for chunk in content.chunks(64 * 1024) {
                session.append(chunk).await.expect("el trozo entra");
                tokio::task::yield_now().await;
            }
            let digest = session
                .commit(CommitExpectation {
                    digest: declared,
                    length: content.len() as u64,
                })
                .await
                .expect("el commit publica");
            (staging, digest)
        }));
    }

    let mut stagings = Vec::new();
    for handle in handles {
        let (staging, digest) = handle.await.expect("la tarea termina");
        assert_eq!(digest, expected, "todos ven el mismo digest");
        if let Some(staging) = staging {
            stagings.push(staging);
        }
    }

    // Each write in progress had its own file: nobody shares.
    let unique: std::collections::BTreeSet<&PathBuf> = stagings.iter().collect();
    assert_eq!(
        unique.len(),
        stagings.len(),
        "dos escrituras en curso compartian fichero de staging"
    );

    // And only one blob remains, with the correct content.
    let path = store.the_path_of(&expected);
    assert!(path.exists(), "el blob esta en su sitio");
    assert_eq!(
        store.get(&expected).await.expect("el contenido se lee"),
        content.as_slice(),
        "y el contenido es el que se subio"
    );
    assert!(
        store
            .staging_entries()
            .expect("el staging se lista")
            .is_empty(),
        "no queda staging: cada commit limpio el suyo"
    );

    // And the same when the contents are different.
    let mut handles = Vec::new();
    for index in 0..WRITERS as u64 {
        let store = std::sync::Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            let content = a_block(100 + index, 4096);
            let expected = ContentDigest::of_bytes(&content);
            let mut session = store.begin().await.expect("la sesion abre");
            session.append(&content).await.expect("el trozo entra");
            session
                .commit(CommitExpectation {
                    digest: expected,
                    length: content.len() as u64,
                })
                .await
                .expect("el commit publica")
        }));
    }
    let mut digests = Vec::new();
    for handle in handles {
        digests.push(handle.await.expect("la tarea termina"));
    }
    let unique: std::collections::HashSet<&String> = digests.iter().map(|d| &d.value).collect();
    assert_eq!(
        unique.len(),
        WRITERS,
        "cada contenido distinto tiene su blob"
    );
}

// ───────────────────────────── AC3 ─────────────────────────────

/// AC3 The commit validates what it publishes.
///
/// Given a commit that declares a digest, a length or an algorithm that
/// are not those of the content, when publishing is attempted, then it is rejected,
/// the staging disappears and no visible reference remains.
#[tokio::test]
async fn har01_ac3_the_commit_validates_algorithm_length_and_digest() {
    let tmp = a_temp_dir("har01-ac3-");
    let store = FileBlobStore::new(tmp.path()).expect("el almacen abre");
    let content = b"the bytes of the artefact".to_vec();
    let real = ContentDigest::of_bytes(&content);

    // The digest is not the content's.
    let mut session = store.begin().await.expect("la sesion abre");
    session.append(&content).await.expect("el trozo entra");
    let declared = ContentDigest::sha256("0".repeat(64));
    let error = session
        .commit(CommitExpectation {
            digest: declared,
            length: content.len() as u64,
        })
        .await
        .expect_err("un digest que no es el del contenido no se publica");
    assert!(
        matches!(error, BlobError::DigestMismatch { .. }),
        "el error dice que el digest no cuadra: {error:?}"
    );
    assert!(
        !store.exists(&real).await.expect("el almacen responde"),
        "no queda ninguna referencia visible"
    );
    assert!(
        store
            .staging_entries()
            .expect("el staging se lista")
            .is_empty(),
        "y el staging se elimina"
    );

    // The length is not the content's.
    let mut session = store.begin().await.expect("la sesion abre");
    session.append(&content).await.expect("el trozo entra");
    let error = session
        .commit(CommitExpectation {
            digest: real.clone(),
            length: content.len() as u64 + 1,
        })
        .await
        .expect_err("una longitud que no es la del contenido no se publica");
    assert!(
        matches!(error, BlobError::LengthMismatch { .. }),
        "el error dice que la longitud no cuadra: {error:?}"
    );
    assert!(
        !store.exists(&real).await.expect("el almacen responde"),
        "no queda ninguna referencia visible"
    );

    // The algorithm is not one the store knows how to compute.
    let mut session = store.begin().await.expect("la sesion abre");
    session.append(&content).await.expect("el trozo entra");
    let sha512 = ContentDigest {
        algorithm: DigestAlgorithm::Sha512,
        value: "a".repeat(128),
    };
    let error = session
        .commit(CommitExpectation {
            digest: sha512,
            length: content.len() as u64,
        })
        .await
        .expect_err("un algoritmo que el almacen no calcula no se publica");
    assert!(
        matches!(error, BlobError::UnsupportedAlgorithm(_)),
        "el error dice que el algoritmo no vale: {error:?}"
    );
    assert!(
        store
            .staging_entries()
            .expect("el staging se lista")
            .is_empty(),
        "y el staging se elimina"
    );

    // And what does add up is published.
    let mut session = store.begin().await.expect("la sesion abre");
    session.append(&content).await.expect("el trozo entra");
    let digest = session
        .commit(CommitExpectation {
            digest: real.clone(),
            length: content.len() as u64,
        })
        .await
        .expect("el commit publica");
    assert_eq!(digest, real);
    assert!(store.exists(&real).await.expect("el almacen responde"));
}

// ───────────────────────────── AC4 ─────────────────────────────

/// AC4 The read distinguishes what is missing from what is broken.
///
/// Given a digest that is not there, a corrupt blob and something that cannot be
/// read, when they are read, then each failure is a distinct failure and the read
/// policy is explicit.
#[tokio::test]
async fn har01_ac4_the_read_tells_missing_corrupted_and_unreadable_apart() {
    let tmp = a_temp_dir("har01-ac4-");
    let store = FileBlobStore::new(tmp.path()).expect("el almacen abre");

    // What is not there, is not there.
    let missing = ContentDigest::of_bytes(b"nothing was ever stored here");
    assert_eq!(
        store
            .get(&missing)
            .await
            .expect_err("lo que no esta no sale"),
        BlobError::NotFound
    );
    assert_eq!(
        store
            .length(&missing)
            .await
            .expect_err("lo que no esta no mide"),
        BlobError::NotFound
    );

    // What is corrupt, is corrupt: it is not confused with being missing.
    let digest = store
        .put(b"the bytes")
        .await
        .expect("el contenido se guarda");
    std::fs::write(store.the_path_of(&digest), b"other bytes").expect("el blob se corrompe");
    let error = store
        .get(&digest)
        .await
        .expect_err("lo corrupto no sale como si estuviera bien");
    assert!(
        matches!(error, BlobError::Corrupted { .. }),
        "el error dice que esta corrupto: {error:?}"
    );
    assert!(
        matches!(
            store
                .verify(&digest)
                .await
                .expect_err("la verificacion falla"),
            BlobError::Corrupted { .. }
        ),
        "y la verificacion lo dice tambien"
    );

    // And `exists` still says it is there: it does not assert integrity.
    assert!(
        store.exists(&digest).await.expect("el almacen responde"),
        "exists dice que esta, no que este bien"
    );

    // And what cannot be read is not "not there".
    let unreadable = ContentDigest::of_bytes(b"this one is a directory");
    std::fs::create_dir_all(store.the_path_of(&unreadable)).expect("el directorio se crea");
    let error = store
        .get(&unreadable)
        .await
        .expect_err("lo que no se puede leer no sale");
    assert!(
        matches!(error, BlobError::Unavailable(_)),
        "el error dice que no se pudo leer: {error:?}"
    );

    // And the read policy is explicit: the trust one does not check.
    let unchecked = store
        .get_with(&digest, ReadPolicy::Trust)
        .await
        .expect("la politica de confianza lee sin comprobar");
    assert_eq!(
        unchecked, b"other bytes",
        "la politica de confianza devuelve lo que hay, sin verificarlo"
    );
}

/// AC4 Expired staging is cleaned up and live staging is not touched.
///
/// Given an old staging of a process that crashes, another live one that is
/// being written right now and a file that is not ours, when the
/// process restarts and cleans up, then only the old one disappears, the live one
/// is respected and what is not ours is not touched.
#[tokio::test]
async fn har01_ac4_the_expired_staging_is_cleaned_and_the_live_one_is_not() {
    let tmp = a_temp_dir("har01-ac4-staging-");
    let store = FileBlobStore::new(tmp.path()).expect("el almacen abre");
    let digest = store
        .put(b"the bytes")
        .await
        .expect("el contenido se guarda");

    // An old staging, like the one a crashing process leaves: with the
    // name the store gives and with nobody writing it.
    let staging_dir = store.the_staging_dir();
    std::fs::create_dir_all(&staging_dir).expect("el staging se crea");
    let old = staging_dir.join(a_staging_name());
    std::fs::write(&old, b"half written").expect("el staging se escribe");
    set_modified(&old, SystemTime::now() - Duration::from_secs(3600));

    // And a live one, which may belong to another process that is uploading: with
    // the name the store gives and **locked**, which is what says
    // that someone is writing it right now.
    //
    // And **expired**, on purpose: if only its date protected it, the
    // startup would take it away, and what protects it is the lock.
    let live = staging_dir.join(a_staging_name());
    let live_file = std::fs::File::create(&live).expect("el staging vivo se escribe");
    live_file.try_lock().expect("el staging vivo se bloquea");
    set_modified(&live, SystemTime::now() - Duration::from_secs(3600));

    // And a file that is not ours: the staging directory is a
    // convention, and what is there may belong to something else.
    let foreign = staging_dir.join("not-ours.txt");
    std::fs::write(&foreign, b"de otra cosa").expect("el fichero se escribe");

    // And a directory with our name, which cannot be deleted: a
    // disk problem cannot prevent the store from continuing to serve.
    let stuck = staging_dir.join(a_staging_name());
    std::fs::create_dir(&stuck).expect("el directorio se crea");

    // The process restarts: a new store over the same directory.
    let restarted = FileBlobStore::new(tmp.path()).expect("el almacen abre");
    let cleanup = restarted
        .clean_expired_staging(Duration::from_secs(60))
        .expect("la limpieza se hace");
    assert_eq!(cleanup.removed, 1, "solo se limpia el staging caducado");
    assert_eq!(cleanup.alive, 1, "el staging vivo se cuenta, no se borra");
    assert!(
        cleanup.failed.is_empty(),
        "nada falla todavia: {:?}",
        cleanup.failed
    );
    assert!(!old.exists(), "el staging caducado ya no esta");
    assert!(live.exists(), "el staging vivo no se toca");
    assert!(foreign.exists(), "un fichero que no es nuestro no se toca");

    // And what cannot be deleted is not a failure: it is counted and the store
    // keeps serving.
    let cleanup = restarted
        .clean_expired_staging(Duration::ZERO)
        .expect("la limpieza se hace");
    assert_eq!(cleanup.removed, 0, "el directorio no se puede borrar");
    assert_eq!(cleanup.alive, 1, "el staging vivo sigue vivo");
    assert_eq!(
        cleanup.failed.len(),
        1,
        "el que no se pudo borrar se cuenta"
    );
    assert!(stuck.exists(), "el directorio se queda");

    // And what was published is still there: cleaning staging neither publishes nor
    // deletes anything.
    assert!(
        restarted
            .exists(&digest)
            .await
            .expect("el almacen responde")
    );
    assert_eq!(
        restarted.get(&digest).await.expect("el contenido se lee"),
        b"the bytes"
    );
}

/// The name the store gives to a write in progress.
///
/// `<pid>-<nanos:x>-<counter:x>.part`. The harness copies it because what it
/// wants is a real staging, with the name the store
/// recognizes: a file with another name is not a write in progress, and
/// the store does not touch it.
fn a_staging_name() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    std::path::PathBuf::from(format!("{}-{nanos:x}-{counter:x}.part", std::process::id()))
}

// ───────────────────────────── AC5 ─────────────────────────────

/// AC5 The store withstands a disk error and the death of a process.
///
/// Given a staging that is not a directory and a process that crashes
/// halfway through an upload, when the store is used, then the disk error is
/// a disk error and nothing is published halfway.
#[tokio::test]
async fn har01_ac5_the_store_survives_a_disk_error_and_a_dead_process() {
    // The disk error: the staging is not a directory.
    let broken_root = a_temp_dir("har01-ac5-broken-");
    let broken = broken_root.path().join("store");
    std::fs::create_dir_all(&broken).expect("el directorio se crea");
    std::fs::write(broken.join("staging"), b"not a directory").expect("el bloqueo se escribe");
    let store = FileBlobStore::new(&broken).expect("el almacen abre");
    let error = match store.begin().await {
        Ok(_) => panic!("un staging que no es un directorio no se puede usar"),
        Err(error) => error,
    };
    assert!(
        matches!(error, BlobError::Unavailable(_)),
        "el error dice que el almacen no esta disponible: {error:?}"
    );

    // The process crashes halfway through an upload.
    let tmp = a_temp_dir("har01-ac5-");
    let store = FileBlobStore::new(tmp.path()).expect("el almacen abre");
    {
        let mut session = store.begin().await.expect("la sesion abre");
        session
            .append(b"half of the content")
            .await
            .expect("el trozo entra");
        // The session is abandoned without commit: an error, a `?` or a panic.
    }
    assert!(
        store
            .staging_entries()
            .expect("el staging se lista")
            .is_empty(),
        "una sesion abandonada no deja staging"
    );
    // And nothing was published halfway: at the root of the store only the
    // staging directory is there, no blob.
    let entries = the_files_in(tmp.path());
    assert_eq!(
        entries.len(),
        1,
        "no se publico ningun blob: la raiz solo tiene el staging, {entries:?}"
    );
    assert_eq!(entries[0].file_name().unwrap(), "staging");

    // A truly dead process does not run its `Drop`: its staging
    // stays. The restart cleans it up when it expires.
    let staging_dir = store.the_staging_dir();
    std::fs::create_dir_all(&staging_dir).expect("el staging se crea");
    let orphan = staging_dir.join(a_staging_name());
    std::fs::write(&orphan, b"half of the content").expect("el staging se escribe");
    set_modified(&orphan, SystemTime::now() - Duration::from_secs(3600));

    let restarted = FileBlobStore::new(tmp.path()).expect("el almacen abre");
    let cleanup = restarted
        .clean_expired_staging(Duration::from_secs(60))
        .expect("la limpieza se hace");
    assert_eq!(
        cleanup.removed, 1,
        "el staging del proceso muerto se limpia"
    );
    assert!(!orphan.exists(), "y ya no esta");
}

// ───────────────────── the contract is the same ─────────────────────

/// The session contract is the same in both stores.
///
/// Streaming has to **keep**: a session that validates the digest and
/// does not leave the content in the store is a digest of nothing.
#[tokio::test]
async fn har01_the_in_memory_store_streams_and_saves_the_same_way() {
    let store = InMemoryBlobStore::empty();
    let content = b"the bytes of the artefact".to_vec();
    let expected = ContentDigest::of_bytes(&content);

    let mut session = store.begin().await.expect("la sesion abre");
    for chunk in content.chunks(7) {
        session.append(chunk).await.expect("el trozo entra");
    }
    assert_eq!(session.bytes_written(), content.len() as u64);
    let digest = session
        .commit(CommitExpectation {
            digest: expected.clone(),
            length: content.len() as u64,
        })
        .await
        .expect("el commit publica y guarda");

    assert_eq!(digest, expected);
    assert!(
        store.exists(&expected).await.expect("el almacen responde"),
        "el contenido se ha guardado de verdad"
    );
    assert_eq!(
        store.get(&expected).await.expect("el contenido se lee"),
        content
    );
}

/// And what does not add up is not kept in the in-memory store either.
#[tokio::test]
async fn har01_the_in_memory_store_does_not_save_what_does_not_match() {
    let store = InMemoryBlobStore::empty();
    let content = b"the bytes of the artefact".to_vec();
    let real = ContentDigest::of_bytes(&content);

    let mut session = store.begin().await.expect("la sesion abre");
    session.append(&content).await.expect("el trozo entra");
    let error = session
        .commit(CommitExpectation {
            digest: ContentDigest::sha256("0".repeat(64)),
            length: content.len() as u64,
        })
        .await
        .expect_err("un digest que no es el del contenido no se publica");
    assert!(
        matches!(error, BlobError::DigestMismatch { .. }),
        "el error dice que el digest no cuadra: {error:?}"
    );
    assert!(
        !store.exists(&real).await.expect("el almacen responde"),
        "y no se guardo nada"
    );
}
