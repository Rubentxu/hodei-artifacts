use std::sync::Arc;

use repository::application::ports::RepositoryStore;
use search::application::ports::SearchIndex;
use iam::application::api::IamApi;


/// Estado global de la aplicación, compartido entre todos los handlers.
pub struct AppState {
    pub repo_store: Arc<dyn RepositoryStore>, // Repositorio de repositorios
    pub search_index: Arc<dyn SearchIndex>,
    pub iam_api: Arc<IamApi>,
}
