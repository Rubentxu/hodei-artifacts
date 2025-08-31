use std::sync::Arc;

use repository::application::ports::RepositoryStore;
use search::application::ports::SearchIndex;
use iam::application::api::IamApi;
use iam::application::ports::Authorization;
use artifact::application::ports::ArtifactEventPublisher;
use artifact::infrastructure::{MongoArtifactRepository, S3ArtifactStorage};


/// Estado global de la aplicación, compartido entre todos los handlers.
pub struct AppState {
    pub repo_store: Arc<dyn RepositoryStore>, // Repositorio de repositorios
    pub search_index: Arc<dyn SearchIndex>,
    pub iam_api: Arc<IamApi>,
    pub authorization: Arc<dyn Authorization>,
    pub artifact_event_publisher: Arc<dyn ArtifactEventPublisher>,
    pub artifact_repository: Arc<MongoArtifactRepository>,
    pub artifact_storage: Arc<S3ArtifactStorage>,
}
