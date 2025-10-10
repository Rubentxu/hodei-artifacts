//! Domain models for the IAM bounded context

pub(crate) mod actions;
pub(crate) mod group;
pub(crate) mod user;

#[allow(unused_imports)]
pub(crate) use group::Group;
pub(crate) use user::User;
