//! Data Transfer Objects for Get Organization Details Feature
//!
//! This module contains all the DTOs for organization retrieval operations,
//! following VSA principles with segregated interfaces.

use serde::{Serialize, Deserialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};

/// Request to get organization details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetOrganizationDetailsRequest {
    /// Unique identifier for the organization
    pub organization_id: String,
    /// Whether to include members in the response
    pub include_members: bool,
    /// Whether to include invitations in the response
    pub include_invitations: bool,
    /// Whether to include policies in the response
    pub include_policies: bool,
}

impl GetOrganizationDetailsRequest {
    pub fn new(organization_id: String) -> Self {
        Self {
            organization_id,
            include_members: false,
            include_invitations: false,
            include_policies: false,
        }
    }
    
    pub fn with_members(mut self, include: bool) -> Self {
        self.include_members = include;
        self
    }
    
    pub fn with_invitations(mut self, include: bool) -> Self {
        self.include_invitations = include;
        self
    }
    
    pub fn with_policies(mut self, include: bool) -> Self {
        self.include_policies = include;
        self
    }
    
    /// Validate the request
    pub fn validate(&self) -> Result<(), OrganizationError> {
        if self.organization_id.trim().is_empty() {
            return Err(OrganizationError::validation("Organization ID cannot be empty"));
        }
        
        // Validate UUID format if it's a UUID
        if Uuid::parse_str(&self.organization_id).is_err() && !self.organization_id.starts_with("org_") {
            return Err(OrganizationError::validation("Invalid organization ID format"));
        }
        
        Ok(())
    }
}

/// Response with organization details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetOrganizationDetailsResponse {
    /// Organization basic information
    pub organization: OrganizationDto,
    /// Organization members (if requested)
    pub members: Option<Vec<MemberDto>>,
    /// Pending invitations (if requested)
    pub invitations: Option<Vec<InvitationDto>>,
    /// Organization policies (if requested)
    pub policies: Option<Vec<PolicyDto>>,
    /// Response metadata
    pub metadata: ResponseMetadata,
}

impl GetOrganizationDetailsResponse {
    pub fn new(
        organization: OrganizationDto,
        members: Option<Vec<MemberDto>>,
        invitations: Option<Vec<InvitationDto>>,
        policies: Option<Vec<PolicyDto>>,
    ) -> Self {
        Self {
            organization,
            members,
            invitations,
            policies,
            metadata: ResponseMetadata::new(),
        }
    }
    
    pub fn with_metadata(mut self, metadata: ResponseMetadata) -> Self {
        self.metadata = metadata;
        self
    }
}

/// Organization data transfer object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationDto {
    pub id: String,
    pub name: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub owner_id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub status: OrganizationStatus,
    pub settings: OrganizationSettings,
    pub metadata: std::collections::HashMap<String, String>,
}

impl OrganizationDto {
    pub fn from_domain(org: crate::domain::organization::Organization) -> Self {
        Self {
            id: org.id().to_string(),
            name: org.name().to_string(),
            display_name: org.display_name().map(|s| s.to_string()),
            description: org.description().map(|s| s.to_string()),
            owner_id: org.owner_id().to_string(),
            created_at: org.created_at(),
            updated_at: org.updated_at(),
            status: org.status().into(),
            settings: org.settings().clone().into(),
            metadata: org.metadata().clone(),
        }
    }
}

/// Organization member data transfer object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberDto {
    pub id: String,
    pub user_id: String,
    pub organization_id: String,
    pub role: MemberRole,
    pub status: MemberStatus,
    pub joined_at: Option<DateTime<Utc>>,
    pub invited_by: Option<String>,
    pub permissions: Vec<String>,
}

impl MemberDto {
    pub fn from_domain(member: crate::domain::member::Member) -> Self {
        Self {
            id: member.id().to_string(),
            user_id: member.user_id().to_string(),
            organization_id: member.organization_id().to_string(),
            role: member.role().clone().into(),
            status: member.status().into(),
            joined_at: member.joined_at(),
            invited_by: member.invited_by().map(|s| s.to_string()),
            permissions: member.permissions().clone(),
        }
    }
}

/// Organization invitation data transfer object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationDto {
    pub id: String,
    pub organization_id: String,
    pub email: String,
    pub role: MemberRole,
    pub invited_by: String,
    pub invited_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub status: InvitationStatus,
    pub token: Option<String>,
}

impl InvitationDto {
    pub fn from_domain(invitation: crate::domain::invitation::Invitation) -> Self {
        Self {
            id: invitation.id().to_string(),
            organization_id: invitation.organization_id().to_string(),
            email: invitation.email().to_string(),
            role: invitation.role().clone().into(),
            invited_by: invitation.invited_by().to_string(),
            invited_at: invitation.invited_at(),
            expires_at: invitation.expires_at(),
            status: invitation.status().into(),
            token: invitation.token().map(|s| s.to_string()),
        }
    }
}

/// Organization policy data transfer object
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDto {
    pub id: String,
    pub organization_id: String,
    pub name: String,
    pub description: Option<String>,
    pub type_: PolicyType,
    pub rules: Vec<PolicyRule>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: String,
    pub is_active: bool,
}

impl PolicyDto {
    pub fn from_domain(policy: crate::domain::policy::Policy) -> Self {
        Self {
            id: policy.id().to_string(),
            organization_id: policy.organization_id().to_string(),
            name: policy.name().to_string(),
            description: policy.description().map(|s| s.to_string()),
            type_: policy.type_().clone().into(),
            rules: policy.rules().iter().map(|r| r.clone().into()).collect(),
            created_at: policy.created_at(),
            updated_at: policy.updated_at(),
            created_by: policy.created_by().to_string(),
            is_active: policy.is_active(),
        }
    }
}

