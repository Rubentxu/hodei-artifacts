//! Ports for get_effective_policies_for_principal feature
//!
//! Define las interfaces (traits) que el caso de uso necesita para obtener
//! las políticas efectivas de un principal (usuario o service account).

use crate::shared::domain::{Group, User};
use policies::shared::domain::hrn::Hrn;

/// Port para encontrar usuarios por HRN
#[async_trait::async_trait]
pub trait UserFinderPort: Send + Sync {
    /// Find a user by their HRN
    async fn find_by_hrn(
        &self,
        hrn: &Hrn,
    ) -> Result<Option<User>, Box<dyn std::error::Error + Send + Sync>>;
}

/// Port para encontrar grupos a los que pertenece un usuario
#[async_trait::async_trait]
pub trait GroupFinderPort: Send + Sync {
    /// Find all groups that a user belongs to
    async fn find_groups_by_user_hrn(
        &self,
        user_hrn: &Hrn,
    ) -> Result<Vec<Group>, Box<dyn std::error::Error + Send + Sync>>;
}

/// Port para encontrar políticas asociadas a un principal
#[async_trait::async_trait]
pub trait PolicyFinderPort: Send + Sync {
    /// Find all policy documents associated with a principal (user or group)
    ///
    /// Returns policy documents in Cedar format as strings
    async fn find_policies_by_principal(
        &self,
        principal_hrn: &Hrn,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>>;
}
