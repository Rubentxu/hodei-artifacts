# Hodei Artifacts - REST API & Policy Playground Design

**Version:** 1.0  
**Date:** 2024  
**Status:** Design Document

---

## Table of Contents

1. [Executive Summary](#executive-summary)
2. [Research: AWS IAM Policy Simulator Analysis](#research-aws-iam-policy-simulator-analysis)
3. [Architecture Overview](#architecture-overview)
4. [API Design - Complete Specification](#api-design---complete-specification)
5. [Policy Playground Feature](#policy-playground-feature)
6. [OpenAPI Schema](#openapi-schema)
7. [Implementation Plan](#implementation-plan)
8. [Examples & Use Cases](#examples--use-cases)

---

## 1. Executive Summary

This document defines the complete REST API surface for **Hodei IAM** and introduces a **Policy Playground** feature inspired by AWS IAM Policy Simulator. The playground enables users to test and validate Cedar policies in a sandbox environment without affecting production data.

**Key Objectives:**
- Expose all Hodei IAM functionality via RESTful API
- Provide a comprehensive policy testing playground
- Generate complete OpenAPI 3.0 specification
- Enable real-time policy validation and evaluation
- Support ad-hoc testing scenarios without persistence

---

## 2. Research: AWS IAM Policy Simulator Analysis

### 2.1 AWS Policy Simulator Capabilities

Based on reverse engineering of AWS IAM Policy Simulator, the following key features were identified:

#### Input Components:
1. **Multiple Policy Sources**
   - Identity-based policies (attached to users/groups/roles)
   - Custom policies (provided as strings)
   - Resource-based policies
   - Service Control Policies (SCPs)
   - Permissions boundaries

2. **Simulation Parameters**
   - **Actions**: List of service actions (e.g., `s3:ListBucket`)
   - **Resources**: ARNs or wildcards (`*`)
   - **Context Keys**: Variables for policy conditions (IP, date, tags, MFA, etc.)
   - **Caller ARN**: Identity making the request

3. **Evaluation Modes**
   - Test attached policies
   - Test custom policies
   - Combine multiple policy types
   - Schema-aware validation

#### Output Components:
1. **Decision**: Allow/Deny for each action+resource combination
2. **Matched Statements**: Specific policy statements that determined the decision (with line/column positions)
3. **Missing Context Values**: Context keys referenced but not provided
4. **Diagnostic Details**: Permissions boundary effects, resource-specific results
5. **Pagination**: Support for large result sets

#### API Structure (SimulateCustomPolicy):
```
Request:
- PolicyInputList.member.N (array of policy strings)
- ActionNames.member.N (array of actions)
- ResourceArns.member.N (array of resource ARNs)
- ContextEntries.member.N (array of key-value pairs)
- CallerArn (optional)
- ResourcePolicy (optional)
- PermissionsBoundaryPolicyInputList.member.N (optional)
- MaxItems, Marker (pagination)

Response:
- EvaluationResults.member.N
  - EvalDecision (allowed/denied)
  - EvalActionName
  - EvalResourceName
  - MatchedStatements (with SourcePolicyId, StartPosition, EndPosition)
  - MissingContextValues
  - PermissionsBoundaryDecisionDetail
  - ResourceSpecificResults
- IsTruncated
- Marker
```

### 2.2 Cedar Policy Engine Mapping

Cedar provides similar capabilities with its own terminology:

| AWS Concept | Cedar Concept | Notes |
|-------------|---------------|-------|
| IAM Policy (JSON) | Cedar Policy (DSL) | Different syntax, same purpose |
| Principal | Principal | User, Group, Role |
| Action | Action | Operation being performed |
| Resource (ARN) | Resource | Entity being accessed |
| Context Keys | Context | Request-time variables |
| Allow/Deny | Permit/Forbid | Decision outcomes |
| Policy Statement | Policy | Individual policy with scope+conditions |
| Policy Set | PolicySet | Collection of policies |
| Schema | Schema | Entity and action type definitions |

**Key Differences:**
- Cedar uses a DSL instead of JSON
- Cedar has explicit forbid (not just absence of allow)
- Cedar evaluation: At least one permit AND zero forbids = Allow
- Cedar supports hierarchical entities natively
- Cedar has stronger type safety with schemas

---

## 3. Architecture Overview

### 3.1 System Components

```
┌─────────────────────────────────────────────────────────────┐
│                     Hodei REST API (Axum)                    │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐     │
│  │   IAM API    │  │ Policies API │  │ Playground   │     │
│  │              │  │              │  │   API        │     │
│  │ - Users      │  │ - Schemas    │  │ - Evaluate   │     │
│  │ - Groups     │  │ - Validate   │  │ - Explain    │     │
│  │ - Policies   │  │ - Build      │  │ - Validate   │     │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘     │
│         │                  │                  │              │
├─────────┼──────────────────┼──────────────────┼─────────────┤
│         ▼                  ▼                  ▼              │
│  ┌────────────────────────────────────────────────────┐    │
│  │         hodei-iam crate (Bounded Context)          │    │
│  │                                                     │    │
│  │  Features:                                          │    │
│  │  - create_user, get_user, list_users, etc.        │    │
│  │  - create_group, add_user_to_group, etc.          │    │
│  │  - create_policy, attach_policy, etc.             │    │
│  │  - evaluate_iam_policies                           │    │
│  │  - register_iam_schema                             │    │
│  └─────────────────────┬──────────────────────────────┘    │
│                        │                                     │
│  ┌────────────────────▼─────────────────────────────────┐  │
│  │      hodei-policies crate (Bounded Context)          │  │
│  │                                                       │  │
│  │  Features:                                            │  │
│  │  - register_entity_type, register_action_type       │  │
│  │  - build_schema, load_schema                        │  │
│  │  - validate_policy                                   │  │
│  │  - evaluate_policies                                 │  │
│  │  - playground_evaluate (NEW)                        │  │
│  └───────────────────────┬───────────────────────────────┘  │
│                          │                                   │
│  ┌──────────────────────▼────────────────────────────────┐ │
│  │              kernel crate (Shared)                     │ │
│  │                                                        │ │
│  │  - HodeiEntity, Principal, Resource traits           │ │
│  │  - Hrn, PolicyId, HodeiPolicy, HodeiPolicySet       │ │
│  │  - AuthContextProvider, EventBus traits             │ │
│  └────────────────────────────────────────────────────────┘ │
│                                                              │
├──────────────────────────────────────────────────────────────┤
│                    Infrastructure Layer                       │
├──────────────────────────────────────────────────────────────┤
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐      │
│  │  SurrealDB   │  │   Cedar      │  │  Object      │      │
│  │  Adapter     │  │   Engine     │  │  Store       │      │
│  └──────────────┘  └──────────────┘  └──────────────┘      │
└──────────────────────────────────────────────────────────────┘
```

### 3.2 New Features Required

#### In `hodei-iam`:
- ✅ create_user, create_group (existing)
- ❌ **get_user**, **list_users**, **update_user**, **delete_user**
- ❌ **get_group**, **list_groups**, **update_group**, **delete_group**
- ❌ **remove_user_from_group**, **list_group_members**
- ❌ **list_user_policies**, **list_group_policies**

#### In `hodei-policies`:
- ❌ **playground_evaluate** - Ad-hoc policy testing without persistence

---

## 4. API Design - Complete Specification

### 4.1 Base URL Structure

```
https://api.hodei.io/v1
```

### 4.2 Endpoint Groups

#### A. Health & Status
```
GET  /health              - Basic health check
GET  /health/ready        - Readiness probe
GET  /health/live         - Liveness probe
```

#### B. User Management (`/users`)
```
POST   /users                           - Create user
GET    /users                           - List users (paginated)
GET    /users/{user_id}                 - Get user details
PUT    /users/{user_id}                 - Update user
DELETE /users/{user_id}                 - Delete user
GET    /users/{user_id}/groups          - List user's groups
GET    /users/{user_id}/policies        - List user's policies
POST   /users/{user_id}/policies        - Attach policy to user
DELETE /users/{user_id}/policies/{policy_id} - Detach policy
```

#### C. Group Management (`/groups`)
```
POST   /groups                          - Create group
GET    /groups                          - List groups (paginated)
GET    /groups/{group_id}               - Get group details
PUT    /groups/{group_id}               - Update group
DELETE /groups/{group_id}               - Delete group
GET    /groups/{group_id}/members       - List group members
POST   /groups/{group_id}/members       - Add user to group
DELETE /groups/{group_id}/members/{user_id} - Remove user from group
GET    /groups/{group_id}/policies      - List group's policies
POST   /groups/{group_id}/policies      - Attach policy to group
DELETE /groups/{group_id}/policies/{policy_id} - Detach policy
```

#### D. Policy Management (`/policies`)
```
POST   /policies                        - Create policy
GET    /policies                        - List policies (paginated)
GET    /policies/{policy_id}            - Get policy details
PUT    /policies/{policy_id}            - Update policy
DELETE /policies/{policy_id}            - Delete policy
POST   /policies/validate               - Validate policy syntax
GET    /policies/{policy_id}/attachments - List where policy is attached
```

#### E. Schema Management (`/schemas`)
```
POST   /schemas/build                   - Build and persist schema
GET    /schemas/load                    - Load schema (specific version or latest)
POST   /schemas/register-iam            - Register IAM entity/action types
GET    /schemas                         - List schema versions
GET    /schemas/{version}               - Get specific schema version
```

#### F. Authorization (`/authorization`)
```
POST   /authorization/evaluate          - Evaluate authorization request (production)
POST   /authorization/check             - Quick check (Allow/Deny only)
GET    /authorization/effective-policies - Get effective policies for principal
```

#### G. Policy Playground (`/playground`)
```
POST   /playground/evaluate             - Simulate policy evaluation (ad-hoc)
POST   /playground/validate             - Validate policies with/without schema
POST   /playground/explain              - Detailed explanation of decision
GET    /playground/schema               - Get current schema for reference
POST   /playground/test-scenario        - Test complete scenario with multiple evaluations
```

### 4.3 Detailed Endpoint Specifications

#### 4.3.1 Playground Evaluate

**Endpoint:** `POST /playground/evaluate`

**Purpose:** Simulate policy evaluation without persisting entities or policies. Similar to AWS SimulateCustomPolicy.

**Request Body:**
```json
{
  "policies": [
    {
      "id": "policy-1",
      "content": "permit(principal == User::\"alice\", action == Action::\"ViewPhoto\", resource == Photo::\"vacation.jpg\");"
    },
    {
      "id": "policy-2",
      "content": "forbid(principal, action, resource) when { resource.private && principal != resource.owner };"
    }
  ],
  "principals": [
    {
      "type": "User",
      "id": "alice",
      "attributes": {
        "department": "Engineering",
        "level": 5
      }
    }
  ],
  "actions": [
    "ViewPhoto",
    "EditPhoto",
    "DeletePhoto"
  ],
  "resources": [
    {
      "type": "Photo",
      "id": "vacation.jpg",
      "attributes": {
        "owner": "alice",
        "private": false
      }
    },
    {
      "type": "Photo",
      "id": "secret.jpg",
      "attributes": {
        "owner": "bob",
        "private": true
      }
    }
  ],
  "context": {
    "ip": "192.168.1.1",
    "time": "2024-01-15T10:30:00Z",
    "mfa_enabled": true
  },
  "schema_version": "1.0.0",
  "evaluation_mode": "BestEffortNoSchema",
  "entities": [
    {
      "type": "User",
      "id": "alice",
      "parents": ["Group::\"engineers\""]
    }
  ]
}
```

**Response:**
```json
{
  "evaluation_results": [
    {
      "principal": "User::\"alice\"",
      "action": "ViewPhoto",
      "resource": "Photo::\"vacation.jpg\"",
      "decision": "Allow",
      "determining_policies": ["policy-1"],
      "matched_statements": [
        {
          "policy_id": "policy-1",
          "effect": "permit",
          "reason": "Principal, action, and resource match exactly"
        }
      ],
      "diagnostics": []
    },
    {
      "principal": "User::\"alice\"",
      "action": "EditPhoto",
      "resource": "Photo::\"vacation.jpg\"",
      "decision": "Deny",
      "determining_policies": [],
      "matched_statements": [],
      "diagnostics": [
        {
          "level": "Info",
          "message": "No policy explicitly permits this action"
        }
      ]
    },
    {
      "principal": "User::\"alice\"",
      "action": "ViewPhoto",
      "resource": "Photo::\"secret.jpg\"",
      "decision": "Deny",
      "determining_policies": ["policy-2"],
      "matched_statements": [
        {
          "policy_id": "policy-2",
          "effect": "forbid",
          "reason": "Resource is private and principal is not owner"
        }
      ],
      "diagnostics": [
        {
          "level": "Warning",
          "message": "Explicitly forbidden by policy-2",
          "policy_id": "policy-2"
        }
      ]
    }
  ],
  "evaluation_metadata": {
    "total_evaluations": 6,
    "schema_version_used": "1.0.0",
    "evaluation_mode": "BestEffortNoSchema",
    "duration_ms": 12
  }
}
```

#### 4.3.2 Playground Explain

**Endpoint:** `POST /playground/explain`

**Purpose:** Get detailed explanation of a specific authorization decision.

**Request Body:**
```json
{
  "policies": ["..."],
  "principal": {
    "type": "User",
    "id": "alice",
    "attributes": {}
  },
  "action": "ViewPhoto",
  "resource": {
    "type": "Photo",
    "id": "vacation.jpg",
    "attributes": {}
  },
  "context": {},
  "schema_version": "1.0.0"
}
```

**Response:**
```json
{
  "decision": "Allow",
  "explanation": {
    "summary": "Access allowed by policy 'photo-viewer-policy'",
    "permit_policies": [
      {
        "policy_id": "photo-viewer-policy",
        "statement": "permit(principal in Group::\"viewers\", action == Action::\"ViewPhoto\", resource);",
        "reason": "Principal alice is member of Group viewers",
        "matched_conditions": [
          "principal in Group::\"viewers\"",
          "action == Action::\"ViewPhoto\""
        ]
      }
    ],
    "forbid_policies": [],
    "evaluated_policies_count": 5,
    "entity_hierarchy": {
      "principal": ["User::\"alice\"", "Group::\"viewers\"", "Group::\"employees\""],
      "resource": ["Photo::\"vacation.jpg\"", "Album::\"2024-vacation\""]
    },
    "context_keys_used": ["ip", "time"],
    "context_keys_unused": ["mfa_enabled"]
  }
}
```

#### 4.3.3 User Creation

**Endpoint:** `POST /users`

**Request Body:**
```json
{
  "username": "alice",
  "email": "alice@example.com",
  "display_name": "Alice Anderson",
  "attributes": {
    "department": "Engineering",
    "level": 5,
    "manager": "bob"
  },
  "tags": {
    "environment": "production",
    "cost_center": "eng-001"
  }
}
```

**Response:**
```json
{
  "user_id": "usr_abc123xyz",
  "hrn": "hrn:hodei:iam::123456789:user/alice",
  "username": "alice",
  "email": "alice@example.com",
  "display_name": "Alice Anderson",
  "attributes": {
    "department": "Engineering",
    "level": 5,
    "manager": "bob"
  },
  "tags": {
    "environment": "production",
    "cost_center": "eng-001"
  },
  "created_at": "2024-01-15T10:30:00Z",
  "updated_at": "2024-01-15T10:30:00Z"
}
```

#### 4.3.4 Policy Validation

**Endpoint:** `POST /policies/validate`

**Request Body:**
```json
{
  "policy_content": "permit(principal, action, resource) when { principal.level >= 5 };",
  "validate_against_schema": true,
  "schema_version": "1.0.0"
}
```

**Response:**
```json
{
  "valid": true,
  "errors": [],
  "warnings": [
    {
      "message": "Policy has broad scope (any principal, any action, any resource)",
      "severity": "Warning",
      "suggestion": "Consider narrowing the scope for better security"
    }
  ],
  "schema_validation": {
    "entities_referenced": ["principal"],
    "actions_referenced": [],
    "attributes_used": ["principal.level"],
    "all_entities_exist": true,
    "all_actions_exist": true,
    "all_attributes_typed": true
  }
}
```

---

## 5. Policy Playground Feature

### 5.1 Feature Structure

**Location:** `crates/hodei-policies/src/features/playground_evaluate/`

**Files:**
```
playground_evaluate/
├── mod.rs                 - Module exports
├── dto.rs                 - DTOs and request/response models
├── use_case.rs            - PlaygroundEvaluateUseCase
├── ports.rs               - SchemaProvider trait (optional)
├── error.rs               - PlaygroundError
├── di.rs                  - Factory for DI
├── use_case_test.rs       - Unit tests with mocks
└── mocks.rs               - Mock implementations
```

### 5.2 Key DTOs

```rust
// dto.rs

/// Command for playground evaluation
pub struct PlaygroundEvaluateCommand {
    /// Policies to evaluate (as Cedar DSL strings)
    pub policies: Vec<PlaygroundPolicy>,
    
    /// Principals to test with
    pub principals: Vec<AdHocEntity>,
    
    /// Actions to test
    pub actions: Vec<String>,
    
    /// Resources to test against
    pub resources: Vec<AdHocEntity>,
    
    /// Context variables
    pub context: Option<HashMap<String, serde_json::Value>>,
    
    /// Schema version to validate against
    pub schema_version: Option<String>,
    
    /// Evaluation mode
    pub evaluation_mode: EvaluationMode,
    
    /// Additional entities for hierarchy
    pub entities: Vec<AdHocEntity>,
}

/// Policy provided as string with ID
pub struct PlaygroundPolicy {
    pub id: String,
    pub content: String,
}

/// Ad-hoc entity (not persisted)
pub struct AdHocEntity {
    pub entity_type: String,
    pub id: String,
    pub attributes: HashMap<String, serde_json::Value>,
    pub parents: Vec<String>, // e.g., ["Group::\"engineers\""]
}

/// Result of playground evaluation
pub struct PlaygroundEvaluationResult {
    /// Individual evaluation results (principal x action x resource)
    pub evaluation_results: Vec<EvaluationResultItem>,
    
    /// Metadata about the evaluation
    pub metadata: EvaluationMetadata,
}

/// Single evaluation result
pub struct EvaluationResultItem {
    pub principal: String,
    pub action: String,
    pub resource: String,
    pub decision: Decision,
    pub determining_policies: Vec<String>,
    pub matched_statements: Vec<MatchedStatement>,
    pub diagnostics: Vec<EvaluationDiagnostic>,
}

/// Matched statement in a policy
pub struct MatchedStatement {
    pub policy_id: String,
    pub effect: String, // "permit" or "forbid"
    pub reason: String,
}

/// Metadata about the evaluation run
pub struct EvaluationMetadata {
    pub total_evaluations: usize,
    pub schema_version_used: Option<String>,
    pub evaluation_mode: EvaluationMode,
    pub duration_ms: u64,
}
```

### 5.3 Use Case Logic

```rust
// use_case.rs

pub struct PlaygroundEvaluateUseCase {
    // Dependencies (if needed)
    schema_provider: Option<Arc<dyn SchemaProvider>>,
}

impl PlaygroundEvaluateUseCase {
    pub async fn execute(
        &self,
        command: PlaygroundEvaluateCommand,
    ) -> Result<PlaygroundEvaluationResult, PlaygroundError> {
        let start = Instant::now();
        
        // 1. Parse all policies
        let policy_set = self.parse_policies(&command.policies)?;
        
        // 2. Load schema if requested
        let schema = self.load_schema_if_needed(&command).await?;
        
        // 3. Validate policies against schema
        if let Some(ref schema) = schema {
            self.validate_policies_with_schema(&policy_set, schema)?;
        }
        
        // 4. Build entity store from ad-hoc entities
        let entities = self.build_entity_store(&command)?;
        
        // 5. Perform cartesian product evaluation (principals x actions x resources)
        let mut evaluation_results = Vec::new();
        
        for principal in &command.principals {
            for action in &command.actions {
                for resource in &command.resources {
                    let result = self.evaluate_single(
                        &policy_set,
                        &entities,
                        principal,
                        action,
                        resource,
                        &command.context,
                        schema.as_ref(),
                    )?;
                    
                    evaluation_results.push(result);
                }
            }
        }
        
        let duration = start.elapsed();
        
        Ok(PlaygroundEvaluationResult {
            evaluation_results,
            metadata: EvaluationMetadata {
                total_evaluations: evaluation_results.len(),
                schema_version_used: command.schema_version,
                evaluation_mode: command.evaluation_mode,
                duration_ms: duration.as_millis() as u64,
            },
        })
    }
    
    fn evaluate_single(
        &self,
        policy_set: &PolicySet,
        entities: &Entities,
        principal: &AdHocEntity,
        action: &str,
        resource: &AdHocEntity,
        context: &Option<HashMap<String, serde_json::Value>>,
        schema: Option<&Schema>,
    ) -> Result<EvaluationResultItem, PlaygroundError> {
        // Use Cedar Authorizer to evaluate
        // Extract diagnostics and matched statements
        // Build detailed result
        
        todo!("Implementation details")
    }
}
```

### 5.4 Integration Points

The playground feature integrates with:

1. **Existing `validate_policy` feature** - For syntax validation
2. **Existing `load_schema` feature** - For schema retrieval
3. **Existing `evaluate_policies` feature** - For core evaluation logic
4. **Cedar Authorizer** - Direct use for evaluation

---

## 6. OpenAPI Schema

### 6.1 Full OpenAPI 3.0 Specification

```yaml
openapi: 3.0.3
info:
  title: Hodei Artifacts API
  description: |
    Complete REST API for Hodei IAM and Policy Management.
    
    Features:
    - User and Group management
    - Policy lifecycle management
    - Authorization evaluation
    - Policy playground for testing
    - Schema management
  version: 1.0.0
  contact:
    name: Hodei Team
    email: support@hodei.io
  license:
    name: Apache 2.0
    url: https://www.apache.org/licenses/LICENSE-2.0.html

servers:
  - url: https://api.hodei.io/v1
    description: Production
  - url: https://staging.api.hodei.io/v1
    description: Staging
  - url: http://localhost:3000/v1
    description: Local development

tags:
  - name: Health
    description: Health check endpoints
  - name: Users
    description: User management
  - name: Groups
    description: Group management
  - name: Policies
    description: Policy management
  - name: Schemas
    description: Schema management
  - name: Authorization
    description: Authorization evaluation
  - name: Playground
    description: Policy testing playground

paths:
  /health:
    get:
      tags: [Health]
      summary: Basic health check
      operationId: healthCheck
      responses:
        '200':
          description: Service is healthy
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/HealthResponse'
  
  /users:
    post:
      tags: [Users]
      summary: Create a new user
      operationId: createUser
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/CreateUserRequest'
      responses:
        '201':
          description: User created successfully
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/UserResponse'
        '400':
          $ref: '#/components/responses/BadRequest'
        '409':
          $ref: '#/components/responses/Conflict'
    
    get:
      tags: [Users]
      summary: List users
      operationId: listUsers
      parameters:
        - $ref: '#/components/parameters/PageParam'
        - $ref: '#/components/parameters/PageSizeParam'
        - name: filter
          in: query
          schema:
            type: string
          description: Filter by username or email
      responses:
        '200':
          description: List of users
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/UserListResponse'
  
  /users/{userId}:
    get:
      tags: [Users]
      summary: Get user by ID
      operationId: getUser
      parameters:
        - $ref: '#/components/parameters/UserIdParam'
      responses:
        '200':
          description: User details
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/UserResponse'
        '404':
          $ref: '#/components/responses/NotFound'
    
    put:
      tags: [Users]
      summary: Update user
      operationId: updateUser
      parameters:
        - $ref: '#/components/parameters/UserIdParam'
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/UpdateUserRequest'
      responses:
        '200':
          description: User updated
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/UserResponse'
        '404':
          $ref: '#/components/responses/NotFound'
    
    delete:
      tags: [Users]
      summary: Delete user
      operationId: deleteUser
      parameters:
        - $ref: '#/components/parameters/UserIdParam'
      responses:
        '204':
          description: User deleted
        '404':
          $ref: '#/components/responses/NotFound'
  
  /playground/evaluate:
    post:
      tags: [Playground]
      summary: Simulate policy evaluation
      description: |
        Test policies in a sandbox environment without affecting production data.
        Evaluates all combinations of principals x actions x resources.
      operationId: playgroundEvaluate
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/PlaygroundEvaluateRequest'
            examples:
              simple:
                summary: Simple evaluation
                value:
                  policies:
                    - id: "policy-1"
                      content: "permit(principal == User::\"alice\", action == Action::\"ViewPhoto\", resource);"
                  principals:
                    - type: "User"
                      id: "alice"
                      attributes: {}
                  actions: ["ViewPhoto"]
                  resources:
                    - type: "Photo"
                      id: "vacation.jpg"
                      attributes:
                        owner: "alice"
      responses:
        '200':
          description: Evaluation results
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/PlaygroundEvaluateResponse'
        '400':
          $ref: '#/components/responses/BadRequest'
  
  /playground/explain:
    post:
      tags: [Playground]
      summary: Explain authorization decision
      description: Get detailed explanation for a specific authorization decision
      operationId: playgroundExplain
      requestBody:
        required: true
        content:
          application/json:
            schema:
              $ref: '#/components/schemas/PlaygroundExplainRequest'
      responses:
        '200':
          description: Detailed explanation
          content:
            application/json:
              schema:
                $ref: '#/components/schemas/PlaygroundExplainResponse'

components:
  parameters:
    UserIdParam:
      name: userId
      in: path
      required: true
      schema:
        type: string
      description: User ID
    
    PageParam:
      name: page
      in: query
      schema:
        type: integer
        minimum: 1
        default: 1
      description: Page number
    
    PageSizeParam:
      name: page_size
      in: query
      schema:
        type: integer
        minimum: 1
        maximum: 100
        default: 20
      description: Items per page
  
  schemas:
    HealthResponse:
      type: object
      required: [status, version]
      properties:
        status:
          type: string
          enum: [healthy]
        version:
          type: string
          example: "1.0.0"
        uptime_seconds:
          type: integer
    
    CreateUserRequest:
      type: object
      required: [username, email]
      properties:
        username:
          type: string
          minLength: 3
          maxLength: 64
          pattern: '^[a-zA-Z0-9_-]+$'
        email:
          type: string
          format: email
        display_name:
          type: string
        attributes:
          type: object
          additionalProperties: true
        tags:
          type: object
          additionalProperties:
            type: string
    
    UserResponse:
      type: object
      required: [user_id, hrn, username, email, created_at, updated_at]
      properties:
        user_id:
          type: string
        hrn:
          type: string
          example: "hrn:hodei:iam::123456789:user/alice"
        username:
          type: string
        email:
          type: string
        display_name:
          type: string
        attributes:
          type: object
        tags:
          type: object
        created_at:
          type: string
          format: date-time
        updated_at:
          type: string
          format: date-time
    
    UpdateUserRequest:
      type: object
      properties:
        email:
          type: string
          format: email
        display_name:
          type: string
        attributes:
          type: object
        tags:
          type: object
    
    UserListResponse:
      type: object
      required: [users, pagination]
      properties:
        users:
          type: array
          items:
            $ref: '#/components/schemas/UserResponse'
        pagination:
          $ref: '#/components/schemas/PaginationInfo'
    
    PaginationInfo:
      type: object
      required: [page, page_size, total_items, total_pages]
      properties:
        page:
          type: integer
        page_size:
          type: integer
        total_items:
          type: integer
        total_pages:
          type: integer
    
    PlaygroundEvaluateRequest:
      type: object
      required: [policies, principals, actions, resources]
      properties:
        policies:
          type: array
          items:
            $ref: '#/components/schemas/PlaygroundPolicy'
        principals:
          type: array
          items:
            $ref: '#/components/schemas/AdHocEntity'
        actions:
          type: array
          items:
            type: string
        resources:
          type: array
          items:
            $ref: '#/components/schemas/AdHocEntity'
        context:
          type: object
          additionalProperties: true
        schema_version:
          type: string
        evaluation_mode:
          type: string
          enum: [Strict, BestEffortNoSchema, NoSchema]
          default: BestEffortNoSchema
        entities:
          type: array
          items:
            $ref: '#/components/schemas/AdHocEntity'
    
    PlaygroundPolicy:
      type: object
      required: [id, content]
      properties:
        id:
          type: string
        content:
          type: string
          description: Cedar policy DSL
    
    AdHocEntity:
      type: object
      required: [type, id]
      properties:
        type:
          type: string
          example: "User"
        id:
          type: string
          example: "alice"
        attributes:
          type: object
          additionalProperties: true
        parents:
          type: array
          items:
            type: string
          example: ["Group::\"engineers\""]
    
    PlaygroundEvaluateResponse:
      type: object
      required: [evaluation_results, evaluation_metadata]
      properties:
        evaluation_results:
          type: array
          items:
            $ref: '#/components/schemas/EvaluationResultItem'
        evaluation_metadata:
          $ref: '#/components/schemas/EvaluationMetadata'
    
    EvaluationResultItem:
      type: object
      required: [principal, action, resource, decision]
      properties:
        principal:
          type: string
        action:
          type: string
        resource:
          type: string
        decision:
          type: string
          enum: [Allow, Deny]
        determining_policies:
          type: array
          items:
            type: string
        matched_statements:
          type: array
          items:
            $ref: '#/components/schemas/MatchedStatement'
        diagnostics:
          type: array
          items:
            $ref: '#/components/schemas/Diagnostic'
    
    MatchedStatement:
      type: object
      required: [policy_id, effect, reason]
      properties:
        policy_id:
          type: string
        effect:
          type: string
          enum: [permit, forbid]
        reason:
          type: string
    
    Diagnostic:
      type: object
      required: [level, message]
      properties:
        level:
          type: string
          enum: [Info, Warning, Error]
        message:
          type: string
        policy_id:
          type: string
    
    EvaluationMetadata:
      type: object
      required: [total_evaluations, duration_ms]
      properties:
        total_evaluations:
          type: integer
        schema_version_used:
          type: string
        evaluation_mode:
          type: string
        duration_ms:
          type: integer
    
    PlaygroundExplainRequest:
      type: object
      required: [policies, principal, action, resource]
      properties:
        policies:
          type: array
          items:
            $ref: '#/components/schemas/PlaygroundPolicy'
        principal:
          $ref: '#/components/schemas/AdHocEntity'
        action:
          type: string
        resource:
          $ref: '#/components/schemas/AdHocEntity'
        context:
          type: object
        schema_version:
          type: string
    
    PlaygroundExplainResponse:
      type: object
      required: [decision, explanation]
      properties:
        decision:
          type: string
          enum: [Allow, Deny]
        explanation:
          $ref: '#/components/schemas/DecisionExplanation'
    
    DecisionExplanation:
      type: object
      required: [summary]
      properties:
        summary:
          type: string
        permit_policies:
          type: array
          items:
            $ref: '#/components/schemas/PolicyExplanation'
        forbid_policies:
          type: array
          items:
            $ref: '#/components/schemas/PolicyExplanation'
        evaluated_policies_count:
          type: integer
        entity_hierarchy:
          $ref: '#/components/schemas/EntityHierarchy'
        context_keys_used:
          type: array
          items:
            type: string
        context_keys_unused:
          type: array
          items:
            type: string
    
    PolicyExplanation:
      type: object
      properties:
        policy_id:
          type: string
        statement:
          type: string
        reason:
          type: string
        matched_conditions:
          type: array
          items:
            type: string
    
    EntityHierarchy:
      type: object
      properties:
        principal:
          type: array
          items:
            type: string
        resource:
          type: array
          items:
            type: string
    
    ErrorResponse:
      type: object
      required: [error, message]
      properties:
        error:
          type: string
        message:
          type: string
        details:
          type: object
  
  responses:
    BadRequest:
      description: Bad request
      content:
        application/json:
          schema:
            $ref: '#/components/schemas/ErrorResponse'
    
    NotFound:
      description: Resource not found
      content:
        application/json:
          schema:
            $ref: '#/components/schemas/ErrorResponse'
    
    Conflict:
      description: Resource already exists
      content:
        application/json:
          schema:
            $ref: '#/components/schemas/ErrorResponse'

  securitySchemes:
    BearerAuth:
      type: http
      scheme: bearer
      bearerFormat: JWT

security:
  - BearerAuth: []
```

---

## 7. Implementation Plan

### Phase 1: Foundation (Week 1-2)
**Goal:** Complete CRUD operations for hodei-iam

- [ ] Implement missing user features
  - [ ] get_user
  - [ ] list_users
  - [ ] update_user
  - [ ] delete_user
- [ ] Implement missing group features
  - [ ] get_group
  - [ ] list_groups
  - [ ] update_group
  - [ ] delete_group
  - [ ] remove_user_from_group
  - [ ] list_group_members
- [ ] Implement policy attachment features
  - [ ] list_user_policies
  - [ ] list_group_policies
  - [ ] attach_policy_to_user
  - [ ] attach_policy_to_group
  - [ ] detach_policy_from_user
  - [ ] detach_policy_from_group
- [ ] Create REST handlers for all new features
- [ ] Update app_state.rs with new use cases

### Phase 2: Playground Core (Week 3-4)
**Goal:** Implement playground_evaluate feature

- [ ] Create `playground_evaluate` feature in hodei-policies
  - [ ] Define DTOs (PlaygroundEvaluateCommand, PlaygroundEvaluateResult)
  - [ ] Implement use case
  - [ ] Implement ports (SchemaProvider)
  - [ ] Write unit tests with mocks
- [ ] Create REST handlers for playground
  - [ ] POST /playground/evaluate
  - [ ] POST /playground/validate
  - [ ] GET /playground/schema
- [ ] Integration tests

### Phase 3: Playground Advanced (Week 5)
**Goal:** Add explain and test scenario features

- [ ] Implement playground_explain feature
  - [ ] Detailed reasoning extraction from Cedar
  - [ ] Entity hierarchy traversal
  - [ ] Context key usage tracking
- [ ] Create REST handler for explain
  - [ ] POST /playground/explain
- [ ] Implement test scenario feature
  - [ ] POST /playground/test-scenario
  - [ ] Support for batch evaluations with assertions

### Phase 4: OpenAPI & Documentation (Week 6)
**Goal:** Complete API documentation

- [ ] Generate OpenAPI YAML file
- [ ] Set up Swagger UI integration
- [ ] Create API usage examples
- [ ] Write playground tutorial
- [ ] Create Postman collection
- [ ] Add interactive examples to docs

### Phase 5: Polish & Testing (Week 7-8)
**Goal:** Production readiness

- [ ] Comprehensive integration tests
- [ ] Performance benchmarks
- [ ] Error handling improvements
- [ ] Rate limiting
- [ ] API versioning strategy
- [ ] Monitoring and observability
- [ ] Security audit

---

## 8. Examples & Use Cases

### 8.1 Example: Testing Photo Access Policy

**Scenario:** You want to test if your photo access policy works correctly for different users.

**Step 1:** Define your policy
```cedar
permit(
  principal in Group::"photographers",
  action in [Action::"ViewPhoto", Action::"EditPhoto"],
  resource
)
when {
  resource.album.owner == principal || 
  principal in resource.album.sharedWith
};

forbid(
  principal,
  action == Action::"DeletePhoto",
  resource
)
unless {
  resource.owner == principal
};
```

**Step 2:** Call playground API
```bash
curl -X POST https://api.hodei.io/v1/playground/evaluate \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "policies": [
      {
        "id": "photo-access",
        "content": "permit(principal in Group::\"photographers\", action in [Action::\"ViewPhoto\", Action::\"EditPhoto\"], resource) when { resource.album.owner == principal || principal in resource.album.sharedWith };"
      },
      {
        "id": "delete-protection",
        "content": "forbid(principal, action == Action::\"DeletePhoto\", resource) unless { resource.owner == principal };"
      }
    ],
    "principals": [
      {
        "type": "User",
        "id": "alice",
        "attributes": {},
        "parents": ["Group::\"photographers\""]
      },
      {
        "type": "User",
        "id": "bob",
        "attributes": {},
        "parents": []
      }
    ],
    "actions": ["ViewPhoto", "EditPhoto", "DeletePhoto"],
    "resources": [
      {
        "type": "Photo",
        "id": "photo1",
        "attributes": {
          "owner": "alice",
          "album": {
            "owner": "alice",
            "sharedWith": ["User::\"charlie\""]
          }
        }
      }
    ],
    "evaluation_mode": "BestEffortNoSchema"
  }'
```

**Step 3:** Analyze results
```json
{
  "evaluation_results": [
    {
      "principal": "User::\"alice\"",
      "action": "ViewPhoto",
      "resource": "Photo::\"photo1\"",
      "decision": "Allow",
      "determining_policies": ["photo-access"],
      "matched_statements": [
        {
          "policy_id": "photo-access",
          "effect": "permit",
          "reason": "Principal is in Group photographers and resource.album.owner matches principal"
        }
      ],
      "diagnostics": []
    },
    {
      "principal": "User::\"alice\"",
      "action": "DeletePhoto",
      "resource": "Photo::\"photo1\"",
      "decision": "Allow",
      "determining_policies": ["delete-protection"],
      "matched_statements": [
        {
          "policy_id": "delete-protection",
          "effect": "forbid",
          "reason": "Unless clause satisfied: resource.owner == principal"
        }
      ],
      "diagnostics": [
        {
          "level": "Info",
          "message": "Forbid policy did not apply due to unless clause"
        }
      ]
    },
    {
      "principal": "User::\"bob\"",
      "action": "ViewPhoto",
      "resource": "Photo::\"photo1\"",
      "decision": "Deny",
      "determining_policies": [],
      "matched_statements": [],
      "diagnostics": [
        {
          "level": "Info",
          "message": "No policy permits this action for this principal"
        }
      ]
    }
  ],
  "evaluation_metadata": {
    "total_evaluations": 6,
    "schema_version_used": null,
    "evaluation_mode": "BestEffortNoSchema",
    "duration_ms": 15
  }
}
```

### 8.2 Example: Validating Against Schema

```bash
curl -X POST https://api.hodei.io/v1/playground/validate \
  -H "Content-Type: application/json" \
  -d '{
    "policy_content": "permit(principal, action, resource) when { principal.clearance_level >= resource.required_clearance };",
    "validate_against_schema": true,
    "schema_version": "1.0.0"
  }'
```

**Response:**
```json
{
  "valid": true,
  "errors": [],
  "warnings": [],
  "schema_validation": {
    "entities_referenced": ["principal", "resource"],
    "actions_referenced": [],
    "attributes_used": ["principal.clearance_level", "resource.required_clearance"],
    "all_entities_exist": true,
    "all_actions_exist": true,
    "all_attributes_typed": true,
    "type_information": {
      "principal.clearance_level": "Long",
      "resource.required_clearance": "Long"
    }
  }
}
```

### 8.3 Example: Explaining a Decision

```bash
curl -X POST https://api.hodei.io/v1/playground/explain \
  -H "Content-Type: application/json" \
  -d '{
    "policies": [...],
    "principal": {
      "type": "User",
      "id": "alice",
      "attributes": {"department": "Engineering"},
      "parents": ["Group::\"engineers\"", "Group::\"employees\""]
    },
    "action": "ReadDocument",
    "resource": {
      "type": "Document",
      "id": "doc123",
      "attributes": {"classification": "internal"},
      "parents": ["Folder::\"engineering-docs\""]
    },
    "context": {"time": "2024-01-15T14:30:00Z"}
  }'
```

**Response:**
```json
{
  "decision": "Allow",
  "explanation": {
    "summary": "Access allowed by policy 'engineering-read-access'",
    "permit_policies": [
      {
        "policy_id": "engineering-read-access",
        "statement": "permit(principal in Group::\"engineers\", action == Action::\"ReadDocument\", resource in Folder::\"engineering-docs\");",
        "reason": "Principal alice is member of Group engineers, and resource doc123 is in Folder engineering-docs",
        "matched_conditions": [
          "principal in Group::\"engineers\" ✓",
          "action == Action::\"ReadDocument\" ✓",
          "resource in Folder::\"engineering-docs\" ✓"
        ]
      }
    ],
    "forbid_policies": [],
    "evaluated_policies_count": 12,
    "entity_hierarchy": {
      "principal": [
        "User::\"alice\"",
        "Group::\"engineers\"",
        "Group::\"employees\""
      ],
      "resource": [
        "Document::\"doc123\"",
        "Folder::\"engineering-docs\""
      ]
    },
    "context_keys_used": [],
    "context_keys_unused": ["time"]
  }
}
```

---

## 9. Checklist: Architecture Compliance

Before implementation, verify compliance with Hodei architecture rules:

- [ ] Each feature is in its own vertical slice with all required files
- [ ] `use_case.rs` depends only on traits from `ports.rs`
- [ ] All `internal/` modules are `pub(crate)`
- [ ] API surface is centralized in `api.rs`
- [ ] No direct coupling between bounded contexts
- [ ] All dependencies injected via DI factory in `di.rs`
- [ ] Comprehensive unit tests in `use_case_test.rs`
- [ ] No `println!` - only `tracing` for logging
- [ ] Code compiles without errors or warnings
- [ ] All tests pass

---

## 10. Next Steps

1. **Review this design document** with the team
2. **Prioritize features** based on business needs
3. **Create implementation tickets** for each phase
4. **Set up CI/CD** for automated testing
5. **Begin Phase 1** implementation

---

**Document Status:** ✅ Complete and ready for review  
**Next Review Date:** TBD  
**Owner:** Engineering Team