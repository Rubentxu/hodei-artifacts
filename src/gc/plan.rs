//! El plan de la recoleccion: el veredicto de cada blob, sin efectos.
//!
//! Es una funcion pura: recibe lo que hay en el almacen, las raices del
//! catalogo, las retenciones, las aristas de los manifiestos y el
//! periodo de gracia, y devuelve el veredicto de cada blob **con su
//! causa**. No lee ni borra nada, y por eso se prueba sin almacen y sin
//! base. Lo que se puede *ver* en un dry-run es exactamente lo que el
//! barrido puede *hacer*: los dos salen de aqui.
//!
//! # Los veredictos
//!
//! - `Retained`: algo lo mantiene vivo. Una raiz del catalogo, un
//!   manifiesto vivo que lo nombra, o una retencion vigente.
//! - `InGrace`: nadie lo referencia, pero es demasiado reciente para
//!   tocarlo. Es la barrera frente a una subida o una promocion que
//!   todavia no llego al catalogo.
//! - `Reclaimable`: huerfano y mas viejo que la gracia.
//!
//! La precedencia es esa: una retencion gana a la gracia, y la gracia
//! gana a la reclamacion.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::Duration;

use chrono::{DateTime, Utc};
use hodei_catalog::{ContentHold, ContentRoot, HoldScope};
use hodei_evidence::{ContentDigest, DigestAlgorithm};
use hodei_hub::StoredBlob;
use serde::Serialize;

/// Las causas que se listan por blob. Un blob compartido por mil
/// repositorios no tiene que producir un informe de mil lineas.
pub const MAX_REASONS: usize = 8;

/// Por que un blob se conserva.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reason {
    /// Una fila del catalogo lo referencia.
    Root {
        /// De que fila: asset, referencia, version o subject.
        root: &'static str,
        /// La cuenta.
        account: String,
        /// El repositorio.
        repository: String,
        /// La ruta o la version.
        subject: String,
    },
    /// Un manifiesto vivo lo nombra: es su configuracion, una capa o un
    /// manifiesto hijo.
    ReferencedBy {
        /// El manifiesto que lo nombra.
        manifest: String,
    },
    /// Una retencion vigente lo alcanza.
    Hold {
        /// La retencion.
        id: String,
        /// Por que se puso.
        reason: String,
    },
}

/// Lo que el plan dice de un blob.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum Verdict {
    /// Se conserva.
    Retained {
        /// Las causas, acotadas a [`MAX_REASONS`].
        reasons: Vec<Reason>,
        /// Cuantas causas mas hay y no se listan.
        omitted: usize,
    },
    /// Huerfano, pero dentro del periodo de gracia.
    InGrace {
        /// Cuanto lleva en el almacen, en segundos.
        age_secs: u64,
        /// Cuanto falta para que sea reclamable, en segundos.
        remaining_secs: u64,
    },
    /// Huerfano y fuera de la gracia.
    Reclaimable {
        /// Cuanto lleva en el almacen, en segundos.
        age_secs: u64,
    },
}

/// El veredicto de un blob.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    /// El contenido, como `sha256:<hex>`.
    pub digest: String,
    /// Los bytes que ocupa.
    pub length: u64,
    /// Lo que el plan dice.
    #[serde(flatten)]
    pub verdict: Verdict,
}

/// El resumen de un plan.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    /// Cuantos blobs hay en el almacen.
    pub total: u64,
    /// Cuantos se conservan.
    pub retained: u64,
    /// Cuantos estan en gracia.
    pub in_grace: u64,
    /// Cuantos son reclamables.
    pub reclaimable: u64,
    /// Los bytes que se liberarian.
    pub reclaimable_bytes: u64,
}

/// El plan completo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Plan {
    /// El periodo de gracia con el que se calculo, en segundos.
    pub grace_secs: u64,
    /// Los totales.
    pub summary: Summary,
    /// El veredicto de cada blob, ordenado por digest.
    pub entries: Vec<Entry>,
}

