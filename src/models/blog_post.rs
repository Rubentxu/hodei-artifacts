use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlogPost {
    pub id: String,
    pub title: String,
    pub content: String,
    pub author_id: String,
    pub published: bool,
    pub tags: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub published_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl BlogPost {
    pub fn new(title: String, content: String, author_id: String) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            title,
            content,
            author_id,
            published: false,
            tags: Vec::new(),
            created_at: now,
            updated_at: now,
            published_at: None,
        }
    }
    
    pub fn publish(&mut self) {
        self.published = true;
        self.published_at = Some(chrono::Utc::now());
        self.updated_at = chrono::Utc::now();
    }
    
    pub fn unpublish(&mut self) {
        self.published = false;
        self.published_at = None;
        self.updated_at = chrono::Utc::now();
    }
    
    pub fn add_tag(&mut self, tag: String) {
        if !self.tags.contains(&tag) {
            self.tags.push(tag);
            self.updated_at = chrono::Utc::now();
        }
    }
}
