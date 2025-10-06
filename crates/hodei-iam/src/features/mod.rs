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
// TODO: REFACTOR (Phase 2) - create_policy is temporarily disabled
// This monolithic feature will be split into: create_policy, delete_policy,
// update_policy, get_policy, list_policies (each with segregated ISP ports)
// pub mod create_policy;
pub mod create_user;
pub mod evaluate_iam_policies;
pub mod get_effective_policies_for_principal;
