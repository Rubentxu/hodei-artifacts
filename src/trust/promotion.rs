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
use chrono::{DateTime, Duration, Utc};
use hodei_evidence::ContentDigest;
use hodei_hub::{BlobStore, ReadPolicy};
use hodei_trust_policy::{
    DecisionStore, DecisionStoreError, PolicyRegistry, PromotionDecision, PromotionService,
    PromotionTarget, PromotionTargets, TrustPolicy,
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
    registry: PolicyRegistry,
    max_validity: Duration,
    policy_id: String,
}

impl TrustPlane {
    /// Un plano sobre un almacen de decisiones y un registro de politicas,
    /// con el techo de vigencia que toda concesion nueva debe respetar.
    pub fn new(
        store: Arc<dyn DecisionStore>,
        registry: PolicyRegistry,
        policy_id: impl Into<String>,
        max_validity: Duration,
    ) -> Self {
        let policy_id = policy_id.into();
        let service = PromotionService::new(
            store,
            Arc::new(TrustedChannelTargets),
            Arc::new(registry.clone()),
        );
        Self {
            service,
            registry,
            max_validity,
            policy_id,
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

    /// La version de la politica que decide **ahora**.
    ///
    /// Es la segunda mitad del compare-and-set de la promocion: la
    /// decision lleva la version con la que se midio, y una concesion
    /// solo se publica si esa version sigue siendo la activa. Sin
    /// politica activa no hay version que oponer: nada se publica.
    pub fn active_policy_version(&self) -> Option<String> {
        self.registry
            .active(&self.policy_id)
            .map(|policy| policy.version)
    }

    /// La vigencia de una concesion emitida por la decision dada: la
    /// primera caducidad exigida, cortada por el techo del plano.
    pub fn validity_of(&self, decision: &PromotionDecision, now: DateTime<Utc>) -> DateTime<Utc> {
        grant_valid_until(
            &decision.trust,
            &decision.context.exceptions,
            now,
            self.max_validity,
        )
    }
}

/// El momento en que la concesion deja de ser digna de fe.
///
/// Es la **primera caducidad** de lo que la decision exigió: el minimo de
/// las caducidades de las excepciones que la decision aplico de verdad
/// (`requirements` con `exception`), cortado siempre por el techo de
/// revalidacion. La politica y la evidencia no tienen fecha propia en el
/// registro —la politica se cambia por version y la evidencia por el
/// riesgo que la reevalua—, y por eso el techo es su caducidad comun:
/// pasada la hora, la concesion vuelve a pasar por una decision nueva.
///
/// Se calcula sobre la decision **persistida**, junto con su contexto:
/// lo que la concesion promete es lo que aquella dijo, no lo que el
/// cliente pidio.
pub fn grant_valid_until(
    trust: &hodei_trust_policy::TrustDecision,
    exceptions: &[hodei_trust_policy::ExceptionFact],
    now: DateTime<Utc>,
    max_validity: Duration,
) -> DateTime<Utc> {
    let ceiling = now + max_validity;
    let exception_limit = trust
        .requirements
        .iter()
        .filter_map(|requirement| requirement.exception.as_ref())
        .filter_map(|id| {
            exceptions
                .iter()
                .find(|exception| &exception.id == id)
                .map(|exception| exception.expires_at)
        })
        .min();
    match exception_limit {
        Some(until) if until < ceiling => until,
        _ => ceiling,
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

    use hodei_trust_policy::{ExceptionFact, Outcome, RequirementOutcome, TrustDecision};

    fn a_decision(requirements: Vec<RequirementOutcome>) -> TrustDecision {
        TrustDecision {
            outcome: Outcome::Allow,
            reasons: Vec::new(),
            requirements,
            context_version: "1".to_string(),
            policy_id: "default".to_string(),
            policy_version: "1.0.0".to_string(),
            schema_version: "1".to_string(),
            capsule_hash: Some("sha256:cap".to_string()),
            evidence_ids: Vec::new(),
            risk_version: 0,
            decided_at: Utc::now(),
        }
    }

    fn waived_by(exception: &str) -> RequirementOutcome {
        RequirementOutcome {
            requirement: "signed".to_string(),
            satisfied: true,
            reason: None,
            exception: Some(exception.to_string()),
        }
    }

    fn an_exception(id: &str, expires_at: DateTime<Utc>) -> ExceptionFact {
        ExceptionFact {
            id: id.to_string(),
            name: format!("ex-{id}"),
            expires_at,
            applies_to_subject: true,
        }
    }

    /// AC2: la vigencia manda la primera caducidad de las excepciones que
    /// la decision aplico de verdad, por delante del techo.
    #[test]
    fn the_validity_is_the_earliest_expiry_of_the_exceptions_applied() {
        let now = Utc::now();
        let decision = a_decision(vec![waived_by("ex-a"), waived_by("ex-b")]);
        let exceptions = [
            an_exception("ex-b", now + Duration::days(2)),
            an_exception("ex-a", now + Duration::hours(3)),
        ];
        let until = grant_valid_until(&decision, &exceptions, now, Duration::days(30));
        assert_eq!(until, now + Duration::hours(3));
    }

    /// El techo de revalidacion corta por delante de todo: sin excepciones
    /// exigidas —o con excepciones lejanas— la concesion vive hasta el
    /// techo, y ni un segundo mas.
    #[test]
    fn the_ceiling_bounds_the_validity_when_nothing_expires_before() {
        let now = Utc::now();
        let ceiling = Duration::hours(12);

        let unbounded = grant_valid_until(&a_decision(Vec::new()), &[], now, ceiling);
        assert_eq!(unbounded, now + ceiling);

        let decision = a_decision(vec![waived_by("ex-a")]);
        let far = [an_exception("ex-a", now + Duration::days(90))];
        let bounded = grant_valid_until(&decision, &far, now, ceiling);
        assert_eq!(bounded, now + ceiling);
    }

    /// Una excepcion que la decision no nombro no caduca nada: la vigencia
    /// se calcula con lo exigido, no con lo que habia por ahí.
    #[test]
    fn an_exception_the_decision_did_not_apply_bounds_nothing() {
        let now = Utc::now();
        let decision = a_decision(vec![waived_by("ex-used")]);
        let exceptions = [an_exception("ex-other", now + Duration::minutes(1))];
        let until = grant_valid_until(&decision, &exceptions, now, Duration::hours(6));
        assert_eq!(until, now + Duration::hours(6));
    }

    /// La segunda mitad del compare-and-set de AC3: el plano dice la
    /// version activa **ahora**, no la que conocia al nacer. Si la
    /// politica se troca en medio de una promocion, la comparacion del
    /// handler ve la nueva y la decision medida con la vieja no se
    /// publica.
    #[test]
    fn the_active_version_is_the_live_state_of_the_registry() {
        struct NoDecisions;

        #[async_trait]
        impl DecisionStore for NoDecisions {
            async fn put(&self, _decision: &PromotionDecision) -> Result<(), DecisionStoreError> {
                Ok(())
            }
            async fn get(
                &self,
                _id: &hodei_trust_policy::DecisionId,
            ) -> Result<Option<PromotionDecision>, DecisionStoreError> {
                Ok(None)
            }
            async fn list_for_subject(
                &self,
                _account: &str,
                _subject: &str,
            ) -> Result<Vec<PromotionDecision>, DecisionStoreError> {
                Ok(Vec::new())
            }
            async fn find_by_correlation(
                &self,
                _correlation_id: &str,
            ) -> Result<Option<PromotionDecision>, DecisionStoreError> {
                Ok(None)
            }
        }

        let registry = PolicyRegistry::empty();
        registry.activate(
            TrustPolicy::new("default", "1", Vec::new()),
            "operator",
            "first",
            Utc::now(),
        );
        let plane = TrustPlane::new(
            Arc::new(NoDecisions),
            registry.clone(),
            "default",
            Duration::hours(1),
        );
        assert_eq!(plane.active_policy_version().as_deref(), Some("1"));

        registry.activate(
            TrustPolicy::new("default", "2", Vec::new()),
            "operator",
            "swap",
            Utc::now(),
        );
        assert_eq!(
            plane.active_policy_version().as_deref(),
            Some("2"),
            "a decision measured with 1 is stale the moment 2 is active"
        );
    }
}
