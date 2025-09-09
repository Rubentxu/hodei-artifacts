# Policy CRUD Operations - Design Document

## Overview

This document outlines the technical design for implementing CRUD operations for Cedar policies in the Hodei Artifacts system. The design extends the existing IAM crate with policy management capabilities, following the project's Modular Monolith architecture with Vertical Slice Architecture (VSA) and Hexagonal Architecture patterns.

## Architecture

### High-Level Architecture

The policy CRUD functionality will be implemented primarily in the `iam` crate, with integration points to the `security` crate for validation and the `repository` crate for data persistence.

```
crates/iam/
├── src/
│   ├── domain/
│   │   ├── policy.rs                    # Policy domain model
│   │   └── policy_test.rs               # Unit tests for policy domain
│   ├── application/
│   │   ├── ports.rs                     # Policy repository traits
│   │   └── ports_test.rs                # Unit tests for ports
│   ├── features/
│   │   ├── create_policy/
│   │   │   ├── command.rs               # Create policy DTOs
│   │   │   ├── handler.rs               # Create policy business logic
│   │   │   ├── handler_test.rs          # Unit tests
│   │   │   └── api.rs                   # HTTP endpoint
│   │   ├── get_policy/
│   │   │   ├── query.rs                 # Get policy DTOs
│   │   │   ├── handler.rs               # Get policy business logic
│   │   │   ├── handler_test.rs          # Unit tests
│   │   │   └── api.rs                   # HTTP endpoint
│   │   ├── update_policy/
│   │   │   ├── command.rs               # Update policy DTOs
│   │   │   ├── handler.rs               # Update policy business logic
│   │   │   ├── handler_test.rs          # Unit tests
│   │   │   └── api.rs                   # HTTP endpoint
│   │   ├── delete_policy/
│   │   │   ├── command.rs               # Delete policy DTOs
│   │   │   ├── handler.rs               # Delete policy business logic
│   │   │   ├── handler_test.rs          # Unit tests
│   │   │   └── api.rs                   # HTTP endpoint
│   │   └── list_policies/
│   │       ├── query.rs                 # List policies DTOs
│   │       ├── handler.rs               # List policies business logic
│   │       ├── handler_test.rs          # Unit tests
│   │       └── api.rs                   # HTTP endpoint
│   ├── infrastructure/
│   │   ├── repository/
│   │   │   ├── policy_repository.rs     # MongoDB policy repository
│   │   │   └── policy_repository_test.rs # Unit tests
│   │   ├── validation/
│   │   │   ├── cedar_validator.rs       # Cedar syntax validation
│   │   │   └── cedar_validator_test.rs  # Unit tests
│   │   └── errors.rs                    # IAM-specific error types
│   └── api/
│       ├── routes.rs                    # API route definitions
│       └── middleware.rs                # Authentication/authorization middleware
└── tests/
    └── it_policy_crud.rs                # Integration tests
```

### Component Responsibilities

#### Domain Layer (`domain/`)
- **Policy Model**: Core policy domain type with validation rules
- **Policy Status**: Enum for policy lifecycle states (Draft, Active, Inactive, Deprecated)
- **Policy Events**: Domain events for policy lifecycle changes
- **Business Rules**: Policy-specific business logic and constraints

#### Application Layer (`application/`)
- **Policy Repository Port**: Trait defining policy persistence operations
- **Policy Validator Port**: Trait for Cedar syntax validation
- **Event Publisher Port**: Trait for publishing policy-related events

#### Features Layer (`features/`)
- **Create Policy**: Handle policy creation with validation
- **Get Policy**: Retrieve individual policies by ID
- **List Policies**: Paginated policy listing with filtering
- **Update Policy**: Policy modification with validation
- **Delete Policy**: Policy removal with safety checks

#### Infrastructure Layer (`infrastructure/`)
- **MongoDB Repository**: Concrete implementation of policy persistence
- **Cedar Validator**: Integration with Cedar engine for syntax validation
- **Error Types**: IAM-specific error handling

## Components and Interfaces

### Core Domain Types

