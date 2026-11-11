//! The actions the service authorizes with.
//!
//! An action has two parts: the service it belongs to and its name. The gate
//! builds the Cedar uid from them -- `Artifacts::Action::"ReadRepository"` --
//! while the persisted evaluator resolves the verb against the schema that
//! governs its requests, and the schema decides the namespace.
//!
//! Both have to name the same action, so the bundle has to declare the
//! action under the namespace the service builds. Nothing enforced that: a
//! bundle that declared the verb under another namespace would make the gate
//! unable to decide (Cedar refuses a request it cannot fit) while the
//! evaluator resolved it happily. The deployment checks it at boot and
//! refuses to start when the bundle and the service disagree.
//!
//! The actions live here so that the routes and the check name them from one
//! place: a route that invented its own action would be invisible to the
//! check.

use cedar_policy::Schema;
use hodei_authz::model::ActionId;

/// Reading a repository.
pub fn read_repository() -> ActionId {
    ActionId::new("artifacts", "ReadRepository").expect("a valid action")
}

/// Creating a repository.
pub fn create_repository() -> ActionId {
    ActionId::new("artifacts", "CreateRepository").expect("a valid action")
}

/// Reaching the administrative surface.
pub fn access_admin_api() -> ActionId {
    ActionId::new("admin", "AccessAdminApi").expect("a valid action")
}

/// Every action the service authorizes with.
pub fn all() -> Vec<ActionId> {
    vec![read_repository(), create_repository(), access_admin_api()]
}

/// Check that the bundle declares the action the service authorizes with.
///
/// The gate builds the Cedar uid of an action from its service and its name,
/// and the bundle has to declare exactly that action: named once, and under
/// the namespace the service builds. A bundle that declares the verb under
/// another namespace, or that declares it twice, would make the two circuits
/// of the platform decide about different actions.
///
/// The reason is returned when the bundle and the service disagree, because
/// a deployment that does not start has to say why.
pub fn check_declared(schema: &Schema, action: &ActionId) -> Result<(), String> {
    let expected = action.to_cedar_uid();
    let declared: Vec<String> = schema
        .actions()
        .filter(|uid| uid.id().unescaped() == action.name())
        .map(|uid| uid.to_string())
        .collect();

    match declared.len() {
        0 => Err(format!(
            "the authorization bundle does not declare the action {expected}"
        )),
        1 => {
            let declared = &declared[0];
            if declared == &expected {
                Ok(())
            } else {
                Err(format!(
                    "the authorization bundle declares the action {declared}, but the service \
                     authorizes with {expected}; the two have to name the same action"
                ))
            }
        }
        _ => Err(format!(
            "the authorization bundle declares the action {} more than once ({}), so the verb does \
             not identify one action",
            action.name(),
            declared.join(", ")
        )),
    }
}

/// Check that the bundle declares every action the service authorizes with.
///
/// The administrative action is only checked when the administrative surface
/// is enabled: a deployment that closes it never builds that action, so it
/// never needs to declare it.
pub fn check_service_declared(schema: &Schema, admin_enabled: bool) -> Result<(), String> {
    check_declared(schema, &read_repository())?;
    check_declared(schema, &create_repository())?;
    if admin_enabled {
        check_declared(schema, &access_admin_api())?;
    }
    Ok(())
}
