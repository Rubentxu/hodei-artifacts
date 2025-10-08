# Hodei Artifacts - Policy Playground Implementation Plan

**Version:** 1.0  
**Date:** 2024  
**Status:** Implementation Plan

---

## Table of Contents

1. [Current State Analysis](#current-state-analysis)
2. [Missing IAM Actions](#missing-iam-actions)
3. [Playground Feature Design](#playground-feature-design)
4. [REST API Endpoints](#rest-api-endpoints)
5. [Implementation Steps](#implementation-steps)
6. [Testing Strategy](#testing-strategy)

---

## 1. Current State Analysis

### 1.1 Existing Entities

**User** (`hodei-iam/src/internal/domain/user.rs`)
- Implements: `HodeiEntity`, `Principal`, `Resource`
- Attributes: `name`, `email`, `tags`
- Parents: `group_hrns` (users belong to groups)
- Service: `iam`
- Type: `User`

**Group** (`hodei-iam/src/internal/domain/group.rs`)
- Implements: `HodeiEntity`, `Resource` (NOT Principal)
- Attributes: `name`, `description`, `tags`
- Parents: None
- Service: `iam`
- Type: `Group`

### 1.2 Existing Actions

Currently implemented in `hodei-iam/src/internal/domain/actions.rs`:

1. `CreateUserAction` - Create a new user
2. `CreateGroupAction` - Create a new group
3. `DeleteUserAction` - Delete a user
4. `DeleteGroupAction` - Delete a group
5. `AddUserToGroupAction` - Add user to group
6. `RemoveUserFromGroupAction` - Remove user from group

### 1.3 Existing Features in hodei-iam

✅ Already Implemented:
- `create_user` - Creates a new user
- `create_group` - Creates a new group
- `add_user_to_group` - Adds user to group
- `create_policy` - Creates a Cedar policy
- `get_policy` - Retrieves a policy
- `list_policies` - Lists policies
- `update_policy` - Updates a policy
- `delete_policy` - Deletes a policy
- `evaluate_iam_policies` - Evaluates policies
- `get_effective_policies` - Gets effective policies for a user
- `register_iam_schema` - Registers IAM schema with Cedar

### 1.4 Existing Features in hodei-policies

✅ Already Implemented:
- `register_entity_type` - Registers entity types generically
- `register_action_type` - Registers action types generically
- `build_schema` - Builds and persists Cedar schema
- `load_schema` - Loads persisted schema
- `validate_policy` - Validates Cedar policy syntax
- `evaluate_policies` - Evaluates policies against entities

---

## 2. Missing IAM Actions

To have a complete IAM system, we need these additional actions:

### 2.1 User Management Actions

```rust
// GetUser - View user details
pub struct GetUserAction;
impl ActionTrait for GetUserAction {
    fn name() -> &'static str { "GetUser" }
    fn service_name() -> ServiceName { ServiceName::new("iam").expect("Valid") }
    fn applies_to_principal() -> String { "Iam::User".to_string() }
    fn applies_to_resource() -> String { "Iam::User".to_string() }
}

// ListUsers - List all users
pub struct ListUsersAction;
impl ActionTrait for ListUsersAction {
    fn name() -> &'static str { "ListUsers" }
    fn service_name() -> ServiceName { ServiceName::new("iam").expect("Valid") }
    fn applies_to_principal() -> String { "Iam::User".to_string() }
    fn applies_to_resource() -> String { "Iam::User".to_string() }
}

// UpdateUser - Update user attributes
pub struct UpdateUserAction;
impl ActionTrait for UpdateUserAction {
    fn name() -> &'static str { "UpdateUser" }
    fn service_name() -> ServiceName { ServiceName::new("iam").expect("Valid") }
    fn applies_to_principal() -> String { "Iam::User".to_string() }
    fn applies_to_resource() -> String { "Iam::User".to_string() }
}
```

### 2.2 Group Management Actions

```rust
// GetGroup - View group details
pub struct GetGroupAction;
impl ActionTrait for GetGroupAction {
    fn name() -> &'static str { "GetGroup" }
    fn service_name() -> ServiceName { ServiceName::new("iam").expect("Valid") }
    fn applies_to_principal() -> String { "Iam::User".to_string() }
    fn applies_to_resource() -> String { "Iam::Group".to_string() }
}

// ListGroups - List all groups
pub struct ListGroupsAction;
impl ActionTrait for ListGroupsAction {
    fn name() -> &'static str { "ListGroups" }
    fn service_name() -> ServiceName { ServiceName::new("iam").expect("Valid") }
    fn applies_to_principal() -> String { "Iam::User".to_string() }
    fn applies_to_resource() -> String { "Iam::Group".to_string() }
}

// UpdateGroup - Update group attributes
pub struct UpdateGroupAction;
impl ActionTrait for UpdateGroupAction {
    fn name() -> &'static str { "UpdateGroup" }
    fn service_name() -> ServiceName { ServiceName::new("iam").expect("Valid") }
    fn applies_to_principal() -> String { "Iam::User".to_string() }
    fn applies_to_resource() -> String { "Iam::Group".to_string() }
}

// ListGroupMembers - List members of a group
pub struct ListGroupMembersAction;
impl ActionTrait for ListGroupMembersAction {
    fn name() -> &'static str { "ListGroupMembers" }
    fn service_name() -> ServiceName { ServiceName::new("iam").expect("Valid") }
    fn applies_to_principal() -> String { "Iam::User".to_string() }
    fn applies_to_resource() -> String { "Iam::Group".to_string() }
}
```

### 2.3 Policy Management Actions

```rust
// AttachPolicy - Attach policy to user/group
pub struct AttachPolicyAction;
impl ActionTrait for AttachPolicyAction {
    fn name() -> &'static str { "AttachPolicy" }
    fn service_name() -> ServiceName { ServiceName::new("iam").expect("Valid") }
    fn applies_to_principal() -> String { "Iam::User".to_string() }
    fn applies_to_resource() -> String { "Iam::User,Iam::Group".to_string() }
}

// DetachPolicy - Detach policy from user/group
pub struct DetachPolicyAction;
impl ActionTrait for DetachPolicyAction {
    fn name() -> &'static str { "DetachPolicy" }
    fn service_name() -> ServiceName { ServiceName::new("iam").expect("Valid") }
    fn applies_to_principal() -> String { "Iam::User".to_string() }
    fn applies_to_resource() -> String { "Iam::User,Iam::Group".to_string() }
}

// GetPolicy - Get policy details
pub struct GetPolicyAction;
impl ActionTrait for GetPolicyAction {
    fn name() -> &'static str { "GetPolicy" }
    fn service_name() -> ServiceName { ServiceName::new("iam").expect("Valid") }
    fn applies_to_principal() -> String { "Iam::User".to_string() }
    fn applies_to_resource() -> String { "Iam::Policy".to_string() }
}

// ListPolicies - List policies
pub struct ListPoliciesAction;
impl ActionTrait for ListPoliciesAction {
    fn name() -> &'static str { "ListPolicies" }
    fn service_name() -> ServiceName { ServiceName::new("iam").expect("Valid") }
    fn applies_to_principal() -> String { "Iam::User".to_string() }
    fn applies_to_resource() -> String { "Iam::Policy".to_string() }
}

// UpdatePolicy - Update policy
pub struct UpdatePolicyAction;
impl ActionTrait for UpdatePolicyAction {
    fn name() -> &'static str { "UpdatePolicy" }
    fn service_name() -> ServiceName { ServiceName::new("iam").expect("Valid") }
    fn applies_to_principal() -> String { "Iam::User".to_string() }
    fn applies_to_resource() -> String { "Iam::Policy".to_string() }
}

// DeletePolicy - Delete policy
pub struct DeletePolicyAction;
impl ActionTrait for DeletePolicyAction {
    fn name() -> &'static str { "DeletePolicy" }
    fn service_name() -> ServiceName { ServiceName::new("iam").expect("Valid") }
    fn applies_to_principal() -> String { "Iam::User".to_string() }
    fn applies_to_resource() -> String { "Iam::Policy".to_string() }
}
```

---

## 3. Playground Feature Design

### 3.1 Feature: `playground_evaluate`

**Location:** `crates/hodei-policies/src/features/playground_evaluate/`

**Purpose:** Allow ad-hoc testing of Cedar policies against Hodei IAM entities without persisting test data.

**Key Differences from `evaluate_policies`:**
- Accepts entities as JSON (ad-hoc, not persisted)
- Tests multiple combinations (N principals × M actions × P resources)
- Returns detailed diagnostics and explanations
- Can work with or without schema validation

### 3.2 DTOs

```rust
// dto.rs

/// Command for playground evaluation
pub struct PlaygroundEvaluateCommand {
    /// Cedar policies as strings
    pub policies: Vec<PlaygroundPolicy>,
    
    /// Test principals (ad-hoc User entities)
    pub principals: Vec<AdHocUser>,
    
    /// Actions to test (e.g., "CreateUser", "DeleteGroup")
    pub actions: Vec<String>,
    
    /// Test resources (ad-hoc User/Group entities)
    pub resources: Vec<AdHocResource>,
    
    /// Optional context for evaluation
    pub context: Option<HashMap<String, serde_json::Value>>,
    
    /// Schema version to validate against (optional)
    pub schema_version: Option<String>,
    
    /// Evaluation mode
    pub evaluation_mode: EvaluationMode,
}

/// Policy with ID
pub struct PlaygroundPolicy {
    pub id: String,
    pub content: String, // Cedar DSL
}

/// Ad-hoc User entity for testing
pub struct AdHocUser {
    pub hrn: String, // e.g., "hrn:hodei:iam::account123:User/alice"
    pub name: String,
    pub email: String,
    pub group_hrns: Vec<String>, // e.g., ["hrn:hodei:iam::account123:Group/admins"]
    pub tags: Vec<String>,
}

/// Ad-hoc resource (User or Group)
pub enum AdHocResource {
    User(AdHocUser),
    Group(AdHocGroup),
}

/// Ad-hoc Group entity for testing
pub struct AdHocGroup {
    pub hrn: String,
    pub name: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
}

/// Result of playground evaluation
pub struct PlaygroundEvaluationResult {
    /// Individual evaluation results (principals × actions × resources)
    pub evaluation_results: Vec<EvaluationResultItem>,
    
    /// Metadata about the evaluation
    pub metadata: EvaluationMetadata,
}

/// Single evaluation result
pub struct EvaluationResultItem {
    pub principal_hrn: String,
    pub action: String,
    pub resource_hrn: String,
    pub decision: Decision, // Allow/Deny
    pub determining_policies: Vec<String>, // Policy IDs that affected decision
    pub diagnostics: Vec<EvaluationDiagnostic>,
}

/// Evaluation metadata
pub struct EvaluationMetadata {
    pub total_evaluations: usize,
    pub allowed_count: usize,
    pub denied_count: usize,
    pub schema_version_used: Option<String>,
    pub duration_ms: u64,
}
```

### 3.3 Use Case Logic

```rust
// use_case.rs

pub struct PlaygroundEvaluateUseCase {
    schema_loader: Option<Arc<dyn SchemaProvider>>,
}

impl PlaygroundEvaluateUseCase {
    pub async fn execute(
        &self,
        command: PlaygroundEvaluateCommand,
    ) -> Result<PlaygroundEvaluationResult, PlaygroundError> {
        let start = Instant::now();
        
        // 1. Parse Cedar policies
        let policy_set = self.parse_policies(&command.policies)?;
        
        // 2. Load schema if requested
        let schema = if command.evaluation_mode == EvaluationMode::Strict {
            Some(self.load_schema(&command.schema_version).await?)
        } else {
            None
        };
        
        // 3. Convert ad-hoc entities to Cedar entities
        let entities = self.build_entity_store(&command)?;
        
        // 4. Create Cedar authorizer
        let authorizer = Authorizer::new();
        
        // 5. Evaluate all combinations
        let mut results = Vec::new();
        let mut allowed_count = 0;
        let mut denied_count = 0;
        
        for principal in &command.principals {
            for action in &command.actions {
                for resource in &command.resources {
                    let request = Request::new(
                        EntityUid::from_str(&principal.hrn)?,
                        EntityUid::from_str(&format!("Iam::Action::\"{}\"", action))?,
                        EntityUid::from_str(&resource.hrn())?,
                        Context::from_json_value(
                            command.context.clone().unwrap_or_default(),
                            schema.as_ref(),
                        )?,
                        schema.as_ref(),
                    )?;
                    
                    let response = authorizer.is_authorized(&request, &policy_set, &entities);
                    
                    let decision = match response.decision() {
                        cedar_policy::Decision::Allow => Decision::Allow,
                        cedar_policy::Decision::Deny => Decision::Deny,
                    };
                    
                    if decision == Decision::Allow {
                        allowed_count += 1;
                    } else {
                        denied_count += 1;
                    }
                    
                    results.push(EvaluationResultItem {
                        principal_hrn: principal.hrn.clone(),
                        action: action.clone(),
                        resource_hrn: resource.hrn(),
                        decision,
                        determining_policies: response
                            .diagnostics()
                            .reason()
                            .iter()
                            .map(|p| p.policy_id().to_string())
                            .collect(),
                        diagnostics: self.extract_diagnostics(&response),
                    });
                }
            }
        }
        
        let duration = start.elapsed();
        
        Ok(PlaygroundEvaluationResult {
            evaluation_results: results.clone(),
            metadata: EvaluationMetadata {
                total_evaluations: results.len(),
                allowed_count,
                denied_count,
                schema_version_used: command.schema_version,
                duration_ms: duration.as_millis() as u64,
            },
        })
    }
    
    fn build_entity_store(
        &self,
        command: &PlaygroundEvaluateCommand,
    ) -> Result<Entities, PlaygroundError> {
        // Convert AdHocUser and AdHocGroup to Cedar Entities
        // Build entity hierarchy from parent_hrns
        todo!()
    }
    
    fn extract_diagnostics(
        &self,
        response: &cedar_policy::Response,
    ) -> Vec<EvaluationDiagnostic> {
        // Extract reasons, errors, warnings from Cedar response
        todo!()
    }
}
```

---

## 4. REST API Endpoints

### 4.1 Complete Endpoint List (Based on Real Features)

#### Health
```
GET  /health              - Health check
GET  /health/ready        - Readiness probe
GET  /health/live         - Liveness probe
```

#### Users (hodei-iam features)
```
POST   /users                    - create_user
GET    /users/{user_id}          - get_user (NEW FEATURE NEEDED)
PUT    /users/{user_id}          - update_user (NEW FEATURE NEEDED)
DELETE /users/{user_id}          - delete_user (NEW FEATURE NEEDED)
GET    /users                    - list_users (NEW FEATURE NEEDED)
```

#### Groups (hodei-iam features)
```
POST   /groups                   - create_group
GET    /groups/{group_id}        - get_group (NEW FEATURE NEEDED)
PUT    /groups/{group_id}        - update_group (NEW FEATURE NEEDED)
DELETE /groups/{group_id}        - delete_group (NEW FEATURE NEEDED)
GET    /groups                   - list_groups (NEW FEATURE NEEDED)
POST   /groups/{group_id}/members/{user_id}  - add_user_to_group
DELETE /groups/{group_id}/members/{user_id}  - remove_user_from_group (NEW FEATURE NEEDED)
GET    /groups/{group_id}/members - list_group_members (NEW FEATURE NEEDED)
```

#### Policies (hodei-iam features)
```
POST   /policies                 - create_policy
GET    /policies/{policy_id}     - get_policy
PUT    /policies/{policy_id}     - update_policy
DELETE /policies/{policy_id}     - delete_policy
GET    /policies                 - list_policies
POST   /policies/validate        - validate_policy (from hodei-policies)
```

#### Authorization (hodei-iam features)
```
POST   /authorization/evaluate   - evaluate_iam_policies
GET    /authorization/effective-policies/{user_id}  - get_effective_policies
```

#### Schemas (hodei-policies features)
```
POST   /schemas/build            - build_schema
GET    /schemas/load             - load_schema
POST   /schemas/register-iam     - register_iam_schema
```

#### Playground (NEW - hodei-policies features)
```
POST   /playground/evaluate      - playground_evaluate (NEW)
POST   /playground/explain       - playground_explain (NEW, optional)
```

### 4.2 Playground Endpoint Specification

**POST /playground/evaluate**

Request:
```json
{
  "policies": [
    {
      "id": "admin-policy",
      "content": "permit(principal in Iam::Group::\"admins\", action, resource);"
    }
  ],
  "principals": [
    {
      "hrn": "hrn:hodei:iam::account123:User/alice",
      "name": "Alice",
      "email": "alice@example.com",
      "group_hrns": ["hrn:hodei:iam::account123:Group/admins"],
      "tags": ["employee"]
    }
  ],
  "actions": ["CreateUser", "DeleteUser", "GetUser"],
  "resources": [
    {
      "User": {
        "hrn": "hrn:hodei:iam::account123:User/bob",
        "name": "Bob",
        "email": "bob@example.com",
        "group_hrns": [],
        "tags": []
      }
    }
  ],
  "context": {
    "ip": "192.168.1.1",
    "time": "2024-01-15T10:30:00Z"
  },
  "schema_version": "1.0.0",
  "evaluation_mode": "BestEffortNoSchema"
}
```

Response:
```json
{
  "evaluation_results": [
    {
      "principal_hrn": "hrn:hodei:iam::account123:User/alice",
      "action": "CreateUser",
      "resource_hrn": "hrn:hodei:iam::account123:User/bob",
      "decision": "Allow",
      "determining_policies": ["admin-policy"],
      "diagnostics": [
        {
          "level": "Info",
          "message": "Principal is member of Iam::Group::\"admins\""
        }
      ]
    },
    {
      "principal_hrn": "hrn:hodei:iam::account123:User/alice",
      "action": "DeleteUser",
      "resource_hrn": "hrn:hodei:iam::account123:User/bob",
      "decision": "Allow",
      "determining_policies": ["admin-policy"],
      "diagnostics": []
    },
    {
      "principal_hrn": "hrn:hodei:iam::account123:User/alice",
      "action": "GetUser",
      "resource_hrn": "hrn:hodei:iam::account123:User/bob",
      "decision": "Allow",
      "determining_policies": ["admin-policy"],
      "diagnostics": []
    }
  ],
  "metadata": {
    "total_evaluations": 3,
    "allowed_count": 3,
    "denied_count": 0,
    "schema_version_used": "1.0.0",
    "duration_ms": 15
  }
}
```

---

## 5. Implementation Steps

### Phase 1: Complete IAM Actions (Week 1)

**Task 1.1: Add Missing Actions to `actions.rs`**

File: `crates/hodei-iam/src/internal/domain/actions.rs`

Add all actions from section 2:
- [ ] GetUserAction
- [ ] ListUsersAction
- [ ] UpdateUserAction
- [ ] GetGroupAction
- [ ] ListGroupsAction
- [ ] UpdateGroupAction
- [ ] ListGroupMembersAction
- [ ] AttachPolicyAction
- [ ] DetachPolicyAction
- [ ] GetPolicyAction
- [ ] ListPoliciesAction
- [ ] UpdatePolicyAction
- [ ] DeletePolicyAction

**Task 1.2: Update `register_iam_schema` Use Case**

File: `crates/hodei-iam/src/features/register_iam_schema/use_case.rs`

Register all new actions in the `execute` method:
```rust
// Register all new actions
self.register_action::<GetUserAction>().await?;
self.register_action::<ListUsersAction>().await?;
// ... etc
```

**Task 1.3: Verify Schema Registration**

- [ ] Run tests
- [ ] Verify all actions appear in persisted schema
- [ ] Verify Cedar can validate policies with new actions

---

### Phase 2: Playground Feature (Week 2)

**Task 2.1: Create Feature Structure**

```
crates/hodei-policies/src/features/playground_evaluate/
├── mod.rs
├── dto.rs
├── use_case.rs
├── ports.rs (optional - SchemaProvider if needed)
├── error.rs
├── di.rs
├── use_case_test.rs
```

**Task 2.2: Implement DTOs**

File: `crates/hodei-policies/src/features/playground_evaluate/dto.rs`

Implement all DTOs from section 3.2:
- [ ] PlaygroundEvaluateCommand
- [ ] PlaygroundPolicy
- [ ] AdHocUser
- [ ] AdHocGroup
- [ ] AdHocResource
- [ ] PlaygroundEvaluationResult
- [ ] EvaluationResultItem
- [ ] EvaluationMetadata
- [ ] EvaluationDiagnostic

**Task 2.3: Implement Use Case**

File: `crates/hodei-policies/src/features/playground_evaluate/use_case.rs`

Key methods:
- [ ] `execute` - Main orchestration
- [ ] `parse_policies` - Parse Cedar policies from strings
- [ ] `build_entity_store` - Convert ad-hoc entities to Cedar Entities
- [ ] `extract_diagnostics` - Extract diagnostic info from Cedar response

**Task 2.4: Implement Error Handling**

File: `crates/hodei-policies/src/features/playground_evaluate/error.rs`

```rust
#[derive(Debug, thiserror::Error)]
pub enum PlaygroundError {
    #[error("Policy parse error: {0}")]
    PolicyParseError(String),
    
    #[error("Invalid entity: {0}")]
    InvalidEntity(String),
    
    #[error("Schema load error: {0}")]
    SchemaLoadError(String),
    
    #[error("Evaluation error: {0}")]
    EvaluationError(String),
}
```

**Task 2.5: Implement DI Factory**

File: `crates/hodei-policies/src/features/playground_evaluate/di.rs`

```rust
pub struct PlaygroundEvaluateUseCaseFactory;

impl PlaygroundEvaluateUseCaseFactory {
    pub fn build(
        schema_provider: Option<Arc<dyn SchemaProvider>>,
    ) -> Arc<PlaygroundEvaluateUseCase> {
        Arc::new(PlaygroundEvaluateUseCase::new(schema_provider))
    }
}
```

**Task 2.6: Write Unit Tests**

File: `crates/hodei-policies/src/features/playground_evaluate/use_case_test.rs`

Test scenarios:
- [ ] Single principal, single action, single resource → Allow
- [ ] Single principal, single action, single resource → Deny
- [ ] Multiple principals × actions × resources
- [ ] With group membership (hierarchy)
- [ ] With context variables
- [ ] With schema validation
- [ ] With invalid policy syntax
- [ ] With missing entities

---

### Phase 3: REST API Integration (Week 3)

**Task 3.1: Add Playground Handler**

File: `src/handlers/playground.rs`

```rust
pub async fn evaluate_playground(
    State(state): State<AppState>,
    Json(request): Json<PlaygroundEvaluateRequest>,
) -> Result<Json<PlaygroundEvaluateResponse>, AppError> {
    let command = PlaygroundEvaluateCommand {
        // Map request to command
    };
    
    let result = state
        .playground_evaluate_use_case
        .execute(command)
        .await?;
    
    Ok(Json(PlaygroundEvaluateResponse::from(result)))
}
```

**Task 3.2: Update AppState**

File: `src/app_state.rs`

```rust
pub struct AppState {
    // ... existing use cases
    pub playground_evaluate_use_case: Arc<PlaygroundEvaluateUseCase>,
}
```

**Task 3.3: Update Bootstrap**

File: `src/bootstrap.rs`

```rust
// Build playground use case
let playground_evaluate_use_case = PlaygroundEvaluateUseCaseFactory::build(
    Some(schema_adapter.clone()),
);

// Add to AppState
AppState {
    // ... existing
    playground_evaluate_use_case,
}
```

**Task 3.4: Add Routes**

File: `src/main.rs`

```rust
let app = Router::new()
    // ... existing routes
    .route("/playground/evaluate", post(handlers::playground::evaluate_playground))
    .with_state(app_state);
```

**Task 3.5: Create OpenAPI Spec**

File: `openapi.yaml`

Generate OpenAPI 3.0 spec with:
- [ ] All entity schemas (User, Group)
- [ ] All endpoints
- [ ] Request/response schemas
- [ ] Examples
- [ ] Authentication

---

### Phase 4: Testing & Documentation (Week 4)

**Task 4.1: Integration Tests**

File: `tests/playground_integration_test.rs`

- [ ] Test complete flow: register schema → evaluate playground
- [ ] Test with real User/Group entities
- [ ] Test with SurrealDB (if using persistence)
- [ ] Test error scenarios

**Task 4.2: Create Examples**

File: `examples/playground_usage.rs`

```rust
#[tokio::main]
async fn main() {
    // Example 1: Admin can do everything
    // Example 2: Regular user limited access
    // Example 3: Group-based permissions
}
```

**Task 4.3: Documentation**

- [ ] Update README.md with playground usage
- [ ] Create PLAYGROUND.md with detailed examples
- [ ] Add inline code documentation
- [ ] Create Postman collection

**Task 4.4: End-to-End Testing**

- [ ] Start server
- [ ] Register IAM schema
- [ ] Create test users and groups
- [ ] Evaluate policies via REST API
- [ ] Test playground with various scenarios

---

## 6. Testing Strategy

### 6.1 Unit Tests

**Location:** Next to each module

**Coverage:**
- [ ] All actions have tests
- [ ] DTOs serialize/deserialize correctly
- [ ] Use case logic with mocks
- [ ] Error handling paths

**Example:**
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_playground_evaluate_allows_admin() {
        // Mock dependencies
        let use_case = PlaygroundEvaluateUseCase::new(None);
        
        let command = PlaygroundEvaluateCommand {
            policies: vec![PlaygroundPolicy {
                id: "admin".to_string(),
                content: "permit(principal in Iam::Group::\"admins\", action, resource);".to_string(),
            }],
            principals: vec![/* alice in admins */],
            actions: vec!["CreateUser".to_string()],
            resources: vec![/* bob */],
            context: None,
            schema_version: None,
            evaluation_mode: EvaluationMode::NoSchema,
        };
        
        let result = use_case.execute(command).await.unwrap();
        
        assert_eq!(result.metadata.allowed_count, 1);
    }
}
```

### 6.2 Integration Tests

**Location:** `tests/` directory

**Scenarios:**
- [ ] Complete IAM schema registration flow
- [ ] Playground evaluation with schema
- [ ] Multiple policies interacting
- [ ] Entity hierarchies (user in group)

### 6.3 E2E Tests

**Setup:**
1. Start server
2. Initialize database
3. Register IAM schema

**Test Cases:**
- [ ] Register schema via API
- [ ] Evaluate playground scenarios via API
- [ ] Verify policy decisions are correct
- [ ] Test with invalid inputs

---

## 7. Example Cedar Policies for Testing

### 7.1 Basic Admin Policy

```cedar
// Admins can do everything
permit(
  principal in Iam::Group::"admins",
  action,
  resource
);
```

### 7.2 User Self-Management

```cedar
// Users can view their own details
permit(
  principal,
  action == Iam::Action::"GetUser",
  resource
)
when {
  principal == resource
};
```

### 7.3 Group-Based Access

```cedar
// Engineers can create and update users
permit(
  principal in Iam::Group::"engineers",
  action in [
    Iam::Action::"CreateUser",
    Iam::Action::"UpdateUser",
    Iam::Action::"GetUser",
    Iam::Action::"ListUsers"
  ],
  resource
);
```

### 7.4 Restricted Deletion

```cedar
// Only super-admins can delete users
permit(
  principal in Iam::Group::"super-admins",
  action == Iam::Action::"DeleteUser",
  resource
);

// Forbid deletion of admin users by non-super-admins
forbid(
  principal,
  action == Iam::Action::"DeleteUser",
  resource
)
when {
  resource in Iam::Group::"admins"
}
unless {
  principal in Iam::Group::"super-admins"
};
```

### 7.5 Context-Based Access

```cedar
// Only allow user management during business hours
permit(
  principal,
  action in [
    Iam::Action::"CreateUser",
    Iam::Action::"UpdateUser",
    Iam::Action::"DeleteUser"
  ],
  resource
)
when {
  context.hour >= 9 &&
  context.hour <= 17 &&
  context.day_of_week != "Saturday" &&
  context.day_of_week != "Sunday"
};
```

---

## 8. Success Criteria

### Phase 1 Complete:
- [ ] All 13 new actions implemented
- [ ] All actions registered in IAM schema
- [ ] Tests passing
- [ ] Schema builds successfully

### Phase 2 Complete:
- [ ] `playground_evaluate` feature implemented
- [ ] All DTOs working
- [ ] Unit tests passing
- [ ] Can evaluate ad-hoc scenarios

### Phase 3 Complete:
- [ ] REST API endpoints working
- [ ] Integration with AppState
- [ ] Can call playground via HTTP
- [ ] OpenAPI spec generated

### Phase 4 Complete:
- [ ] All tests passing
- [ ] Documentation complete
- [ ] Examples working
- [ ] Ready for production

---

## 9. Architecture Compliance Checklist

Before considering any feature complete:

- [ ] Feature follows VSA structure (use_case, ports, dto, error, di, tests)
- [ ] `use_case.rs` depends only on traits (ports)
- [ ] No direct dependencies between bounded contexts
- [ ] All `internal/` modules are `pub(crate)`
- [ ] API surface centralized in `api.rs`
- [ ] DI factory in `di.rs`
- [ ] Comprehensive tests in `use_case_test.rs`
- [ ] No `println!` - only `tracing`
- [ ] `cargo check` passes
- [ ] `cargo clippy -- -D warnings` passes
- [ ] `cargo nextest run` passes

---

## 10. Next Steps

1. **Review this plan** with the team
2. **Create GitHub issues** for each task
3. **Set up project board** with 4 columns (Phase 1-4)
4. **Begin Phase 1** implementation
5. **Iterate and refine** as needed

---

**Document Status:** ✅ Ready for Implementation  
**Estimated Effort:** 4 weeks (1 person)  
**Priority:** High  
**Owner:** Engineering Team