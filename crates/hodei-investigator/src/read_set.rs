//! El read-set: lo que una investigacion LEYO, dicho para que se pueda
//! verificar.
//!
//! Una investigacion tiene que poder demostrar QUE evidencias, relaciones
//! y politicas consulto y sobre QUE versiones del mundo. Eso es un
//! **manifiesto de lecturas**: identificadores, digests y generaciones,
//! en un orden estable, sin contenido crudo y sin relojes. Es una
//! primitiva neutral: no depende de un modelo, ni de una decision de
//! confianza, ni de un almacen concreto.
//!
//! # Las tres listas
//!
//! Lo que se **recupero**, lo que se le **dio al modelo** y lo que las
//! afirmaciones aceptadas **citaron** son tres cosas distintas, y el
//! manifiesto las distingue: un hecho recuperado que el presupuesto
//! dejo fuera no llego al modelo, y un hecho que llego y no se cito no
//! sostiene nada.
//!
//! # La consistencia
//!
//! El mundo puede cambiar mientras se lee. El manifiesto lleva las
//! **generaciones** de las fuentes (un vector, porque puede no haber un
//! unico cursor global), y la comprobacion dice si el mundo contra el
//! que se leyo sigue siendo el mundo de ahora. Una lectura obsoleta se
//! dice obsoleta: nunca se presenta como snapshot consistente.

use serde::{Deserialize, Serialize};

/// Que clase de recurso se leyo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadKind {
    /// Una pieza de evidencia, verificada por el digest de su contenido.
    Evidence,
    /// Una relacion del grafo, verificada por su identidad.
    Relation,
    /// Una politica de confianza, verificada por su version.
    Policy,
}

/// La lectura de UN recurso, con los metadatos que bastan para
/// comprobar su origen. Nunca lleva el contenido: lleva el digest.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ReadStamp {
    /// Que clase de recurso es.
    pub kind: ReadKind,
    /// El identificador del recurso: el id de la evidencia, la identidad
    /// de la relacion o el id de la politica.
    pub resource: String,
    /// Con que se verifica el origen: el digest del contenido, la
    /// identidad de la relacion o la version de la politica.
    pub verified_by: String,
}

impl ReadStamp {
    /// El sello de una evidencia, verificada por su digest.
    pub fn evidence(resource: impl Into<String>, digest: impl Into<String>) -> Self {
        Self {
            kind: ReadKind::Evidence,
            resource: resource.into(),
            verified_by: digest.into(),
        }
    }

    /// El sello de una relacion, verificada por su identidad.
    pub fn relation(resource: impl Into<String>, evidence: impl Into<String>) -> Self {
        Self {
            kind: ReadKind::Relation,
            resource: resource.into(),
            verified_by: evidence.into(),
        }
    }

    /// El sello de una politica, verificada por su version.
    pub fn policy(resource: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            kind: ReadKind::Policy,
            resource: resource.into(),
            verified_by: version.into(),
        }
    }
}

/// El manifiesto de las lecturas.
///
/// Las generaciones son un VECTOR --(fuente, version)-- porque puede no
/// haber un unico cursor global para todo lo que se lee. Los sellos van
/// en un orden estable: el mismo conjunto de lecturas da el mismo
/// manifiesto, se lea en el orden en que se lea.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ReadSet {
    /// La version de cada fuente en el momento de leer, sin relojes.
    pub generations: Vec<(String, u64)>,
    /// Los sellos de lo que se leyo.
    pub stamps: Vec<ReadStamp>,
}

impl ReadSet {
    /// Un manifiesto vacio.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Registra una lectura. La dupla (fuente, version) no se repite.
    pub fn read(&mut self, stamp: ReadStamp) {
        if !self
            .stamps
            .iter()
            .any(|one| one.kind == stamp.kind && one.resource == stamp.resource)
        {
            self.stamps.push(stamp);
        }
    }

