// The policy of the community lab.
//
// It says the minimum a tutorial needs and nothing more: the principal of
// the lab may create and read the repositories of **its own** account, and
// publish and download content in them. It may not promote anything to the
// trusted channel, it may not reach the administrative surface, and it may
// not touch another account: the account is inside the principal itself, so
// a principal of another account does not match and the decision is a
// denial, not a permission.
//
// The two placeholders are replaced by `scripts/community-quickstart.sh`
// when it writes the policy of the lab. The schema of the bundle is
// `config/community/schema.json`; it declares the actions the service
// authorizes with, and a bundle that does not declare them stops the
// service from starting.
//
// The action `ReadCapsule` is declared in the schema because the service
// always builds it, and it is not granted here on purpose: reading the
// trust capsule is not part of a first publish and pull.

permit(
    principal == Iam::User::"hrn:hodei:iam::__ACCOUNT__:User/__USER__",
    action == Artifacts::Action::"CreateRepository",
    resource
);

permit(
    principal == Iam::User::"hrn:hodei:iam::__ACCOUNT__:User/__USER__",
    action == Artifacts::Action::"ReadRepository",
    resource
);

permit(
    principal == Iam::User::"hrn:hodei:iam::__ACCOUNT__:User/__USER__",
    action == Artifacts::Action::"UploadAsset",
    resource
);

permit(
    principal == Iam::User::"hrn:hodei:iam::__ACCOUNT__:User/__USER__",
    action == Artifacts::Action::"DownloadAsset",
    resource
);
