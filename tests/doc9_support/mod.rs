//! El arnes de la UAT de doc-9.
//!
//! La UAT de `doc-9` son dieciseis escenarios: CAS-01..04, CAT-01..03,
//! PUB-01..03 y RAW-01..06. Los ejecutan las suites que hay en
//! `tests/doc9_uat.rs`, y este modulo es lo que tienen en comun: el
//! resultado de un escenario, el informe que los reune y los ayudantes.
//!
//! Tres reglas que el arnes respeta, y que son la diferencia entre una
//! UAT y un Printing de argumentos:
//!
//! 1. Cada escenario se aprueba por su **oráculo**, no por la forma de la
//!    respuesta. Un 200 no es un PASS: el PASS es lo que el oráculo dice,
//!    medido con `sha256sum`, con los bytes del disco, o con el estado
//!    que el catálogo afirma.
//! 2. El comando se **registra**, con lo que realmente se ejecutó. Una
//!    evidencia sin comando no se puede repetir.
//! 3. Un escenario que no se puede certificar hoy **se dice**, con el
//!    motivo y con lo que falta. No se marca PASS, y no se esconde: un
//!    PASS que no se ha medido es peor que un FAILED sin explicar.
//!
//! El informe se escribe en `target/uat/doc9.md`, que es lo que la
//! pipeline archiva. Es la evidencia que queda dentro de seis meses.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub use crate::support::{
    Deployment, FIRST_ACCOUNT, FIRST_ONLY_REPOSITORY, FIRST_REPOSITORY, SECOND_ACCOUNT,
    SECOND_REPOSITORY,
};

/// El resultado de un escenario.
///
/// El veredicto no es un booleano: dice **por que** se aprueba o por que
/// no, porque un FAIL sin motivo no se puede corregir.
#[derive(Debug, Clone)]
pub struct Outcome {
    /// El identificador del escenario.
    pub id: &'static str,
    /// El "given".
    given: &'static str,
    /// El "when".
    when: &'static str,
    /// El comando que se ejecutó.
    command: String,
    /// El oráculo que se comprobó.
    oracle: &'static str,
    /// Lo que se midió.
    measured: String,
    /// Si el escenario se aprueba.
    passed: bool,
}

impl Outcome {
    /// Un escenario que se aprueba.
    pub fn passed(
        id: &'static str,
        given: &'static str,
        when: &'static str,
        command: String,
        oracle: &'static str,
        measured: String,
    ) -> Self {
        Self {
            id,
            given,
            when,
            command,
            oracle,
            measured,
            passed: true,
        }
    }

    /// Un escenario que no se puede certificar.
    ///
    /// El motivo dice **que** falta, no solo que falta: un "no se puede"
    /// sin mas no dice si falta una pieza de codigo, una de infraestructura
    /// o una decision.
    pub fn uncertified(
        id: &'static str,
        given: &'static str,
        when: &'static str,
        command: String,
        oracle: &'static str,
        reason: String,
    ) -> Self {
        Self {
            id,
            given,
            when,
            command,
            oracle,
            measured: reason,
            passed: false,
        }
    }
}

/// Los resultados de la UAT, en orden.
static OUTCOMES: Mutex<Vec<Outcome>> = Mutex::new(Vec::new());

/// Anota el resultado de un escenario.
///
/// El informe se reescribe con cada anotacion, para que el fichero
/// tenga siempre todo lo que se sabe hasta ese momento. Asi el informe
/// no depende de **que** test corre el ultimo: si el proceso se muere a
/// mitad, el informe tiene lo que se certified hasta ahi, que es
/// exactamente lo que un informe a medias debe tener.
pub fn record(outcome: Outcome) {
    {
        let mut outcomes = outcomes();
        outcomes.push(outcome);
    }
    write_the_report();
}

/// Los resultados, con el cerrojo recuperado.
///
/// Un panic envenena el cerrojo, y un cerrojo envenenado que se queda
/// ahi hace que los escenarios que vienen despues no puedan ni
/// registrar su resultado: un fallo tuerto se convierte en una UAT
/// muda. El informe tiene que escribirse **pase lo que pase**: por eso
/// el cerrojo se recupera en vez de propagar el envenenamiento.
pub fn outcomes() -> std::sync::MutexGuard<'static, Vec<Outcome>> {
    OUTCOMES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Escribe el informe de la UAT.
