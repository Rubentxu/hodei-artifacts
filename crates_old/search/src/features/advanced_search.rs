use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct AdvancedSearchQuery {
    // Placeholder
    pub q: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdvancedSearchResult {
    // Placeholder
    pub total: u64,
    pub hits: Vec<String>,
}
