//! The conformance profile of every format and modality.
//!
//! [`RepositoryFormat::serves_modes`] says what this release serves, and
//! [`RepositoryFormat::served`] says what a client may create. This module
//! is the third voice of the same registry: which suites accredit each
//! cell of the matrix, so that "the release serves it" and "the release
//! proved it" are one declaration and not two.
//!
//! The matrix is not written by hand anywhere else. The capabilities
//! endpoint reads the registry, the creation of a repository validates the
//! format against the modality with [`RepositoryFormat::supports`], and
//! the meta-conformance client (the test that consumes this module)
//! selects the suites named here and checks they exist and that they
//! speak of the format and the modality they accredit. A table in a
//! document, a table in the frontend, or a list in the CI would be a
//! second truth about the same thing, and a second truth drifts.
//!
//! # The declaration can be audited, and a fraudulent one fails
//!
//! A client of the matrix does not have to trust it. [`audit`] checks a
//! declared matrix against the declared service: a cell accredited for a
//! modality the release does not serve is a fraud ([`AuditFailure::AccreditedButNotServed`]),
//! a modality served without an accrediting suite is a promise without
//! proof ([`AuditFailure::ServedWithoutAccreditation`]), and evidence that
//! names nothing is noise ([`AuditFailure::UnknownEvidence`]). The content
//! of the evidence is checked on top of that: [`accredits`] reads the
//! suite and demands that it names the format, and the modality when the
//! modality is a composed one. A declaration that marks cargo hosted as
//! accredited by the generic proxy suite fails there: the suite does not
//! speak of cargo.

use std::collections::BTreeSet;

use crate::model::{RepositoryFormat, RepositoryMode};

/// The version of the product that this profile belongs to.
///
/// The registry is one artifact of one release: every cell it accredits
/// is accredited by the same build. The meta-conformance client stamps
/// the exact commit of the product next to this version when it writes
/// its report, so the question "which build proved this cell" has an
/// answer that is read, not remembered.
pub const PROFILE_VERSION: &str = concat!("hodei-catalog ", env!("CARGO_PKG_VERSION"));

/// What the registry declares about one cell of the matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accreditation {
    /// The release serves the modality and the suites named in the cell
    /// accredit it.
    Accredited,
    /// The release refuses the modality for the format: the creation of
    /// such a repository is rejected with a dialect error, and no suite
    /// can accredit what is not served.
    NotSupported,
}

impl Accreditation {
    /// The name of the status, the one that goes to the report.
    pub fn as_str(&self) -> &'static str {
        match self {
            Accreditation::Accredited => "ACCREDITED",
            Accreditation::NotSupported => "NOT_SUPPORTED",
        }
    }
}

/// One cell of the matrix: a format in a modality, and what accredits it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConformanceCell {
    /// The format of the cell.
    pub format: RepositoryFormat,
    /// The modality of the cell.
    pub mode: RepositoryMode,
    /// What the registry declares about the cell.
    pub status: Accreditation,
    /// The suites that accredit the cell. Empty when the cell is refused.
    pub evidence: &'static [&'static str],
    /// The suites that prove the refusal of the cell in its own dialect,
    /// when a client-facing surface exists to be refused. The refusal of
    /// the configuration is proven by the meta-conformance client itself,
    /// which walks every declared combination.
    pub refusal_evidence: &'static [&'static str],
    /// The limit the cell carries, when the refusal is a decision and not
    /// an absence. Empty when there is nothing to say beyond the status.
    pub note: &'static str,
}

impl ConformanceCell {
    /// The names of every suite the cell points at, accreditation and
    /// refusal together.
    pub fn suites(&self) -> impl Iterator<Item = &'static str> {
        self.evidence
            .iter()
            .copied()
            .chain(self.refusal_evidence.iter().copied())
    }
}

