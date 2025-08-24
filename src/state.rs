use std::sync::Arc;

use repository::infrastructure::MongoRepositoryStore;
use search::infrastructure::persistence::MongoSearchIndex;

#[derive(Clone)]
pub struct AppState {
    pub repo_store: Arc<MongoRepositoryStore>,
    pub search_index: Arc<MongoSearchIndex>,
}