impl Plan {
    /// Los blobs reclamables, con su longitud.
    pub fn reclaimable(&self) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .filter(|entry| matches!(entry.verdict, Verdict::Reclaimable { .. }))
    }
}

/// Lo que el plan necesita saber.
pub struct Input<'a> {
    /// Los blobs del almacen.
    pub blobs: &'a [StoredBlob],
    /// Las raices del catalogo.
    pub roots: &'a [ContentRoot],
    /// Las retenciones, vigentes o no.
    pub holds: &'a [ContentHold],
    /// Las aristas de los manifiestos: de un manifiesto a lo que nombra.
    pub edges: &'a HashMap<ContentDigest, Vec<ContentDigest>>,
    /// El periodo de gracia.
    pub grace: Duration,
    /// El instante contra el que caducan las retenciones.
    pub now: DateTime<Utc>,
}

/// Calcula el plan.
pub fn plan(input: &Input<'_>) -> Plan {
    // Quien es dueno de cada contenido vivo: los pares cuenta/repositorio
    // de las raices que lo alcanzan, directa o transitivamente. Una
    // retencion de repositorio o de cuenta se resuelve contra esto.
    let mut owners: HashMap<&ContentDigest, BTreeSet<(&str, &str)>> = HashMap::new();
    let mut direct: HashMap<&ContentDigest, Vec<&ContentRoot>> = HashMap::new();
    let mut work: Vec<&ContentDigest> = Vec::new();
    for root in input.roots {
        direct.entry(&root.digest).or_default().push(root);
        owners
            .entry(&root.digest)
            .or_default()
            .insert((root.account.as_str(), root.repository.as_str()));
        work.push(&root.digest);
    }

    // La alcanzabilidad: un manifiesto vivo mantiene vivo lo que nombra,
    // y lo hace con los mismos duenos. Se itera hasta que no cambia nada:
    // un digest se visita otra vez solo si gana un dueno.
    let mut parents: HashMap<&ContentDigest, Vec<&ContentDigest>> = HashMap::new();
    while let Some(parent) = work.pop() {
        let Some(children) = input.edges.get(parent) else {
            continue;
        };
        let parent_owners = owners.get(parent).cloned().unwrap_or_default();
        for child in children {
            let named_by = parents.entry(child).or_default();
            if !named_by.contains(&parent) {
                named_by.push(parent);
            }
            let slot = owners.entry(child).or_default();
            let before = slot.len();
            slot.extend(parent_owners.iter().copied());
            if slot.len() != before || before == 0 {
                work.push(child);
            }
        }
    }

    let active: Vec<&ContentHold> = input
        .holds
        .iter()
        .filter(|hold| hold.is_active(input.now))
        .collect();

    let mut entries: Vec<Entry> = input
        .blobs
        .iter()
        .map(|blob| {
            let mut reasons: Vec<Reason> = Vec::new();

            if let Some(roots) = direct.get(&blob.digest) {
                reasons.extend(roots.iter().map(|root| Reason::Root {
                    root: root.kind.as_str(),
                    account: root.account.clone(),
                    repository: root.repository.clone(),
                    subject: root.subject.clone(),
                }));
            }
            if let Some(manifests) = parents.get(&blob.digest) {
                let mut named: Vec<String> = manifests.iter().map(|m| m.to_string()).collect();
                named.sort();
                reasons.extend(
                    named
                        .into_iter()
                        .map(|manifest| Reason::ReferencedBy { manifest }),
                );
            }
            let blob_owners = owners.get(&blob.digest);
            for hold in &active {
                let reaches = match &hold.scope {
                    HoldScope::Digest { digest } => digest == &blob.digest,
                    HoldScope::Repository {
                        account,
                        repository,
                    } => blob_owners.is_some_and(|set| {
                        set.iter().any(|(a, r)| a == account && r == repository)
                    }),
                    HoldScope::Account { account } => {
                        blob_owners.is_some_and(|set| set.iter().any(|(a, _)| a == account))
                    }
                };
                if reaches {
                    reasons.push(Reason::Hold {
                        id: hold.id.clone(),
                        reason: hold.reason.clone(),
                    });
                }
            }

            let verdict = if !reasons.is_empty() {
                let omitted = reasons.len().saturating_sub(MAX_REASONS);
                reasons.truncate(MAX_REASONS);
                Verdict::Retained { reasons, omitted }
            } else if blob.age < input.grace {
                Verdict::InGrace {
                    age_secs: blob.age.as_secs(),
                    remaining_secs: (input.grace - blob.age).as_secs().max(1),
                }
            } else {
                Verdict::Reclaimable {
                    age_secs: blob.age.as_secs(),
                }
            };
            Entry {
                digest: blob.digest.to_string(),
                length: blob.length,
                verdict,
            }
        })
        .collect();
    entries.sort_by(|a, b| a.digest.cmp(&b.digest));

    let mut summary = Summary {
        total: entries.len() as u64,
        ..Summary::default()
    };
    for entry in &entries {
        match entry.verdict {
            Verdict::Retained { .. } => summary.retained += 1,
            Verdict::InGrace { .. } => summary.in_grace += 1,
            Verdict::Reclaimable { .. } => {
                summary.reclaimable += 1;
                summary.reclaimable_bytes += entry.length;
            }
        }
    }

    Plan {
        grace_secs: input.grace.as_secs(),
        summary,
        entries,
    }
}