/// The suites that accredit a served cell.
///
/// Every served modality carries its suites here: a modality that is
/// served and names no suite cannot pass [`audit`], so serving a modality
/// without proving it is not a state the registry can be in. The suites
/// are the test targets of the workspace, named by file stem, and the
/// meta-conformance client reads them from disk to be sure they exist and
/// that they speak of what they accredit.
fn evidence_of(format: RepositoryFormat, mode: RepositoryMode) -> &'static [&'static str] {
    match (format, mode) {
        (RepositoryFormat::Raw, RepositoryMode::Hosted) => &[
            "har04_raw_hosted_contract",
            "raw_conformity_uat",
            "raw_hosted_restart_contract",
        ],
        (RepositoryFormat::Raw, RepositoryMode::Proxy) => {
            &["hsp106_proxy_e2e", "hsp107_proxy_cache_e2e"]
        }
        (RepositoryFormat::Raw, RepositoryMode::Group) => &["hsp108_group_e2e"],
        (RepositoryFormat::Oci, RepositoryMode::Hosted) => &[
            "oci_push_contract",
            "oci_pull_contract",
            "oci_index_contract",
            "oci_e2e_contract",
        ],
        (RepositoryFormat::Oci, RepositoryMode::Proxy) => &["hsp228_oci_proxy"],
        (RepositoryFormat::Oci, RepositoryMode::Group) => &["hsp229_oci_group"],
        (RepositoryFormat::Maven2, RepositoryMode::Hosted) => {
            &["hsp109_maven_e2e", "hsp109_maven_cli"]
        }
        (RepositoryFormat::Maven2, RepositoryMode::Proxy) => {
            &["hsp110_maven_remote_e2e", "hsp110_maven_cli"]
        }
        (RepositoryFormat::Maven2, RepositoryMode::Group) => &["hsp110_maven_remote_e2e"],
        (RepositoryFormat::Npm, RepositoryMode::Hosted) => &["hsp111_npm_e2e", "hsp111_npm_cli"],
        (RepositoryFormat::Npm, RepositoryMode::Proxy) => &["hsp111_npm_cli"],
        (RepositoryFormat::Npm, RepositoryMode::Group) => &["hsp111_npm_cli", "hsp111_npm_e2e"],
        (RepositoryFormat::PyPi, RepositoryMode::Hosted) => &["hsp112_pypi_e2e", "hsp112_pypi_cli"],
        (RepositoryFormat::PyPi, RepositoryMode::Proxy) => &["hsp112_pypi_e2e"],
        (RepositoryFormat::PyPi, RepositoryMode::Group) => &["hsp112_pypi_e2e"],
        (RepositoryFormat::Cargo, RepositoryMode::Hosted) => {
            &["hsp119_cargo_e2e", "hsp119_cargo_cli"]
        }
        (RepositoryFormat::NuGet, RepositoryMode::Hosted) => {
            &["hsp120_nuget_e2e", "hsp120_dotnet_client"]
        }
        (RepositoryFormat::NuGet, RepositoryMode::Proxy) => &["hsp120_nuget_e2e"],
        (RepositoryFormat::Go, RepositoryMode::Hosted) => &["hsp121_go_e2e", "hsp121_go_client"],
        (RepositoryFormat::Go, RepositoryMode::Proxy) => &["hsp121_go_e2e", "hsp121_go_client"],
        (RepositoryFormat::Go, RepositoryMode::Group) => &["hsp232_go_group"],
        (RepositoryFormat::Apt, RepositoryMode::Hosted) => &["hsp122_apt_e2e", "hsp122_apt_client"],
        (RepositoryFormat::Rpm, RepositoryMode::Hosted) => &["hsp122_yum_e2e", "hsp122_yum_client"],
        (RepositoryFormat::Rpm, RepositoryMode::Proxy) => &["hsp235_rpm_proxy"],
        (RepositoryFormat::Rpm, RepositoryMode::Group) => &["hsp236_rpm_group"],
        (RepositoryFormat::Conan, RepositoryMode::Hosted) => &["hsp159_conan_client"],
        (RepositoryFormat::Galaxy, RepositoryMode::Hosted) => &["hsp217_galaxy_client"],
        (RepositoryFormat::Terraform, RepositoryMode::Hosted) => {
            &["hsp158_terraform_modules_client"]
        }
        (RepositoryFormat::Terraform, RepositoryMode::Proxy) => &["hsp157_terraform_mirror"],
        _ => &[],
    }
}

