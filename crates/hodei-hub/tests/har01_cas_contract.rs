//! El contrato del CAS de produccion: HAR-01.
//!
//! El CAS de M9 era correcto en su garantia central — el mismo contenido
//! no se guarda dos veces y lo que sale por el digest es lo que entro —
//! pero no era operable con bytes de clientes reales. Estos cinco criterios
//! son los que lo hacen:
//!
//! 1. La subida **fluye**: el contenido no cabe entero en memoria.
//! 2. Dos productores del mismo contenido **no se pisen**.
//! 3. El commit **valida** lo que publica, y si no cuadra, no publica.
//! 4. La lectura **distingue** lo que falta de lo que esta roto, y el
//!    staging caducado se limpia.
//! 5. El almacen **aguanta** un error de disco y la muerte de un proceso.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use hodei_evidence::{ContentDigest, DigestAlgorithm};
use hodei_hub::blob::{
    BlobError, BlobStore, CommitExpectation, FileBlobStore, InMemoryBlobStore, ReadPolicy,
};
use sha2::{Digest, Sha256};

// ───────────────────────────── utilidades ─────────────────────────────

/// Un directorio temporal para las pruebas.
///
/// El contenido grande va al directorio del target, que esta en disco: en
/// `/tmp` se iria a un tmpfs, ocuparia memoria del sistema y falsearia la
/// medida de memoria del proceso.
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

/// La memoria residente del proceso, en bytes.
///
/// Es la medida que dice si el proceso **guarda** el contenido o si lo
/// **hace pasar**. Solo tiene sentido en Linux.
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

