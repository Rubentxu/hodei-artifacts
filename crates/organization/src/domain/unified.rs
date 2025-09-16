//! Unified domain model for organization bounded context
//! Merges existing implementations with DDD patterns

use crate::domain::{
    organization::Organization as LegacyOrganization,
    member::Member as LegacyMember,
    team::Team as LegacyTeam,
};
use shared::{
    hrn::Hrn,
    lifecycle::Lifecycle,
    enums::OrganizationType,
};
use time::OffsetDateTime;
use std::collections::HashSet;

/// Unified Organization aggregate root
#[derive(Debug, Clone)]
pub struct Organization {
    pub id: OrganizationId,
    pub name: OrganizationName,
    pub display_name: String,
    pub description: Option<String>,
    pub organization_type: OrganizationType,
    pub state: OrganizationState,
    pub settings: OrganizationSettings,
    pub members: HashSet<Member>,
    pub teams: HashSet<Team>,
    pub repositories: HashSet<RepositoryId>,
    pub lifecycle: Lifecycle,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

impl Organization {
    pub fn from_legacy(legacy: LegacyOrganization) -> Self {
        Self {
            id: OrganizationId(legacy.hrn),
            name: OrganizationName(legacy.name),
            display_name: legacy.display_name,
            description: legacy.description,
            organization_type: legacy.organization_type,
            state: OrganizationState::Active,
            settings: OrganizationSettings::default(),
            members: HashSet::new(),
            teams: HashSet::new(),
            repositories: HashSet::new(),
            lifecycle: legacy.lifecycle,
            created_at: legacy.created_at,
            updated_at: legacy.updated_at,
        }
    }

    pub fn to_legacy(&self) -> LegacyOrganization {
        LegacyOrganization {
            hrn: self.id.0.clone(),
            name: self.name.0.clone(),
            display_name: self.display_name.clone(),
            description: self.description.clone(),
            organization_type: self.organization_type,
            lifecycle: self.lifecycle.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }

    pub fn add_member(&mut self, member: Member) {
        self.members.insert(member);
    }

    pub fn remove_member(&mut self, user_id: &UserId) {
        self.members.retain(|m| &m.user_id != user_id);
    }

    pub fn add_team(&mut self, team: Team) {
        self.teams.insert(team);
    }

    pub fn remove_team(&mut self, team_id: &TeamId) {
        self.teams.retain(|t| &t.id != team_id);
    }

    pub fn add_repository(&mut self, repository_id: RepositoryId) {
        self.repositories.insert(repository_id);
    }

    pub fn remove_repository(&mut self, repository_id: &RepositoryId) {
        self.repositories.remove(repository_id);
    }

    pub fn get_member(&self, user_id: &UserId) -> Option<&Member> {
        self.members.iter().find(|m| &m.user_id == user_id)
    }

    pub fn get_team(&self, team_id: &TeamId) -> Option<&Team> {
        self.teams.iter().find(|t| &t.id == team_id)
    }
}

/// Unified Member entity
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Member {
    pub user_id: UserId,
    pub organization_id: OrganizationId,
    pub role: MemberRole,
    pub permissions: HashSet<Permission>,
    pub joined_at: OffsetDateTime,
    pub invited_by: Option<UserId>,
}

impl Member {
    pub fn from_legacy(legacy: LegacyMember) -> Self {
        Self {
            user_id: UserId(legacy.user_hrn),
            organization_id: OrganizationId(legacy.organization_hrn),
            role: MemberRole::from_legacy(legacy.role),
            permissions: HashSet::new(),
            joined_at: legacy.joined_at,
            invited_by: legacy.invited_by.map(UserId),
        }
    }

    pub fn to_legacy(&self) -> LegacyMember {
        LegacyMember {
            user_hrn: self.user_id.0.clone(),
            organization_hrn: self.organization_id.0.clone(),
            role: self.role.to_legacy(),
            joined_at: self.joined_at,
            invited_by: self.invited_by.as_ref().map(|id| id.0.clone()),
        }
    }

    pub fn has_permission(&self, permission: &Permission) -> bool {
        self.permissions.contains(permission)
    }

    pub fn add_permission(&mut self, permission: Permission) {
        self.permissions.insert(permission);
    }

    pub fn remove_permission(&mut self, permission: &Permission) {
        self.permissions.remove(permission);
    }
}

/// Unified Team entity
#[derive(Debug, Clone)]
pub struct Team {
    pub id: TeamId,
    pub organization_id: OrganizationId,
    pub name: TeamName,
    pub display_name: String,
    pub description: Option<String>,
    pub members: HashSet<UserId>,
    pub repositories: HashSet<RepositoryId>,
    pub permissions: HashSet<Permission>,
    pub lifecycle: Lifecycle,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

impl Team {
    pub fn from_legacy(legacy: LegacyTeam) -> Self {
        Self {
            id: TeamId(legacy.hrn),
            organization_id: OrganizationId(legacy.organization_hrn),
            name: TeamName(legacy.name),
            display_name: legacy.display_name,
            description: legacy.description,
            members: HashSet::new(),
            repositories: HashSet::new(),
            permissions: HashSet::new(),
            lifecycle: legacy.lifecycle,
            created_at: legacy.created_at,
            updated_at: legacy.updated_at,
        }
    }

