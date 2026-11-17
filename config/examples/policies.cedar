// Example only: replace the account/principal with your deployment values.
permit(
    principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
    action == Artifacts::Action::"CreateRepository",
    resource
);
permit(
    principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
    action == Artifacts::Action::"ReadRepository",
    resource
);
permit(
    principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
    action == Admin::Action::"AccessAdminApi",
    resource
);
permit(
    principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
    action == Artifacts::Action::"ReadCapsule",
    resource
);
permit(
    principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
    action == Artifacts::Action::"Investigate",
    resource
);
permit(
    principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
    action == Artifacts::Action::"UploadAsset",
    resource
);
permit(
    principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
    action == Artifacts::Action::"DownloadAsset",
    resource
);
permit(
    principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
    action == Artifacts::Action::"PromoteAsset",
    resource
);
permit(
    principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
    action == Artifacts::Action::"RevokePromotion",
    resource
);
