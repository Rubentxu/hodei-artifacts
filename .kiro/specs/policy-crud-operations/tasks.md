# Policy CRUD Operations - Implementation Tasks

## Task Overview

This document outlines the detailed implementation tasks for creating CRUD operations for Cedar policies in the Hodei Artifacts system. Each task follows the project's TDD approach and Vertical Slice Architecture patterns, building upon the Cedar policy engine integration from Story 4.1.

## Implementation Tasks

- [ ] 1. Initialize IAM Crate Foundation for Policy Management
  - Create the basic policy domain model and infrastructure setup
  - Set up proper module organization following VSA patterns
  - Configure dependencies and basic project structure
  - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5_

- [x] 1.1 Create Policy Domain Model
  - Implement `Policy` struct in `src/domain/policy.rs` with all required fields
  - Create `PolicyStatus` enum with Draft, Active, Inactive, Deprecated variants
  - Implement `PolicyMetadata` struct with creation/update tracking
  - Add domain methods for policy lifecycle management (activate, deactivate, update)
  - _Requirements: 1.1, 1.2, 1.3, 1.4_

- [x] 1.2 Configure IAM Crate Dependencies
  - Add required dependencies to `crates/iam/Cargo.toml`: `cedar-policy`, `mongodb`, `axum`, `serde`, `time`
  - Add `thiserror` for error handling and `async-trait` for async traits
  - Add `uuid` for ID generation and `validator` for input validation
  - Verify dependencies compile correctly with `cargo check`
  - _Requirements: 3.5, 8.4_

- [x] 1.3 Create IAM Error Types
  - Implement comprehensive error types in `src/infrastructure/errors.rs`
  - Create `IamError` enum with variants for PolicyNotFound, ValidationFailed, DatabaseError, etc.
  - Use `thiserror` derive macro for proper error implementation
  - Implement proper error message formatting and context
  - _Requirements: 5.1, 5.2, 5.3, 5.4, 8.4_

- [ ] 1.4 Write Unit Tests for Domain Model
  - Create `src/domain/policy_test.rs` for policy domain tests
  - Test policy creation, status transitions, and update operations
  - Test policy validation rules and business logic
  - Test error conditions and edge cases
  - Verify proper serialization/deserialization
  - _Requirements: 7.1, 7.5_

- [ ] 2. Define Application Layer Ports and Interfaces
  - Create the application service traits and request/response types
  - Implement proper abstractions for dependency inversion
  - Set up the foundation for hexagonal architecture
  - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6_

- [x] 2.1 Create Policy Repository Port
  - Define `PolicyRepository` trait in `src/application/ports.rs`
  - Add async methods for create, get_by_id, update, delete, list operations
  - Create `PolicyFilter` and `PolicyList` types for querying
  - Ensure trait is `Send + Sync` for async compatibility
  - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6_

- [ ] 2.2 Create Policy Validator Port
  - Define `PolicyValidator` trait for Cedar syntax validation
  - Create `ValidationResult` and `ValidationError` types
  - Add async `validate_syntax` method signature
  - Design for integration with Cedar policy engine
  - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5_

- [ ] 2.3 Create Event Publisher Port
  - Define `EventPublisher` trait for domain events
  - Add methods for policy lifecycle events (created, updated, deleted, status_changed)
  - Design for integration with event bus system
  - Ensure proper async support
  - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5_

- [ ] 2.4 Write Unit Tests for Ports
  - Create `src/application/ports_test.rs` for port tests
  - Test trait object creation and basic functionality
  - Test request/response type creation and validation
  - Verify error type conversion and display implementations
  - _Requirements: 7.1, 7.5_

- [ ] 3. Implement Cedar Policy Validator
  - Create the concrete Cedar-based implementation of policy validation
  - Implement proper Cedar engine integration for syntax checking
  - Handle validation errors and provide detailed feedback
  - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5_

- [x] 3.1 Create Cedar Validator Implementation
  - Implement `CedarPolicyValidator` in `src/infrastructure/validation/cedar_validator.rs`
  - Implement `PolicyValidator` trait using Cedar's PolicySet parsing
  - Add proper error handling and validation result mapping
  - Ensure async compatibility
  - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5_

- [ ] 3.2 Write Unit Tests for Cedar Validator
  - Create `src/infrastructure/validation/cedar_validator_test.rs`
  - Test validation with valid Cedar policies
  - Test validation with invalid Cedar syntax
  - Test error message formatting and line number reporting
  - Test edge cases and malformed input
  - _Requirements: 7.1, 7.4, 7.5_

- [ ] 4. Implement MongoDB Policy Repository
  - Create the concrete MongoDB-based implementation of policy persistence
  - Implement proper database operations with error handling
  - Add indexing and query optimization
  - _Requirements: 1.2, 2.1, 2.2, 2.3, 2.4, 2.5, 2.6_