/// The suites that prove a refusal in the dialect of the format.
///
/// A refused modality is refused by the configuration and by the surface
/// of the format when the format has one. Where the refusal has been
/// proven against the wire —a group of go answered with the dialect error
/// of go, not with a raw answer— the suite that proved it is named here,
/// and the meta-conformance client checks it like the accreditation
/// suites: it exists, and it speaks of the format and the modality it
/// refuses.
fn refusal_evidence_of(format: RepositoryFormat, mode: RepositoryMode) -> &'static [&'static str] {
    match (format, mode) {
        (RepositoryFormat::NuGet, RepositoryMode::Group) => &["hsp120_nuget_e2e"],
        _ => &[],
    }
}

/// The limit a cell carries, when the refusal is a decision.
fn note_of(format: RepositoryFormat, mode: RepositoryMode) -> &'static str {
    match (format, mode) {
        (RepositoryFormat::Cargo, RepositoryMode::Proxy) => {
            "the proxy of cargo needs a client-verified upstream before it is declared (HSP-156); \
             the configuration refuses it today"
        }
        (RepositoryFormat::Cargo, RepositoryMode::Group) => {
            "cargo has no group until a client-verified surface declares one (HSP-156)"
        }
        (RepositoryFormat::Go, RepositoryMode::Group) => {
            "a go group resolves a version from a single member: its .info, .mod and .zip never come from two members, so the go.sum of the client describes one version"
        }
        (RepositoryFormat::NuGet, RepositoryMode::Group) => {
            "a nuget group is refused in the dialect of nuget: it never degrades to a raw answer"
        }
        (RepositoryFormat::Terraform, RepositoryMode::Proxy) => {
            "accredited by protocol evidence, not by a native client: hsp157_terraform_mirror \
             speaks the network mirror protocol with curl against a fake upstream; the terraform \
             CLI is only exercised on the hosted registry (hsp158_terraform_modules_client)"
        }
        (RepositoryFormat::Hf, _) => {
            "hf is read-only seeded content, not a served modality: there is no ingestion by API, \
             so no repository can be created; the read dialect is proven with the real client by \
             hsp218_hf_client"
        }
        (RepositoryFormat::Helm, _) => {
            "no helm repository is served: charts ride on the OCI surface (hsp121_helm_oci)"
        }
        _ => "",
    }
}

/// The cell the registry declares for a format in a modality.
///
/// The status is derived, not written: a cell is accredited when suites
/// name it, and refused when they do not. Deriving it is what keeps one
/// truth —to serve a modality is to name the suites that prove it— and
/// what [`audit`] enforces on any declaration it is handed.
pub fn cell(format: RepositoryFormat, mode: RepositoryMode) -> ConformanceCell {
    let evidence = evidence_of(format, mode);
    let status = if evidence.is_empty() {
        Accreditation::NotSupported
    } else {
        Accreditation::Accredited
    };
    ConformanceCell {
        format,
        mode,
        status,
        evidence,
        refusal_evidence: refusal_evidence_of(format, mode),
        note: note_of(format, mode),
    }
}

/// Every cell of the matrix: each declared format in each modality.
pub fn matrix() -> Vec<ConformanceCell> {
    let modes = [
        RepositoryMode::Hosted,
        RepositoryMode::Proxy,
        RepositoryMode::Group,
    ];
    RepositoryFormat::declared()
        .iter()
        .flat_map(|format| modes.iter().map(move |mode| cell(*format, *mode)))
        .collect()
}

