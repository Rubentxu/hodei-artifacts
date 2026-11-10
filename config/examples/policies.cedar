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
