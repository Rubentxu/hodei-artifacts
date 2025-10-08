use crate::features::evaluate_policies::error::EvaluatePoliciesError;
use cedar_policy::{Entity, EntityId, EntityTypeName, EntityUid, Policy, PolicyId, PolicySet};
use kernel::{HodeiEntity, Hrn, domain::policy::HodeiPolicySet};
use std::collections::HashMap;
use std::str::FromStr;

/// Translates an HRN to a Cedar EntityUid.
/// Algorithm: format!("{}::\"{}\"", hrn.entity_type_name(), hrn.resource_id())
#[allow(dead_code)]
pub(crate) fn to_cedar_euid(hrn: &Hrn) -> Result<EntityUid, EvaluatePoliciesError> {
    // Simplified: use the full entity_type_name as the type name
    let type_name = EntityTypeName::from_str(&hrn.entity_type_name()).map_err(|e| {
        EvaluatePoliciesError::TranslationError(format!("Invalid EntityTypeName: {}", e))
    })?;
    let entity_id = EntityId::new(hrn.resource_id());
    Ok(EntityUid::from_type_name_and_id(type_name, entity_id))
}

/// Translates a HodeiEntity to a Cedar Entity.
/// Simplified version that creates entities without schema validation
#[allow(dead_code)]
pub(crate) fn to_cedar_entity(entity: &dyn HodeiEntity) -> Result<Entity, EvaluatePoliciesError> {
    let uid = to_cedar_euid(entity.hrn())?;

    // Create entities with no attributes to avoid schema validation issues
    let attrs = HashMap::new();

    // Use Entity::new without passing entities to avoid schema validation
    // This allows basic functionality to work without complex schema matching
    Entity::new(uid, attrs, std::collections::HashSet::new()).map_err(|e| {
        EvaluatePoliciesError::TranslationError(format!("Entity creation failed: {}", e))
    })
}

/// Translates a HodeiPolicySet to a Cedar PolicySet.
#[allow(dead_code)]
pub(crate) fn to_cedar_policy_set(
    set: &HodeiPolicySet,
) -> Result<PolicySet, EvaluatePoliciesError> {
    let mut policy_set = PolicySet::new();

    for policy in set.policies() {
        let id = PolicyId::new(policy.id());
        let cedar_policy = Policy::parse(Some(id.clone()), policy.content()).map_err(|e| {
            EvaluatePoliciesError::TranslationError(format!(
                "Invalid policy {}: {}",
                policy.id(),
                e
            ))
        })?;

        policy_set.add(cedar_policy).map_err(|e| {
            EvaluatePoliciesError::TranslationError(format!(
                "Failed to add policy {}: {}",
                policy.id(),
                e
            ))
        })?;
    }

    Ok(policy_set)
}