- [x] 4.1 Create MongoDB Repository Implementation
  - Implement `MongoPolicyRepository` in `src/infrastructure/repository/policy_repository.rs`
  - Implement all `PolicyRepository` trait methods with MongoDB operations
  - Add proper error handling and connection management
  - Implement filtering, pagination, and sorting
  - _Requirements: 1.2, 2.1, 2.2, 2.3, 2.4, 2.5, 2.6_

- [ ] 4.2 Add Database Indexes and Optimization
  - Create database indexes for frequently queried fields (status, created_by, tags)
  - Implement efficient pagination with proper cursor handling
  - Add query optimization for complex filters
  - Ensure proper connection pooling
  - _Requirements: 1.2, 2.4, 2.6_

- [ ] 4.3 Write Unit Tests for MongoDB Repository
  - Create `src/infrastructure/repository/policy_repository_test.rs`
  - Use testcontainers for MongoDB integration testing
  - Test all CRUD operations with real database
  - Test error scenarios (connection failures, constraint violations)
  - Test concurrent operations and data consistency
  - _Requirements: 7.1, 7.2, 7.5_

- [ ] 5. Implement Policy CRUD Feature Handlers
  - Create the business logic handlers following VSA patterns
  - Add request validation and orchestration logic
  - Integrate with repository and validator ports
  - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6, 4.1, 4.2, 4.3, 4.4, 4.5_

- [x] 5.1 Create Policy Feature Structure
  - Create feature directories: `create_policy`, `get_policy`, `update_policy`, `delete_policy`, `list_policies`
  - Create `command.rs`/`query.rs` files for request/response DTOs
  - Create `handler.rs` files for business logic orchestration
  - Set up proper module exports in `mod.rs` files
  - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6_

- [ ] 5.2 Implement Create Policy Handler
  - Create `CreatePolicyHandler` in `src/features/create_policy/handler.rs`
  - Implement request validation, Cedar syntax validation, and persistence
  - Add proper error handling and event publishing
  - Ensure handler follows dependency inversion principle
  - _Requirements: 2.1, 3.1, 3.2, 3.3, 5.1, 6.1_

- [ ] 5.3 Implement Get Policy Handler
  - Create `GetPolicyHandler` in `src/features/get_policy/handler.rs`
  - Implement policy retrieval by ID with proper error handling
  - Add authorization checks for policy access
  - Handle not found scenarios gracefully
  - _Requirements: 2.3, 5.2, 6.2_

- [ ] 5.4 Implement Update Policy Handler
  - Create `UpdatePolicyHandler` in `src/features/update_policy/handler.rs`
  - Implement policy content updates with validation
  - Add version management and conflict detection
  - Ensure proper event publishing for updates
  - _Requirements: 2.4, 3.1, 3.2, 3.3, 5.1, 6.2_

- [ ] 5.5 Implement Delete Policy Handler
  - Create `DeletePolicyHandler` in `src/features/delete_policy/handler.rs`
  - Implement safe policy deletion with dependency checks
  - Add proper authorization and safety validations
  - Ensure proper event publishing for deletions
  - _Requirements: 2.5, 5.2, 6.4_

- [ ] 5.6 Implement List Policies Handler
  - Create `ListPoliciesHandler` in `src/features/list_policies/handler.rs`
  - Implement filtering, pagination, and sorting
  - Add proper authorization for policy listing
  - Optimize for performance with large policy sets
  - _Requirements: 2.6, 4.2, 4.3_

- [ ] 5.7 Write Unit Tests for Feature Handlers
  - Create handler test files for each feature
  - Test handlers with mocked dependencies (repository, validator, event publisher)
  - Test request validation logic and error scenarios
  - Test successful operation flows and edge cases
  - _Requirements: 7.1, 7.5_

- [ ] 6. Implement REST API Endpoints
  - Create HTTP endpoints for all policy CRUD operations
  - Add proper authentication and authorization middleware
  - Implement request/response serialization
  - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6, 5.3, 8.5_

- [ ] 6.1 Create API Route Definitions
  - Create `src/api/routes.rs` with Axum router configuration
  - Define routes: POST /policies, GET /policies/{id}, PUT /policies/{id}, DELETE /policies/{id}, GET /policies
  - Add proper HTTP method mappings and path parameters
  - Integrate with authentication middleware
  - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6_

- [ ] 6.2 Implement API Handlers
  - Create API handler functions in each feature's `api.rs` file
  - Implement request deserialization and response serialization
  - Add proper HTTP status code mapping for different scenarios
  - Implement error response formatting
  - _Requirements: 5.1, 5.2, 5.3, 5.4, 8.5_

- [ ] 6.3 Add Authentication and Authorization Middleware
  - Create `src/api/middleware.rs` for JWT token validation
  - Implement role-based access control for policy operations
  - Add request logging and correlation ID generation
  - Ensure proper security headers and CORS handling
  - _Requirements: 5.3, 8.1, 8.2_

- [ ] 6.4 Write API Integration Tests
  - Create API test files for each endpoint
  - Test complete HTTP request/response cycles
  - Test authentication and authorization scenarios
  - Test error responses and edge cases
  - _Requirements: 7.2, 7.3, 7.4_

