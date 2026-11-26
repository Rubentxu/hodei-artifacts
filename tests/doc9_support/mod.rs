//! The doc-9 UAT harness.
//!
//! The `doc-9` UAT is sixteen scenarios: CAS-01..04, CAT-01..03,
//! PUB-01..03 and RAW-01..06. They are run by the suites in
//! `tests/doc9_uat.rs`, and this module is what they have in common: the
//! result of a scenario, the report that gathers them, and the helpers.
//!
//! Three rules the harness respects, which are the difference between a
//! UAT and a printout of arguments:
//!
//! 1. Each scenario is approved by its **oracle**, not by the shape of the
//!    response. A 200 is not a PASS: the PASS is what the oracle says,
//!    measured with `sha256sum`, with the bytes on disk, or with the state
//!    the catalog asserts.
//! 2. The command is **recorded**, with what was actually run. Evidence
//!    without a command cannot be repeated.
//! 3. A scenario that cannot be certified today **says so**, with the
//!    reason and what is missing. It is not marked PASS, and it is not hidden: a
//!    PASS that has not been measured is worse than an unexplained FAILED.
//!
//! The report is written to `target/uat/doc9.md`, which is what the
//! pipeline archives. It is the evidence that remains six months from now.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub use crate::support::{
    Deployment, FIRST_ACCOUNT, FIRST_ONLY_REPOSITORY, FIRST_REPOSITORY, SECOND_ACCOUNT,
    SECOND_REPOSITORY,
};

/// The result of a scenario.
///
/// The verdict is not a boolean: it says **why** it is approved or why
/// not, because a FAIL without a reason cannot be fixed.
#[derive(Debug, Clone)]
pub struct Outcome {
    /// The scenario identifier.
    pub id: &'static str,
    /// The "given".
    given: &'static str,
    /// The "when".
    when: &'static str,
    /// The command that was run.
    command: String,
    /// The oracle that was checked.
    oracle: &'static str,
    /// What was measured.
    measured: String,
    /// Whether the scenario is approved.
    passed: bool,
}

impl Outcome {
    /// A scenario that is approved.
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

    /// A scenario that cannot be certified.
    ///
    /// The reason says **what** is missing, not just that something is missing: a "cannot be done"
    /// with nothing more does not say whether a piece of code, a piece of infrastructure
    /// or a decision is missing.
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

/// The UAT results, in order.
static OUTCOMES: Mutex<Vec<Outcome>> = Mutex::new(Vec::new());

/// Records the result of a scenario.
///
/// The report is rewritten with each entry, so that the file
/// always holds everything known up to that moment. This way the report
/// does not depend on **which** test runs last: if the process dies
/// midway, the report has what was certified up to there, which is
/// exactly what a half-finished report should have.
pub fn record(outcome: Outcome) {
    {
        let mut outcomes = outcomes();
        outcomes.push(outcome);
    }
    write_the_report();
}

/// The results, with the lock recovered.
///
/// A panic poisons the lock, and a poisoned lock that stays
/// that way means the scenarios that come later cannot even
/// record their result: a one-eyed failure becomes a
/// mute UAT. The report has to be written **no matter what**: that is why
/// the lock is recovered instead of propagating the poisoning.
pub fn outcomes() -> std::sync::MutexGuard<'static, Vec<Outcome>> {
    OUTCOMES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Writes the UAT report.
///
/// The report also says **how many** scenarios remain uncertified and
/// why: a report with a PASS and a "cannot be done" hidden in a cell
/// is a report that does not tell the truth.
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

    // The report is written **atomically**: to a temporary file and then
    // renamed. A report that is being written is read half-done, and
    // a half-done report looks like a missing report: the gate would
    // read it as if scenarios were missing.
    let path = the_report_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("the directory of the report");
    }
    let temporary = path.with_extension("md.temporal");
    std::fs::write(&temporary, text).expect("the report is written");
    std::fs::rename(&temporary, &path).expect("the report is published");
}

/// The path of the report.
pub fn the_report_path() -> PathBuf {
    let configured = std::env::var_os("HODEI_DOC9_REPORT").map(PathBuf::from);
    configured.unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("uat")
            .join("doc9.md")
    })
}

/// The moment of execution.
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

/// The tools it was run with.
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

/// The path of the content within the repository.
pub fn the_path(repository: &str, path: &str) -> String {
    format!("/repository/{repository}/{path}")
}

/// The content that is published.
pub fn the_content() -> Vec<u8> {
    (0u16..4096).map(|value| (value % 251) as u8).collect()
}

/// The digest of a content.
pub fn the_digest(content: &[u8]) -> String {
    hodei_evidence::ContentDigest::of_bytes(content).value
}

/// Where a blob lives in the byte store.
pub fn blob_path(root: &Path, digest: &str) -> PathBuf {
    root.join(&digest[..2]).join(&digest[2..4]).join(digest)
}

/// A clean deployment with the catalog seeded.
pub fn a_deployment() -> Deployment {
    let deployment = Deployment::new();
    deployment.seed();
    deployment
}