```rust
// domain/policy.rs
use serde::{Deserialize, Serialize};
use shared::hrn::PolicyId;
use time::OffsetDateTime;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Policy {
    pub id: PolicyId,
    pub name: String,
    pub description: Option<String>,
    pub content: String, // Cedar DSL content
    pub status: PolicyStatus,
    pub metadata: PolicyMetadata,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PolicyStatus {
    Draft,
    Active,
    Inactive,
    Deprecated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PolicyMetadata {
    pub created_at: OffsetDateTime,
    pub created_by: String, // User ID
    pub updated_at: OffsetDateTime,
    pub updated_by: String, // User ID
    pub version: u32,
    pub tags: Vec<String>,
}

impl Policy {
    pub fn new(
        id: PolicyId,
        name: String,
        content: String,
        created_by: String,
    ) -> Self {
        let now = OffsetDateTime::now_utc();
        Self {
            id,
            name,
            description: None,
            content,
            status: PolicyStatus::Draft,
            metadata: PolicyMetadata {
                created_at: now,
                created_by: created_by.clone(),
                updated_at: now,
                updated_by: created_by,
                version: 1,
                tags: Vec::new(),
            },
        }
    }

    pub fn update_content(&mut self, content: String, updated_by: String) {
        self.content = content;
        self.metadata.updated_at = OffsetDateTime::now_utc();
        self.metadata.updated_by = updated_by;
        self.metadata.version += 1;
    }

    pub fn activate(&mut self, updated_by: String) -> Result<(), PolicyError> {
        match self.status {
            PolicyStatus::Draft | PolicyStatus::Inactive => {
                self.status = PolicyStatus::Active;
                self.metadata.updated_at = OffsetDateTime::now_utc();
                self.metadata.updated_by = updated_by;
                Ok(())
            }
            _ => Err(PolicyError::InvalidStatusTransition {
                from: self.status.clone(),
                to: PolicyStatus::Active,
            }),
        }
    }
}
```

### Application Ports

```rust
// application/ports.rs
use crate::domain::policy::{Policy, PolicyStatus};
use crate::infrastructure::errors::IamError;
use async_trait::async_trait;
use shared::hrn::PolicyId;

#[async_trait]
pub trait PolicyRepository: Send + Sync {
    async fn create(&self, policy: Policy) -> Result<Policy, IamError>;
    async fn get_by_id(&self, id: &PolicyId) -> Result<Option<Policy>, IamError>;
    async fn update(&self, policy: Policy) -> Result<Policy, IamError>;
    async fn delete(&self, id: &PolicyId) -> Result<(), IamError>;
    async fn list(&self, filter: PolicyFilter) -> Result<PolicyList, IamError>;
    async fn list_by_status(&self, status: PolicyStatus) -> Result<Vec<Policy>, IamError>;
}

#[async_trait]
pub trait PolicyValidator: Send + Sync {
    async fn validate_syntax(&self, content: &str) -> Result<ValidationResult, IamError>;
}

#[async_trait]
pub trait EventPublisher: Send + Sync {
    async fn publish_policy_created(&self, policy: &Policy) -> Result<(), IamError>;
    async fn publish_policy_updated(&self, policy: &Policy) -> Result<(), IamError>;
    async fn publish_policy_deleted(&self, policy_id: &PolicyId) -> Result<(), IamError>;
    async fn publish_policy_status_changed(&self, policy: &Policy, old_status: PolicyStatus) -> Result<(), IamError>;
}

#[derive(Debug, Clone)]
pub struct PolicyFilter {
    pub status: Option<PolicyStatus>,
    pub name_contains: Option<String>,
    pub tags: Vec<String>,
    pub created_by: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct PolicyList {
    pub policies: Vec<Policy>,
    pub total_count: u64,
    pub has_more: bool,
}

#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
}

#[derive(Debug, Clone)]
pub struct ValidationError {
    pub message: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
}
```

### Feature Handlers

