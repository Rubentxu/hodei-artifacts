//! El plano de la promocion: quien decide y que se concede.
//!
//! Promover un digest al canal trusted es una decision en dos mitades que
//! no se confunden: Cedar dice **quien** puede promover, y la politica de
//! confianza dice si el **software** lo merece. Esta pieza compone el
//! servicio de promocion con su almacen duradero y con la politica
//! activa, y calcula **que digests** abarca una promocion.
//!
//! # Que abarca una promocion
//!
//! Promover un manifiesto OCI sin su configuracion y sus capas dejaria una
//! imagen que se anuncia y no se puede bajar. Por eso la concesion abarca
//! el digest pedido **y** los digests que ese manifiesto nombra; un
//! indice, ademas, los manifiestos hijos que el propio repositorio tiene,
//! con una profundidad y un numero de nodos acotados. Un manifiesto que
//! no se puede leer concede solo su propio digest: una cadena a medias no
//! se amplia por conjetura.

use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use hodei_evidence::ContentDigest;
use hodei_hub::{BlobStore, ReadPolicy};
use hodei_trust_policy::{
    DecisionStore, DecisionStoreError, PolicyRegistry, PromotionService, PromotionTarget,
    PromotionTargets, TrustPolicy,
};

/// La distribucion a la que se promueve: el canal trusted.
pub const TRUSTED_DISTRIBUTION: &str = "trusted";

/// Hasta que profundidad se sigue un indice de manifiestos.
const MAX_DEPTH: usize = 4;
/// Cuantos manifiestos se leen como maximo en una promocion.
const MAX_MANIFESTS: usize = 256;

/// El tope de digests que abarca una promocion: un manifiesto con muchos
/// descriptores no multiplica las consultas al catalogo. Pasado el tope se
/// concede menos, nunca mas.
const MAX_COVERED: usize = 4096;

/// El servicio de promocion y la politica con la que decide.
pub struct TrustPlane {
    service: PromotionService,
    policy_id: String,
}

impl TrustPlane {
    /// Un plano sobre un almacen de decisiones y un registro de politicas.
    pub fn new(
        store: Arc<dyn DecisionStore>,
        registry: PolicyRegistry,
        policy_id: impl Into<String>,
    ) -> Self {
        Self {
            service: PromotionService::new(
                store,
                Arc::new(TrustedChannelTargets),
                Arc::new(registry),
            ),
            policy_id: policy_id.into(),
        }
    }

    /// El servicio que decide y escribe la decision.
    pub fn service(&self) -> &PromotionService {
        &self.service
    }

    /// El identificador de la politica con la que se decide.
    pub fn policy_id(&self) -> &str {
        &self.policy_id
    }
}

/// El destino de toda promocion: el canal trusted de la cuenta.
pub struct TrustedChannelTargets;

#[async_trait]
impl PromotionTargets for TrustedChannelTargets {
    async fn target_of(
        &self,
        account: &str,
        _subject: &str,
    ) -> Result<Option<PromotionTarget>, DecisionStoreError> {
        Ok(Some(PromotionTarget {
            distribution: TRUSTED_DISTRIBUTION.to_string(),
            account: account.to_string(),
        }))
    }
}

/// Un registro con la politica de un fichero JSON activada, o vacio.
///
/// Un registro vacio no es un registro que permite: toda promocion queda
/// rechazada con una decision `PolicyUnavailable` escrita. Un fichero que
/// se declara y no se puede leer o entender **es un error de arranque**,
/// no una politica ausente a la que nadie ha prestado atencion.
pub fn registry_from(policy_file: Option<&str>, policy_id: &str) -> Result<PolicyRegistry, String> {
    let registry = PolicyRegistry::empty();
    let Some(path) = policy_file else {
        return Ok(registry);
    };
    let text = std::fs::read_to_string(Path::new(path))
        .map_err(|error| format!("the trust policy {path:?} cannot be read: {error}"))?;
    let mut policy: TrustPolicy = serde_json::from_str(&text)
        .map_err(|error| format!("the trust policy {path:?} is not a valid policy: {error}"))?;
    policy.id = policy_id.to_string();
    registry.activate(
        policy,
        "bootstrap",
        "loaded from the trust policy file at the boot",
        chrono::Utc::now(),
    );
    Ok(registry)
}

/// Lo que se ha leido de un manifiesto, sin interpretar.
enum Read {
    /// Los digests que nombra.
    Edges(Vec<ContentDigest>),
    /// No se pudo leer o entender.
    Opaque,
}

async fn read_edges(content: &dyn BlobStore, digest: &ContentDigest) -> Read {
    match content.get_with(digest, ReadPolicy::Verify).await {
        Ok(bytes) => match crate::gc::plan::manifest_edges(&bytes) {
            Ok(edges) => Read::Edges(edges),
            Err(_) => Read::Opaque,
        },
        Err(_) => Read::Opaque,
    }
}

