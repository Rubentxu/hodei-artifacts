# Hodei Artifacts - Policy Playground: Executive Summary

**Date:** 2024  
**Version:** 1.0  
**Status:** Ready for Implementation

---

## TL;DR

After analyzing the AWS IAM Policy Simulator and the current Hodei codebase, we have a clear path to implement a complete policy playground. The existing architecture already supports 90% of what we need. We must:

1. **Add 13 missing IAM actions** to complete CRUD operations
2. **Create 1 new feature** (`playground_evaluate` in hodei-policies)
3. **Expose REST endpoints** for existing + new features
4. **Generate OpenAPI spec** documenting the complete API

**Estimated Effort:** 4 weeks (1 developer)  
**Complexity:** Medium  
**Risk:** Low (building on solid foundation)

---

## Current State

### ✅ What We Have

**Entities (hodei-iam):**
- `User` - Fully implemented with HodeiEntity, Principal, Resource traits
- `Group` - Fully implemented with HodeiEntity, Resource trait

**Actions (hodei-iam):**
- CreateUser, CreateGroup
- DeleteUser, DeleteGroup  
- AddUserToGroup, RemoveUserFromGroup

**Features (hodei-iam):**
- create_user, create_group, add_user_to_group
- create_policy, get_policy, list_policies, update_policy, delete_policy
- evaluate_iam_policies, get_effective_policies
- register_iam_schema

**Features (hodei-policies):**
- register_entity_type, register_action_type
- build_schema, load_schema
- validate_policy, evaluate_policies

**Infrastructure:**
- Cedar policy engine integration
- Schema management and persistence
- Vertical Slice Architecture throughout
- Complete DI and testing framework

---

## What's Missing

### ❌ Missing IAM Actions (13 total)

**User Management:**
- GetUser, ListUsers, UpdateUser

**Group Management:**
- GetGroup, ListGroups, UpdateGroup, ListGroupMembers

**Policy Management:**
- AttachPolicy, DetachPolicy, GetPolicyAction, ListPoliciesAction, UpdatePolicyAction, DeletePolicyAction

### ❌ Missing Features (1 total)

**Playground Evaluation:**
- `playground_evaluate` - Ad-hoc policy testing without persistence

### ❌ Missing REST API

Currently no REST endpoints exposed for any features.

---

## Key Findings from AWS Research

### AWS Policy Simulator Capabilities

**Inputs:**
- Multiple policies as JSON strings
- Multiple actions to test
- Multiple resources (ARNs)
- Context variables (IP, time, MFA status, etc.)
- Caller identity

**Outputs:**
- Allow/Deny decision for each action+resource combination
- Matched policy statements (with line numbers)
- Missing context values
- Detailed diagnostics

**Key Insight:** It's essentially a **cartesian product evaluation** - N principals × M actions × P resources, all evaluated against a policy set.

### Mapping to Cedar/Hodei

| AWS Concept | Cedar/Hodei Equivalent |
|-------------|------------------------|
| IAM Policy (JSON) | Cedar Policy (DSL) |
| Principal (ARN) | User (HodeiEntity + Principal trait) |
| Action (API call) | Action (ActionTrait implementations) |
| Resource (ARN) | User/Group (HodeiEntity + Resource trait) |
| Context Keys | Context (HashMap) |
| PolicySet | HodeiPolicySet |
| Allow/Deny | Decision enum |

**Key Difference:** Cedar is more powerful - explicit forbid policies, hierarchical entities, stronger type safety with schemas.

---

## Proposed Solution

### Phase 1: Complete IAM Actions (Week 1)

**Task:** Add 13 missing actions to `hodei-iam/src/internal/domain/actions.rs`

Each action follows this pattern:
```rust
pub struct GetUserAction;

impl ActionTrait for GetUserAction {
    fn name() -> &'static str { "GetUser" }
    fn service_name() -> ServiceName { ServiceName::new("iam").unwrap() }
    fn applies_to_principal() -> String { "Iam::User".to_string() }
    fn applies_to_resource() -> String { "Iam::User".to_string() }
}
```

**Update:** `register_iam_schema` use case to register all new actions.

**Deliverable:** Complete IAM action set registered in Cedar schema.

---

### Phase 2: Playground Feature (Week 2)

**Create:** `hodei-policies/src/features/playground_evaluate/`

**Structure:**
```
playground_evaluate/
├── mod.rs              - Module exports
├── dto.rs              - Request/response models
├── use_case.rs         - Core evaluation logic
├── error.rs            - Error types
├── di.rs               - Factory for DI
└── use_case_test.rs    - Unit tests
```

**Key DTOs:**
- `PlaygroundEvaluateCommand` - Input with policies, principals, actions, resources, context
- `AdHocUser` / `AdHocGroup` - Ad-hoc entities (not persisted)
- `PlaygroundEvaluationResult` - Output with decisions and diagnostics

**Logic:**
1. Parse Cedar policies from strings
2. Convert ad-hoc entities to Cedar Entities
3. Build entity store with hierarchies
4. For each principal × action × resource:
   - Create Cedar Request
   - Call Cedar Authorizer
   - Extract decision and diagnostics
5. Return aggregated results

**Deliverable:** Working playground feature with tests.

---

### Phase 3: REST API (Week 3)

**Expose endpoints for:**

