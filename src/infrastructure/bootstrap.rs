use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::sync::Mutex;
use tracing_subscriber::{fmt, EnvFilter};

use crate::state::AppState;
use artifact::infrastructure::MongoArtifactRepository;
use iam::application::api::IamApi;
use iam::infrastructure::cedar_policy_validator::CedarPolicyValidator;
use iam::infrastructure::mongo_policy_repository::MongoPolicyRepository;
use iam::infrastructure::mongo_user_repository::MongoUserRepository;
use infra_mongo::{MongoClientFactory, MongoConfig};
use repository::infrastructure::MongoRepositoryStore;
use search::infrastructure::persistence::MongoSearchIndex;

/// Inicializa tracing, configuración, y construye el estado de la aplicación.
pub async fn bootstrap() -> Result<Arc<Mutex<AppState>>> {
    init_tracing();

    let factory = bootstrap_indexes()
        .await
        .context("bootstrap índices mongo")?;

    let repo_store = Arc::new(MongoRepositoryStore::new(factory.clone()));
    let search_index = Arc::new(
        MongoSearchIndex::new(factory.client().await?)
            .await
            .context("failed to create search index")?,
    );
    let client = factory.client().await?;
    let user_repo = Arc::new(MongoUserRepository::new(
        client.database("iam").collection("users"),
    ));
    let policy_repo = Arc::new(MongoPolicyRepository::new(
        client.database("iam").collection("policies"),
    ));
    let policy_validator = Arc::new(CedarPolicyValidator);

    let iam_api = Arc::new(IamApi::new(
        user_repo.clone(),
        policy_repo.clone(),
        policy_validator.clone(),
    ));

    let app_state = AppState {
        repo_store,
        search_index,
        iam_api,
    };

    Ok(Arc::new(Mutex::new(app_state)))
}

/// Bootstrap de índices Mongo necesarios para garantizar idempotencia y unicidad.
async fn bootstrap_indexes() -> Result<Arc<MongoClientFactory>> {
    let cfg = MongoConfig::from_env().context("cargar MongoConfig desde entorno")?;
    let factory = Arc::new(MongoClientFactory::new(cfg));

    // Artifact indexes
    let artifact_repo = MongoArtifactRepository::new(factory.clone());
    artifact_repo
        .ensure_indexes()
        .await
        .map_err(|e| anyhow::anyhow!("ensure_indexes artifact: {e}"))?;

    // Repository indexes
    let repo_store = MongoRepositoryStore::new(factory.clone());
    repo_store
        .ensure_indexes()
        .await
        .map_err(|e| anyhow::anyhow!("ensure_indexes repository: {e}"))?;

    Ok(factory)
}

/// Inicializa tracing en formato JSON (nivel info por defecto, configurable vía RUST_LOG).
fn init_tracing() {
    let _ = fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .json()
        .try_init();
}