/// Los digests que abarca promover `digest`, que vive en `asset_path`.
///
/// `is_manifest_of` dice si un digest es un manifiesto **de este
/// repositorio** (tiene su asset `manifests/<hex>` y apunta a ese digest);
/// es lo unico que se sigue de un indice. El primero de la lista es
/// siempre el digest pedido.
pub async fn digests_covered<F, Fut>(
    content: &dyn BlobStore,
    digest: &ContentDigest,
    asset_path: &str,
    is_manifest_of: F,
) -> Vec<ContentDigest>
where
    F: Fn(ContentDigest) -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let mut covered = vec![digest.clone()];
    if !asset_path.starts_with("manifests/") {
        return covered;
    }
    let mut seen: HashSet<ContentDigest> = HashSet::from([digest.clone()]);
    let mut queue: VecDeque<(ContentDigest, usize)> = VecDeque::from([(digest.clone(), 0)]);
    let mut read = 0usize;
    while let Some((current, depth)) = queue.pop_front() {
        if read >= MAX_MANIFESTS {
            break;
        }
        read += 1;
        let Read::Edges(edges) = read_edges(content, &current).await else {
            continue;
        };
        for edge in edges {
            if covered.len() >= MAX_COVERED {
                return covered;
            }
            if !seen.insert(edge.clone()) {
                continue;
            }
            covered.push(edge.clone());
            if depth + 1 < MAX_DEPTH && is_manifest_of(edge.clone()).await {
                queue.push_back((edge, depth + 1));
            }
        }
    }
    covered
}

#[cfg(test)]
mod tests {
    use super::*;
    use hodei_hub::FileBlobStore;

    async fn put(store: &FileBlobStore, bytes: &[u8]) -> ContentDigest {
        store.put(bytes).await.expect("the content is written")
    }

    fn manifest(config: &ContentDigest, layers: &[&ContentDigest]) -> Vec<u8> {
        let layers: Vec<_> = layers
            .iter()
            .map(|layer| serde_json::json!({ "digest": format!("sha256:{}", layer.value) }))
            .collect();
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 2,
            "config": { "digest": format!("sha256:{}", config.value) },
            "layers": layers,
        }))
        .expect("the manifest serializes")
    }

    #[tokio::test]
    async fn a_blob_covers_only_itself() {
        let dir = tempfile::tempdir().expect("a directory");
        let store = FileBlobStore::new(dir.path()).expect("a store");
        let blob = put(&store, b"layer").await;
        let covered = digests_covered(&store, &blob, &format!("blobs/{}", blob.value), |_| async {
            false
        })
        .await;
        assert_eq!(covered, vec![blob]);
    }

    #[tokio::test]
    async fn a_manifest_covers_its_config_and_its_layers() {
        let dir = tempfile::tempdir().expect("a directory");
        let store = FileBlobStore::new(dir.path()).expect("a store");
        let config = put(&store, b"config").await;
        let layer = put(&store, b"layer").await;
        let image = put(&store, &manifest(&config, &[&layer])).await;
        let covered = digests_covered(
            &store,
            &image,
            &format!("manifests/{}", image.value),
            |_| async { false },
        )
        .await;
        assert_eq!(covered[0], image);
        assert!(covered.contains(&config));
        assert!(covered.contains(&layer));
        assert_eq!(covered.len(), 3);
    }

    #[tokio::test]
    async fn an_unreadable_manifest_covers_only_itself() {
        let dir = tempfile::tempdir().expect("a directory");
        let store = FileBlobStore::new(dir.path()).expect("a store");
        let not_a_manifest = put(&store, b"just bytes").await;
        let covered = digests_covered(
            &store,
            &not_a_manifest,
            &format!("manifests/{}", not_a_manifest.value),
            |_| async { false },
        )
        .await;
        assert_eq!(covered, vec![not_a_manifest]);
    }

    #[tokio::test]
    async fn an_index_covers_the_children_the_repository_has_and_what_they_name() {
        let dir = tempfile::tempdir().expect("a directory");
        let store = FileBlobStore::new(dir.path()).expect("a store");
        let config = put(&store, b"config").await;
        let layer = put(&store, b"layer").await;
        let child = put(&store, &manifest(&config, &[&layer])).await;
        let index_bytes = serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 2,
            "manifests": [{ "digest": format!("sha256:{}", child.value) }],
        }))
        .expect("the index serializes");
        let index = put(&store, &index_bytes).await;
        let known = child.clone();
        let covered = digests_covered(
            &store,
            &index,
            &format!("manifests/{}", index.value),
            move |digest| {
                let known = known.clone();
                async move { digest == known }
            },
        )
        .await;
        for expected in [&index, &child, &config, &layer] {
            assert!(covered.contains(expected), "{expected:?}");
        }
    }

    #[tokio::test]
    async fn a_child_the_repository_does_not_have_is_not_followed() {
        let dir = tempfile::tempdir().expect("a directory");
        let store = FileBlobStore::new(dir.path()).expect("a store");
        let config = put(&store, b"config").await;
        let child = put(&store, &manifest(&config, &[])).await;
        let index_bytes = serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 2,
            "manifests": [{ "digest": format!("sha256:{}", child.value) }],
        }))
        .expect("the index serializes");
        let index = put(&store, &index_bytes).await;
        let covered = digests_covered(
            &store,
            &index,
            &format!("manifests/{}", index.value),
            |_| async { false },
        )
        .await;
        assert!(covered.contains(&child));
        assert!(!covered.contains(&config), "the child was not read");
    }

    #[test]
    fn a_missing_policy_file_is_a_boot_error_and_no_file_is_an_empty_registry() {
        assert!(registry_from(None, "default").is_ok());
        assert!(registry_from(Some("/nonexistent/policy.json"), "default").is_err());
    }
}
