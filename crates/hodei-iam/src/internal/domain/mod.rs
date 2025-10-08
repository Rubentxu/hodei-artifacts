//! Domain models for the IAM bounded context

pub(crate) mod actions;
pub(crate) mod artifact;
pub(crate) mod group;
pub(crate) mod user;

// pub(crate) use artifact::Artifact; // Temporarily disabled - unused
#[allow(unused_imports)]
pub(crate) use group::Group;
pub(crate) use user::User;