/// The modality pairs this release serves, derived from the registry.
///
/// The capabilities endpoint and the meta-conformance client both start
/// here: what is served is what the registry says, and nothing else.
pub fn served_pairs() -> Vec<(RepositoryFormat, RepositoryMode)> {
    RepositoryFormat::served()
        .iter()
        .flat_map(|format| {
            format
                .serves_modes()
                .iter()
                .map(move |mode| (*format, *mode))
        })
        .collect()
}

/// What can fail when a declared matrix is audited.
///
/// The failure names the cell and the rule it broke: an explanation the
/// client can answer with, and not a bare refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditFailure {
    /// The matrix does not cover every declared format in every modality,
    /// or it covers one twice, or it names a format that is not declared.
    Coverage {
        /// What is wrong about the coverage.
        detail: String,
    },
    /// A cell points at a name that is not a suite of this workspace.
    UnknownEvidence {
        /// The format of the cell.
        format: &'static str,
        /// The modality of the cell.
        mode: &'static str,
        /// The name that is not a suite.
        stem: String,
    },
    /// A cell is accredited for a modality the declared service does not
    /// serve: an accreditation of something that does not exist.
    AccreditedButNotServed {
        /// The format of the cell.
        format: &'static str,
        /// The modality of the cell.
        mode: &'static str,
    },
    /// A cell is accredited and names no suite: an accreditation without
    /// proof.
    AccreditedWithoutEvidence {
        /// The format of the cell.
        format: &'static str,
        /// The modality of the cell.
        mode: &'static str,
    },
    /// A refused cell carries accreditation suites: a refusal that
    /// pretends to be served.
    NotSupportedWithEvidence {
        /// The format of the cell.
        format: &'static str,
        /// The modality of the cell.
        mode: &'static str,
    },
    /// The declared service serves a modality that no suite accredits: a
    /// promise without proof.
    ServedWithoutAccreditation {
        /// The format of the pair.
        format: &'static str,
        /// The modality of the pair.
        mode: &'static str,
    },
}

impl std::fmt::Display for AuditFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditFailure::Coverage { detail } => write!(
                f,
                "the matrix does not cover the declared formats and modalities: {detail}"
            ),
            AuditFailure::UnknownEvidence { format, mode, stem } => {
                write!(
                    f,
                    "the cell {format}/{mode} names `{stem}`, which is not a suite of this workspace"
                )
            }
            AuditFailure::AccreditedButNotServed { format, mode } => {
                write!(
                    f,
                    "the cell {format}/{mode} is accredited for a modality the release does not serve: an accreditation of something that does not exist"
                )
            }
            AuditFailure::AccreditedWithoutEvidence { format, mode } => {
                write!(
                    f,
                    "the cell {format}/{mode} is accredited without a single suite: an accreditation without proof"
                )
            }
            AuditFailure::NotSupportedWithEvidence { format, mode } => {
                write!(
                    f,
                    "the cell {format}/{mode} is refused and still names accreditation suites: a refusal that pretends to be served"
                )
            }
            AuditFailure::ServedWithoutAccreditation { format, mode } => {
                write!(
                    f,
                    "the release serves {format}/{mode} and no suite accredits it: a promise without proof"
                )
            }
        }
    }
}

/// Whether a name can be a suite of this workspace.
///
/// The suites are the integration test targets, named by file stem: lower
/// case letters, digits and underscores, long enough to be a name and
/// short enough to be one. The existence of the file is the client's
/// check; this is only the shape a name must have to be one at all.
fn plausible_suite(stem: &str) -> bool {
    (4..=64).contains(&stem.len())
        && stem.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        })
        && stem
            .chars()
            .any(|character| character.is_ascii_alphabetic())
}

