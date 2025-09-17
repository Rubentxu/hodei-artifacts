# 7. Development Process

## 7.1 Workflow for New Feature

1. **Requirement Analysis:**
  - Review PRD and existing implementation
  - Define feature structure and components
  - Create detailed task list

2. **Implementation:**
  - Create feature directory with required files
  - Implement domain models and validation
  - Define ports and adapters
  - Implement use case and API
  - Write unit and integration tests

3. **Validation:**
  - Verify compilation without errors
  - Ensure no warnings with clippy
  - Confirm all tests pass
  - Validate against PRD requirements

## 7.2 Feature Structure Template

```
features/<feature_name>/
├── mod.rs
├── use_case.rs              # Main business logic
├── ports.rs                 # Segregated interfaces for external services
├── adapter.rs               # Concrete implementations of ports
├── dto.rs                   # Data transfer objects
├── error.rs                 # Feature-specific errors
├── event_handler.rs         # Domain event handler
├── di.rs                    # Dependency injection configuration
├── mocks.rs                 # Mocks for testing
├── use_case_test.rs         # Unit tests for use case
└── event_handler_test.rs    # Tests for event handler
```

## 7.3 Mandatory File Structure Rules

* **use_case.rs:** Must contain the main business logic with no external dependencies
* **ports.rs:** Must define segregated interfaces following SOLID principles
* **adapter.rs:** Must implement concrete adapters for the defined ports
* **All tests:** Must be implemented with proper mocks and coverage
