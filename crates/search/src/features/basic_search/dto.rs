use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    pub page: Option<usize>,
    pub page_size: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactDocument {
    pub id: String,
    pub name: String,
    pub version: String,
    pub package_type: String,
    pub repository: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    pub artifacts: Vec<ArtifactDocument>,
    pub total_count: usize,
    pub page: usize,
    pub page_size: usize,
    pub total_pages: usize,
}

impl SearchResults {
    pub fn new(artifacts: Vec<ArtifactDocument>, total_count: usize, page: usize, page_size: usize) -> Self {
        let total_pages = if page_size > 0 {
            total_count.div_ceil(page_size)
        } else {
            0
        };
        
        Self {
            artifacts,
            total_count,
            page,
            page_size,
            total_pages,
        }
    }
}