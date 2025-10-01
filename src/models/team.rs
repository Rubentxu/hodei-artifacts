use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub members: Vec<String>, // User IDs
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Team {
    pub fn new(name: String, description: Option<String>) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            description,
            members: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }
    
    pub fn add_member(&mut self, user_id: String) {
        if !self.members.contains(&user_id) {
            self.members.push(user_id);
            self.updated_at = chrono::Utc::now();
        }
    }
    
    pub fn remove_member(&mut self, user_id: &str) {
        self.members.retain(|id| id != user_id);
        self.updated_at = chrono::Utc::now();
    }
}