```rust
// features/create_policy/handler.rs
use crate::application::ports::{PolicyRepository, PolicyValidator, EventPublisher};
use crate::domain::policy::Policy;
use crate::infrastructure::errors::IamError;
use crate::features::create_policy::command::{CreatePolicyCommand, CreatePolicyResponse};
use shared::hrn::PolicyId;
use std::sync::Arc;

pub struct CreatePolicyHandler {
    repository: Arc<dyn PolicyRepository>,
    validator: Arc<dyn PolicyValidator>,
    event_publisher: Arc<dyn EventPublisher>,
}

impl CreatePolicyHandler {
    pub fn new(
        repository: Arc<dyn PolicyRepository>,
        validator: Arc<dyn PolicyValidator>,
        event_publisher: Arc<dyn EventPublisher>,
    ) -> Self {
        Self {
            repository,
            validator,
            event_publisher,
        }
    }

    pub async fn handle(&self, command: CreatePolicyCommand) -> Result<CreatePolicyResponse, IamError> {
        // Validate command
        self.validate_command(&command)?;

        // Validate Cedar syntax
        let validation_result = self.validator.validate_syntax(&command.content).await?;
        if !validation_result.is_valid {
            return Err(IamError::PolicyValidationFailed {
                errors: validation_result.errors,
            });
        }

        // Generate policy ID
        let policy_id = PolicyId::new();

        // Create policy domain object
        let policy = Policy::new(
            policy_id,
            command.name,
            command.content,
            command.created_by,
        );

        // Persist policy
        let created_policy = self.repository.create(policy).await?;

        // Publish domain event
        self.event_publisher.publish_policy_created(&created_policy).await?;

        Ok(CreatePolicyResponse {
            id: created_policy.id,
            name: created_policy.name,
            status: created_policy.status,
            created_at: created_policy.metadata.created_at,
        })
    }

    fn validate_command(&self, command: &CreatePolicyCommand) -> Result<(), IamError> {
        if command.name.trim().is_empty() {
            return Err(IamError::InvalidInput("Policy name cannot be empty".to_string()));
        }

        if command.content.trim().is_empty() {
            return Err(IamError::InvalidInput("Policy content cannot be empty".to_string()));
        }

        if command.name.len() > 255 {
            return Err(IamError::InvalidInput("Policy name too long (max 255 characters)".to_string()));
        }

        Ok(())
    }
}
```

### MongoDB Repository Implementation

```rust
// infrastructure/repository/policy_repository.rs
use crate::application::ports::{PolicyRepository, PolicyFilter, PolicyList};
use crate::domain::policy::{Policy, PolicyStatus};
use crate::infrastructure::errors::IamError;
use async_trait::async_trait;
use mongodb::{Collection, Database, bson::doc};
use shared::hrn::PolicyId;
use std::sync::Arc;

pub struct MongoPolicyRepository {
    collection: Collection<Policy>,
}

impl MongoPolicyRepository {
    pub fn new(database: Arc<Database>) -> Self {
        Self {
            collection: database.collection("policies"),
        }
    }
}

#[async_trait]
impl PolicyRepository for MongoPolicyRepository {
    async fn create(&self, policy: Policy) -> Result<Policy, IamError> {
        self.collection
            .insert_one(&policy, None)
            .await
            .map_err(|e| IamError::DatabaseError(e.to_string()))?;

        Ok(policy)
    }

    async fn get_by_id(&self, id: &PolicyId) -> Result<Option<Policy>, IamError> {
        let filter = doc! { "_id": id.to_string() };
        
        self.collection
            .find_one(filter, None)
            .await
            .map_err(|e| IamError::DatabaseError(e.to_string()))
    }

    async fn update(&self, policy: Policy) -> Result<Policy, IamError> {
        let filter = doc! { "_id": policy.id.to_string() };
        
        let result = self.collection
            .replace_one(filter, &policy, None)
            .await
            .map_err(|e| IamError::DatabaseError(e.to_string()))?;

        if result.matched_count == 0 {
            return Err(IamError::PolicyNotFound(policy.id));
        }

        Ok(policy)
    }

    async fn delete(&self, id: &PolicyId) -> Result<(), IamError> {
        let filter = doc! { "_id": id.to_string() };
        
        let result = self.collection
            .delete_one(filter, None)
            .await
            .map_err(|e| IamError::DatabaseError(e.to_string()))?;

        if result.deleted_count == 0 {
            return Err(IamError::PolicyNotFound(id.clone()));
        }

        Ok(())
    }

    async fn list(&self, filter: PolicyFilter) -> Result<PolicyList, IamError> {
        let mut query = doc! {};

        // Apply filters
        if let Some(status) = filter.status {
            query.insert("status", status.to_string());
        }

        if let Some(name_contains) = filter.name_contains {
            query.insert("name", doc! { "$regex": name_contains, "$options": "i" });
        }

        if !filter.tags.is_empty() {
            query.insert("metadata.tags", doc! { "$in": filter.tags });
        }

        if let Some(created_by) = filter.created_by {
            query.insert("metadata.created_by", created_by);
        }

        // Count total documents
        let total_count = self.collection
            .count_documents(query.clone(), None)
            .await
            .map_err(|e| IamError::DatabaseError(e.to_string()))?;

        // Apply pagination
        let mut find_options = mongodb::options::FindOptions::default();
        if let Some(limit) = filter.limit {
            find_options.limit = Some(limit as i64);
        }
        if let Some(offset) = filter.offset {
            find_options.skip = Some(offset as u64);
        }

        // Execute query
        let mut cursor = self.collection
            .find(query, find_options)
            .await
            .map_err(|e| IamError::DatabaseError(e.to_string()))?;

        let mut policies = Vec::new();
        while cursor.advance().await.map_err(|e| IamError::DatabaseError(e.to_string()))? {
            let policy = cursor.deserialize_current()
                .map_err(|e| IamError::DatabaseError(e.to_string()))?;
            policies.push(policy);
        }

        let has_more = filter.offset.unwrap_or(0) + policies.len() as u32 < total_count as u32;

        Ok(PolicyList {
            policies,
            total_count,
            has_more,
        })
    }

    async fn list_by_status(&self, status: PolicyStatus) -> Result<Vec<Policy>, IamError> {
        let filter = doc! { "status": status.to_string() };
        
        let mut cursor = self.collection
            .find(filter, None)
            .await
            .map_err(|e| IamError::DatabaseError(e.to_string()))?;

        let mut policies = Vec::new();
        while cursor.advance().await.map_err(|e| IamError::DatabaseError(e.to_string()))? {
            let policy = cursor.deserialize_current()
                .map_err(|e| IamError::DatabaseError(e.to_string()))?;
            policies.push(policy);
        }

        Ok(policies)
    }
}
```

