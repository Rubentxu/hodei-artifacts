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
        read_capsule(),
        access_admin_api(),
        upload_asset(),
        download_asset(),
        promote_asset(),
        revoke_promotion(),
        investigate(),
    ]
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
