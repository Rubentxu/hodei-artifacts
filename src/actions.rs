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

/// Managing the ABAC tags of a repository.
///
/// Tags feed tag-based policies, so whoever writes them can widen who is
/// admitted. It is its own action and no other (not `CreateRepository`, nor
/// any future update verb) implies it: a policy has to grant it explicitly.
pub fn manage_repository_tags() -> ActionId {
    ActionId::new("artifacts", "ManageRepositoryTags").expect("a valid action")
}

/// Declaring evidence admission on a repository (HSP-153).
///
/// The admission declaration names the provider the service will call out to,
/// so it is its own action: `CreateRepository` never implies it. Admission is
/// **self-declared** by whoever may manage it; it is a gate for the artifacts
/// published into that repository, not a boundary against its own owner.
pub fn manage_repository_admission() -> ActionId {
    ActionId::new("artifacts", "ManageRepositoryAdmission").expect("a valid action")
}

/// Creating and releasing a dependency freeze of a repository (HSP-161).
///
/// A freeze pins a set of digests against the garbage collection and names
/// them as an immutable, reproducible view. It is its own action: reading a
/// repository, uploading to it or administering its tags never implies it,
/// because whoever holds it decides which bytes the platform must keep.
pub fn manage_dependency_freeze() -> ActionId {
    ActionId::new("artifacts", "ManageDependencyFreeze").expect("a valid action")
}

/// Reading the history of a repository through the time machine (HSP-165).
///
/// The history lists paths, versions and digests of frozen views, and says
/// which bytes are still in the store. It is its own action: being able to
/// download an asset does not imply being able to compare what a repository
/// served at different moments.
pub fn explore_repository_history() -> ActionId {
    ActionId::new("artifacts", "ExploreRepositoryHistory").expect("a valid action")
}

/// Repairing a damaged or missing blob of a repository from the configured
/// repair source (HSP-164).
///
/// A repair writes into the content store, so it is its own action: reading
/// or uploading to a repository, freezing it or reading its history never
/// implies it. The bytes come only from the source the deployment configured,
/// and only after they hash to the digest asked for.
pub fn manage_storage_repair() -> ActionId {
    ActionId::new("artifacts", "ManageStorageRepair").expect("a valid action")
}

/// Managing the trusted publishers of a repository (HSP-152).
///
/// A trusted publisher binds an OIDC identity to the publication into the
/// repository, so whoever writes it decides who can publish without a
/// long-lived secret. It is its own action: `CreateRepository`,
/// `UploadAsset` and the other management actions never imply it.
pub fn manage_trusted_publishers() -> ActionId {
    ActionId::new("artifacts", "ManageTrustedPublishers").expect("a valid action")
}

/// Reading the trust capsule of a release.
///
/// The capsule is a read: it is a view of the evidence that is there, and
/// reading it does not change anything. It has its own action because it
/// answers a different question than the repository does, and a deployment
/// may want to let one and not the other.
pub fn read_capsule() -> ActionId {
    ActionId::new("artifacts", "ReadCapsule").expect("a valid action")
}

/// Reaching the administrative surface.
pub fn access_admin_api() -> ActionId {
    ActionId::new("admin", "AccessAdminApi").expect("a valid action")
}

/// Publishing content in a repository.
///
/// Publishing and downloading are two different actions because a
/// deployment may want one without the other: a principal that only
/// publishes is a write-only principal, and a principal that only reads
/// is a mirror. With a single action, a permission cannot be said.
pub fn upload_asset() -> ActionId {
    ActionId::new("artifacts", "UploadAsset").expect("a valid action")
}

/// Downloading content from a repository.
pub fn download_asset() -> ActionId {
    ActionId::new("artifacts", "DownloadAsset").expect("a valid action")
}

/// Promoting a digest of a repository to its trusted channel.
///
/// It is the permission of **who** may promote. Whether the software
/// qualifies is decided by the trust policy, and both have to say yes.
pub fn promote_asset() -> ActionId {
    ActionId::new("artifacts", "PromoteAsset").expect("a valid action")
}

/// Revoking the promotion of a digest, or of a whole decision.
///
/// It is its own action: whoever may promote is not thereby the one who
/// may take a promotion back, and a policy can grant either alone.
pub fn revoke_promotion() -> ActionId {
    ActionId::new("artifacts", "RevokePromotion").expect("a valid action")
}

/// Investigating a subject.
///
/// It is a **read**: the investigation explains what the service knows
/// about a subject, and it does not change anything. It has its own
/// action because it answers a different question than the download does:
/// a deployment may want to let a principal read the bytes of a package
/// without letting it ask why the package is affected.
pub fn investigate() -> ActionId {
    ActionId::new("artifacts", "Investigate").expect("a valid action")
}

/// Every action the service authorizes with.
pub fn all() -> Vec<ActionId> {
    vec![
        read_repository(),
        create_repository(),
        manage_repository_tags(),
        manage_repository_admission(),
        manage_dependency_freeze(),
        explore_repository_history(),
        manage_storage_repair(),
        manage_trusted_publishers(),
        read_capsule(),
        access_admin_api(),
        upload_asset(),
        download_asset(),
        promote_asset(),
        revoke_promotion(),
        investigate(),
    ]
}