/// The audit of a declared matrix against a declared service.
///
/// `matrix` is the cells a client was handed, `declared` the formats the
/// registry declares, and `served` the modality pairs the release claims
/// to serve. The audit demands coherence between the three: everything
/// declared is covered once, an accreditation is always backed by suites
/// and always about a served modality, and a served modality is always
/// accredited. A fraudulent declaration —cargo hosted accredited by the
/// proxy suite, cargo proxy promised without evidence, a suite name
/// erased— fails here with the rule it broke.
pub fn audit(
    matrix: &[ConformanceCell],
    declared: &[RepositoryFormat],
    served: &[(RepositoryFormat, RepositoryMode)],
) -> Result<(), AuditFailure> {
    let modes = [
        RepositoryMode::Hosted,
        RepositoryMode::Proxy,
        RepositoryMode::Group,
    ];
    let mut expected = BTreeSet::new();
    for format in declared {
        for mode in modes {
            expected.insert((format.as_str(), mode.as_str()));
        }
    }
    let served_names: BTreeSet<(&str, &str)> = served
        .iter()
        .map(|(format, mode)| (format.as_str(), mode.as_str()))
        .collect();
    let mut found = BTreeSet::new();
    for cell in matrix {
        if !RepositoryFormat::declared().contains(&cell.format) {
            return Err(AuditFailure::Coverage {
                detail: format!(
                    "the cell {}/{} names a format the registry does not declare",
                    cell.format.as_str(),
                    cell.mode.as_str()
                ),
            });
        }
        let coordinates = (cell.format.as_str(), cell.mode.as_str());
        if !found.insert(coordinates) {
            return Err(AuditFailure::Coverage {
                detail: format!(
                    "the cell {}/{} is declared twice",
                    cell.format.as_str(),
                    cell.mode.as_str()
                ),
            });
        }
        let (format, mode) = coordinates;
        let is_served = served_names.contains(&coordinates);
        match cell.status {
            Accreditation::Accredited => {
                if !is_served {
                    return Err(AuditFailure::AccreditedButNotServed { format, mode });
                }
                if cell.evidence.is_empty() {
                    return Err(AuditFailure::AccreditedWithoutEvidence { format, mode });
                }
                for stem in cell.suites() {
                    if !plausible_suite(stem) {
                        return Err(AuditFailure::UnknownEvidence {
                            format,
                            mode,
                            stem: stem.to_string(),
                        });
                    }
                }
            }
            Accreditation::NotSupported => {
                if is_served && cell.evidence.is_empty() {
                    return Err(AuditFailure::ServedWithoutAccreditation { format, mode });
                }
                if !cell.evidence.is_empty() {
                    return Err(AuditFailure::NotSupportedWithEvidence { format, mode });
                }
                for stem in cell.suites() {
                    if !plausible_suite(stem) {
                        return Err(AuditFailure::UnknownEvidence {
                            format,
                            mode,
                            stem: stem.to_string(),
                        });
                    }
                }
            }
        }
    }
    if let Some(missing) = expected.difference(&found).next() {
        return Err(AuditFailure::Coverage {
            detail: format!(
                "the cell {}/{} is missing from the matrix",
                missing.0, missing.1
            ),
        });
    }
    for (format, mode) in served {
        let accredited = matrix.iter().any(|cell| {
            cell.format == *format && cell.mode == *mode && cell.status == Accreditation::Accredited
        });
        if !accredited {
            return Err(AuditFailure::ServedWithoutAccreditation {
                format: format.as_str(),
                mode: mode.as_str(),
            });
        }
    }
    Ok(())
}

