//! El perfil de arranque sobre la autoridad única del grafo (HSP-209).
//!
//! Este módulo es la costura que faltaba entre las piezas ya construidas:
//! cada tienda vertical del grafo (`DgraphCatalogStore`, `DgraphIamAdapter`,
//! `DgraphTrustStore`, `DgraphDecisionStore`, `DgraphProvenanceLedger`,
//! `DgraphJournal`) existe y está probada de forma aislada contra un alpha
//! real. Aquí se ensamblan **todas contra el mismo cliente conectado**, en
//! el orden que las dependencias imponen, y se obtiene el `AppState` del
//! despliegue productivo: Dgraph para el metadato, S3 para el byte, Cedar
//! para la decisión. No hay un segundo almacén de metadato en ningún sitio:
//! el grafo es el único perfil (la validación de la configuración exige
//! `[graph]`, [`crate::config::AppConfig::validate`]) — si el perfil no puede
//! montarse, el arranque muere con el error de la pieza concreta en vez de
//! degradar.
//!
//! La composición es intencionadamente **fallida-antes-que-degradada**: la
//! guarda del esquema versionado (`ensure_ready`, v7) es la primera puerta
//! y la ejecuta el adapter IAM —ninguna tienda existe sobre un servidor que
//! el binario no entiende—, y cada apertura posterior propaga su error
//! tipado. Un alpha inalcanzable, un esquema incompatible o un bucket
//! declarado inalcanzable producen el mismo resultado: el servicio no
//! arranca.
//!
//! Superficies deliberadamente **no cableadas** en este perfil (bloqueadores
//! registrados en `target/uat/hsp209/evidencia.md`, no degradaciones):
//!
//! - Persistencia del esquema Cedar: la implementa `DgraphSchemaStore`
//!   (HSP-314 P2), un tenant del mismo grafo. El esquema del bundle y el
//!   del registro IAM sobreviven al reinicio y las políticas se validan
//!   contra él.
//! - Registro `/repositories` (superficie `RepositoryReader/Writer`):
//!   sin implementación sobre Dgraph (registro en memoria). El
//!   repositorio del catálogo (`/catalog/repositories`) sí está cableado,
//!   que es el que usan las modalidades verticales.
//! - Evidencia, cápsulas e investigación sobre el grafo: los almacenes
//!   existen (`hodei-evidence-storage`, `src/bundles/capsule_dgraph.rs`)
//!   pero los wrappers de lectura del servicio están genéricos sobre el
//!   motor embebido o sin adaptador de handler; el perfil responde con el
//!   comportamiento por defecto (503/404 honestos).
//! - Cuotas durables: `DgraphQuotaStore` existe (HSP-198) sin consumidor en
//!   producción; el perfil usa el ledger de memoria que usa hoy el
//!   despliegue embebido — mismo comportamiento, cero degradación de
//!   autoridad (los contadores no son metadato de contenido).

use std::sync::Arc;

use hodei_catalog::AllowAllAuthorizer;
use hodei_graph_client::{GraphClient, Tenant};
use hodei_iam::infrastructure::DgraphSchemaStore;
use hodei_iam::infrastructure::dgraph::DgraphIamAdapter;
use hodei_trust_policy_storage::{DgraphDecisionStore, DgraphTrustStore};

use crate::app_state::AppState;
use crate::audit::TracingAuditSink;
use crate::blob_probe::BlobStoreProbe;
use crate::bootstrap::BootstrapError;
use crate::composition_root::CompositionRoot;
use crate::config::AppConfig;
use crate::oci::session_journal::DgraphJournal;
use crate::proxy::dgraph_ledger::DgraphProvenanceLedger;
use crate::trust::cache::CachedTrustLedger;

/// El grafo del perfil: un solo cliente conectado, autenticado, con la
/// guarda del esquema versionado ya pasada, compartido por todas las
/// tiendas. Dos clientes serían dos sesiones contra el mismo alpha; el
/// perfil habla con uno.
pub struct GraphAuthority {
    /// The shared connected client every store of the profile is opened on.
    pub client: Arc<GraphClient>,
    /// The tenant name the durable writers of this profile key under.
    pub tenant: Tenant,
    /// The IAM adapter: the six policy ports of the composition root.
    pub iam: Arc<DgraphIamAdapter>,
}

