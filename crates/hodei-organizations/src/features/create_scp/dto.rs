use crate::internal::domain::ServiceControlPolicy;
use kernel::Hrn;
use serde::{Deserialize, Serialize};

/// Command to create a new Service Control Policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateScpCommand {
    /// Human-readable name for the SCP
    pub name: String,
    /// Raw Cedar policy document
    pub document: String,
    /// HRN for the SCP
    pub hrn: Hrn,
}

/// Command to delete an existing Service Control Policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteScpCommand {
    /// HRN of the SCP to delete
    pub hrn: Hrn,
}

/// Command to update an existing Service Control Policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateScpCommand {
    /// HRN of the SCP to update
    pub hrn: Hrn,
    /// New name (optional)
    pub name: Option<String>,
    /// New document (optional)
    pub document: Option<String>,
}

/// Query to get a specific Service Control Policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetScpQuery {
    /// HRN of the SCP to retrieve
    pub hrn: Hrn,
}

/// Query to list Service Control Policies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListScpsQuery {
    /// Maximum number of results to return
    pub limit: Option<u32>,
    /// Number of results to skip
    pub offset: Option<u32>,
}

/// Data Transfer Object for Service Control Policy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScpDto {
    /// Unique identifier HRN
    pub hrn: Hrn,
    /// Human-friendly name
    pub name: String,
    /// Raw Cedar policy document
    pub document: String,
}

impl From<ServiceControlPolicy> for ScpDto {
    fn from(scp: ServiceControlPolicy) -> Self {
        ScpDto {
            hrn: scp.hrn,
            name: scp.name,
            document: scp.document,
        }
    }
}

impl From<ScpDto> for ServiceControlPolicy {
    fn from(dto: ScpDto) -> Self {
        ServiceControlPolicy::new(dto.hrn, dto.name, dto.document)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_hrn() -> Hrn {
        Hrn::new(
            "aws".to_string(),
            "organizations".to_string(),
            "default".to_string(),
            "ServiceControlPolicy".to_string(),
            "scp-123".to_string(),
        )
    }

    #[test]
    fn scp_dto_from_service_control_policy() {
        let scp = ServiceControlPolicy::new(
            sample_hrn(),
            "TestPolicy".to_string(),
            "permit(principal, action, resource);".to_string(),
        );

        let dto = ScpDto::from(scp.clone());
        assert_eq!(dto.hrn, scp.hrn);
        assert_eq!(dto.name, scp.name);
        assert_eq!(dto.document, scp.document);
    }

    #[test]
    fn service_control_policy_from_scp_dto() {
        let dto = ScpDto {
            hrn: sample_hrn(),
            name: "TestPolicy".to_string(),
            document: "permit(principal, action, resource);".to_string(),
        };

        let scp = ServiceControlPolicy::from(dto.clone());
        assert_eq!(scp.hrn, dto.hrn);
        assert_eq!(scp.name, dto.name);
        assert_eq!(scp.document, dto.document);
    }
}