/// The names a suite must speak of, for the format of the cell.
///
/// The first alias is the name on the wire; the others are the names the
/// suites and the clients use. A suite that accredits a format names it:
/// that is the rule a fraudulent declaration cannot pass, because the
/// generic proxy suite does not speak of cargo and the cargo suite does
/// not speak of nuget.
fn aliases_of(format: RepositoryFormat) -> &'static [&'static str] {
    match format {
        RepositoryFormat::Raw => &["raw"],
        RepositoryFormat::Oci => &["oci"],
        RepositoryFormat::Maven2 => &["maven2", "maven"],
        RepositoryFormat::Npm => &["npm"],
        RepositoryFormat::PyPi => &["pypi"],
        RepositoryFormat::Cargo => &["cargo"],
        RepositoryFormat::NuGet => &["nuget"],
        RepositoryFormat::Go => &["go"],
        RepositoryFormat::Helm => &["helm"],
        RepositoryFormat::Apt => &["apt"],
        RepositoryFormat::Rpm => &["rpm", "yum", "dnf"],
        RepositoryFormat::Terraform => &["terraform", "tofu", "opentofu"],
        RepositoryFormat::Conan => &["conan"],
        RepositoryFormat::Galaxy => &["galaxy", "ansible"],
        RepositoryFormat::Hf => &["hf", "huggingface"],
    }
}

/// The words a suite must speak of, for a composed modality.
///
/// The hosted modality is the write surface every publish exercise runs
/// on, and asking every suite for the word would be a rule about prose
/// and not about proof. The proxy and the group are composed modalities:
/// a suite that accredits one names it, because resolution against an
/// upstream or across members is machinery the file exercises on purpose.
fn mode_words_of(mode: RepositoryMode) -> &'static [&'static str] {
    match mode {
        RepositoryMode::Proxy => &["proxy"],
        RepositoryMode::Group => &["group", "grupo"],
        RepositoryMode::Hosted => &[],
    }
}

/// What can fail when the content of a suite is matched against a cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceMismatch {
    /// Why the suite does not speak of the cell.
    pub detail: String,
}

impl std::fmt::Display for EvidenceMismatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.detail)
    }
}

/// Whether `content` is the text of a suite that the cell declares and
/// that speaks of the format and the modality of the cell.
///
/// The words are matched whole and without case: a mention of cargo
/// inside a longer word is not a mention of cargo, and the match is the
/// one a reader makes, not the one a substring makes. This is the check
/// that a fraudulent declaration cannot pass: an accreditation of cargo
/// hosted by the generic proxy suite carries the right shape and the
/// right names, and the suite still does not say cargo.
pub fn accredits(
    cell: &ConformanceCell,
    stem: &str,
    content: &str,
) -> Result<(), EvidenceMismatch> {
    if cell.status != Accreditation::Accredited {
        return Err(EvidenceMismatch {
            detail: format!(
                "the cell {}/{} is refused: nothing accredits what is not served",
                cell.format.as_str(),
                cell.mode.as_str()
            ),
        });
    }
    names_the_cell(cell, stem, content, cell.evidence)
}

/// Whether `content` is the text of a suite that proves the refusal the
/// cell declares.
pub fn refuses(cell: &ConformanceCell, stem: &str, content: &str) -> Result<(), EvidenceMismatch> {
    if cell.status != Accreditation::NotSupported {
        return Err(EvidenceMismatch {
            detail: format!(
                "the cell {}/{} is served: nothing is refused to prove",
                cell.format.as_str(),
                cell.mode.as_str()
            ),
        });
    }
    names_the_cell(cell, stem, content, cell.refusal_evidence)
}

fn names_the_cell(
    cell: &ConformanceCell,
    stem: &str,
    content: &str,
    declared: &[&str],
) -> Result<(), EvidenceMismatch> {
    let (format, mode) = (cell.format.as_str(), cell.mode.as_str());
    if !declared.contains(&stem) {
        return Err(EvidenceMismatch {
            detail: format!("the suite `{stem}` is not one the cell {format}/{mode} declares"),
        });
    }
    let words = words_of(content);
    let aliases = aliases_of(cell.format);
    if !aliases.iter().any(|alias| words.contains(*alias)) {
        return Err(EvidenceMismatch {
            detail: format!(
                "the suite `{stem}` does not name the format {format}: it cannot accredit it"
            ),
        });
    }
    let mode_words = mode_words_of(cell.mode);
    if !mode_words.is_empty() && !mode_words.iter().any(|word| words.contains(*word)) {
        return Err(EvidenceMismatch {
            detail: format!(
                "the suite `{stem}` does not name the modality {mode}: it cannot accredit it"
            ),
        });
    }
    Ok(())
}

