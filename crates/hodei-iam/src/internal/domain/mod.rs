//! Domain models for the IAM bounded context

pub(crate) mod user;
pub(crate) mod group;

pub(crate) use user::User;
pub(crate) use group::Group;