impl GraphAuthority {
    /// Abre la autoridad del perfil: conexión real (TLS con el CA y el
    /// server name declarados), login ACL con las credenciales declaradas,
    /// y —dentro del open del adapter IAM— la guarda del esquema v7. Cada
    /// paso es una puerta: ninguno degrada al siguiente ni al motor
    /// embebido.
    pub async fn connect(config: &AppConfig) -> Result<Self, BootstrapError> {
        let graph_config = config
            .graph
            .as_ref()
            .ok_or_else(|| {
                BootstrapError::Initialization(
                    "the graph profile requires a [graph] section".to_string(),
                )
            })?
            .clone();
        let endpoint = graph_config.endpoint.clone();
        let mut client = GraphClient::connect(graph_config.clone())
            .await
            .map_err(|error| {
                BootstrapError::DatabaseConnection(format!(
                    "cannot reach the Dgraph alpha at {endpoint}: {error}"
                ))
            })?;
        client.login().await.map_err(|error| {
            BootstrapError::DatabaseConnection(format!(
                "the Dgraph alpha at {endpoint} refused the login: {error}"
            ))
        })?;
        // La puerta del esquema vive en el open del adapter: ensure_ready
        // (v7) decide si este binario puede escribir en ese servidor. Un
        // alpha compatible pero esquema viejísimo o nuevo de más devuelve
        // el error tipado y el arranque muere ahí, sin tocar nada local.
        let iam = Arc::new(
            DgraphIamAdapter::open(Arc::new(client))
                .await
                .map_err(|error| {
                    BootstrapError::Initialization(format!(
                        "the graph at {endpoint} is not ready for this binary: {error}"
                    ))
                })?,
        );
        let client = iam.client().clone();
        // El tenant durables del perfil es el namespace del login: la
        // convención de quien escribe, no una segunda autoridad. (El
        // namespace de ACL es numérico por diseño; los tenants de llave del
        // grafo son texto y aquí se nombra por ese número.)
        let namespace = graph_config.namespace.to_string();
        let tenant = Tenant::new(&namespace).map_err(|error| {
            BootstrapError::Initialization(format!(
                "the graph namespace names an invalid tenant: {error}"
            ))
        })?;
        Ok(Self {
            client,
            tenant,
            iam,
        })
    }
}