/// The words of a text, lower case, whole: what a reader sees as a word.
fn words_of(content: &str) -> BTreeSet<String> {
    content
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(|word| word.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_honest_matrix_passes_the_audit() {
        audit(&matrix(), RepositoryFormat::declared(), &served_pairs())
            .expect("the registry is coherent with itself");
    }

    /// The fraud of the encargo: cargo hosted accredited by the generic
    /// proxy suite. The shape is right, the names are right, and the
    /// suite does not speak of cargo — so the client fails, and it is
    /// told why.
    #[test]
    fn a_cell_accredited_by_a_suite_that_does_not_speak_of_the_format_is_refused() {
        let matrix = matrix();
        let cargo_hosted = matrix
            .iter()
            .find(|cell| {
                cell.format == RepositoryFormat::Cargo && cell.mode == RepositoryMode::Hosted
            })
            .expect("cargo hosted is in the matrix");
        let fraudulent = ConformanceCell {
            evidence: &["hsp106_proxy_e2e"],
            ..*cargo_hosted
        };
        let content = "//! El repository proxy remoto (HSP-106 / HAR-12), contra el wire.\n\
                       //! El upstream es un servidor local que este fichero controla.\n";
        let failure = accredits(&fraudulent, "hsp106_proxy_e2e", content)
            .expect_err("the proxy suite does not speak of cargo");
        assert!(
            failure.detail.contains("cargo"),
            "the explanation names the format: {failure}"
        );
    }

    /// A cell accredited for a modality the release does not serve: the
    /// declaration says cargo proxy is accredited, and the release does
    /// not serve cargo proxy at all.
    #[test]
    fn a_cell_accredited_outside_the_service_is_refused() {
        let mut matrix = matrix();
        for cell in matrix.iter_mut() {
            if cell.format == RepositoryFormat::Cargo && cell.mode == RepositoryMode::Proxy {
                cell.status = Accreditation::Accredited;
                cell.evidence = &["hsp106_proxy_e2e"];
                cell.note = "";
            }
        }
        let failure = audit(&matrix, RepositoryFormat::declared(), &served_pairs())
            .expect_err("cargo proxy is not served: the accreditation is a fraud");
        assert!(
            failure.to_string().contains("cargo") && failure.to_string().contains("proxy"),
            "the explanation names the cell: {failure}"
        );
    }

    /// The release promises cargo proxy —the served declaration is
    /// doctored— and the matrix still refuses it: no suite accredits the
    /// promise, so the client fails instead of trusting it.
    #[test]
    fn a_modality_promised_without_evidence_is_refused() {
        let mut served = served_pairs();
        served.push((RepositoryFormat::Cargo, RepositoryMode::Proxy));
        let failure = audit(&matrix(), RepositoryFormat::declared(), &served)
            .expect_err("no suite accredits cargo proxy: the promise is a fraud");
        assert!(
            failure.to_string().contains("cargo") && failure.to_string().contains("proxy"),
            "the explanation names the promise: {failure}"
        );
    }

    /// Erasing the suites of an accredited cell is the quiet fraud: the
    /// cell keeps its status and loses its proof.
    #[test]
    fn an_accreditation_without_suites_is_refused() {
        let mut matrix = matrix();
        for cell in matrix.iter_mut() {
            if cell.format == RepositoryFormat::Cargo && cell.mode == RepositoryMode::Hosted {
                cell.evidence = &[];
            }
        }
        let failure = audit(&matrix, RepositoryFormat::declared(), &served_pairs())
            .expect_err("cargo hosted is accredited without a single suite");
        assert!(
            failure.to_string().contains("cargo"),
            "the explanation names the cell: {failure}"
        );
    }

    /// A refused cell that carries accreditation suites is a refusal that
    /// pretends to be served.
    #[test]
    fn a_refusal_with_accreditation_suites_is_refused() {
        let mut matrix = matrix();
        for cell in matrix.iter_mut() {
            if cell.format == RepositoryFormat::Helm && cell.mode == RepositoryMode::Hosted {
                cell.evidence = &["hsp121_helm_oci"];
            }
        }
        let failure = audit(&matrix, RepositoryFormat::declared(), &served_pairs())
            .expect_err("helm hosted is refused and cannot carry accreditation suites");
        assert!(
            failure.to_string().contains("helm"),
            "the explanation names the cell: {failure}"
        );
    }

    /// Evidence that does not name the modality it accredits: the npm
    /// suite speaks of npm, and a proxy accreditation without the word
    /// proxy is an accreditation of nothing.
    #[test]
    fn evidence_that_does_not_name_the_modality_is_refused() {
        let npm_proxy = cell(RepositoryFormat::Npm, RepositoryMode::Proxy);
        let failure = accredits(
            &npm_proxy,
            "hsp111_npm_cli",
            "npm hosted, and the publish of a package: nothing composed anywhere",
        )
        .expect_err("the suite does not name the modality");
        assert!(
            failure.detail.contains("proxy"),
            "the explanation names the modality: {failure}"
        );
    }

    /// A refusal proven in the dialect of the format: the nuget group suite
    /// speaks of nuget and of the group it refuses.
    #[test]
    fn a_refusal_suite_must_speak_of_the_format_and_the_modality() {
        let nuget_group = cell(RepositoryFormat::NuGet, RepositoryMode::Group);
        assert_eq!(nuget_group.refusal_evidence, &["hsp120_nuget_e2e"]);
        accredits(&nuget_group, "hsp120_nuget_e2e", "a nuget group is refused")
            .expect_err("accredits refuses what refuses is for");
        refuses(
            &nuget_group,
            "hsp120_nuget_e2e",
            "a nuget group is refused explicitly",
        )
        .expect("the suite speaks of nuget and of the group it refuses");
        let failure = refuses(
            &nuget_group,
            "hsp120_nuget_e2e",
            "nuget, refusing a composed answer without naming it",
        )
        .expect_err("the suite does not name the modality it refuses");
        assert!(
            failure.detail.contains("group"),
            "the explanation names the modality: {failure}"
        );
    }

    /// A name that cannot be a suite of this workspace is refused before
    /// the client goes looking for the file.
    #[test]
    fn a_name_that_is_not_a_suite_is_refused_by_the_audit() {
        let mut matrix = matrix();
        for cell in matrix.iter_mut() {
            if cell.format == RepositoryFormat::Raw && cell.mode == RepositoryMode::Hosted {
                cell.evidence = &["Suite With Spaces"];
            }
        }
        let failure = audit(&matrix, RepositoryFormat::declared(), &served_pairs())
            .expect_err("a name with spaces and capitals is not a suite stem");
        assert!(
            failure.to_string().contains("Suite With Spaces"),
            "the explanation names the stem: {failure}"
        );
    }

    /// The matrix covers every declared format in every modality once:
    /// removing a cell is a hole the audit sees.
    #[test]
    fn a_matrix_with_a_hole_is_refused() {
        let matrix: Vec<ConformanceCell> = matrix()
            .into_iter()
            .filter(|cell| {
                !(cell.format == RepositoryFormat::Helm && cell.mode == RepositoryMode::Group)
            })
            .collect();
        let failure = audit(&matrix, RepositoryFormat::declared(), &served_pairs())
            .expect_err("helm/group is declared and cannot be missing");
        assert!(
            failure.to_string().contains("helm") && failure.to_string().contains("group"),
            "the explanation names the missing cell: {failure}"
        );
    }
}
