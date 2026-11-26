//! HSP-213: el binario productivo rechaza el perfil embebido.
//!
//! La mitad del corte que se observa con el **proceso real**, no con una
//! unit que lo simula: un despliegue sin `[graph]` (el perfil antiguo de
//! `HODEI_ROCKSDB__PATH`) muere con el mensaje de migracion: nombra la
//! retirada, la autoridad externa y el retiro de las herramientas de
//! operacion (HSP-314 F4). Y no crea nada local: la negativa es previa a
//! tocar el almacen de contenido o cualquier path del perfil antiguo.
//!
//! Desde HSP-314 F4 el perfil de grafo es el unico: no hay variante del
//! binario que pueda arrancar el perfil embebido, asi que este rechazo es
//! el contrato permanente de un despliegue sin `[graph]`.

mod hsp213_profile_env;

use hsp213_profile_env::{BIN, free_port, retired_profile_env};
use std::process::{Command, Stdio};

/// AC2: el binario productivo rechaza el perfil embebido con el mensaje de
/// migracion y no toca el almacen local.
#[test]
fn the_production_binary_refuses_the_embedded_profile_with_the_migration_message() {
    let tmp = tempfile::tempdir().expect("a temp dir");
    let db = tmp.path().join("db");
    let cas = tmp.path().join("cas");

    let mut command = Command::new(BIN);
    command.env_remove("RUN_MODE").stdin(Stdio::null());
    for (key, value) in retired_profile_env(tmp.path(), free_port()) {
        command.env(key, value);
    }
    let output = command.output().expect("the binary runs");

    assert!(
        !output.status.success(),
        "the production binary must refuse the embedded profile, not boot it"
    );
    let text = String::from_utf8_lossy(&output.stderr).into_owned();
    // El mensaje nombra la retirada, el destino y el retiro de las
    // herramientas: quien lea el log sabe exactamente a donde migrar.
    for fragment in [
        "retired (HSP-213",
        "declare a [graph] section",
        "HSP-314 F4",
    ] {
        assert!(
            text.contains(fragment),
            "the refusal must name `{fragment}`; stderr was:\n{text}"
        );
    }
    assert!(
        !db.exists() && !cas.exists(),
        "the refusal must not create the local stores of the profile it refuses"
    );
}
