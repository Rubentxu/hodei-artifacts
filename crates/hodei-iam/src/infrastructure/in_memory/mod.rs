//! In-memory infrastructure adapters for testing and development
//!
//! TEMPORARY: Deshabilitando todos los adaptadores excepto create_policy_adapter
//! para estabilización incremental de features

// pub mod policy_adapter;
// pub mod user_adapter;
// pub mod group_adapter;
pub mod create_policy_adapter;
// pub mod update_policy_adapter;
// pub mod delete_policy_adapter;
// pub mod get_policy_adapter;
// pub mod list_policies_adapter;

// pub use policy_adapter::InMemoryPolicyAdapter;
// pub use user_adapter::InMemoryUserAdapter;
// pub use group_adapter::InMemoryGroupAdapter;
pub use create_policy_adapter::InMemoryCreatePolicyAdapter;
// pub use update_policy_adapter::InMemoryUpdatePolicyAdapter;
// pub use delete_policy_adapter::InMemoryDeletePolicyAdapter;
// pub use get_policy_adapter::InMemoryPolicyReaderAdapter;
// pub use list_policies_adapter::InMemoryPolicyListerAdapter;
