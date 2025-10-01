use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub email: String,
    pub full_name: String,
    pub department: Option<String>,
    pub job_title: Option<String>,
    pub active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl User {
    pub fn new(username: String, email: String, full_name: String) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            username,
            email,
            full_name,
            department: None,
            job_title: None,
            active: true,
            created_at: now,
            updated_at: now,
        }
    }
}