/// Ensambla el `AppState` del perfil productivo sobre la autoridad única.
///
/// El orden no es decorativo:
///
/// 1. `GraphAuthority::connect` — un solo cliente, un solo login, la guarda
///    v7 ya pasada (lo hace el bootstrap antes de llamar aquí).
/// 2. El almacén de contenido llega abierto desde el bootstrap: el CAS de
///    bytes se abre antes que el catálogo porque la sonda de integridad que
///    el catálogo recibe depende de él.
/// 3. `DgraphCatalogStore::open` + `with_probe`: repositorios del catálogo,
///    assets, versiones, etiquetas, búsqueda, ledger de contenido y libro
///    de la GC coordinada, todo sobre el grafo.
/// 4. `DgraphTrustStore` (con la caché TTL del perfil embebido encima) y
///    `DgraphDecisionStore`: el plano de promoción con las decisiones sobre
///    la misma autoridad.
/// 5. `DgraphProvenanceLedger` + `ProxyPlane`: el libro de procedencia del
///    proxy remoto sobre el grafo.
/// 6. `DgraphJournal` + `UploadRegistry::with_journal`: sesiones OCI
///    reanudables cuya autoridad es el grafo, no la memoria del proceso.
///
/// Cualquier fallo es terminal: no hay una rama que "siga sin el grafo".
pub async fn assemble_graph_profile(
    state: AppState,
    config: &AppConfig,
    graph: &GraphAuthority,
    content: Arc<dyn hodei_hub::blob::BlobStore>,
) -> Result<AppState, BootstrapError> {
    let client = graph.client.clone();
    let endpoint = config
        .graph
        .as_ref()
        .map(|g| g.endpoint.clone())
        .unwrap_or_default();

    // 3a. El libro de confianza: las concesiones viven en la misma autoridad
    //    que el catálogo que las menciona. Se abre antes del catálogo porque
    //    la búsqueda lo lee. Encima, la misma caché TTL de lectura que usa
    //    el perfil embebido.
    let trust: Arc<dyn hodei_catalog::TrustLedger> = Arc::new(
        DgraphTrustStore::open(client.clone(), graph.tenant.clone())
            .await
            .map_err(|error| {
                BootstrapError::Initialization(format!(
                    "the trust ledger cannot open on the graph at {endpoint}: {error}"
                ))
            })?,
    );
    let trust_ledger: Arc<dyn hodei_catalog::TrustLedger> =
        Arc::new(CachedTrustLedger::with_default_ttl(trust));

    // 3. El catálogo sobre el grafo. El autorizador es el de paso: la
    //    decisión real la toma el borde con Cedar, igual que en el perfil
    //    embebido; la sonda de integridad viene del CAS abierto antes.
    let catalog = Arc::new(
        hodei_catalog_storage::DgraphCatalogStore::open(
            client.clone(),
            Arc::new(AllowAllAuthorizer),
        )
        .await
        .map_err(|error| {
            BootstrapError::Initialization(format!(
                "the catalog cannot open on the graph at {endpoint}: {error}"
            ))
        })?
        .with_probe(Arc::new(BlobStoreProbe::new(content.clone())))
        // The search serves the trusted channel and the trust state of every
        // result from the same ledger the download path reads.
        .with_trust_ledger(trust_ledger.clone()),
    );
    let reader: Arc<dyn hodei_catalog::CatalogReader> = catalog.clone();
    let writer: Arc<dyn hodei_catalog::CatalogWriter> = catalog.clone();
    let searcher: Arc<dyn hodei_catalog::CatalogSearch> = catalog.clone();
    let ledger: Arc<dyn hodei_catalog::ContentLedger> = catalog.clone();

    // Las piezas del plano de promoción: el libro de decisiones sobre el
    // grafo y la política que el despliegue declara. Igual que en el
    // embebido: una política declarada e ilegible detiene el arranque; no
    // declarada deja toda promoción rechazada con decisión registrada.
    let policies = crate::trust::promotion::registry_from(
        config.trust.policy_file.as_deref(),
        &config.trust.policy_id,
    )
    .map_err(BootstrapError::Initialization)?;
    let decisions = Arc::new(
        DgraphDecisionStore::open(client.clone())
            .await
            .map_err(|error| {
                BootstrapError::Initialization(format!(
                    "the trust decisions cannot open on the graph at {endpoint}: {error}"
                ))
            })?,
    );
    let plane = Arc::new(crate::trust::promotion::TrustPlane::new(
        decisions,
        policies,
        config.trust.policy_id.clone(),
        chrono::Duration::seconds(i64::from(
            u32::try_from(config.trust.grant_max_validity_secs.max(1)).unwrap_or(u32::MAX),
        )),
    ));

    // 5. La procedencia del proxy remoto: el libro durable sobre el grafo.
    let provenance_ledger: Arc<dyn hodei_catalog::ProvenanceLedger> = Arc::new(
        DgraphProvenanceLedger::open(client.clone())
            .await
            .map_err(|error| {
                BootstrapError::Initialization(format!(
                    "the provenance ledger cannot open on the graph at {endpoint}: {error}"
                ))
            })?,
    );
    // El plano del proxy: el mismo cable que el perfil embebido, con el
    // libro del grafo.
    let egress = crate::proxy::egress::EgressPolicy::new(config.proxy.allowed_hosts.clone())
        .map_err(|error| BootstrapError::Initialization(error.to_string()))?;
    for name in &config.proxy.allowed_credentials {
        hodei_catalog::CredentialRef::parse(&format!("env:{name}"))
            .map_err(|error| BootstrapError::Initialization(error.to_string()))?;
    }
    let fetcher = crate::proxy::fetch::ProxyFetcher::new(
        egress,
        crate::proxy::fetch::FetchLimits {
            connect_timeout: std::time::Duration::from_secs(config.proxy.connect_timeout_secs),
            total_timeout: std::time::Duration::from_secs(config.proxy.total_timeout_secs),
            max_bytes: config.proxy.max_bytes,
        },
        config.proxy.max_concurrent,
    )
    .with_allowed_credentials(config.proxy.allowed_credentials.iter().cloned())
    .with_offline(config.proxy.offline);
    let proxy_plane = Arc::new(crate::proxy::ProxyPlane::new(fetcher, provenance_ledger));

    // GC y scrub: leen las raíces del mismo catálogo del grafo y reclaman
    // del mismo CAS; es lo único que borra bytes.
    let scrub = crate::recovery::Scrubber::new(
        ledger.clone(),
        content.clone(),
        std::time::Duration::from_secs(config.content.gc_grace_secs),
    );
    let gc = crate::gc::GcService::new(
        ledger,
        content.clone(),
        std::time::Duration::from_secs(config.content.gc_grace_secs),
    );

    // 6. Las sesiones OCI reanudables: el journal durable sobre el grafo
    //    en lugar del registro de memoria.
    let journal = Arc::new(
        DgraphJournal::open(client.clone(), graph.tenant.as_str())
            .await
            .map_err(|error| {
                BootstrapError::Initialization(format!(
                    "the OCI session journal cannot open on the graph at {endpoint}: {error}"
                ))
            })?,
    );
    let uploads = crate::oci::uploads::UploadRegistry::with_journal(
        std::time::Duration::from_secs(config.security.oci_upload_ttl_secs),
        config.security.oci_max_concurrent_uploads,
        journal,
    );

    // La auditoría del perfil: el mismo sink de diagnóstico (tracing) y el
    // outbox durable sobre el grafo (el libro por el que HSP-190/220
    // dreinan). El dispatcher del outbox se deja cableado en el perfil
    // embebido como en él: la superficie HTTP registra, el outbox guarda,
    // el drenaje lo prueba su IT propia.
    let outbox = Arc::new(hodei_catalog_storage::DgraphAuditOutbox::new(client));
    // `outbox` queda disponible para el drenaje; el sink del servicio sigue
    // siendo el de trazas mientras no exista un consumidor HTTP del
    // dispatcher en producción (mismo estado que el perfil embebido).
    let _ = &outbox;

    let state = state
        .with_catalog(reader, writer)
        .with_catalog_search(searcher)
        .with_apt_snapshot_store(catalog.clone())
        .with_trust_ledger(trust_ledger)
        .with_proxy_plane(proxy_plane)
        .with_trust_plane(plane)
        .with_content_store(content)
        .with_uploads(Arc::new(uploads));
    // GC y scrub abren o avisan, igual que en el perfil embebido: una
    // superficie de borrado que no puede comprobar qué está vivo es una
    // superficie que debe permanecer cerrada, no una que borre a ciegas.
    let state = match scrub {
        Ok(scrub) => state.with_scrub(Arc::new(scrub)),
        Err(error) => {
            tracing::warn!(%error, "the integrity scrub is not available");
            state
        }
    };
    let state = match gc {
        Ok(gc) => state.with_gc(Arc::new(gc)),
        Err(error) => {
            tracing::warn!(%error, "the garbage collection is not available");
            state
        }
    };
    let state = state.with_audit_sink(Arc::new(TracingAuditSink));

    tracing::info!(
        endpoint = %endpoint,
        tenant = %graph.tenant.as_str(),
        "the graph profile assembled: catalog, trust, decisions, provenance, journal, GC and the CAS of bytes"
    );
    Ok(state)
}