///
/// El informe dice tambien **cuantos** escenarios quedan sin certificar y
/// por que: un informe con un PASS y un "no se" escondidos en una celda
/// es un informe que no dice la verdad.
pub fn write_the_report() {
    let mut text = String::new();
    let outcomes = outcomes();
    text.push_str("# UAT doc-9 — Raw Hosted + CAS + Catalog\n\n");
    text.push_str("**Cuando:** ");
    text.push_str(&the_moment());
    text.push_str("\n\n**Herramientas:** ");
    text.push_str(&the_tools());
    text.push_str("\n\n");

    let certified = outcomes.iter().filter(|outcome| outcome.passed).count();
    let uncertified = outcomes.len() - certified;
    text.push_str(&format!(
        "**Resultado:** {certified} de {} escenarios certificados; {uncertified} sin certificar.\n\n",
        outcomes.len()
    ));

    text.push_str("| ID | Given | When | Comando | Oraculo | Resultado | Veredicto |\n");
    text.push_str("|---|---|---|---|---|---|---|\n");
    for outcome in outcomes.iter() {
        text.push_str(&format!(
            "| {} | {} | {} | `{}` | {} | {} | {} |\n",
            outcome.id,
            outcome.given,
            outcome.when,
            outcome.command.replace('|', "\\|"),
            outcome.oracle,
            outcome.measured.replace('|', "\\|"),
            if outcome.passed {
                "PASS"
            } else {
                "SIN CERTIFICAR"
            },
        ));
    }

    text.push_str("\n## Lo que este informe **no** certifica\n\n");
    for outcome in outcomes.iter().filter(|outcome| !outcome.passed) {
        text.push_str(&format!("- **{}**: {}\n", outcome.id, outcome.measured));
    }
    text.push_str(
        "\n- **Ni el hito entero.** La UAT de doc-9 certifica la superficie de contenido \
         crudo, que es la parte entregada. El hito M10 sigue abierto con HAR-06..32.\n",
    );

    // El informe se escribe **atomico**: a un fichero temporal y luego
    // renombrado. Un informe que se esta escribiendo se lee a medias, y
    // un informe a medias parece un informe que no esta: el gate lo
    // leeria como si le faltaran escenarios.
    let path = the_report_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("the directory of the report");
    }
    let temporary = path.with_extension("md.temporal");
    std::fs::write(&temporary, text).expect("the report is written");
    std::fs::rename(&temporary, &path).expect("the report is published");
}

/// La ruta del informe.
pub fn the_report_path() -> PathBuf {
    let configured = std::env::var_os("HODEI_DOC9_REPORT").map(PathBuf::from);
    configured.unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("uat")
            .join("doc9.md")
    })
}

/// El momento de la ejecucion.
fn the_moment() -> String {
    let output = std::process::Command::new("date")
        .arg("--iso-8601=seconds")
        .output();
    output
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|moment| !moment.is_empty())
        .unwrap_or_else(|| "sin medir".to_string())
}

/// Las herramientas con las que se ejecuto.
fn the_tools() -> String {
    let mut tools = Vec::new();
    for (program, args) in [
        ("curl", vec!["--version"]),
        ("sha256sum", vec!["--version"]),
        ("cargo", vec!["--version"]),
    ] {
        let Ok(output) = std::process::Command::new(program).args(&args).output() else {
            continue;
        };
        let first = String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or_default()
            .to_string();
        if !first.is_empty() {
            tools.push(first);
        }
    }
    tools.join("; ")
}

/// La ruta del contenido dentro del repositorio.
pub fn the_path(repository: &str, path: &str) -> String {
    format!("/repository/{repository}/{path}")
}

/// El contenido que se publica.
pub fn the_content() -> Vec<u8> {
    (0u16..4096).map(|value| (value % 251) as u8).collect()
}

/// El digest de un contenido.
pub fn the_digest(content: &[u8]) -> String {
    hodei_evidence::ContentDigest::of_bytes(content).value
}

/// Donde vive un blob en el almacen de bytes.
pub fn blob_path(root: &Path, digest: &str) -> PathBuf {
    root.join(&digest[..2]).join(&digest[2..4]).join(digest)
}

/// Un despliegue limpio con el catalogo sembrado.
pub fn a_deployment() -> Deployment {
    let deployment = Deployment::new();
    deployment.seed();
    deployment
}
