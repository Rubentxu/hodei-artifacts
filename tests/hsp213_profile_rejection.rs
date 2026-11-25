//! HSP-213: el binario productivo rechaza el perfil embebido.
//!
//! La mitad del corte que se observa con el **proceso real**, no con una
//! unit que lo simula: construido `--no-default-features` — el arbol del
//! binario productivo, el que empaqueta `scripts/package-release.sh` — un
//! despliegue sin `[graph]` (el perfil antiguo de `HODEI_ROCKSDB__PATH`)
//! muere con el mensaje de migracion: nombra la retirada, la autoridad
//! externa, la herramienta legacy y la ventana (abierta 2026-11-22). Y no
//! crea el path local: la negativa es previa a tocar el almacen.
//!
//! El arreglo de features
//! esta en el crate: sin `legacy` este target declara el test, y con ella
//! queda vacio (el binario de ese arbol no puede rechazar nada y mentir
//! pasando seria peor que no decir nada).

#[cfg(not(feature = "legacy"))]
mod hsp213_profile_env;

#[cfg(not(feature = "legacy"))]
use hsp213_profile_env::{BIN, embedded_profile_env, free_port};
#[cfg(not(feature = "legacy"))]
use std::process::{Command, Stdio};

/// AC2: el binario productivo (sin `legacy`) rechaza el perfil embebido con
/// el mensaje de migracion y no toca el almacen local.
#[cfg(not(feature = "legacy"))]
#[test]
fn the_production_binary_refuses_the_embedded_profile_with_the_migration_message() {
    let tmp = tempfile::tempdir().expect("a temp dir");
    let db = tmp.path().join("db");

    let mut command = Command::new(BIN);
    command.env_remove("RUN_MODE").stdin(Stdio::null());
    for (key, value) in embedded_profile_env(tmp.path(), free_port()) {
        command.env(key, value);
    }
    let output = command.output().expect("the binary runs");

    assert!(
        !output.status.success(),
        "the production binary must refuse the embedded profile, not boot it"
    );
    let text = String::from_utf8_lossy(&output.stderr).into_owned();
    // El mensaje nombra la retirada, el destino, la herramienta y la
    // ventana: quien lea el log sabe exactamente a donde migrar.
    for fragment in [
        "retired from the production binary (HSP-213)",
        "declare a [graph] section",
        "hodei-recovery",
        "--features legacy",
        "migration window",
        "2026-11-22",
    ] {
        assert!(
            text.contains(fragment),
            "the refusal must name `{fragment}`; stderr was:\n{text}"
        );
    }
    assert!(
        !db.exists(),
        "the refusal must not create the local store it refuses to open"
    );
}
