use std::sync::Arc;
use anyhow::{Context, Result};
use tokio::sync::Mutex;
use tracing_subscriber::{fmt, EnvFilter};
use crate::state::AppState;
use artifact::infrastructure::{MongoArtifactRepository, RabbitMqArtifactEventPublisher, S3ArtifactStorage};
use artifact::application::ports::ArtifactEventPublisher;
use iam::application::api::IamApi;
use iam::infrastructure::cedar_policy_validator::CedarPolicyValidator;
use iam::infrastructure::cedar_authorizer::CedarAuthorizer;
use cedar_policy::PolicySet;
use iam::infrastructure::redis_decision_cache::RedisDecisionCache;
use iam::infrastructure::mongo_policy_repository::MongoPolicyRepository;
use iam::infrastructure::mongo_user_repository::MongoUserRepository;
use infra_mongo::{MongoClientFactory, MongoConfig};
use repository::infrastructure::MongoRepositoryStore;
use search::infrastructure::persistence::MongoSearchIndex;
use std::env;

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
    let redis_cache = Arc::new(RedisDecisionCache::new("redis://127.0.0.1:6379").unwrap());
    let authorization = Arc::new(CedarAuthorizer::new(PolicySet::new(), redis_cache.clone()));

    let iam_api = Arc::new(IamApi::new(
        user_repo.clone(),
        policy_repo.clone(),
        policy_validator.clone(),
    ));

    // Instantiate ArtifactRepository and S3ArtifactStorage
    let artifact_repository = Arc::new(MongoArtifactRepository::new(factory.clone()));
    let s3_endpoint = env::var("S3_ENDPOINT").context("S3_ENDPOINT environment variable not set")?;
    let s3_region = env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".to_string());
    let s3_bucket = env::var("S3_BUCKET").context("S3_BUCKET environment variable not set")?;
    // TODO: Implement S3 client creation from environment variables
    let artifact_storage = Arc::new(S3ArtifactStorage::new(
        aws_sdk_s3::Client::new(&aws_config::from_env().load().await),
        s3_bucket,
    ));

    // Configure event broker based on environment variable
    let artifact_event_publisher: Arc<dyn ArtifactEventPublisher> = match env::var("EVENT_BROKER_TYPE").unwrap_or_else(|_| "rabbitmq".to_string()).as_str() {
        "rabbitmq" => {
            let amqp_addr = env::var("AMQP_ADDR")
                .context("AMQP_ADDR environment variable not set for RabbitMQ")?;
            Arc::new(RabbitMqArtifactEventPublisher::new(&amqp_addr, "hodei_artifacts_exchange").await?)
        }
        "kafka" => {
            // Kafka implementation would go here when available
            anyhow::bail!("Kafka event broker is not currently implemented. Use 'rabbitmq' instead.")
        }
        broker_type => {
            anyhow::bail!("Unsupported event broker type: {}. Supported types: rabbitmq", broker_type)
        }
    };

    let app_state = AppState {
        repo_store,
        search_index,
        iam_api,
        authorization,
        artifact_event_publisher,
        artifact_repository,
        artifact_storage,
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