/// Organization status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrganizationStatus {
    Active,
    Inactive,
    Suspended,
    Archived,
}

impl From<crate::domain::organization::OrganizationStatus> for OrganizationStatus {
    fn from(status: crate::domain::organization::OrganizationStatus) -> Self {
        match status {
            crate::domain::organization::OrganizationStatus::Active => OrganizationStatus::Active,
            crate::domain::organization::OrganizationStatus::Inactive => OrganizationStatus::Inactive,
            crate::domain::organization::OrganizationStatus::Suspended => OrganizationStatus::Suspended,
            crate::domain::organization::OrganizationStatus::Archived => OrganizationStatus::Archived,
        }
    }
}

/// Organization settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrganizationSettings {
    pub allow_public_visibility: bool,
    pub allow_member_invitations: bool,
    pub require_approval_for_join: bool,
    pub max_members: Option<u32>,
    pub default_member_role: String,
    pub custom_settings: std::collections::HashMap<String, String>,
}

impl From<crate::domain::organization::OrganizationSettings> for OrganizationSettings {
    fn from(settings: crate::domain::organization::OrganizationSettings) -> Self {
        Self {
            allow_public_visibility: settings.allow_public_visibility(),
            allow_member_invitations: settings.allow_member_invitations(),
            require_approval_for_join: settings.require_approval_for_join(),
            max_members: settings.max_members(),
            default_member_role: settings.default_member_role().to_string(),
            custom_settings: settings.custom_settings().clone(),
        }
    }
}

/// Member role
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemberRole {
    Owner,
    Admin,
    Member,
    Guest,
}

impl From<crate::domain::member::MemberRole> for MemberRole {
    fn from(role: crate::domain::member::MemberRole) -> Self {
        match role {
            crate::domain::member::MemberRole::Owner => MemberRole::Owner,
            crate::domain::member::MemberRole::Admin => MemberRole::Admin,
            crate::domain::member::MemberRole::Member => MemberRole::Member,
            crate::domain::member::MemberRole::Guest => MemberRole::Guest,
        }
    }
}

/// Member status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemberStatus {
    Active,
    Inactive,
    Suspended,
    Pending,
}

impl From<crate::domain::member::MemberStatus> for MemberStatus {
    fn from(status: crate::domain::member::MemberStatus) -> Self {
        match status {
            crate::domain::member::MemberStatus::Active => MemberStatus::Active,
            crate::domain::member::MemberStatus::Inactive => MemberStatus::Inactive,
            crate::domain::member::MemberStatus::Suspended => MemberStatus::Suspended,
            crate::domain::member::MemberStatus::Pending => MemberStatus::Pending,
        }
    }
}

/// Invitation status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InvitationStatus {
    Pending,
    Accepted,
    Declined,
    Expired,
    Cancelled,
}

impl From<crate::domain::invitation::InvitationStatus> for InvitationStatus {
    fn from(status: crate::domain::invitation::InvitationStatus) -> Self {
        match status {
            crate::domain::invitation::InvitationStatus::Pending => InvitationStatus::Pending,
            crate::domain::invitation::InvitationStatus::Accepted => InvitationStatus::Accepted,
            crate::domain::invitation::InvitationStatus::Declined => InvitationStatus::Declined,
            crate::domain::invitation::InvitationStatus::Expired => InvitationStatus::Expired,
            crate::domain::invitation::InvitationStatus::Cancelled => InvitationStatus::Cancelled,
        }
    }
}

/// Policy type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PolicyType {
    AccessControl,
    ResourceQuota,
    Security,
    Compliance,
    Custom,
}

impl From<crate::domain::policy::PolicyType> for PolicyType {
    fn from(type_: crate::domain::policy::PolicyType) -> Self {
        match type_ {
            crate::domain::policy::PolicyType::AccessControl => PolicyType::AccessControl,
            crate::domain::policy::PolicyType::ResourceQuota => PolicyType::ResourceQuota,
            crate::domain::policy::PolicyType::Security => PolicyType::Security,
            crate::domain::policy::PolicyType::Compliance => PolicyType::Compliance,
            crate::domain::policy::PolicyType::Custom(name) => PolicyType::Custom,
        }
    }
}

/// Policy rule
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRule {
    pub name: String,
    pub condition: String,
    pub action: String,
    pub effect: PolicyEffect,
    pub priority: u32,
}

impl From<crate::domain::policy::PolicyRule> for PolicyRule {
    fn from(rule: crate::domain::policy::PolicyRule) -> Self {
        Self {
            name: rule.name().to_string(),
            condition: rule.condition().to_string(),
            action: rule.action().to_string(),
            effect: rule.effect().into(),
            priority: rule.priority(),
        }
    }
}

/// Policy effect
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PolicyEffect {
    Allow,
    Deny,
}

impl From<crate::domain::policy::PolicyEffect> for PolicyEffect {
    fn from(effect: crate::domain::policy::PolicyEffect) -> Self {
        match effect {
            crate::domain::policy::PolicyEffect::Allow => PolicyEffect::Allow,
            crate::domain::policy::PolicyEffect::Deny => PolicyEffect::Deny,
        }
    }
}

/// Response metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseMetadata {
    pub request_id: String,
    pub timestamp: DateTime<Utc>,
    pub version: String,
    pub processing_time_ms: u64,
}

impl ResponseMetadata {
    pub fn new() -> Self {
        Self {
            request_id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            version: "1.0".to_string(),
            processing_time_ms: 0,
        }
    }
    
    pub fn with_processing_time(mut self, time_ms: u64) -> Self {
        self.processing_time_ms = time_ms;
        self
    }
}

impl Default for ResponseMetadata {
    fn default() -> Self {
        Self::new()
    }
}