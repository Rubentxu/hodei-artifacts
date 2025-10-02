#[derive(Debug, Clone)]
pub struct ListPoliciesQuery {
    /// Pagination: number of items to skip
    pub offset: Option<usize>,
    /// Pagination: maximum number of items to return
    pub limit: Option<usize>,
    /// Filter: only return policies with IDs containing this string
    pub filter_id: Option<String>,
}

impl ListPoliciesQuery {
    pub fn new() -> Self {
        Self {
            offset: None,
            limit: None,
            filter_id: None,
        }
    }

    pub fn with_pagination(offset: usize, limit: usize) -> Self {
        Self {
            offset: Some(offset),
            limit: Some(limit),
            filter_id: None,
        }
    }

    pub fn with_filter(filter_id: String) -> Self {
        Self {
            offset: None,
            limit: None,
            filter_id: Some(filter_id),
        }
    }

    pub fn validate(&self) -> Result<(), ListPoliciesValidationError> {
        // Validate limit is reasonable
        if let Some(limit) = self.limit {
            if limit == 0 {
                return Err(ListPoliciesValidationError::InvalidLimit(
                    "Limit must be greater than 0".to_string(),
                ));
            }
            if limit > 1000 {
                return Err(ListPoliciesValidationError::InvalidLimit(
                    "Limit cannot exceed 1000".to_string(),
                ));
            }
        }
        Ok(())
    }
}

impl Default for ListPoliciesQuery {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ListPoliciesValidationError {
    #[error("invalid limit: {0}")]
    InvalidLimit(String),
}
