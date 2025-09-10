use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQueryExecuted {
    pub query: String,
    pub result_count: usize,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}

impl SearchQueryExecuted {
    pub fn new(
        query: String,
        result_count: usize,
        user_id: Option<String>,
        session_id: Option<String>,
        ip_address: Option<String>,
        user_agent: Option<String>,
    ) -> Self {
        Self {
            query,
            result_count,
            timestamp: chrono::Utc::now(),
            user_id,
            session_id,
            ip_address,
            user_agent,
        }
    }
}

impl fmt::Display for SearchQueryExecuted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SearchQueryExecuted: query='{}', result_count={}",
            self.query, self.result_count
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultClicked {
    pub artifact_id: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
}

impl SearchResultClicked {
    pub fn new(
        artifact_id: String,
        user_id: Option<String>,
        session_id: Option<String>,
        ip_address: Option<String>,
        user_agent: Option<String>,
    ) -> Self {
        Self {
            artifact_id,
            timestamp: chrono::Utc::now(),
            user_id,
            session_id,
            ip_address,
            user_agent,
        }
    }
}

impl fmt::Display for SearchResultClicked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SearchResultClicked: artifact_id='{}'", self.artifact_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchIndexUpdated {
    pub index_name: String,
    pub update_type: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub updated_count: usize,
    pub failed_count: usize,
}

impl SearchIndexUpdated {
    pub fn new(
        index_name: String,
        update_type: String,
        updated_count: usize,
        failed_count: usize,
    ) -> Self {
        Self {
            index_name,
            update_type,
            timestamp: chrono::Utc::now(),
            updated_count,
            failed_count,
        }
    }
}

impl fmt::Display for SearchIndexUpdated {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SearchIndexUpdated: index='{}', type='{}', updated={}, failed={}",
            self.index_name, self.update_type, self.updated_count, self.failed_count
        )
    }
}