/// Por que un manifiesto no se pudo leer como grafo.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EdgeError {
    /// No es JSON.
    #[error("the manifest is not valid JSON")]
    NotJson,
    /// No es ni una imagen ni un indice.
    #[error("the manifest is neither an image nor an index")]
    Unrecognized,
    /// Un descriptor no nombra un digest `sha256:<hex>` bien formado.
    #[error("a descriptor does not name a well formed digest")]
    BadDigest,
}

/// Lo que un manifiesto nombra.
///
/// Para una imagen o un artefacto: la configuracion y las capas. Para un
/// indice: los manifiestos hijos. El `subject` **no** es una arista: un
/// referrer no mantiene vivo su sujeto, es el sujeto el que lo hace
/// alcanzable. Es una lectura **estricta**: un manifiesto que no se
/// entiende es un error, porque un grafo a medias dejaria sin proteger
/// lo que ese manifiesto nombra.
pub fn manifest_edges(bytes: &[u8]) -> Result<Vec<ContentDigest>, EdgeError> {
    let manifest: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| EdgeError::NotJson)?;
    let object = manifest.as_object().ok_or(EdgeError::Unrecognized)?;

    let mut edges = Vec::new();
    let mut recognized = false;

    if let Some(children) = object.get("manifests") {
        recognized = true;
        for child in children.as_array().ok_or(EdgeError::Unrecognized)? {
            edges.push(descriptor_digest(child)?);
        }
    }
    if let Some(config) = object.get("config") {
        recognized = true;
        edges.push(descriptor_digest(config)?);
    }
    if let Some(layers) = object.get("layers") {
        recognized = true;
        for layer in layers.as_array().ok_or(EdgeError::Unrecognized)? {
            edges.push(descriptor_digest(layer)?);
        }
    }
    if let Some(blobs) = object.get("blobs") {
        // El manifiesto de artefacto de la version anterior del
        // protocolo nombra su contenido aqui.
        recognized = true;
        for blob in blobs.as_array().ok_or(EdgeError::Unrecognized)? {
            edges.push(descriptor_digest(blob)?);
        }
    }

    if !recognized {
        return Err(EdgeError::Unrecognized);
    }
    let unique: HashSet<ContentDigest> = edges.into_iter().collect();
    let mut edges: Vec<ContentDigest> = unique.into_iter().collect();
    edges.sort_by_key(ToString::to_string);
    Ok(edges)
}

