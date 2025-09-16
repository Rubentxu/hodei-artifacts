// crates/organization/src/domain/team.rs

use serde::{Deserialize, Serialize};
use shared::hrn::{OrganizationId, TeamId, UserId};
use shared::lifecycle::Lifecycle;
use std::collections::HashSet;

/// Equipo dentro de una organización, alineado con el diagrama de dominio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub id: TeamId,
    pub name: String,
    pub organization: OrganizationId,
    pub description: Option<String>,
    pub members: HashSet<UserId>,
    pub lifecycle: Lifecycle,
}
