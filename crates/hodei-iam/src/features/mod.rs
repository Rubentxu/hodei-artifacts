/// Features module for hodei-iam
///
/// This module contains all the use cases (features) organized as vertical slices.
/// Each feature is self-contained with its own:
/// - Use case (business logic)
/// - DTOs (data transfer objects)
/// - Ports (interface definitions)
/// - Adapters (infrastructure implementations)
/// - Tests (unit and integration)
pub mod add_user_to_group;
pub mod create_group;
pub mod create_policy_new;
pub mod create_user;
pub mod delete_policy;
pub mod evaluate_iam_policies;
pub mod get_effective_policies_for_principal;
pub mod get_policy;
pub mod list_policies;
pub mod update_policy;
