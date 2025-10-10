#!/bin/bash

# Hodei Artifacts API - Curl Examples
# This file contains curl commands to test the Hodei Artifacts API endpoints

# Configuration
API_BASE_URL="http://localhost:3000"

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}=== Hodei Artifacts API Curl Examples ===${NC}"
echo

# 1. Health Check
echo -e "${GREEN}1. Health Check${NC}"
curl -s "$API_BASE_URL/health" | jq '.'
echo

# 2. OpenAPI Specification
echo -e "${GREEN}2. OpenAPI Specification${NC}"
curl -s "$API_BASE_URL/api-docs/openapi.json" | jq '.info'
echo

# 3. Schema Management
echo -e "${GREEN}3. Schema Management${NC}"

# Build Schema
echo "Building schema..."
curl -X POST "$API_BASE_URL/api/v1/schemas/build" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "test-schema",
    "version": "1.0.0",
    "entities": {
      "User": {
        "attributes": {
          "name": "string",
          "email": "string",
          "role": "string"
        }
      },
      "File": {
        "attributes": {
          "name": "string",
          "owner": "string",
          "permissions": "array"
        }
      }
    }
  }' | jq '.'
echo

# Load Schema
echo "Loading schema..."
curl -s "$API_BASE_URL/api/v1/schemas/load?name=test-schema&version=1.0.0" | jq '.'
echo

# Register IAM Schema
echo "Registering IAM schema..."
curl -X POST "$API_BASE_URL/api/v1/schemas/register-iam" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "iam-schema",
    "version": "1.0.0"
  }' | jq '.'
echo

# 4. Policy Validation & Evaluation
echo -e "${GREEN}4. Policy Validation & Evaluation${NC}"

# Validate Policy
echo "Validating policy..."
curl -s -X POST "$API_BASE_URL/api/v1/policies/validate" \
  -H "Content-Type: application/json" \
  -d '{
    "policy": "permit(principal == User::\"alice\", action == Action::\"read\", resource == File::\"report.pdf\");"
  }' | jq '.' 2>/dev/null || echo "Response not valid JSON"
echo

# Evaluate Policies
echo "Evaluating policies..."
curl -s -X POST "$API_BASE_URL/api/v1/policies/evaluate" \
  -H "Content-Type: application/json" \
  -d '{
    "principal": "User::\"alice\"",
    "action": "Action::\"read\"",
    "resource": "File::\"report.pdf\"",
    "context": {
      "ip_address": "192.168.1.100",
      "time_of_day": "09:00"
    }
  }' | jq '.' 2>/dev/null || echo "Response not valid JSON"
echo

# 5. IAM Policy Management
echo -e "${GREEN}5. IAM Policy Management${NC}"

# List Policies
echo "Listing IAM policies..."
curl -s "$API_BASE_URL/api/v1/iam/policies" | jq '.'
echo

# Create Policy
echo "Creating IAM policy..."
CREATE_RESPONSE=$(curl -s -X POST "$API_BASE_URL/api/v1/iam/policies" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "admin-access",
    "description": "Full administrative access",
    "policy": "permit(principal in Group::\"admins\", action, resource);"
  }')

echo "$CREATE_RESPONSE" | jq '.' 2>/dev/null || echo "Response not valid JSON"
POLICY_ID=$(echo "$CREATE_RESPONSE" | jq -r '.id // empty' 2>/dev/null || echo "")
echo

if [ ! -z "$POLICY_ID" ] && [ "$POLICY_ID" != "null" ]; then
  # Get Policy
  echo "Getting IAM policy..."
  curl -s -X POST "$API_BASE_URL/api/v1/iam/policies/get" \
    -H "Content-Type: application/json" \
    -d "{\"id\": \"$POLICY_ID\"}" | jq '.' 2>/dev/null || echo "Response not valid JSON"
  echo

  # Update Policy
  echo "Updating IAM policy..."
  curl -s -X PUT "$API_BASE_URL/api/v1/iam/policies/update" \
    -H "Content-Type: application/json" \
    -d "{
      \"id\": \"$POLICY_ID\",
      \"name\": \"admin-access-updated\",
      \"description\": \"Updated administrative access policy\",
      \"policy\": \"permit(principal in Group::\\\"admins\\\", action, resource) when { context.time_of_day == \\\"09:00\\\" };\"
    }" | jq '.' 2>/dev/null || echo "Response not valid JSON"
  echo

  # Delete Policy
  echo "Deleting IAM policy..."
  curl -s -X DELETE "$API_BASE_URL/api/v1/iam/policies/delete" \
    -H "Content-Type: application/json" \
    -d "{\"id\": \"$POLICY_ID\"}" | jq '.' 2>/dev/null || echo "Response not valid JSON"
  echo
fi

# 6. Playground Evaluation
echo -e "${GREEN}6. Playground Evaluation${NC}"

echo "Testing playground evaluation..."
curl -s -X POST "$API_BASE_URL/api/v1/playground/evaluate" \
  -H "Content-Type: application/json" \
  -d '{
    "principal": "User::\"bob\"",
    "action": "Action::\"write\"",
    "resource": "File::\"document.txt\"",
    "context": {
      "department": "engineering",
      "clearance_level": "high"
    },
    "policies": [
      "permit(principal, action, resource) when { context.department == \"engineering\" };",
      "forbid(principal, action, resource) when { context.clearance_level != \"high\" };"
    ]
  }' | jq '.' 2>/dev/null || echo "Response not valid JSON"
echo

# 7. Advanced Examples
echo -e "${GREEN}7. Advanced Examples${NC}"

# Complex Policy with Multiple Conditions
echo "Complex policy evaluation..."
curl -s -X POST "$API_BASE_URL/api/v1/playground/evaluate" \
  -H "Content-Type: application/json" \
  -d '{
    "principal": "User::\"charlie\"",
    "action": "Action::\"delete\"",
    "resource": "File::\"sensitive-data.db\"",
    "context": {
      "role": "admin",
      "location": "office",
      "time": "14:30",
      "mfa_enabled": true
    },
    "policies": [
      "permit(principal, action, resource) when { principal.role == \"admin\" && context.mfa_enabled };",
      "forbid(principal, action, resource) when { context.location != \"office\" };",
      "permit(principal, action, resource) when { context.time >= \"09:00\" && context.time <= \"17:00\" };"
    ]
  }' | jq '.' 2>/dev/null || echo "Response not valid JSON"
echo

# Error Case - Invalid Policy
echo "Testing error case (invalid policy)..."
curl -s -X POST "$API_BASE_URL/api/v1/policies/validate" \
  -H "Content-Type: application/json" \
  -d '{
    "policy": "permit(principal, action, resource) when { invalid_syntax };"
  }' | jq '.' 2>/dev/null || echo "Response not valid JSON"
echo

echo -e "${BLUE}=== All curl examples completed ===${NC}"
echo
echo "Additional endpoints:"
echo "- Swagger UI: $API_BASE_URL/swagger-ui"
echo "- OpenAPI Spec: $API_BASE_URL/api-docs/openapi.json"
echo "- Health Check: $API_BASE_URL/health"
