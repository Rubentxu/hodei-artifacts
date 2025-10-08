# Hodei Policy Playground - Practical Examples & Use Cases

**Version:** 1.0  
**Last Updated:** 2024

---

## Table of Contents

1. [Introduction](#introduction)
2. [Basic Examples](#basic-examples)
3. [Advanced Scenarios](#advanced-scenarios)
4. [Real-World Use Cases](#real-world-use-cases)
5. [Testing Patterns](#testing-patterns)
6. [Troubleshooting Guide](#troubleshooting-guide)
7. [Best Practices](#best-practices)

---

## 1. Introduction

This document provides practical examples for using the Hodei Policy Playground to test and validate Cedar policies. The playground allows you to simulate authorization decisions without affecting production data.

**Key Benefits:**
- Test policies before deploying to production
- Debug authorization issues
- Validate policy logic with different scenarios
- Educate team members about policy behavior
- Document authorization requirements through examples

---

## 2. Basic Examples

### 2.1 Simple Allow Policy

**Scenario:** Allow Alice to view a specific photo.

**Cedar Policy:**
```cedar
permit(
  principal == User::"alice",
  action == Action::"ViewPhoto",
  resource == Photo::"vacation.jpg"
);
```

**Playground Request:**
```json
{
  "policies": [
    {
      "id": "alice-view-vacation",
      "content": "permit(principal == User::\"alice\", action == Action::\"ViewPhoto\", resource == Photo::\"vacation.jpg\");"
    }
  ],
  "principals": [
    {
      "type": "User",
      "id": "alice",
      "attributes": {}
    }
  ],
  "actions": ["ViewPhoto"],
  "resources": [
    {
      "type": "Photo",
      "id": "vacation.jpg",
      "attributes": {}
    }
  ]
}
```

**Expected Result:**
- Decision: **Allow**
- Determining Policy: `alice-view-vacation`

---

### 2.2 Wildcard Resource Policy

**Scenario:** Allow all users in the "viewers" group to view any photo.

**Cedar Policy:**
```cedar
permit(
  principal in Group::"viewers",
  action == Action::"ViewPhoto",
  resource
);
```

**Playground Request:**
```json
{
  "policies": [
    {
      "id": "viewers-can-view-all",
      "content": "permit(principal in Group::\"viewers\", action == Action::\"ViewPhoto\", resource);"
    }
  ],
  "principals": [
    {
      "type": "User",
      "id": "alice",
      "attributes": {},
      "parents": ["Group::\"viewers\""]
    }
  ],
  "actions": ["ViewPhoto"],
  "resources": [
    {
      "type": "Photo",
      "id": "photo1.jpg",
      "attributes": {}
    },
    {
      "type": "Photo",
      "id": "photo2.jpg",
      "attributes": {}
    }
  ],
  "entities": [
    {
      "type": "Group",
      "id": "viewers",
      "attributes": {
        "name": "Photo Viewers"
      }
    }
  ]
}
```

**Expected Result:**
- Both photo1.jpg and photo2.jpg: **Allow**
- Reason: Alice is member of Group "viewers"

---

### 2.3 Attribute-Based Access Control (ABAC)

**Scenario:** Allow users with clearance level ≥ 5 to view confidential documents.

**Cedar Policy:**
```cedar
permit(
  principal,
  action == Action::"ViewDocument",
  resource
)
when {
  principal.clearance_level >= 5 &&
  resource.classification == "confidential"
};
```

**Playground Request:**
```json
{
  "policies": [
    {
      "id": "clearance-based-access",
      "content": "permit(principal, action == Action::\"ViewDocument\", resource) when { principal.clearance_level >= 5 && resource.classification == \"confidential\" };"
    }
  ],
  "principals": [
    {
      "type": "User",
      "id": "alice",
      "attributes": {
        "clearance_level": 7
      }
    },
    {
      "type": "User",
      "id": "bob",
      "attributes": {
        "clearance_level": 3
      }
    }
  ],
  "actions": ["ViewDocument"],
  "resources": [
    {
      "type": "Document",
      "id": "secret-doc",
      "attributes": {
        "classification": "confidential"
      }
    }
  ]
}
```

**Expected Result:**
- Alice + secret-doc: **Allow** (clearance 7 >= 5)
- Bob + secret-doc: **Deny** (clearance 3 < 5)

---

### 2.4 Explicit Forbid Policy

**Scenario:** Forbid deletion of resources unless the user is the owner.

**Cedar Policy:**
```cedar
forbid(
  principal,
  action == Action::"DeletePhoto",
  resource
)
unless {
  resource.owner == principal
};
```

**Playground Request:**
```json
{
  "policies": [
    {
      "id": "protect-deletion",
      "content": "forbid(principal, action == Action::\"DeletePhoto\", resource) unless { resource.owner == principal };"
    }
  ],
  "principals": [
    {
      "type": "User",
      "id": "alice",
      "attributes": {}
    },
    {
      "type": "User",
      "id": "bob",
      "attributes": {}
    }
  ],
  "actions": ["DeletePhoto"],
  "resources": [
    {
      "type": "Photo",
      "id": "alice-photo",
      "attributes": {
        "owner": "alice"
      }
    }
  ]
}
```

**Expected Result:**
- Alice + alice-photo: **Deny** (unless clause satisfied, forbid doesn't apply, but no permit exists)
- Bob + alice-photo: **Deny** (explicitly forbidden)

**Note:** For Alice to delete, you need BOTH a permit policy AND the forbid unless clause to be satisfied.

---

## 3. Advanced Scenarios

### 3.1 Hierarchical Resource Access

**Scenario:** Users can access photos in albums they have access to.

**Cedar Policies:**
```cedar
// Policy 1: Grant access to album
permit(
  principal in Group::"team-members",
  action in [Action::"ViewPhoto", Action::"ViewAlbum"],
  resource in Album::"team-album"
);

// Policy 2: Inherit permissions through hierarchy
permit(
  principal,
  action == Action::"ViewPhoto",
  resource
)
when {
  resource in Album::"team-album"
};
```

**Playground Request:**
```json
{
  "policies": [
    {
      "id": "team-album-access",
      "content": "permit(principal in Group::\"team-members\", action in [Action::\"ViewPhoto\", Action::\"ViewAlbum\"], resource in Album::\"team-album\");"
    }
  ],
  "principals": [
    {
      "type": "User",
      "id": "alice",
      "attributes": {},
      "parents": ["Group::\"team-members\""]
    }
  ],
  "actions": ["ViewPhoto", "ViewAlbum"],
  "resources": [
    {
      "type": "Photo",
      "id": "team-photo-1",
      "attributes": {},
      "parents": ["Album::\"team-album\""]
    },
    {
      "type": "Album",
      "id": "team-album",
      "attributes": {}
    }
  ],
  "entities": [
    {
      "type": "Group",
      "id": "team-members",
      "attributes": {}
    },
    {
      "type": "Album",
      "id": "team-album",
      "attributes": {}
    }
  ]
}
```

---

### 3.2 Context-Based Access Control

**Scenario:** Allow document editing only during business hours and with MFA.

**Cedar Policy:**
```cedar
permit(
  principal,
  action == Action::"EditDocument",
  resource
)
when {
  context.mfa_authenticated == true &&
  context.hour >= 9 &&
  context.hour <= 17 &&
  context.day_of_week != "Saturday" &&
  context.day_of_week != "Sunday"
};
```

**Playground Request:**
```json
{
  "policies": [
    {
      "id": "business-hours-mfa-edit",
      "content": "permit(principal, action == Action::\"EditDocument\", resource) when { context.mfa_authenticated == true && context.hour >= 9 && context.hour <= 17 && context.day_of_week != \"Saturday\" && context.day_of_week != \"Sunday\" };"
    }
  ],
  "principals": [
    {
      "type": "User",
      "id": "alice",
      "attributes": {}
    }
  ],
  "actions": ["EditDocument"],
  "resources": [
    {
      "type": "Document",
      "id": "work-doc",
      "attributes": {}
    }
  ],
  "context": {
    "mfa_authenticated": true,
    "hour": 14,
    "day_of_week": "Monday"
  }
}
```

**Test Different Scenarios:**

**Scenario A: During business hours with MFA**
```json
"context": {
  "mfa_authenticated": true,
  "hour": 14,
  "day_of_week": "Monday"
}
```
Result: **Allow**

**Scenario B: After hours**
```json
"context": {
  "mfa_authenticated": true,
  "hour": 20,
  "day_of_week": "Monday"
}
```
Result: **Deny**

**Scenario C: Weekend**
```json
"context": {
  "mfa_authenticated": true,
  "hour": 14,
  "day_of_week": "Saturday"
}
```
Result: **Deny**

**Scenario D: No MFA**
```json
"context": {
  "mfa_authenticated": false,
  "hour": 14,
  "day_of_week": "Monday"
}
```
Result: **Deny**

---

### 3.3 Multiple Policy Interaction

**Scenario:** Combine multiple policies to create complex authorization logic.

**Cedar Policies:**
```cedar
// Base access: Engineers can view technical docs
permit(
  principal in Group::"engineers",
  action == Action::"ViewDocument",
  resource
)
when {
  resource.category == "technical"
};

// Additional access: Managers can view all docs
permit(
  principal in Group::"managers",
  action == Action::"ViewDocument",
  resource
);

// Restriction: Forbid access to confidential docs during probation
forbid(
  principal,
  action == Action::"ViewDocument",
  resource
)
when {
  principal.on_probation == true &&
  resource.confidential == true
};
```

**Playground Request:**
```json
{
  "policies": [
    {
      "id": "engineers-technical-docs",
      "content": "permit(principal in Group::\"engineers\", action == Action::\"ViewDocument\", resource) when { resource.category == \"technical\" };"
    },
    {
      "id": "managers-all-docs",
      "content": "permit(principal in Group::\"managers\", action == Action::\"ViewDocument\", resource);"
    },
    {
      "id": "probation-restriction",
      "content": "forbid(principal, action == Action::\"ViewDocument\", resource) when { principal.on_probation == true && resource.confidential == true };"
    }
  ],
  "principals": [
    {
      "type": "User",
      "id": "alice",
      "attributes": {
        "on_probation": false
      },
      "parents": ["Group::\"engineers\""]
    },
    {
      "type": "User",
      "id": "bob",
      "attributes": {
        "on_probation": true
      },
      "parents": ["Group::\"engineers\""]
    },
    {
      "type": "User",
      "id": "charlie",
      "attributes": {
        "on_probation": false
      },
      "parents": ["Group::\"managers\""]
    }
  ],
  "actions": ["ViewDocument"],
  "resources": [
    {
      "type": "Document",
      "id": "tech-doc",
      "attributes": {
        "category": "technical",
        "confidential": false
      }
    },
    {
      "type": "Document",
      "id": "secret-tech-doc",
      "attributes": {
        "category": "technical",
        "confidential": true
      }
    },
    {
      "type": "Document",
      "id": "hr-doc",
      "attributes": {
        "category": "hr",
        "confidential": true
      }
    }
  ],
  "entities": [
    {
      "type": "Group",
      "id": "engineers",
      "attributes": {}
    },
    {
      "type": "Group",
      "id": "managers",
      "attributes": {}
    }
  ]
}
```

**Expected Results:**

| Principal | Resource | Decision | Reason |
|-----------|----------|----------|--------|
| Alice (engineer, not probation) | tech-doc | **Allow** | Permit by engineers-technical-docs |
| Alice | secret-tech-doc | **Allow** | Permit by engineers-technical-docs, forbid doesn't apply (not on probation) |
| Bob (engineer, on probation) | tech-doc | **Allow** | Permit by engineers-technical-docs, doc not confidential |
| Bob | secret-tech-doc | **Deny** | Explicitly forbidden by probation-restriction |
| Charlie (manager) | hr-doc | **Allow** | Permit by managers-all-docs (managers can view all) |
| Alice | hr-doc | **Deny** | No permit policy applies |

---

## 4. Real-World Use Cases

### 4.1 Document Management System

**Requirements:**
- Authors can edit their own documents
- Editors can edit any document in their assigned folders
- Viewers can only read documents
- Confidential documents require clearance level ≥ 5

**Cedar Policies:**
```cedar
// Authors own their documents
permit(
  principal,
  action in [Action::"ViewDocument", Action::"EditDocument", Action::"DeleteDocument"],
  resource
)
when {
  resource.author == principal
};

// Editors in assigned folders
permit(
  principal in Group::"editors",
  action in [Action::"ViewDocument", Action::"EditDocument"],
  resource
)
when {
  resource.folder in principal.assigned_folders
};

// Viewers can read
permit(
  principal in Group::"viewers",
  action == Action::"ViewDocument",
  resource
);

// Clearance requirement for confidential docs
forbid(
  principal,
  action,
  resource
)
when {
  resource.confidential == true &&
  principal.clearance_level < 5
};
```

**Test Case:**
```json
{
  "policies": [
    { "id": "author-owns", "content": "..." },
    { "id": "editors-assigned", "content": "..." },
    { "id": "viewers-read", "content": "..." },
    { "id": "clearance-required", "content": "..." }
  ],
  "principals": [
    {
      "type": "User",
      "id": "author-alice",
      "attributes": {
        "clearance_level": 3,
        "assigned_folders": []
      }
    },
    {
      "type": "User",
      "id": "editor-bob",
      "attributes": {
        "clearance_level": 7,
        "assigned_folders": ["Folder::\"marketing\""]
      },
      "parents": ["Group::\"editors\""]
    }
  ],
  "actions": ["ViewDocument", "EditDocument", "DeleteDocument"],
  "resources": [
    {
      "type": "Document",
      "id": "doc1",
      "attributes": {
        "author": "author-alice",
        "folder": "Folder::\"marketing\"",
        "confidential": false
      }
    },
    {
      "type": "Document",
      "id": "doc2",
      "attributes": {
        "author": "author-alice",
        "folder": "Folder::\"marketing\"",
        "confidential": true
      }
    }
  ]
}
```

---

### 4.2 Healthcare Records System (HIPAA Compliance)

**Requirements:**
- Doctors can view/edit records of their patients
- Nurses can view records of patients in their ward
- Admin staff can only view billing information
- Emergency access requires audit logging
- Break-glass access for emergencies

**Cedar Policies:**
```cedar
// Doctors access their patients
permit(
  principal in Role::"doctor",
  action in [Action::"ViewRecord", Action::"EditRecord"],
  resource
)
when {
  principal in resource.treating_physicians
};

// Nurses in ward
permit(
  principal in Role::"nurse",
  action == Action::"ViewRecord",
  resource
)
when {
  resource.ward == principal.assigned_ward
};

// Admin billing only
permit(
  principal in Role::"admin",
  action == Action::"ViewBilling",
  resource
);

// Emergency break-glass
permit(
  principal,
  action in [Action::"ViewRecord", Action::"EditRecord"],
  resource
)
when {
  context.emergency == true &&
  context.break_glass_authorized == true
};

// Audit requirement
forbid(
  principal,
  action,
  resource
)
when {
  resource.sensitive == true &&
  context.audit_enabled == false
};
```

---

### 4.3 Multi-Tenant SaaS Application

**Requirements:**
- Users can only access resources in their organization
- Organization admins have full access within their org
- Super admins can access any organization
- Trial accounts have feature restrictions

**Cedar Policies:**
```cedar
// Tenant isolation
forbid(
  principal,
  action,
  resource
)
unless {
  principal.organization == resource.organization
};

// Org admins
permit(
  principal in Role::"org_admin",
  action,
  resource
)
when {
  principal.organization == resource.organization
};

// Super admins
permit(
  principal in Role::"super_admin",
  action,
  resource
);

// Trial limitations
forbid(
  principal,
  action in [Action::"ExportData", Action::"APIAccess", Action::"CustomIntegration"],
  resource
)
when {
  principal.organization.subscription_type == "trial"
};
```

**Test Multi-Tenant Isolation:**
```json
{
  "policies": [...],
  "principals": [
    {
      "type": "User",
      "id": "alice",
      "attributes": {
        "organization": "org-acme"
      }
    },
    {
      "type": "User",
      "id": "bob",
      "attributes": {
        "organization": "org-globex"
      }
    }
  ],
  "actions": ["ViewData"],
  "resources": [
    {
      "type": "Data",
      "id": "acme-data",
      "attributes": {
        "organization": "org-acme"
      }
    },
    {
      "type": "Data",
      "id": "globex-data",
      "attributes": {
        "organization": "org-globex"
      }
    }
  ]
}
```

**Expected:**
- Alice can access acme-data: **Possible** (if she has a permit policy)
- Alice cannot access globex-data: **Deny** (tenant isolation)
- Bob can access globex-data: **Possible** (if he has a permit policy)
- Bob cannot access acme-data: **Deny** (tenant isolation)

---

## 5. Testing Patterns

### 5.1 Regression Testing Pattern

Test that policy changes don't break existing access:

```json
{
  "test_name": "Regression: Existing user access",
  "policies": [
    { "id": "new-policy-v2", "content": "..." }
  ],
  "test_cases": [
    {
      "description": "Admin still has full access",
      "principal": { "type": "User", "id": "admin" },
      "action": "DeleteUser",
      "resource": { "type": "User", "id": "test-user" },
      "expected_decision": "Allow"
    },
    {
      "description": "Regular user still cannot delete",
      "principal": { "type": "User", "id": "regular-user" },
      "action": "DeleteUser",
      "resource": { "type": "User", "id": "test-user" },
      "expected_decision": "Deny"
    }
  ]
}
```

### 5.2 Boundary Testing Pattern

Test edge cases and boundaries:

```json
{
  "test_name": "Boundary: Clearance levels",
  "policies": [
    {
      "id": "clearance-policy",
      "content": "permit(principal, action, resource) when { principal.clearance >= 5 };"
    }
  ],
  "test_cases": [
    { "clearance": 4, "expected": "Deny" },
    { "clearance": 5, "expected": "Allow" },
    { "clearance": 6, "expected": "Allow" }
  ]
}
```

### 5.3 Negative Testing Pattern

Verify that unauthorized access is denied:

```json
{
  "test_name": "Negative: Unauthorized access attempts",
  "test_cases": [
    {
      "description": "Non-owner cannot delete",
      "principal": { "type": "User", "id": "alice" },
      "action": "Delete",
      "resource": { "type": "File", "id": "bob-file", "attributes": { "owner": "bob" } },
      "expected_decision": "Deny"
    },
    {
      "description": "Suspended user cannot access",
      "principal": { "type": "User", "id": "suspended-user", "attributes": { "status": "suspended" } },
      "action": "ViewData",
      "resource": { "type": "Data", "id": "any-data" },
      "expected_decision": "Deny"
    }
  ]
}
```

---

## 6. Troubleshooting Guide

### 6.1 Common Issues

#### Issue: "No policy permits this action"

**Symptom:** Decision is Deny, but you expected Allow.

**Troubleshooting Steps:**
1. Check if you have at least one `permit` policy
2. Verify principal matches the policy's principal scope
3. Verify action matches exactly (case-sensitive)
4. Verify resource matches
5. Check `when` clause conditions are satisfied
6. Use `/playground/explain` endpoint for detailed reasoning

**Example:**
```json
{
  "decision": "Deny",
  "diagnostics": [
    {
      "level": "Info",
      "message": "No policy explicitly permits this action"
    }
  ]
}
```

**Solution:** Add a permit policy or check existing policy conditions.

---

#### Issue: "Forbid policy overrides permit"

**Symptom:** Have a permit policy, but still getting Deny.

**Explanation:** Cedar's evaluation: At least one forbid = overall Deny (explicit deny always wins).

**Example:**
```cedar
// This permits
permit(principal in Group::"users", action, resource);

// But this forbids if condition is true
forbid(principal, action, resource) when { resource.locked == true };
```

**Solution:** Review your forbid policies and their conditions.

---

#### Issue: "Missing context values"

**Symptom:** Policy references context keys that aren't provided.

**Example Response:**
```json
{
  "decision": "Deny",
  "diagnostics": [
    {
      "level": "Warning",
      "message": "Context key 'mfa_authenticated' referenced but not provided"
    }
  ]
}
```

**Solution:** Add the missing context values to your request:
```json
{
  "context": {
    "mfa_authenticated": true
  }
}
```

---

### 6.2 Debugging Workflow

1. **Start Simple:** Test with a single principal, action, and resource
2. **Add Complexity Gradually:** Add more entities one at a time
3. **Check Hierarchy:** Verify parent relationships are correct
4. **Validate Syntax:** Use `/playground/validate` first
5. **Use Explain:** Call `/playground/explain` for detailed reasoning
6. **Check Attributes:** Ensure all referenced attributes exist and have correct types

---

## 7. Best Practices

### 7.1 Policy Design

1. **Principle of Least Privilege:** Start with deny-all, explicitly permit what's needed
2. **Use Groups/Roles:** Don't hard-code user IDs in policies
3. **Hierarchies:** Leverage parent-child relationships for inheritance
4. **Conditions:** Use `when`/`unless` for fine-grained control
5. **Explicit Forbids:** Use sparingly, only for explicit security requirements

### 7.2 Testing Strategy

1. **Test Before Deploy:** Always test policies in playground before production
2. **Test Matrix:** Test all combinations of principals, actions, resources
3. **Edge Cases:** Test boundary conditions and edge cases
4. **Negative Tests:** Verify unauthorized access is denied
5. **Regression Suite:** Maintain a suite of tests for policy changes

### 7.3 Playground Usage

1. **Version Control:** Store test scenarios in version control
2. **Documentation:** Document expected behavior in test names
3. **Automation:** Integrate playground API into CI/CD pipelines
4. **Monitoring:** Track policy evaluation patterns in production
5. **Iteration:** Use playground to iteratively refine policies

### 7.4 Security Considerations

1. **Never Hard-Code Secrets:** Don't put sensitive data in policies
2. **Audit Logging:** Enable audit logs for sensitive operations
3. **Regular Reviews:** Periodically review and update policies
4. **Least Privilege:** Grant minimum necessary permissions
5. **Defense in Depth:** Combine policies with other security measures

---

## Appendix: Quick Reference

### Cedar Policy Syntax

```cedar
// Basic permit
permit(principal, action, resource);

// Specific entity
permit(principal == User::"alice", action, resource);

// Group membership
permit(principal in Group::"admins", action, resource);

// Action list
permit(principal, action in [Action::"Read", Action::"Write"], resource);

// With condition
permit(principal, action, resource)
when {
  principal.level >= 5
};

// With unless
forbid(principal, action, resource)
unless {
  resource.owner == principal
};

// Hierarchy
permit(principal, action, resource in Folder::"shared");
```

### Common Operators

- `==` - Equality
- `!=` - Inequality
- `<`, `<=`, `>`, `>=` - Comparison
- `&&` - Logical AND
- `||` - Logical OR
- `!` - Logical NOT
- `in` - Hierarchy membership
- `has` - Attribute existence check
- `.` - Attribute access

### API Endpoints Quick Reference

| Endpoint | Method | Purpose |
|----------|--------|---------|
| `/playground/evaluate` | POST | Simulate policy evaluation |
| `/playground/explain` | POST | Get detailed explanation |
| `/playground/validate` | POST | Validate policy syntax |
| `/playground/schema` | GET | Get current schema |

---

**For more information, see:**
- [DESIGN_REST_API_AND_PLAYGROUND.md](./DESIGN_REST_API_AND_PLAYGROUND.md) - Complete API specification
- [Cedar Documentation](https://docs.cedarpolicy.com/) - Official Cedar docs
- Hodei IAM Documentation - Internal documentation

---

**Document Version:** 1.0  
**Last Updated:** 2024  
**Maintained By:** Engineering Team