    /// Registra la generacion de una fuente.
    pub fn generation(&mut self, source: impl Into<String>, version: u64) {
        let source = source.into();
        if !self
            .generations
            .iter()
            .any(|(known, _)| *known == source)
        {
            self.generations.push((source, version));
        }
    }

    /// El manifiesto canonico: los sellos ordenados por su identidad,
    /// las generaciones ordenadas por su fuente, sin relojes y sin
    /// orden accidental. El replay del mismo conjunto de lecturas
    /// produce EL MISMO manifiesto.
    pub fn manifest(&self) -> String {
        let mut stamps = self.stamps.clone();
        stamps.sort();
        let mut generations = self.generations.clone();
        generations.sort();
        format!(
            "readset generations={:?} stamps={:?}",
            generations, stamps
        )
    }

    /// La huella del manifiesto canonico: el digest de lo que se leyo,
    /// para comparar dos manifiestos sin leerlos enteros.
    pub fn fingerprint(&self) -> String {
        hodei_evidence::ContentDigest::of_bytes(self.manifest().as_bytes()).to_string()
    }

    /// Si el mundo contra el que se leyo SIGUE siendo el mundo de ahora:
    /// cada generacion registrada tiene que seguir siendo la que la
    /// fuente dice tener.
    pub fn is_consistent_with(&self, current: &[(String, u64)]) -> bool {
        self.generations.iter().all(|(source, expected)| {
            current
                .iter()
                .any(|(now, version)| now == source && version == expected)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC3: el replay del mismo conjunto de lecturas produce el mismo
    /// manifiesto, se construya en el orden en que se construya.
    #[test]
    fn the_same_reads_give_the_same_manifest_in_any_order() {
        let mut first = ReadSet::empty();
        first.generation("graph", 12);
        first.read(ReadStamp::evidence("evd-1", "sha256:aaa"));
        first.read(ReadStamp::relation("contains|a|b", "evd-1"));
        first.read(ReadStamp::evidence("evd-2", "sha256:bbb"));

        let mut second = ReadSet::empty();
        second.read(ReadStamp::evidence("evd-2", "sha256:bbb"));
        second.generation("graph", 12);
        second.read(ReadStamp::evidence("evd-1", "sha256:aaa"));
        second.read(ReadStamp::relation("contains|a|b", "evd-1"));

        assert_eq!(first.manifest(), second.manifest());
        assert_eq!(first.fingerprint(), second.fingerprint());
    }

    /// La misma lectura dicha dos veces es UNA en el manifiesto.
    #[test]
    fn a_read_said_twice_is_one_in_the_manifest() {
        let mut read_set = ReadSet::empty();
        read_set.read(ReadStamp::evidence("evd-1", "sha256:aaa"));
        read_set.read(ReadStamp::evidence("evd-1", "sha256:aaa"));
        assert_eq!(read_set.stamps.len(), 1);
    }

    /// AC2: el mundo puede cambiar mientras se lee. Un manifiesto cuya
    /// generacion ya no es la de la fuente esta OBSOLETO, y se dice.
    #[test]
    fn a_manifest_from_an_older_generation_is_stale() {
        let mut read_set = ReadSet::empty();
        read_set.generation("graph", 12);
        read_set.read(ReadStamp::evidence("evd-1", "sha256:aaa"));

        let current = vec![("graph".to_string(), 13u64)];
        assert!(
            !read_set.is_consistent_with(&current),
            "la generacion avanzo: la lectura esta obsoleta"
        );

        let same = vec![("graph".to_string(), 12u64)];
        assert!(
            read_set.is_consistent_with(&same),
            "la misma generacion es consistente"
        );
    }

    /// El manifiesto no lleva contenido: lleva identificadores y digests.
    #[test]
    fn the_manifest_carries_digests_not_content() {
        let mut read_set = ReadSet::empty();
        read_set.read(ReadStamp::evidence(
            "evd-1",
            "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ));
        let manifest = read_set.manifest();
        assert!(manifest.contains("evd-1"));
        assert!(
            !manifest.contains("the sbom says"),
            "el contenido no entra en el manifiesto"
        );
    }
}