    pub fn to_legacy(&self) -> LegacyTeam {
        LegacyTeam {
            hrn: self.id.0.clone(),
            organization_hrn: self.organization_id.0.clone(),
            name: self.name.0.clone(),
            display_name: self.display_name.clone(),
            description: self.description.clone(),
            lifecycle: self.lifecycle.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }

    pub fn add_member(&mut self, user_id: UserId) {
        self.members.insert(user_id);
    }

    pub fn remove_member(&mut self, user_id: &UserId) {
        self.members.remove(user_id);
    }

    pub fn add_repository(&mut self, repository_id: RepositoryId) {
        self.repositories.insert(repository_id);
    }

    pub fn remove_repository(&mut self, repository_id: &RepositoryId) {
        self.repositories.remove(repository_id);
    }

    pub fn add_permission(&mut self, permission: Permission) {
        self.permissions.insert(permission);
    }

    pub fn remove_permission(&mut self, permission: &Permission) {
        self.permissions.remove(permission);
    }
}

/// Organization settings value object
#[derive(Debug, Clone, Default)]
pub struct OrganizationSettings {
    pub allow_public_repositories: bool,
    pub require_approval_for_new_members: bool,
    pub max_teams: Option<u32>,
    pub max_members: Option<u32>,
    pub custom_properties: std::collections::HashMap<String, String>,
}

/// Member role enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemberRole {
    Owner,
    Admin,
    Maintainer,
    Developer,
    Viewer,
}

impl MemberRole {
    pub fn from_legacy(legacy: crate::domain::member::MemberRole) -> Self {
        match legacy {
            crate::domain::member::MemberRole::Owner => Self::Owner,
            crate::domain::member::MemberRole::Admin => Self::Admin,
            crate::domain::member::MemberRole::Maintainer => Self::Maintainer,
            crate::domain::member::MemberRole::Developer => Self::Developer,
            crate::domain::member::MemberRole::Viewer => Self::Viewer,
        }
    }

    pub fn to_legacy(&self) -> crate::domain::member::MemberRole {
        match self {
            Self::Owner => crate::domain::member::MemberRole::Owner,
            Self::Admin => crate::domain::member::MemberRole::Admin,
            Self::Maintainer => crate::domain::member::MemberRole::Maintainer,
            Self::Developer => crate::domain::member::MemberRole::Developer,
            Self::Viewer => crate::domain::member::MemberRole::Viewer,
        }
    }
}

/// Permission value object
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Permission {
    pub resource: String,
    pub action: String,
    pub conditions: std::collections::HashMap<String, String>,
}

/// Organization state enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrganizationState {
    Active,
    Suspended,
    Deactivated,
}

/// Value objects
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OrganizationId(pub Hrn);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OrganizationName(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TeamId(pub Hrn);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TeamName(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UserId(pub Hrn);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RepositoryId(pub Hrn);

#[cfg(test)]
mod tests {
    use super::*;
    use shared::hrn::Hrn;

    #[test]
    fn test_organization_creation() {
        let org_hrn = Hrn::new("hrn:hodei:organization:global:acme:organization/acme").unwrap();
        
        let organization = Organization {
            id: OrganizationId(org_hrn),
            name: OrganizationName("acme".to_string()),
            display_name: "ACME Corp".to_string(),
            description: Some("Main organization".to_string()),
            organization_type: OrganizationType::Company,
            state: OrganizationState::Active,
            settings: OrganizationSettings::default(),
            members: HashSet::new(),
            teams: HashSet::new(),
            repositories: HashSet::new(),
            lifecycle: Lifecycle::new(Hrn::new("hrn:hodei:iam:global:acme:user/system").unwrap()),
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
        };

        assert_eq!(organization.name.0, "acme");
        assert_eq!(organization.organization_type, OrganizationType::Company);
    }

    #[test]
    fn test_team_management() {
        let team_hrn = Hrn::new("hrn:hodei:organization:global:acme:team/backend").unwrap();
        let org_hrn = Hrn::new("hrn:hodei:organization:global:acme:organization/acme").unwrap();
        
        let mut team = Team {
            id: TeamId(team_hrn),
            organization_id: OrganizationId(org_hrn),
            name: TeamName("backend".to_string()),
            display_name: "Backend Team".to_string(),
            description: Some("Backend development team".to_string()),
            members: HashSet::new(),
            repositories: HashSet::new(),
            permissions: HashSet::new(),
            lifecycle: Lifecycle::new(Hrn::new("hrn:hodei:iam:global:acme:user/system").unwrap()),
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
        };

        let user_hrn = Hrn::new("hrn:hodei:iam:global:acme:user/dev1").unwrap();
        team.add_member(UserId(user_hrn));
        assert_eq!(team.members.len(), 1);
    }
}