/// El grafo del perfil también porta las políticas IAM: la raíz de
/// composición entra con el adapter del grafo como proveedor de los seis
/// puertos de política y con el almacén de esquema Cedar sobre Dgraph.
pub fn composition_root_for_graph(
    iam: Arc<DgraphIamAdapter>,
    schema_store: Arc<DgraphSchemaStore>,
    authoritative_schema_id: Option<String>,
) -> CompositionRoot {
    CompositionRoot::production_with_authoritative_schema(
        schema_store,
        iam,
        authoritative_schema_id,
    )
}

/// Rechazo explícito de un `[database]` que no declara el grafo: el único
/// backend es Dgraph, y cualquier otro valor (el motor embebido retirado
/// incluido) se rechaza nombrándolo.
///
/// Existe como constante nombrada porque las pruebas de rechazo (el arranque
/// real del binario con `HODEI_DATABASE__DB_TYPE=rocksdb`) necesitan citar el
/// mensaje exacto con el que el perfil se niega; el camino lo recorre
/// `AppConfig::validate`, y esta lo enuncia.
pub const REFUSAL_MESSAGE: &str = crate::config::REFUSED_DATABASE_TYPE;

#[cfg(test)]
mod tests {
    use crate::config::DatabaseConfig;

    use super::REFUSAL_MESSAGE;

    #[test]
    fn the_refusal_names_the_backend_it_refuses() {
        // El mensaje por el que la prueba de rechazo busca es el mismo que
        // enuncia la validación: si uno cambia sin el otro, el arranque
        // negaría con un texto que nadie espera.
        let database = DatabaseConfig {
            db_type: "rocksdb".to_string(),
        };
        let error = database
            .validate()
            .expect_err("the graph profile refuses a backend that is not the graph");
        let text = error.to_string();
        assert!(text.contains(REFUSAL_MESSAGE), "{text}");
        assert!(text.contains("rocksdb"), "the value is named: {text}");
    }

    #[test]
    fn a_database_declaring_the_graph_validates() {
        DatabaseConfig {
            db_type: "dgraph".to_string(),
        }
        .validate()
        .expect("the graph is the supported backend");
    }
}
