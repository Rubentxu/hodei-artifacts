# Hodei Artifacts API - Testing Guide

This document provides comprehensive instructions for testing the Hodei Artifacts API using the provided testing scripts.

## Overview

The Hodei Artifacts API provides a complete authorization system with:
- **Schema Management**: Build, load, and register IAM schemas
- **Policy Validation**: Validate Cedar policies
- **Policy Evaluation**: Evaluate policies against authorization requests
- **IAM Policy Management**: CRUD operations for IAM policies
- **Playground**: Interactive policy evaluation with custom policies

## Available Testing Scripts

### 1. Automated Test Script (`test_api.sh`)

A comprehensive script that:
- Starts the API server
- Runs automated tests against all endpoints
- Verifies Swagger UI functionality
- Provides colored output and detailed logging

#### Usage Options:

```bash
# Run all tests (starts server, runs tests, keeps server running)
./test_api.sh

# Start server only (no tests)
./test_api.sh -s

# Run tests only (assumes server is already running)
./test_api.sh -t

# Use custom API URL
./test_api.sh -u http://localhost:8080

# Show help
./test_api.sh -h
```

#### Features:
- **Dependency checking**: Verifies `curl`, `cargo`, and `jq` are installed
- **Automatic server management**: Starts and stops the server
- **Health checks**: Waits for server to be ready before testing
- **Comprehensive endpoint testing**: Tests all API categories
- **Error handling**: Graceful cleanup on exit
- **Colored output**: Easy-to-read test results

### 2. Curl Examples Script (`curl_examples.sh`)

A collection of manual curl commands for testing individual endpoints:

```bash
# Run all curl examples
./curl_examples.sh
```

#### What it tests:
- **Health check** (`/health`)
- **OpenAPI specification** (`/api-docs/openapi.json`)
- **Schema management** (build, load, register-iam)
- **Policy validation and evaluation**
- **IAM policy management** (create, read, update, delete)
- **Playground evaluation**
- **Advanced examples** with complex policies

## Prerequisites

Ensure you have the following tools installed:

```bash
# On Ubuntu/Debian
sudo apt update
sudo apt install curl jq

# On macOS with Homebrew
brew install curl jq

# Verify installations
curl --version
jq --version
cargo --version
```

## Quick Start

### Option 1: Full Automated Test

```bash
# Make scripts executable (if not already)
chmod +x test_api.sh curl_examples.sh

# Run comprehensive test
./test_api.sh
```

This will:
1. Build the project in release mode
2. Start the API server on `http://localhost:3000`
3. Run tests against all endpoints
4. Keep the server running for interactive testing
5. Show Swagger UI URL for browser testing

### Option 2: Manual Testing with Curl

```bash
# Start server manually
cargo run --release

# In another terminal, run curl examples
./curl_examples.sh
```

## API Endpoints Tested

### Health & Documentation
- `GET /health` - Health check
- `GET /swagger-ui` - Swagger UI interface
- `GET /api-docs/openapi.json` - OpenAPI specification

### Schema Management
- `POST /api/v1/schemas/build` - Build new schema
- `GET /api/v1/schemas/load` - Load existing schema
- `POST /api/v1/schemas/register-iam` - Register IAM schema

### Policy Operations
- `POST /api/v1/policies/validate` - Validate Cedar policy syntax
- `POST /api/v1/policies/evaluate` - Evaluate policies

### IAM Policy Management
- `POST /api/v1/iam/policies` - Create policy
- `GET /api/v1/iam/policies` - List policies
- `POST /api/v1/iam/policies/get` - Get specific policy
- `PUT /api/v1/iam/policies/update` - Update policy
- `DELETE /api/v1/iam/policies/delete` - Delete policy

### Playground
- `POST /api/v1/playground/evaluate` - Interactive policy evaluation

## Expected Test Results

### Successful Responses
- **Health check**: `200 OK` with status information
- **OpenAPI spec**: Valid OpenAPI 3.1.0 specification
- **Swagger UI**: Accessible web interface
- **Policy validation**: `200 OK` for valid policies
- **Policy evaluation**: `200 OK` with authorization decision

### Possible Warnings
Some endpoints may return non-200 status codes during testing due to:
- Missing test data in database
- Schema dependencies not being met
- This is normal and expected in test environment

## Troubleshooting

### Common Issues

1. **Server fails to start**
   - Check if port 3000 is available
   - Verify all dependencies are installed
   - Check `cargo build` completes successfully

2. **Tests fail with connection errors**
   - Ensure server is running before using `-t` flag
   - Verify API_BASE_URL matches running server
   - Check firewall settings

3. **jq parsing errors**
   - Install jq: `sudo apt install jq` or `brew install jq`
   - Some responses may not be valid JSON (normal for some endpoints)

4. **Permission denied**
   - Make scripts executable: `chmod +x *.sh`

### Debug Mode

For detailed debugging, you can modify the scripts:

```bash
# Add verbose output to test_api.sh
set -x  # Add at the beginning of the script

# Or run curl with verbose flag
curl -v http://localhost:3000/health
```

## Manual Testing with Swagger UI

After running the automated tests, you can use Swagger UI for interactive testing:

1. Open browser: `http://localhost:3000/swagger-ui`
2. Explore all available endpoints
3. Try the "Try it out" feature for each endpoint
4. View request/response schemas

## Integration with CI/CD

The test scripts can be integrated into CI/CD pipelines:

```yaml
# Example GitHub Actions workflow
- name: Test API
  run: |
    chmod +x test_api.sh
    ./test_api.sh -t
```

## Development Notes

- The API uses **Cedar policy language** for authorization
- All policies are validated against defined schemas
- IAM policies are stored and managed separately
- The playground allows testing custom policies without persistence

## Support

For issues with the testing scripts or API functionality:
1. Check this documentation first
2. Review server logs for error details
3. Verify all prerequisites are met
4. Test individual endpoints with curl examples

---

**Happy Testing!** 🚀