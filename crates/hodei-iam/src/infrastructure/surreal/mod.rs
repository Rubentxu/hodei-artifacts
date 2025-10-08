//! SurrealDB infrastructure module

pub mod group_adapter;
pub mod policy_adapter;
pub mod user_adapter;

pub use group_adapter::SurrealGroupAdapter;
pub use policy_adapter::SurrealPolicyAdapter;
pub use user_adapter::SurrealUserAdapter;