### Cedar Validator Implementation

```rust
// infrastructure/validation/cedar_validator.rs
use crate::application::ports::{PolicyValidator, ValidationResult, ValidationError};
use crate::infrastructure::errors::IamError;
use async_trait::async_trait;
use cedar_policy::PolicySet;
use std::str::FromStr;

pub struct CedarPolicyValidator;

impl CedarPolicyValidator {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl PolicyValidator for CedarPolicyValidator {
    async fn validate_syntax(&self, content: &str) -> Result<ValidationResult, IamError> {
        match PolicySet::from_str(content) {
            Ok(_) => Ok(ValidationResult {
                is_valid: true,
                errors: Vec::new(),
            }),
            Err(e) => {
                let error_message = e.to_string();
                let validation_error = ValidationError {
                    message: error_message,
                    line: None, // Cedar doesn't provide line numbers in basic validation
                    column: None,
                };

                Ok(ValidationResult {
                    is_valid: false,
                    errors: vec![validation_error],
                })
            }
        }
    }
}
```

## Data Models

### Policy Document Structure (MongoDB)

```json
{
  "_id": "hrn:hodei:iam:global:org_123:policy/policy_456",
  "name": "Engineering Team Repository Access",
  "description": "Allows engineering team members to access private repositories",
  "content": "permit(\n  principal,\n  action == Action::\"read\",\n  resource\n)\nwhen {\n  principal.department == \"engineering\"\n};",
  "status": "Active",
  "metadata": {
    "created_at": "2024-12-19T10:30:00Z",
    "created_by": "hrn:hodei:iam:global:org_123:user/admin_123",
    "updated_at": "2024-12-19T15:45:00Z",
    "updated_by": "hrn:hodei:iam:global:org_123:user/admin_123",
    "version": 2,
    "tags": ["engineering", "repository-access"]
  }
}
```

### API Request/Response Models