/// Check that the bundle declares the action of the trusted publishers.
///
/// Only an instance that wires the publishers has to declare it.
pub fn check_publishers_declared(schema: &Schema) -> Result<(), String> {
    check_declared(schema, &manage_trusted_publishers())
}

/// Check that the bundle declares the two actions of the promotion.
///
/// It is separate from [`check_service_declared`] because it only applies
/// to an instance that wires the promotion service: an instance without
/// one never builds those actions and does not have to declare them.
pub fn check_promotion_declared(schema: &Schema) -> Result<(), String> {
    check_declared(schema, &promote_asset())?;
    check_declared(schema, &revoke_promotion())
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
/// never needs to declare it. The same goes for the upload of the content:
/// an instance that does not mount a surface that writes does not build
/// that action, and does not have to declare it.
///
/// The download of the content is always required: the search of the
/// catalog and the detail of a package are always mounted, and they ask
/// for that permission, because a listing says what is inside. A bundle
/// that does not declare it would leave those two surfaces without a
/// verdict, and every request would end in a 500.
pub fn check_service_declared(
    schema: &Schema,
    admin_enabled: bool,
    raw_enabled: bool,
    oci_enabled: bool,
    investigation_enabled: bool,
) -> Result<(), String> {
    check_declared(schema, &read_repository())?;
    check_declared(schema, &read_capsule())?;
    check_declared(schema, &create_repository())?;
    check_declared(schema, &manage_repository_tags())?;
    check_declared(schema, &manage_repository_admission())?;
    check_scope_attributes_declared(schema)?;
    check_declared(schema, &download_asset())?;
    if admin_enabled {
        check_declared(schema, &access_admin_api())?;
    }
    if raw_enabled || oci_enabled {
        // The content surface **writes**: without the upload declared, the
        // bundle cannot decide on a push, and every request would end in a
        // 403 instead of the service refusing to start.
        check_declared(schema, &upload_asset())?;
    }
    if investigation_enabled {
        // The investigation surface asks for its own action: a bundle that
        // does not declare it cannot grant it, and every request would end
        // in a 403 instead of the service refusing to start.
        check_declared(schema, &investigate())?;
    }
    Ok(())
}

/// Check that the bundle's schema accepts the trusted scope attributes
/// (`namespace`, `tags`) on `Artifacts::Repository`.
///
/// Every repository decision carries those attributes (HSP-321). Cedar
/// refuses an entity with an attribute the schema does not declare, so a
/// bundle that omits them would turn every repository decision into
/// `Indeterminate`. The deployment refuses to start instead. Declare them
/// as optional (`"required": false`) so decisions without a scope still
/// validate.
pub fn check_scope_attributes_declared(schema: &Schema) -> Result<(), String> {
    use cedar_policy::{
        Entities, Entity, EntityId, EntityTypeName, EntityUid, RestrictedExpression,
    };
    use std::collections::HashMap;
    use std::str::FromStr;

    let probe = || -> Result<(), String> {
        let type_name =
            EntityTypeName::from_str("Artifacts::Repository").map_err(|error| error.to_string())?;
        let uid = EntityUid::from_type_name_and_id(type_name, EntityId::new("probe"));
        let attributes = HashMap::from([
            (
                crate::authorization::NAMESPACE_ATTRIBUTE.to_string(),
                RestrictedExpression::new_string("probe".to_string()),
            ),
            (
                crate::authorization::TAGS_ATTRIBUTE.to_string(),
                RestrictedExpression::new_set([RestrictedExpression::new_string(
                    "probe".to_string(),
                )]),
            ),
        ]);
        let entity = Entity::new(uid, attributes, std::collections::HashSet::new())
            .map_err(|error| error.to_string())?;
        Entities::from_entities([entity], Some(schema))
            .map(|_| ())
            .map_err(|error| error.to_string())
    };
    probe().map_err(|reason| {
        format!(
            "the authorization schema must declare the optional attributes `namespace` (String) \
             and `tags` (Set<String>) on Artifacts::Repository: {reason}"
        )
    })
}

#[cfg(test)]
mod scope_tests {
    use super::*;

    fn schema_of(text: &str) -> Schema {
        Schema::from_json_str(text).expect("a schema")
    }

    #[test]
    fn shipped_schemas_declare_the_scope_attributes_and_the_tag_action() {
        for text in [
            include_str!("../config/community/schema.json"),
            include_str!("../config/examples/schema.json"),
        ] {
            let schema = schema_of(text);
            check_scope_attributes_declared(&schema).expect("scope attributes declared");
            check_declared(&schema, &manage_repository_tags()).expect("tag action declared");
            check_declared(&schema, &manage_repository_admission())
                .expect("admission action declared");
            check_declared(&schema, &manage_dependency_freeze())
                .expect("dependency freeze action declared");
            check_declared(&schema, &explore_repository_history())
                .expect("repository history action declared");
            check_declared(&schema, &manage_storage_repair())
                .expect("storage repair action declared");
        }
    }

    #[test]
    fn a_schema_without_the_scope_attributes_is_refused_at_boot() {
        let text = include_str!("../config/community/schema.json")
            .replace("\"namespace\"", "\"unrelated\"");
        let error = check_scope_attributes_declared(&schema_of(&text)).unwrap_err();
        assert!(error.contains("namespace"), "{error}");
    }
}
