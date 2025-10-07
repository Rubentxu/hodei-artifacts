use crate::features::evaluate_policies::error::EvaluatePoliciesError;
use cedar_policy::{Entity, EntityId, EntityTypeName, EntityUid, Policy, PolicyId, PolicySet};
use kernel::{HodeiEntity, Hrn, domain::policy::HodeiPolicySet};
use std::collections::HashMap;
use std::str::FromStr;

/// Translates an HRN to a Cedar EntityUid.
/// Algorithm: format!("{}::\"{}\"", hrn.entity_type_name(), hrn.resource_id())
pub(crate) fn to_cedar_euid(hrn: &Hrn) -> Result<EntityUid, EvaluatePoliciesError> {
    // Simplified: use the full entity_type_name as the type name
    let type_name = EntityTypeName::from_str(&hrn.entity_type_name()).map_err(|e| {
        EvaluatePoliciesError::TranslationError(format!("Invalid EntityTypeName: {}", e))
    })?;
    let entity_id = EntityId::new(hrn.resource_id());
    Ok(EntityUid::from_type_name_and_id(type_name, entity_id))
}

/// Translates a HodeiEntity to a Cedar Entity.
/// Simplified version: only basic attributes
pub(crate) fn to_cedar_entity(entity: &dyn HodeiEntity) -> Result<Entity, EvaluatePoliciesError> {
    let uid = to_cedar_euid(entity.hrn())?;

    // Simplified: create entity with no attributes for now
    let attrs = HashMap::new();

    Entity::new(uid, attrs, std::collections::HashSet::new()).map_err(|e| {
        EvaluatePoliciesError::TranslationError(format!("Entity creation failed: {}", e))
    })
}

/// Translates a HodeiPolicySet to a Cedar PolicySet.
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
