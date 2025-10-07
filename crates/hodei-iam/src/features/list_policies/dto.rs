//! DTOs for List Policies feature

use kernel::Hrn;
use serde::{Deserialize, Serialize};

/// Query para listar políticas IAM con paginación
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPoliciesQuery {
    /// Número máximo de resultados por página (1-100)
    pub limit: Option<u32>,

    /// Número de elementos a saltar para paginación
    pub offset: Option<u32>,
}

impl ListPoliciesQuery {
    pub fn new() -> Self {
        Self {
            limit: None,
            offset: None,
        }
    }

    pub fn with_limit(limit: u32) -> Self {
        Self {
            limit: Some(limit),
            offset: None,
        }
    }

    pub fn with_pagination(limit: u32, offset: u32) -> Self {
        Self {
            limit: Some(limit),
            offset: Some(offset),
        }
    }

    pub fn effective_limit(&self) -> u32 {
        match self.limit {
            Some(l) if l > 0 && l <= 100 => l,
            Some(l) if l > 100 => 100,
            _ => 50,
        }
    }

    pub fn effective_offset(&self) -> u32 {
        self.offset.unwrap_or(0)
    }
}

impl Default for ListPoliciesQuery {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicySummary {
    pub hrn: Hrn,
    pub name: String,
    pub description: Option<String>,
}

impl PolicySummary {
    pub fn new(hrn: Hrn, name: String, description: Option<String>) -> Self {
        Self {
            hrn,
            name,
            description,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PageInfo {
    pub total_count: u64,
    pub page_size: u32,
    pub current_offset: u32,
    pub has_next_page: bool,
    pub has_previous_page: bool,
}

impl PageInfo {
    pub fn from_query(query: &ListPoliciesQuery, total_count: u64, actual_count: usize) -> Self {
        let offset = query.effective_offset();

        Self {
            total_count,
            page_size: actual_count as u32,
            current_offset: offset,
            has_next_page: (offset as u64 + actual_count as u64) < total_count,
            has_previous_page: offset > 0,
        }
    }

    pub fn next_offset(&self) -> Option<u32> {
        if self.has_next_page {
            Some(self.current_offset + self.page_size)
        } else {
            None
        }
    }

    pub fn previous_offset(&self, page_size: u32) -> Option<u32> {
        if self.has_previous_page {
            Some(self.current_offset.saturating_sub(page_size))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListPoliciesResponse {
    pub policies: Vec<PolicySummary>,
    pub page_info: PageInfo,
}

impl ListPoliciesResponse {
    pub fn new(policies: Vec<PolicySummary>, page_info: PageInfo) -> Self {
        Self {
            policies,
            page_info,
        }
    }

    pub fn empty() -> Self {
        Self {
            policies: vec![],
            page_info: PageInfo {
                total_count: 0,
                page_size: 0,
                current_offset: 0,
                has_next_page: false,
                has_previous_page: false,
            },
        }
    }
}
