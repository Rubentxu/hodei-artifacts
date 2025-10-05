# TODO List for Hodei Artifacts Modular Monolith Implementation

## Implementation Tasks

- [x] **Create authorization ports in kernel crate** - Already exists
- [x] **Refactor policies crate to remove CRUD operations and storage implementations**
- [ ] **Implement policy management features in hodei-iam crate**
- [ ] **Implement SCP management features in hodei-organizations crate**
- [ ] **Refactor hodei-authorizer to use new evaluator traits**
- [ ] **Update application state and DI composition**
- [ ] **Create new API handlers for policy management in respective bounded contexts**
- [ ] **Update tests to reflect new architecture**

## Detailed Task Breakdown

### 1. Refactor policies crate
- [x] Remove any storage implementations that exist in policies crate
- [x] Keep only schema-related functionality in engine.rs
- [x] Remove CRUD policy management features if they exist

### 2. Implement policy management in hodei-iam crate
- [ ] Create complete VSA feature structure for create_policy
- [ ] Implement CreatePolicyUseCase with execute() method
- [ ] Implement DeletePolicyUseCase with execute() method
- [ ] Implement UpdatePolicyUseCase with execute() method
- [ ] Implement GetPolicyUseCase with execute() method
- [ ] Implement ListPoliciesUseCase with execute() method
- [ ] Create PolicyRepository for persisting IAM policies
- [ ] Create unit tests for new use cases
- [ ] Create integration tests for new policy management endpoints

### 3. Implement SCP management in hodei-organizations crate
- [ ] Create complete VSA feature structure for create_scp
- [ ] Implement CreateScpUseCase with execute() method
- [ ] Implement DeleteScpUseCase with execute() method
- [ ] Implement UpdateScpUseCase with execute() method
- [ ] Implement GetScpUseCase with execute() method
- [ ] Implement ListScpsUseCase with execute() method
- [ ] Create ScpRepository for persisting SCPs
- [ ] Create unit tests for new use cases
- [ ] Create integration tests for new SCP management endpoints

### 4. Refactor hodei-authorizer crate
- [ ] Update EvaluatePermissionsUseCase to delegate to ScpEvaluator and IamPolicyEvaluator traits
- [ ] Remove direct dependencies on other bounded contexts
- [ ] Simplify authorization logic to orchestrate and delegate
- [ ] Update tests for EvaluatePermissionsUseCase to use new evaluator traits

### 5. Update application state and DI composition
- [ ] Simplify src/app_state.rs to only contain main use cases from each bounded context
- [ ] Update src/lib.rs to wire up new autonomous evaluators
- [ ] Update src/main.rs if needed

### 6. Create new API handlers
- [ ] Create handlers for IAM policy management in src/api/iam.rs
- [ ] Create handlers for SCP management in src/api/organizations.rs
- [ ] Remove old policy_handlers.rs or update it to only contain schema-related functionality
