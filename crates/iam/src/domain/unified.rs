//! Clean domain model for IAM bounded context
//! Pure DDD implementation without legacy compatibility

use shared::{
    hrn::Hrn,
    lifecycle::Lifecycle,
    enums::OrganizationType,
};
use time::OffsetDateTime;
use std::collections::HashSet;

/// Unified User aggregate root
#[derive(Debug, Clone)]
pub struct User {
    pub id: UserId,
    pub email: Email,
    pub username: Username,
    pub profile: UserProfile,
    pub state: UserState,
    pub roles: HashSet<RoleId>,
    pub organizations: HashSet<OrganizationId>,
    pub lifecycle: Lifecycle,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
    pub last_login_at: Option<OffsetDateTime>,
    pub email_verified: bool,
    pub phone_number: Option<String>,
    pub two_factor_enabled: bool,
    pub preferences: UserPreferences,
    pub api_keys: Vec<ApiKey>,
    pub sessions: Vec<UserSession>,
}

impl User {
    pub fn new(
        id: UserId,
        email: Email,
        username: Username,
        creator_hrn: Hrn,
    ) -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
            id,
            email,
            username,
            profile: UserProfile::default(),
            state: UserState::Active,
            roles: HashSet::new(),
            organizations: HashSet::new(),
            lifecycle: Lifecycle::new(creator_hrn),
            created_at: now,
            updated_at: now,
            last_login_at: None,
        }
    }

    pub fn assign_role(&mut self, role_id: RoleId) {
        self.roles.insert(role_id);
    }

    pub fn revoke_role(&mut self, role_id: &RoleId) {
        self.roles.remove(role_id);
    }

    pub fn join_organization(&mut self, org_id: OrganizationId) {
        self.organizations.insert(org_id);
    }

    pub fn leave_organization(&mut self, org_id: &OrganizationId) {
        self.organizations.remove(org_id);
    }

    pub fn has_role(&self, role_id: &RoleId) -> bool {
        self.roles.contains(role_id)
    }

    pub fn is_member_of(&self, org_id: &OrganizationId) -> bool {
        self.organizations.contains(org_id)
    }
}

/// Unified Organization aggregate root
#[derive(Debug, Clone)]
pub struct Organization {
    pub id: OrganizationId,
    pub name: OrganizationName,
    pub display_name: String,
    pub description: Option<String>,
    pub organization_type: OrganizationType,
    pub state: OrganizationState,
    pub members: HashSet<OrganizationMember>,
    pub roles: HashSet<Role>,
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
            members: HashSet::new(),
            roles: HashSet::new(),
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

    pub fn add_member(&mut self, user_id: UserId, role: RoleId) {
        self.members.insert(OrganizationMember {
            user_id,
            role_id: role,
            joined_at: OffsetDateTime::now_utc(),
        });
    }

    pub fn remove_member(&mut self, user_id: &UserId) {
        self.members.retain(|m| &m.user_id != user_id);
    }

    pub fn add_role(&mut self, role: Role) {
        self.roles.insert(role);
    }

    pub fn remove_role(&mut self, role_id: &RoleId) {
        self.roles.retain(|r| &r.id != role_id);
    }
}

/// Unified Role entity
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Role {
    pub id: RoleId,
    pub name: RoleName,
    pub description: Option<String>,
    pub permissions: HashSet<Permission>,
    pub organization_id: Option<OrganizationId>,
    pub is_system_role: bool,
    pub created_at: OffsetDateTime,
}

impl Role {
    pub fn from_legacy(legacy: LegacyRole) -> Self {
        Self {
            id: RoleId(legacy.hrn),
            name: RoleName(legacy.name),
            description: legacy.description,
            permissions: legacy.permissions.into_iter().map(Permission::from_legacy).collect(),
            organization_id: legacy.organization_id.map(OrganizationId),
            is_system_role: legacy.is_system_role,
            created_at: legacy.created_at,
        }
    }

