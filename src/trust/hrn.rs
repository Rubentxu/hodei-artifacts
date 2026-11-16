//! El nombre de un artefacto del catalogo para la politica de confianza.
//!
//! La capsula y la evidencia hablan de HRN de artefacto
//! (`artifact/<repositorio>/<algoritmo>-<hex>`); el catalogo habla de
//! repositorio y digest. Este es el unico sitio donde se traduce una cosa
//! en la otra, para que quien promueve y quien evalua la evidencia
//! nombren el mismo sujeto.

use hodei_evidence::ContentDigest;
use kernel::Hrn;

/// El HRN del artefacto que es `digest` dentro de `repository`.
///
/// La region es la del repositorio: un artefacto vive donde vive su
/// repositorio.
pub fn artifact_hrn(region: &str, account: &str, repository: &str, digest: &ContentDigest) -> Hrn {
    Hrn::with_region(
        "hodei".to_string(),
        "artifacts".to_string(),
        region.to_string(),
        account.to_string(),
        "artifact".to_string(),
        format!("{repository}/{}-{}", digest.algorithm, digest.value),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digest() -> ContentDigest {
        ContentDigest::sha256("a".repeat(64))
    }

    #[test]
    fn the_artifact_name_survives_a_round_trip_through_text() {
        let hrn = artifact_hrn("global", "acme", "libs", &digest());
        let parsed = Hrn::from_string(&hrn.to_string()).expect("the name parses back");
        assert_eq!(parsed, hrn);
        assert_eq!(parsed.resource_type(), "artifact");
        assert_eq!(parsed.account_id(), "acme");
        assert_eq!(parsed.region(), "global");
    }

    #[test]
    fn two_digests_or_two_repositories_have_two_names() {
        let other = ContentDigest::sha256("b".repeat(64));
        assert_ne!(
            artifact_hrn("global", "acme", "libs", &digest()),
            artifact_hrn("global", "acme", "libs", &other)
        );
        assert_ne!(
            artifact_hrn("global", "acme", "libs", &digest()),
            artifact_hrn("global", "acme", "apps", &digest())
        );
    }
}