fn descriptor_digest(descriptor: &serde_json::Value) -> Result<ContentDigest, EdgeError> {
    let text = descriptor
        .get("digest")
        .and_then(|digest| digest.as_str())
        .ok_or(EdgeError::BadDigest)?;
    let (algorithm, value) = text.split_once(':').ok_or(EdgeError::BadDigest)?;
    if algorithm != "sha256" {
        return Err(EdgeError::BadDigest);
    }
    let digest = ContentDigest {
        algorithm: DigestAlgorithm::Sha256,
        value: value.to_ascii_lowercase(),
    };
    if digest.is_well_formed() {
        Ok(digest)
    } else {
        Err(EdgeError::BadDigest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hodei_catalog::RootKind;

    const GRACE: Duration = Duration::from_secs(3600);

    fn digest(n: u8) -> ContentDigest {
        ContentDigest::sha256(format!("{n:064x}"))
    }

    fn blob(n: u8, age_secs: u64) -> StoredBlob {
        StoredBlob {
            digest: digest(n),
            length: 10,
            age: Duration::from_secs(age_secs),
        }
    }

    fn root(kind: RootKind, repository: &str, n: u8) -> ContentRoot {
        ContentRoot {
            kind,
            account: "acme".to_string(),
            repository: repository.to_string(),
            subject: "blobs/x".to_string(),
            digest: digest(n),
        }
    }

    fn hold(id: &str, scope: HoldScope, expires: Option<DateTime<Utc>>) -> ContentHold {
        ContentHold {
            id: id.to_string(),
            scope,
            reason: "legal".to_string(),
            actor: "ops".to_string(),
            created_at: Utc::now(),
            expires_at: expires,
        }
    }

    fn run(
        blobs: &[StoredBlob],
        roots: &[ContentRoot],
        holds: &[ContentHold],
        edges: &HashMap<ContentDigest, Vec<ContentDigest>>,
    ) -> Plan {
        plan(&Input {
            blobs,
            roots,
            holds,
            edges,
            grace: GRACE,
            now: Utc::now(),
        })
    }

    fn verdict_of(plan: &Plan, n: u8) -> &Verdict {
        let wanted = digest(n).to_string();
        &plan
            .entries
            .iter()
            .find(|entry| entry.digest == wanted)
            .expect("the entry")
            .verdict
    }

    #[test]
    fn an_old_orphan_is_reclaimable_and_a_young_one_is_in_grace() {
        let plan = run(&[blob(1, 7200), blob(2, 60)], &[], &[], &HashMap::new());
        assert!(matches!(verdict_of(&plan, 1), Verdict::Reclaimable { .. }));
        assert!(matches!(
            verdict_of(&plan, 2),
            Verdict::InGrace {
                remaining_secs: 3540,
                ..
            }
        ));
        assert_eq!(plan.summary.reclaimable, 1);
        assert_eq!(plan.summary.reclaimable_bytes, 10);
        assert_eq!(plan.summary.in_grace, 1);
    }

    #[test]
    fn a_root_retains_even_an_old_blob() {
        let roots = [root(RootKind::Asset, "app", 1)];
        let plan = run(&[blob(1, 99_999)], &roots, &[], &HashMap::new());
        match verdict_of(&plan, 1) {
            Verdict::Retained { reasons, .. } => {
                assert!(matches!(reasons[0], Reason::Root { root: "asset", .. }));
            }
            other => panic!("expected retained, got {other:?}"),
        }
    }

    #[test]
    fn a_blob_shared_by_two_repositories_names_both() {
        let roots = [
            root(RootKind::Asset, "app", 1),
            root(RootKind::Asset, "lib", 1),
        ];
        let plan = run(&[blob(1, 99_999)], &roots, &[], &HashMap::new());
        match verdict_of(&plan, 1) {
            Verdict::Retained { reasons, .. } => assert_eq!(reasons.len(), 2),
            other => panic!("expected retained, got {other:?}"),
        }
    }

    #[test]
    fn a_live_manifest_retains_its_config_and_layers() {
        let roots = [root(RootKind::Asset, "app", 1)];
        let mut edges = HashMap::new();
        edges.insert(digest(1), vec![digest(2), digest(3)]);
        let plan = run(
            &[
                blob(1, 99_999),
                blob(2, 99_999),
                blob(3, 99_999),
                blob(4, 99_999),
            ],
            &roots,
            &[],
            &edges,
        );
        for layer in [2, 3] {
            match verdict_of(&plan, layer) {
                Verdict::Retained { reasons, .. } => {
                    assert!(matches!(reasons[0], Reason::ReferencedBy { .. }));
                }
                other => panic!("layer {layer}: expected retained, got {other:?}"),
            }
        }
        assert!(matches!(verdict_of(&plan, 4), Verdict::Reclaimable { .. }));
    }

    #[test]
    fn retention_is_transitive_through_an_index() {
        // indice (1) -> manifiesto (2) -> capa (3)
        let roots = [root(RootKind::Asset, "app", 1)];
        let mut edges = HashMap::new();
        edges.insert(digest(1), vec![digest(2)]);
        edges.insert(digest(2), vec![digest(3)]);
        let plan = run(&[blob(3, 99_999)], &roots, &[], &edges);
        assert!(matches!(verdict_of(&plan, 3), Verdict::Retained { .. }));
    }

    #[test]
    fn a_manifest_that_is_not_alive_does_not_retain_what_it_names() {
        // El manifiesto 1 no es raiz: sus capas no se conservan por el.
        let mut edges = HashMap::new();
        edges.insert(digest(1), vec![digest(2)]);
        let plan = run(&[blob(1, 99_999), blob(2, 99_999)], &[], &[], &edges);
        assert!(matches!(verdict_of(&plan, 2), Verdict::Reclaimable { .. }));
    }

    #[test]
    fn a_cycle_in_the_edges_terminates() {
        let roots = [root(RootKind::Asset, "app", 1)];
        let mut edges = HashMap::new();
        edges.insert(digest(1), vec![digest(2)]);
        edges.insert(digest(2), vec![digest(1)]);
        let plan = run(&[blob(2, 99_999)], &roots, &[], &edges);
        assert!(matches!(verdict_of(&plan, 2), Verdict::Retained { .. }));
    }

    #[test]
    fn a_hold_beats_the_grace_and_the_age() {
        let holds = [hold("h1", HoldScope::Digest { digest: digest(1) }, None)];
        let plan = run(
            &[blob(1, 99_999), blob(2, 60)],
            &[],
            &holds,
            &HashMap::new(),
        );
        match verdict_of(&plan, 1) {
            Verdict::Retained { reasons, .. } => {
                assert!(matches!(&reasons[0], Reason::Hold { id, .. } if id == "h1"));
            }
            other => panic!("expected retained, got {other:?}"),
        }
        assert!(matches!(verdict_of(&plan, 2), Verdict::InGrace { .. }));
    }

    #[test]
    fn an_expired_hold_retains_nothing() {
        let expired = Utc::now() - chrono::Duration::seconds(5);
        let holds = [hold(
            "h1",
            HoldScope::Digest { digest: digest(1) },
            Some(expired),
        )];
        let plan = run(&[blob(1, 99_999)], &[], &holds, &HashMap::new());
        assert!(matches!(verdict_of(&plan, 1), Verdict::Reclaimable { .. }));
    }

    #[test]
    fn a_repository_hold_reaches_what_the_repository_keeps_alive() {
        // El manifiesto 1 esta en `app`; su capa 2 es del mismo repo.
        // La capa 3 es de `lib` y no la alcanza.
        let roots = [
            root(RootKind::Asset, "app", 1),
            root(RootKind::Asset, "lib", 3),
        ];
        let mut edges = HashMap::new();
        edges.insert(digest(1), vec![digest(2)]);
        let holds = [hold(
            "h1",
            HoldScope::Repository {
                account: "acme".to_string(),
                repository: "app".to_string(),
            },
            None,
        )];
        let plan = run(
            &[blob(1, 5), blob(2, 5), blob(3, 5)],
            &roots,
            &holds,
            &edges,
        );
        let held = |n: u8| match verdict_of(&plan, n) {
            Verdict::Retained { reasons, .. } => {
                reasons.iter().any(|r| matches!(r, Reason::Hold { .. }))
            }
            _ => false,
        };
        assert!(held(1));
        assert!(held(2), "the layer is reached through the manifest");
        assert!(!held(3), "another repository is not reached");
    }

    #[test]
    fn an_account_hold_reaches_every_repository_of_the_account() {
        let mut other = root(RootKind::Asset, "app", 2);
        other.account = "globex".to_string();
        let roots = [root(RootKind::Asset, "app", 1), other];
        let holds = [hold(
            "h1",
            HoldScope::Account {
                account: "acme".to_string(),
            },
            None,
        )];
        let plan = run(&[blob(1, 5), blob(2, 5)], &roots, &holds, &HashMap::new());
        let held = |n: u8| match verdict_of(&plan, n) {
            Verdict::Retained { reasons, .. } => {
                reasons.iter().any(|r| matches!(r, Reason::Hold { .. }))
            }
            _ => false,
        };
        assert!(held(1));
        assert!(!held(2));
    }

    #[test]
    fn the_reasons_are_bounded_and_the_rest_is_counted() {
        let roots: Vec<ContentRoot> = (0..20)
            .map(|i| root(RootKind::Asset, &format!("repo-{i}"), 1))
            .collect();
        let plan = run(&[blob(1, 5)], &roots, &[], &HashMap::new());
        match verdict_of(&plan, 1) {
            Verdict::Retained { reasons, omitted } => {
                assert_eq!(reasons.len(), MAX_REASONS);
                assert_eq!(*omitted, 12);
            }
            other => panic!("expected retained, got {other:?}"),
        }
    }

    #[test]
    fn the_plan_is_deterministic_and_sorted() {
        let blobs = [blob(3, 99_999), blob(1, 99_999), blob(2, 99_999)];
        let first = run(&blobs, &[], &[], &HashMap::new());
        let second = run(&blobs, &[], &[], &HashMap::new());
        assert_eq!(first, second);
        let digests: Vec<&String> = first.entries.iter().map(|e| &e.digest).collect();
        let mut sorted = digests.clone();
        sorted.sort();
        assert_eq!(digests, sorted);
    }

    #[test]
    fn the_edges_of_an_image_are_its_config_and_layers() {
        let manifest = format!(
            r#"{{"config":{{"digest":"sha256:{:064x}"}},"layers":[{{"digest":"sha256:{:064x}"}},{{"digest":"sha256:{:064x}"}}]}}"#,
            1, 2, 2,
        );
        let edges = manifest_edges(manifest.as_bytes()).expect("edges");
        assert_eq!(edges, vec![digest(1), digest(2)]);
    }

    #[test]
    fn the_edges_of_an_index_are_its_manifests() {
        let manifest = format!(r#"{{"manifests":[{{"digest":"sha256:{:064x}"}}]}}"#, 7,);
        assert_eq!(
            manifest_edges(manifest.as_bytes()).expect("edges"),
            vec![digest(7)]
        );
    }

    #[test]
    fn the_subject_is_not_an_edge() {
        let manifest = format!(
            r#"{{"config":{{"digest":"sha256:{:064x}"}},"layers":[],"subject":{{"digest":"sha256:{:064x}"}}}}"#,
            1, 9,
        );
        assert_eq!(
            manifest_edges(manifest.as_bytes()).expect("edges"),
            vec![digest(1)]
        );
    }

    #[test]
    fn a_manifest_that_is_not_understood_is_an_error_and_never_an_empty_graph() {
        assert_eq!(manifest_edges(b"not json"), Err(EdgeError::NotJson));
        assert_eq!(manifest_edges(b"{}"), Err(EdgeError::Unrecognized));
        assert_eq!(manifest_edges(b"[]"), Err(EdgeError::Unrecognized));
        assert_eq!(
            manifest_edges(br#"{"layers":[{"digest":"md5:abc"}]}"#),
            Err(EdgeError::BadDigest)
        );
        assert_eq!(
            manifest_edges(br#"{"layers":[{}]}"#),
            Err(EdgeError::BadDigest)
        );
    }
}