    pub fn to_legacy(&self) -> LegacyRole {
        LegacyRole {
            hrn: self.id.0.clone(),
            name: self.name.0.clone(),
            description: self.description.clone(),
            permissions: self.permissions.iter().map(|p| p.to_legacy()).collect(),
            organization_id: self.organization_id.as_ref().map(|id| id.0.clone()),
            is_system_role: self.is_system_role,
            created_at: self.created_at,
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

/// Unified Permission entity
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Permission {
    pub id: PermissionId,
    pub name: PermissionName,
    pub description: Option<String>,
    pub resource: String,
    pub action: String,
    pub conditions: HashMap<String, String>,
    pub created_at: OffsetDateTime,
}

impl Permission {
    pub fn from_legacy(legacy: LegacyPermission) -> Self {
        Self {
            id: PermissionId(legacy.hrn),
            name: PermissionName(legacy.name),
            description: legacy.description,
            resource: legacy.resource,
            action: legacy.action,
            conditions: legacy.conditions,
            created_at: legacy.created_at,
        }
    }

    pub fn to_legacy(&self) -> LegacyPermission {
        LegacyPermission {
            hrn: self.id.0.clone(),
            name: self.name.0.clone(),
            description: self.description.clone(),
            resource: self.resource.clone(),
            action: self.action.clone(),
            conditions: self.conditions.clone(),
            created_at: self.created_at,
        }
    }
}

/// Value objects
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UserId(pub Hrn);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OrganizationId(pub Hrn);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RoleId(pub Hrn);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PermissionId(pub Hrn);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Email(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Username(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OrganizationName(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RoleName(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PermissionName(pub String);

/// User profile value object
#[derive(Debug, Clone, Default)]
pub struct UserProfile {
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub bio: Option<String>,
    pub metadata: std::collections::HashMap<String, String>,
}

/// Organization member value object
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OrganizationMember {
    pub user_id: UserId,
    pub role_id: RoleId,
    pub joined_at: OffsetDateTime,
}

/// User state enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserState {
    Active,
    Suspended,
    Deactivated,
    PendingVerification,
}

/// Organization state enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrganizationState {
    Active,
    Suspended,
    Deactivated,
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::hrn::Hrn;

    #[test]
    fn test_user_role_management() {
        let user_hrn = Hrn::new("hrn:hodei:iam:global:acme:user/dev1").unwrap();
        let role_hrn = Hrn::new("hrn:hodei:iam:global:acme:role/developer").unwrap();
        
        let mut user = User {
            id: UserId(user_hrn),
            email: Email("dev@example.com".to_string()),
            username: Username("dev1".to_string()),
            profile: UserProfile::default(),
            state: UserState::Active,
            roles: HashSet::new(),
            organizations: HashSet::new(),
            lifecycle: Lifecycle::new(Hrn::new("hrn:hodei:iam:global:acme:user/system").unwrap()),
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
            last_login_at: None,
        };

        assert!(!user.has_role(&RoleId(role_hrn.clone())));
        
        user.assign_role(RoleId(role_hrn.clone()));
        assert!(user.has_role(&RoleId(role_hrn)));
    }

    #[test]
    fn test_organization_member_management() {
        let org_hrn = Hrn::new("hrn:hodei:iam:global:acme:organization/main").unwrap();
        let user_hrn = Hrn::new("hrn:hodei:iam:global:acme:user/dev1").unwrap();
        let role_hrn = Hrn::new("hrn:hodei:iam:global:acme:role/developer").unwrap();
        
        let mut org = Organization {
            id: OrganizationId(org_hrn),
            name: OrganizationName("Main Org".to_string()),
            display_name: "Main Organization".to_string(),
            description: None,
            organization_type: OrganizationType::Company,
            state: OrganizationState::Active,
            members: HashSet::new(),
            roles: HashSet::new(),
            lifecycle: Lifecycle::new(Hrn::new("hrn:hodei:iam:global:acme:user/system").unwrap()),
            created_at: OffsetDateTime::now_utc(),
            updated_at: OffsetDateTime::now_utc(),
        };

        assert!(!org.is_member_of(&UserId(user_hrn.clone())));
        
        org.add_member(UserId(user_hrn.clone()), RoleId(role_hrn));
        assert!(org.is_member_of(&UserId(user_hrn)));
    }
}