- [ ] 7. Implement Comprehensive Integration Tests
  - Create end-to-end tests for the complete policy CRUD flow
  - Test integration between all components
  - Verify proper error handling and edge cases
  - _Requirements: 7.1, 7.2, 7.3, 7.4, 7.5_

- [ ] 7.1 Create Integration Test Structure
  - Create `tests/it_policy_crud.rs` for integration tests
  - Set up test fixtures and helper functions
  - Create test data factories for policies and users
  - Set up proper test environment and cleanup
  - _Requirements: 7.2_

- [ ] 7.2 Implement Complete CRUD Lifecycle Tests
  - Create tests that perform full Create → Read → Update → Delete cycles
  - Test policy status transitions and lifecycle management
  - Verify data consistency across operations
  - Test concurrent operations and race conditions
  - _Requirements: 7.2, 7.3_

- [ ] 7.3 Implement Validation and Error Scenario Tests
  - Test invalid Cedar syntax validation
  - Test policy not found scenarios
  - Test authorization failures and access control
  - Test database connection failures and recovery
  - _Requirements: 7.4, 7.5_

- [ ] 7.4 Implement Performance and Load Tests
  - Test policy operations under load
  - Verify pagination performance with large datasets
  - Test concurrent policy modifications
  - Measure and validate response times
  - _Requirements: 7.5_

- [x] 8. Ensure Code Quality and Standards Compliance
  - Run all code quality checks and ensure compliance with project standards
  - Verify architecture alignment and documentation
  - Perform final validation and cleanup
  - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5, 8.6_

- [ ] 8.1 Run Code Formatting and Linting
  - Execute `cargo fmt` on the IAM crate
  - Run `cargo clippy -- -D warnings` and fix all warnings
  - Ensure all code follows project naming conventions
  - Verify proper use of `snake_case`, `PascalCase`, and `SCREAMING_SNAKE_CASE`
  - _Requirements: 8.1, 8.2, 8.4_

- [ ] 8.2 Verify Architecture Compliance
  - Review module structure against VSA patterns
  - Ensure proper separation of domain, application, and infrastructure layers
  - Verify dependency inversion is correctly implemented
  - Check that no domain layer has async/await or external dependencies
  - _Requirements: 8.3, 8.6_

- [ ] 8.3 Validate Error Handling Standards
  - Ensure all error types use `thiserror` derive macro
  - Verify no use of `.unwrap()` or `.expect()` in production code
  - Check that all fallible operations return `Result` types
  - Validate proper error propagation and context
  - _Requirements: 8.4_

- [ ] 8.4 Validate API Standards Compliance
  - Ensure all endpoints follow RESTful conventions
  - Verify proper HTTP status codes for all scenarios
  - Check request/response format consistency
  - Validate API documentation and examples
  - _Requirements: 8.5_

- [ ] 8.5 Run Complete Test Suite
  - Execute `cargo test --lib` for unit tests
  - Run `cargo test --test 'it_*'` for integration tests
  - Verify test coverage meets >90% requirement for core business logic
  - Ensure all tests pass consistently
  - _Requirements: 7.5_

- [ ] 8.6 Final Documentation and Cleanup
  - Add proper documentation comments to public APIs
  - Update module exports and public interfaces
  - Create usage examples and API documentation
  - Remove any temporary or debug code
  - _Requirements: 8.1, 8.2, 8.3_

## Dependencies and Sequencing

### Critical Path
1. Tasks 1.1-1.4 must be completed before any other work can begin
2. Task 2.1 (Policy Repository Port) must be completed before 4.1 (MongoDB implementation)
3. Task 2.2 (Policy Validator Port) must be completed before 3.1 (Cedar validator implementation)
4. Tasks 3.1-3.2 and 4.1-4.3 must be completed before feature handlers (5.1-5.7)
5. Tasks 5.1-5.7 must be completed before API endpoints (6.1-6.4)
6. All implementation tasks must be completed before quality checks (8.1-8.6)

### Parallel Work Opportunities
- Tasks 1.4, 2.4, 3.2, 4.3, 5.7 (unit tests) can be written in parallel with implementation
- Tasks 3.1-3.2 (Cedar validator) and 4.1-4.3 (MongoDB repository) can be developed in parallel
- API tests (6.4) can be prepared while API implementation is in progress
- Integration tests (7.1-7.4) can be developed alongside feature implementation

### Risk Mitigation
- Start with simple policy CRUD operations to validate MongoDB integration
- Implement comprehensive error handling early to catch issues
- Write tests incrementally to catch regressions quickly
- Use TDD approach to ensure requirements are met
- Test Cedar validation thoroughly with various policy examples

## Integration Points

### With Security Crate
- Policy changes should trigger cache invalidation in security crate
- Active policies should be loaded by authorization service
- Policy status changes should be communicated via events

### With Repository Crate
- Use existing MongoDB connection and infrastructure
- Follow established patterns for data access
- Reuse common repository utilities and helpers

### With Shared Crate
- Use common error types and utilities
- Follow established HRN patterns for policy IDs
- Reuse common validation and serialization patterns