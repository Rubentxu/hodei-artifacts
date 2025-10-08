# Integration & E2E Tests Implementation Plan

## Test Coverage Goal: 80%+

### Test Categories

#### 1. Integration Tests (tests/integration/)
Individual handler testing with real database:

**test_create_policy.rs** ✅ COMPLETE
- ✅ Success: Valid policy creation
- ✅ Error: Empty policy ID
- ✅ Error: Empty content
- ✅ Error: Invalid Cedar syntax
- ✅ Error: Duplicate policy
- ✅ Edge: Multiple policies sequential
- ✅ Edge: Special characters in ID
- ✅ Edge: Very long content
- ✅ Edge: Unicode in description
- ✅ Verify: Timestamps

**test_get_policy.rs** ✅ COMPLETE
- ✅ Success: Get existing policy
- ✅ Error: Policy not found
- ✅ Error: Invalid HRN type
- ✅ Edge: Multiple gets sequential
- ✅ Edge: Special characters
- ✅ Edge: Immediately after creation
- ✅ Edge: Long content
- ✅ Concurrent: Multiple reads
- ✅ Performance: < 1s

**test_list_policies.rs** (TO IMPLEMENT)
- Success: List with pagination
- Success: First page
- Success: Middle page
- Success: Last page
- Success: Empty list
- Edge: Limit boundary (1, 50, 100)
- Edge: Offset beyond total
- Edge: Large dataset (1000+ policies)
- Verify: Pagination metadata
- Performance: < 2s for 100 items

**test_update_policy.rs** (TO IMPLEMENT)
- Success: Update content
- Success: Update description
- Error: Policy not found
- Error: Empty content
- Error: Invalid Cedar syntax
- Edge: Update immediately after creation
- Edge: Multiple updates sequential
- Concurrent: Conflict detection
- Verify: Updated timestamp changes

**test_delete_policy.rs** (TO IMPLEMENT)
- Success: Delete existing policy
- Error: Policy not found
- Error: Policy in use (if applicable)
- Edge: Delete immediately after creation
- Edge: Delete and recreate same ID
- Concurrent: Multiple deletes
- Verify: Policy removed from DB

**test_policy_lifecycle.rs** (TO IMPLEMENT)
- Full lifecycle: Create → Get → Update → Get → Delete
- Bulk operations: Create many → List → Delete all
- State transitions verification
- Rollback scenarios

**test_concurrency.rs** (TO IMPLEMENT)
- Concurrent creates (different IDs)
- Concurrent creates (same ID) - conflict
- Concurrent updates (same policy) - version conflict
- Concurrent get while updating
- Race conditions handling

#### 2. E2E Tests (tests/e2e/)

**test_policy_workflows.rs** (TO IMPLEMENT)
- Complete CRUD workflow
- Policy validation workflow
- Bulk policy management
- Error recovery workflows

**test_policy_pagination.rs** (TO IMPLEMENT)
- Navigate through pages forward
- Navigate backward
- Jump to specific page
- Handle dynamic data changes

**test_policy_edge_cases.rs** (TO IMPLEMENT)
- Maximum policies per account
- Very long policy IDs
- Complex Cedar expressions
- Unicode and special characters
- Boundary conditions

### Test Utilities

**common/test_db.rs** ✅ COMPLETE
- TestDb with testcontainers
- Clean/seed operations
- Namespace/DB management

**common/fixtures.rs** ✅ COMPLETE
- Sample policies
- Error scenarios
- Pagination scenarios
- Test users

**common/helpers.rs** ✅ COMPLETE
- TestClient for HTTP
- Database operations
- Assertions
- Performance measurement
- MockSchemaStorage

### Coverage Metrics

| Component | Target | Current |
|-----------|--------|---------|
| create_policy | 90% | 95% ✅ |
| get_policy | 90% | 95% ✅ |
| list_policies | 90% | 0% 🔴 |
| update_policy | 90% | 0% 🔴 |
| delete_policy | 90% | 0% 🔴 |
| Lifecycle | 80% | 0% 🔴 |
| Concurrency | 80% | 0% 🔴 |
| E2E Workflows | 80% | 0% 🔴 |
| **TOTAL** | **80%** | **~25%** |

### Test Execution Strategy

1. **Unit Tests**: Fast, isolated (already done in use_case_test.rs)
2. **Integration Tests**: Real DB, isolated per test
3. **E2E Tests**: Full stack, realistic scenarios
4. **Performance Tests**: Embedded in integration tests

### Next Steps

1. ✅ Setup test infrastructure (DONE)
2. ✅ Create_policy tests (DONE)
3. ✅ Get_policy tests (DONE)
4. ⏭️ List_policies tests (NEXT)
5. ⏭️ Update_policy tests
6. ⏭️ Delete_policy tests
7. ⏭️ Lifecycle tests
8. ⏭️ Concurrency tests
9. ⏭️ E2E workflows
10. ⏭️ Generate coverage report

