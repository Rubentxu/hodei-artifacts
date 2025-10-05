# Implementation Plan

## Overview

This plan outlines the comprehensive refactorization of the Hodei Artifacts project to establish a modular monolith architecture with clear bounded contexts. The goal is to transform the current implementation into a clean architecture following Vertical Slice Architecture (VSA) principles, where each crate represents an independent bounded context with its own data and policies. The refactorization will focus on decoupling components, centralizing shared abstractions in the kernel crate, and ensuring each bounded context manages and evaluates its own policies autonomously.

The implementation will follow the user stories documented in docs/historias-usuario.md, which define five epics:
1. Establish fundamental architectural contracts and boundaries
2. Simplify the policies crate to a pure business logic library
3. Transform domains into autonomous evaluators and managers
4. Simplify the authorizer to a pure orchestrator
5. Compose and expose the monolithic application

## Types

The refactorization will centralize domain abstractions in the kernel crate, which will act as the shared kernel. The following types will be moved or defined there:

1. Hrn (Hodei Resource Name) - Already in kernel crate
2. HodeiEntity, HodeiEntityType, Principal, Resource, ActionTrait - Already in kernel crate
3. PolicyStorage and PolicyStorageError traits - Already in kernel crate
4. New authorization-related DTOs in kernel crate:
   - EvaluationRequest and EvaluationDecision for policy evaluation
   - ScpEvaluator and IamPolicyEvaluator traits for delegated evaluation

## Files

The implementation will involve several file modifications across the project:

### New files to be created:
- crates/kernel/src/ports/authorization.rs - Contains DTOs for policy evaluation (EvaluationRequest, EvaluationDecision) and traits (ScpEvaluator, IamPolicyEvaluator)
- crates/hodei-iam/src/features/create_policy/ - Complete VSA feature structure for IAM policy management
- crates/hodei-organizations/src/features/create_scp/ - Complete VSA feature structure for SCP management

### Existing files to be modified:
- crates/policies/src/lib.rs - Remove CRUD use cases and storage implementations
- crates/hodei-iam/src/lib.rs - Make internal modules private, expose only public use cases
- crates/hodei-organizations/src/lib.rs - Make internal modules private, expose only public use cases
- src/app_state.rs - Simplify to only contain main use cases from each bounded context
- src/lib.rs - Update DI composition to wire up new autonomous evaluators

### Files to be deleted:
- crates/policies/src/features/create_policy/ - Entire directory
- crates/policies/src/features/delete_policy/ - Entire directory
- crates/policies/src/features/update_policy/ - Entire directory
- crates/policies/src/features/get_policy/ - Entire directory
- crates/policies/src/features/list_policies/ - Entire directory
- crates/policies/src/shared/application/store.rs - Policy storage implementation
- crates/policies/src/shared/application/engine.rs - Authorization engine (keep only schema-related functionality)
- src/api/policy_handlers.rs - Will be replaced by domain-specific handlers

## Functions

### New functions:
- In kernel crate:
  - ScpEvaluator::evaluate_scps() - Method to evaluate SCPs
  - IamPolicyEvaluator::evaluate_iam_policies() - Method to evaluate IAM policies

### Modified functions:
- In hodei-authorizer crate:
  - EvaluatePermissionsUseCase::evaluate_authorization() - Will delegate to ScpEvaluator and IamPolicyEvaluator traits instead of directly accessing other bounded contexts
  - EvaluatePermissionsUseCase::evaluate_with_policy_set() - Simplified to only handle evaluation with provided policy set

### Removed functions:
- From policies crate:
  - AuthorizationEngine::is_authorized() - Policy evaluation logic moved to bounded contexts
  - All CRUD policy management functions - Moved to respective bounded contexts

## Classes

### New classes:
- In hodei-iam crate:
  - CreatePolicyUseCase - For managing IAM policies
  - DeletePolicyUseCase - For managing IAM policies
  - UpdatePolicyUseCase - For managing IAM policies
  - GetPolicyUseCase - For managing IAM policies
  - ListPoliciesUseCase - For managing IAM policies
  - PolicyRepository - For persisting IAM policies
  - CreatePolicyUseCase::execute() method
  - DeletePolicyUseCase::execute() method
  - UpdatePolicyUseCase::execute() method
  - GetPolicyUseCase::execute() method
  - ListPoliciesUseCase::execute() method

- In hodei-organizations crate:
  - CreateScpUseCase - For managing SCPs
  - DeleteScpUseCase - For managing SCPs
  - UpdateScpUseCase - For managing SCPs
  - GetScpUseCase - For managing SCPs
  - ListScpsUseCase - For managing SCPs
  - ScpRepository - For persisting SCPs
  - CreateScpUseCase::execute() method
  - DeleteScpUseCase::execute() method
  - UpdateScpUseCase::execute() method
  - GetScpUseCase::execute() method
  - ListScpsUseCase::execute() method

### Modified classes:
- EvaluatePermissionsUseCase in hodei-authorizer crate:
  - Remove direct dependencies on other bounded contexts
  - Add dependencies on ScpEvaluator and IamPolicyEvaluator traits
  - Simplify authorization logic to orchestrate and delegate

### Removed classes:
- From policies crate:
  - PolicyStore - Storage implementation moved to bounded contexts
  - All CRUD policy use case classes - Moved to bounded contexts

## Dependencies

### New dependencies:
- Add necessary dependencies to hodei-iam and hodei-organizations crates for policy management

### Modified dependencies:
- Update hodei-authorizer crate to depend on ScpEvaluator and IamPolicyEvaluator traits from kernel
- Remove direct dependencies from hodei-authorizer to hodei-iam and hodei-organizations crates

### Removed dependencies:
- Remove dependencies from policies crate to storage implementations
- Remove dependencies from hodei-iam and hodei-organizations to policies crate for CRUD operations

## Testing

### New tests:
- Create unit tests for new use cases in hodei-iam and hodei-organizations crates
- Create integration tests for new policy management endpoints

### Modified tests:
- Update tests for EvaluatePermissionsUseCase in hodei-authorizer crate to use new evaluator traits

### Removed tests:
- Remove integration tests from policies crate related to CRUD operations
- Remove tests that directly access internal modules of hodei-iam and hodei-organizations

## Implementation Order

The implementation should follow this logical sequence to minimize conflicts and ensure successful integration:

1. Create the new authorization ports in the kernel crate
2. Refactor the policies crate to remove CRUD operations and storage implementations
3. Implement policy management features in hodei-iam crate
4. Implement SCP management features in hodei-organizations crate
5. Refactor hodei-authorizer to use the new evaluator traits
6. Update the application state and DI composition in src/
7. Create new API handlers for policy management in respective bounded contexts
8. Update tests to reflect the new architecture