```rust
// features/create_policy/command.rs
#[derive(Debug, Deserialize)]
pub struct CreatePolicyCommand {
    pub name: String,
    pub description: Option<String>,
    pub content: String,
    pub tags: Vec<String>,
    pub created_by: String, // Will be extracted from JWT token
}

#[derive(Debug, Serialize)]
pub struct CreatePolicyResponse {
    pub id: PolicyId,
    pub name: String,
    pub status: PolicyStatus,
    pub created_at: OffsetDateTime,
}

// features/get_policy/query.rs
#[derive(Debug, Serialize)]
pub struct GetPolicyResponse {
    pub id: PolicyId,
    pub name: String,
    pub description: Option<String>,
    pub content: String,
    pub status: PolicyStatus,
    pub metadata: PolicyMetadata,
}

// features/list_policies/query.rs
#[derive(Debug, Deserialize)]
pub struct ListPoliciesQuery {
    pub status: Option<PolicyStatus>,
    pub name_contains: Option<String>,
    pub tags: Option<String>, // Comma-separated
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct ListPoliciesResponse {
    pub policies: Vec<PolicySummary>,
    pub total_count: u64,
    pub has_more: bool,
}

#[derive(Debug, Serialize)]
pub struct PolicySummary {
    pub id: PolicyId,
    pub name: String,
    pub description: Option<String>,
    pub status: PolicyStatus,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}
```

## Error Handling Strategy

### Custom Error Types

```rust
// infrastructure/errors.rs
use thiserror::Error;
use shared::hrn::PolicyId;
use crate::domain::policy::PolicyStatus;
use crate::application::ports::ValidationError;

#[derive(Error, Debug)]
pub enum IamError {
    #[error("Policy not found: {0}")]
    PolicyNotFound(PolicyId),
    
    #[error("Policy validation failed")]
    PolicyValidationFailed { errors: Vec<ValidationError> },
    
    #[error("Invalid policy status transition from {from:?} to {to:?}")]
    InvalidStatusTransition { from: PolicyStatus, to: PolicyStatus },
    
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    
    #[error("Database error: {0}")]
    DatabaseError(String),
    
    #[error("Authorization error: {0}")]
    AuthorizationError(String),
    
    #[error("Configuration error: {0}")]
    ConfigurationError(String),
}
```

## Testing Strategy

### Unit Tests
- **Domain Models**: Test policy creation, updates, status transitions
- **Handlers**: Test business logic with mocked dependencies
- **Repository**: Test MongoDB operations with test containers
- **Validator**: Test Cedar syntax validation with various policy examples

### Integration Tests
- **Complete CRUD Flow**: Create → Read → Update → Delete policy lifecycle
- **Validation Scenarios**: Valid and invalid Cedar syntax
- **Error Handling**: Database failures, validation errors, not found scenarios
- **Concurrent Operations**: Multiple policy operations simultaneously

### API Tests
- **HTTP Endpoints**: All CRUD operations via REST API
- **Authentication**: Proper JWT token validation
- **Authorization**: Role-based access to policy operations
- **Error Responses**: Proper HTTP status codes and error messages

## Security Considerations

### Access Control
- Policy CRUD operations require appropriate permissions
- Only authorized users can create/modify policies
- Policy content is validated before storage
- Audit logging for all policy changes

### Data Validation
- Strict input validation for all API endpoints
- Cedar syntax validation using official parser
- SQL injection prevention through parameterized queries
- XSS prevention through proper input sanitization

## Performance Considerations

### Database Optimization
- Indexes on frequently queried fields (status, created_by, tags)
- Pagination for large policy lists
- Connection pooling for MongoDB operations
- Query optimization for complex filters

### Caching Strategy
- Cache frequently accessed policies
- Invalidate cache on policy updates
- Use Redis for distributed caching
- Cache validation results for identical content

## Implementation Phases

### Phase 1: Core CRUD Operations
- Implement basic policy domain model
- Create MongoDB repository
- Implement create, read, update, delete handlers
- Add Cedar syntax validation

### Phase 2: Advanced Features
- Add policy status management
- Implement filtering and pagination
- Add comprehensive error handling
- Create integration tests

### Phase 3: API and Integration
- Implement REST API endpoints
- Add authentication and authorization
- Integrate with security crate for policy refresh
- Add comprehensive logging and monitoring