/// El hexadecimal de los bytes.
fn hex_of(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// El bloque base: se genera una vez y se copia para cada trozo.
///
/// Generarlo por cada trozo costaba mas que todo el test, y aqui no hay
/// nada que depurar.
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

/// Un bloque determinista, distinto en cada indice.
///
/// El contenido es siempre el mismo para el mismo indice, para que el
/// digest esperado se pueda calcular, y distinto entre indices, para que
/// un blob sea un blob y no mil copias del mismo.
fn a_block(index: u64, size: usize) -> Vec<u8> {
    let base = base_block();
    let mut block = Vec::with_capacity(size);
    while block.len() < size {
        let remaining = size - block.len();
        block.extend_from_slice(&base[..remaining.min(base.len())]);
    }
    // Y el indice lo hace distinto en cada bloque.
    let key = (index as u32).wrapping_mul(0x9E37_79B9);
    for (position, byte) in block[..size.min(256)].iter_mut().enumerate() {
        *byte ^= (key >> ((position % 4) * 8)) as u8 ^ (position as u8);
    }
    block
}

/// El digest de un fichero, leido a trozos.
///
/// Verifica el contenido **sin** cargarlo entero en memoria: si la
/// verificacion necesitara un buffer del tamaño del blob, no serviria
/// para probar que el almacen no bufferiza.
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

/// La fecha de modificacion de un fichero, para simular un staging viejo.
fn set_modified(path: &Path, when: SystemTime) {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("el staging se abre");
    file.set_times(std::fs::FileTimes::new().set_modified(when))
        .expect("la fecha se pone");
}

/// Los ficheros que hay en un directorio, ordenados.
fn the_files_in(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("el directorio se lee")
        .map(|entry| entry.expect("la entrada se lee").path())
        .collect();
    files.sort();
    files
}

// ───────────────────────────── AC1 ─────────────────────────────

/// AC1 La subida fluye: el contenido no cabe entero en memoria.
///
/// Given un contenido de 1 GiB, when se sube a trozos, then la memoria
/// del proceso no crece con el tamano del artefacto y lo que se publica
/// es exactamente lo que se subio.
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
        // El limite es una octava parte del blob: bufferizar el
        // contenido entero lo reventaria de largo, y el margen deja
        // sitio al resto de pruebas que corren en paralelo.
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

    // Y lo que esta en disco es lo que se subio, sin cargarlo entero.
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

/// AC2 Dos productores del mismo contenido no se pisen.
///
/// Given varios productores concurrentes del mismo contenido, when
/// escriben a la vez, then cada uno tiene su staging, solo queda un blob
/// y su contenido es el correcto.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn har01_ac2_concurrent_producers_never_share_the_staging() {
    const WRITERS: usize = 16;
    const SIZE: usize = 512 * 1024;

    let tmp = a_temp_dir("har01-ac2-");
    let store = std::sync::Arc::new(FileBlobStore::new(tmp.path()).expect("el almacen abre"));
    let content = std::sync::Arc::new(a_block(1, SIZE));
    let expected = ContentDigest::of_bytes(&content);

    // Todos escriben el mismo contenido, a la vez, por trozos: para que
    // las escrituras se solapen en el tiempo y no solo en el codigo.
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

    // Cada escritura en curso tenia su propio fichero: nadie comparte.
    let unique: std::collections::BTreeSet<&PathBuf> = stagings.iter().collect();
    assert_eq!(
        unique.len(),
        stagings.len(),
        "dos escrituras en curso compartian fichero de staging"
    );

    // Y solo queda un blob, con el contenido correcto.
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

    // Y lo mismo cuando los contenidos son distintos.
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

/// AC3 El commit valida lo que publica.
///
/// Given un commit que declara un digest, una longitud o un algoritmo que
/// no son los del contenido, when se intenta publicar, then se rechaza,
/// el staging desaparece y no queda ninguna referencia visible.
#[tokio::test]
async fn har01_ac3_the_commit_validates_algorithm_length_and_digest() {
    let tmp = a_temp_dir("har01-ac3-");
    let store = FileBlobStore::new(tmp.path()).expect("el almacen abre");
    let content = b"the bytes of the artefact".to_vec();
    let real = ContentDigest::of_bytes(&content);

    // El digest no es el del contenido.
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

    // La longitud no es la del contenido.
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

    // El algoritmo no es uno que el almacen sepa calcular.
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

    // Y lo que si cuadra, se publica.
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

/// AC4 La lectura distingue lo que falta de lo que esta roto.
///
/// Given un digest que no esta, un blob corrupto y algo que no se puede
/// leer, when se leen, then cada fallo es un fallo distinto y la politica
/// de lectura es explicita.
#[tokio::test]
async fn har01_ac4_the_read_tells_missing_corrupted_and_unreadable_apart() {
    let tmp = a_temp_dir("har01-ac4-");
    let store = FileBlobStore::new(tmp.path()).expect("el almacen abre");

    // Lo que no esta, no esta.
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

    // Lo que esta corrupto, esta corrupto: no se confunde con que falte.
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

    // Y `exists` sigue diciendo que esta: no afirma integridad.
    assert!(
        store.exists(&digest).await.expect("el almacen responde"),
        "exists dice que esta, no que este bien"
    );

    // Y lo que no se puede leer no es "no esta".
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

    // Y la politica de lectura es explicita: la de confianza no comprueba.
    let unchecked = store
        .get_with(&digest, ReadPolicy::Trust)
        .await
        .expect("la politica de confianza lee sin comprobar");
    assert_eq!(
        unchecked, b"other bytes",
        "la politica de confianza devuelve lo que hay, sin verificarlo"
    );
}

/// AC4 El staging caducado se limpia y el vivo no se toca.
///
/// Given un staging viejo de un proceso que se cae, otro vivo que esta
/// escribiendo ahora mismo y un fichero que no es nuestro, when el
/// proceso se reinicia y limpia, then solo desaparece el viejo, el vivo
/// se respeta y lo que no es nuestro no se toca.
#[tokio::test]
async fn har01_ac4_the_expired_staging_is_cleaned_and_the_live_one_is_not() {
    let tmp = a_temp_dir("har01-ac4-staging-");
    let store = FileBlobStore::new(tmp.path()).expect("el almacen abre");
    let digest = store
        .put(b"the bytes")
        .await
        .expect("el contenido se guarda");

    // Un staging viejo, como el que deja un proceso que se cae: con el
    // nombre que pone el almacen y sin nadie escribiendolo.
    let staging_dir = store.the_staging_dir();
    std::fs::create_dir_all(&staging_dir).expect("el staging se crea");
    let old = staging_dir.join(a_staging_name());
    std::fs::write(&old, b"half written").expect("el staging se escribe");
    set_modified(&old, SystemTime::now() - Duration::from_secs(3600));

    // Y uno vivo, que puede ser de otro proceso que esta subiendo: con
    // el nombre que pone el almacen y **bloqueado**, que es lo que dice
    // que alguien lo esta escribiendo ahora mismo.
    //
    // Y **caducado**, a proposito: si solo lo protegiera su fecha, el
    // arranque se lo llevaria, y lo que lo protege es el bloqueo.
    let live = staging_dir.join(a_staging_name());
    let live_file = std::fs::File::create(&live).expect("el staging vivo se escribe");
    live_file.try_lock().expect("el staging vivo se bloquea");
    set_modified(&live, SystemTime::now() - Duration::from_secs(3600));

    // Y un fichero que no es nuestro: el directorio del staging es una
    // convencion, y lo que hay ahi puede ser de otra cosa.
    let foreign = staging_dir.join("not-ours.txt");
    std::fs::write(&foreign, b"de otra cosa").expect("el fichero se escribe");

    // Y un directorio con nuestro nombre, que no se puede borrar: un
    // problema de disco no puede impedir que el almacen siga sirviendo.
    let stuck = staging_dir.join(a_staging_name());
    std::fs::create_dir(&stuck).expect("el directorio se crea");

    // El proceso se reinicia: un almacen nuevo sobre el mismo directorio.
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

    // Y lo que no se puede borrar no es un fallo: se cuenta y el almacen
    // sigue sirviendo.
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

    // Y lo que estaba publicado sigue ahi: limpiar staging no publica ni
    // borra nada.
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

/// El nombre que el almacen le pone a una escritura en curso.
///
/// `<pid>-<nanos:x>-<counter:x>.part`. El arnés lo copia porque lo que
/// quiere es un staging de verdad, con el nombre que el almacen
/// reconoce: un fichero con otro nombre no es una escritura en curso, y
/// el almacen no lo toca.
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

/// AC5 El almacen aguanta un error de disco y la muerte de un proceso.
///
/// Given un staging que no es un directorio y un proceso que se cae a
/// mitad de una subida, when se usa el almacen, then el error de disco es
/// un error de disco y no se publica nada a medias.
#[tokio::test]
async fn har01_ac5_the_store_survives_a_disk_error_and_a_dead_process() {
    // El error de disco: el staging no es un directorio.
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

    // El proceso se cae a mitad de una subida.
    let tmp = a_temp_dir("har01-ac5-");
    let store = FileBlobStore::new(tmp.path()).expect("el almacen abre");
    {
        let mut session = store.begin().await.expect("la sesion abre");
        session
            .append(b"half of the content")
            .await
            .expect("el trozo entra");
        // La sesion se abandona sin commit: un error, un `?` o un panic.
    }
    assert!(
        store
            .staging_entries()
            .expect("el staging se lista")
            .is_empty(),
        "una sesion abandonada no deja staging"
    );
    // Y no se publico nada a medias: en la raiz del almacen solo esta el
    // directorio de staging, ningun blob.
    let entries = the_files_in(tmp.path());
    assert_eq!(
        entries.len(),
        1,
        "no se publico ningun blob: la raiz solo tiene el staging, {entries:?}"
    );
    assert_eq!(entries[0].file_name().unwrap(), "staging");

    // Un proceso muerto de verdad no ejecuta su `Drop`: su staging se
    // queda. El reinicio lo limpia cuando caduca.
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

// ───────────────────── el contrato es el mismo ─────────────────────

/// El contrato de la sesion es el mismo en los dos almacenes.
///
/// El streaming tiene que **guardar**: una sesion que valida el digest y
/// no deja el contenido en el almacen es un digest de nada.
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

/// Y lo que no cuadra, en el almacen en memoria tampoco se guarda.
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