```
# Health
GET  /health

# Users
POST   /users
GET    /users/{id}
PUT    /users/{id}
DELETE /users/{id}
GET    /users

# Groups
POST   /groups
GET    /groups/{id}
PUT    /groups/{id}
DELETE /groups/{id}
GET    /groups
POST   /groups/{id}/members/{user_id}
DELETE /groups/{id}/members/{user_id}
GET    /groups/{id}/members

# Policies
POST   /policies
GET    /policies/{id}
PUT    /policies/{id}
DELETE /policies/{id}
GET    /policies
POST   /policies/validate

# Authorization
POST   /authorization/evaluate
GET    /authorization/effective-policies/{user_id}

# Schemas
POST   /schemas/build
GET    /schemas/load
POST   /schemas/register-iam

# Playground
POST   /playground/evaluate
```

**Deliverable:** Complete REST API with Axum handlers.

---

### Phase 4: Documentation (Week 4)

**Create:**
- OpenAPI 3.0 specification
- Postman collection
- Usage examples
- Integration tests

**Deliverable:** Production-ready API with documentation.

---

## Example Usage

### Scenario: Test Admin Policy

**Policy:**
```cedar
permit(
  principal in Iam::Group::"admins",
  action,
  resource
);
```

**Playground Request:**
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
      "tags": []
    }
  ],
  "actions": ["CreateUser", "DeleteUser", "UpdateUser"],
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
  ]
}
```

**Expected Result:**
- All 3 actions: **Allow** (Alice is admin)
- Total evaluations: 3 (1 principal × 3 actions × 1 resource)
- Duration: ~15ms

---

## Benefits

### For Developers
- **Test policies before deployment** - No production impact
- **Debug authorization issues** - See exactly why access was denied
- **Understand policy behavior** - Visual feedback on policy effects
- **Iterate quickly** - Instant feedback loop

### For Security Teams
- **Validate access controls** - Verify policies work as intended
- **Document requirements** - Policies as executable documentation
- **Audit readiness** - Clear trail of authorization decisions
- **Compliance** - Prove least-privilege enforcement

### For Operations
- **No downtime testing** - Playground doesn't affect production
- **Scenario planning** - Test edge cases and failures
- **Training tool** - Educate team on Cedar policies
- **Migration validation** - Test before moving to production

---

## Architecture Compliance

✅ **All features follow Clean Architecture:**
- Vertical Slice Architecture per feature
- Dependency Inversion (traits, not concrete types)
- No coupling between bounded contexts
- Complete test coverage
- No `println!` (only `tracing`)

✅ **Quality gates:**
- `cargo check` - No compilation errors
- `cargo clippy -- -D warnings` - No warnings
- `cargo nextest run` - All tests pass

---

## Risks & Mitigations

| Risk | Impact | Probability | Mitigation |
|------|--------|-------------|------------|
| Cedar API changes | High | Low | Pin Cedar version, monitor releases |
| Performance with large policy sets | Medium | Medium | Implement caching, pagination |
| Complex entity hierarchies | Medium | Low | Thorough testing, examples |
| Schema evolution | Medium | Medium | Versioning strategy, backward compatibility |

---

## Success Metrics

**Phase 1:**
- [ ] 13 actions implemented and tested
- [ ] IAM schema contains all actions
- [ ] All tests passing

**Phase 2:**
- [ ] Playground feature working
- [ ] Can evaluate ad-hoc scenarios
- [ ] Unit tests at 100% coverage

**Phase 3:**
- [ ] All REST endpoints functional
- [ ] Can call playground via HTTP
- [ ] Integration tests passing

**Phase 4:**
- [ ] OpenAPI spec complete
- [ ] Documentation published
- [ ] Ready for production

---

## Recommendations

### Immediate Actions

1. **Approve this plan** - Get stakeholder buy-in
2. **Assign developer** - Allocate 1 FTE for 4 weeks
3. **Create GitHub issues** - Break down into tasks
4. **Set up project board** - Track progress

### Technical Priorities

1. **Start with Phase 1** - Actions are foundational
2. **Don't skip tests** - They save time later
3. **Follow architecture rules strictly** - No shortcuts
4. **Document as you go** - Don't defer to end

### Long-term Considerations

1. **Performance optimization** - Profile with realistic loads
2. **UI/Frontend** - Build web UI for playground (future)
3. **Policy library** - Curate common policy patterns
4. **Monitoring** - Track policy evaluation patterns

---

## Conclusion

We have a **solid foundation** and a **clear path forward**. The existing architecture is well-designed and supports exactly what we need. The AWS research validates our approach and provides a proven model to follow.

**This is highly achievable in 4 weeks** with low risk and high value.

**Recommendation:** ✅ **Proceed with implementation**

---

## Appendix: Related Documents

- [DESIGN_REST_API_AND_PLAYGROUND.md](./DESIGN_REST_API_AND_PLAYGROUND.md) - Complete API specification
- [PLAYGROUND_EXAMPLES.md](./PLAYGROUND_EXAMPLES.md) - Practical usage examples
- [PLAYGROUND_IMPLEMENTATION_PLAN.md](./PLAYGROUND_IMPLEMENTATION_PLAN.md) - Detailed implementation steps

---

**Prepared by:** Engineering Team  
**Review Status:** Ready for Approval  
**Next Review:** After Phase 1 completion