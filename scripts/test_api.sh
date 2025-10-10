#!/bin/bash

# Hodei Artifacts API Test Script
# This script tests the Hodei Artifacts API endpoints and verifies Swagger UI functionality

set -e  # Exit on any error

# Configuration
API_BASE_URL="http://localhost:3000"
SWAGGER_UI_URL="$API_BASE_URL/swagger-ui/"
OPENAPI_SPEC_URL="$API_BASE_URL/api-docs/openapi.json"
HEALTH_URL="$API_BASE_URL/health"
SERVER_PID=""

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Logging functions
log_info() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

log_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

log_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check if required tools are installed
check_dependencies() {
    log_info "Checking dependencies..."

    local missing_deps=()

    if ! command -v curl &> /dev/null; then
        missing_deps+=("curl")
    fi

    if ! command -v cargo &> /dev/null; then
        missing_deps+=("cargo")
    fi

    if [ ${#missing_deps[@]} -ne 0 ]; then
        log_error "Missing dependencies: ${missing_deps[*]}"
        log_error "Please install them before running this script."
        exit 1
    fi

    log_success "All dependencies are available"
}

# Start the API server
start_server() {
    log_info "Starting Hodei Artifacts API server..."

    # Use relative path to project root
    SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
    BINARY_PATH="$PROJECT_ROOT/target/release/hodei-artifacts-api"

    # Check if binary exists, if not build it
    if [ ! -f "$BINARY_PATH" ]; then
        log_warning "Release binary not found, building project..."
        cd "$PROJECT_ROOT"
        if ! cargo build --release; then
            log_error "Failed to build the project"
            exit 1
        fi
    else
        log_info "Using existing release binary"
    fi

    # Start server in background with release mode
    cd "$PROJECT_ROOT"
    RUN_MODE=release ./target/release/hodei-artifacts-api &
    SERVER_PID=$!

    log_info "Server started with PID: $SERVER_PID"

    # Wait for server to be ready
    log_info "Waiting for server to be ready..."
    echo -n "Waiting"
    local max_attempts=30
    local attempt=0

    while [ $attempt -lt $max_attempts ]; do
        if curl -s "$HEALTH_URL" > /dev/null 2>&1; then
            log_success "Server is ready!"
            return 0
        fi

        attempt=$((attempt + 1))
        echo -n "."
        sleep 2

        if [ $attempt -eq $max_attempts ]; then
            echo ""
            log_error "Server failed to start within timeout period"
            log_error "Check if port 3000 is already in use or check server logs"
            stop_server
            exit 1
        fi
    done
    echo ""
}

# Stop the API server
stop_server() {
    if [ ! -z "$SERVER_PID" ]; then
        log_info "Stopping server (PID: $SERVER_PID)..."
        kill $SERVER_PID 2>/dev/null || true
        wait $SERVER_PID 2>/dev/null || true
        log_success "Server stopped"
    fi
}

# Cleanup function
cleanup() {
    log_info "Cleaning up..."
    stop_server
}

# Set up trap for cleanup
trap cleanup EXIT INT TERM

# Test health endpoint
test_health_endpoint() {
    log_info "Testing health endpoint..."

    local response=$(curl -s -w "%{http_code}" "$HEALTH_URL")
    local status_code=${response: -3}
    local body=${response%???}

    if [ "$status_code" -eq 200 ]; then
        log_success "Health endpoint returned 200 OK"
        echo "Response: $body"
    else
        log_error "Health endpoint returned $status_code"
        return 1
    fi
}

# Test OpenAPI specification
test_openapi_spec() {
    log_info "Testing OpenAPI specification..."

    # Check if jq is available
    if command -v jq &> /dev/null; then
        if curl -s "$OPENAPI_SPEC_URL" | jq -e '.openapi' > /dev/null 2>&1; then
            log_success "OpenAPI specification is valid"

            # Count endpoints and schemas
            local endpoints=$(curl -s "$OPENAPI_SPEC_URL" | jq '.paths | keys | length')
            local schemas=$(curl -s "$OPENAPI_SPEC_URL" | jq '.components.schemas | keys | length')

            log_info "Found $endpoints endpoints and $schemas schemas in OpenAPI spec"
        else
            log_error "OpenAPI specification is invalid or missing"
            return 1
        fi
    else
        # Fallback without jq
        local response=$(curl -s "$OPENAPI_SPEC_URL")
        if echo "$response" | grep -q '"openapi"'; then
            log_success "OpenAPI specification is accessible (jq not available for validation)"
        else
            log_error "OpenAPI specification is invalid or missing"
            return 1
        fi
    fi
}

# Test Swagger UI
test_swagger_ui() {
    log_info "Testing Swagger UI..."

    if curl -s "$SWAGGER_UI_URL" | grep -q "swagger-ui"; then
        log_success "Swagger UI is accessible"
    else
        log_error "Swagger UI is not accessible"
        return 1
    fi
}

# Test schema endpoints
test_schema_endpoints() {
    log_info "Testing schema endpoints..."

    # Test build schema endpoint
    local build_payload='{
        "name": "test-schema",
        "version": "1.0.0",
        "entities": {
            "User": {
                "attributes": {
                    "name": "string",
                    "email": "string",
                    "role": "string"
                }
            }
        }
    }'

    local response=$(curl -s -X POST "$API_BASE_URL/api/v1/schemas/build" \
        -H "Content-Type: application/json" \
        -d "$build_payload" \
        -w "%{http_code}")

    local status_code=${response: -3}

    if [ "$status_code" -eq 200 ] || [ "$status_code" -eq 201 ]; then
        log_success "Build schema endpoint returned $status_code"
    else
        log_warning "Build schema endpoint returned $status_code (might be expected for test data)"
    fi

    # Test load schema endpoint
    response=$(curl -s "$API_BASE_URL/api/v1/schemas/load?name=test-schema&version=1.0.0" \
        -w "%{http_code}")
    status_code=${response: -3}

    if [ "$status_code" -eq 200 ]; then
        log_success "Load schema endpoint returned 200"
    else
        log_info "Load schema endpoint returned $status_code (schema might not exist)"
    fi
}

# Test policy endpoints
test_policy_endpoints() {
    log_info "Testing policy endpoints..."

    # Test policy validation
    local validate_payload='{
        "policy": "permit(principal, action, resource);"
    }'

    local response=$(curl -s -X POST "$API_BASE_URL/api/v1/policies/validate" \
        -H "Content-Type: application/json" \
        -d "$validate_payload" \
        -w "%{http_code}")

    local status_code=${response: -3}

    if [ "$status_code" -eq 200 ]; then
        log_success "Policy validation endpoint returned 200"
    else
        log_warning "Policy validation endpoint returned $status_code"
    fi

    # Test policy evaluation
    local evaluate_payload='{
        "principal": "User::\"test-user\"",
        "action": "Action::\"read\"",
        "resource": "File::\"test-file\"",
        "context": {}
    }'

    response=$(curl -s -X POST "$API_BASE_URL/api/v1/policies/evaluate" \
        -H "Content-Type: application/json" \
        -d "$evaluate_payload" \
        -w "%{http_code}")
    status_code=${response: -3}

    if [ "$status_code" -eq 200 ]; then
        log_success "Policy evaluation endpoint returned 200"
    else
        log_warning "Policy evaluation endpoint returned $status_code"
    fi
}

# Test IAM endpoints
test_iam_endpoints() {
    log_info "Testing IAM endpoints..."

    # Test list policies
    local response=$(curl -s "$API_BASE_URL/api/v1/iam/policies" -w "%{http_code}")
    local status_code=${response: -3}

    if [ "$status_code" -eq 200 ]; then
        log_success "List IAM policies endpoint returned 200"
    else
        log_warning "List IAM policies endpoint returned $status_code"
    fi

    # Test create policy
    local create_payload='{
        "name": "test-policy",
        "description": "Test policy",
        "policy": "permit(principal, action, resource);"
    }'

    response=$(curl -s -X POST "$API_BASE_URL/api/v1/iam/policies" \
        -H "Content-Type: application/json" \
        -d "$create_payload" \
        -w "%{http_code}")
    status_code=${response: -3}

    if [ "$status_code" -eq 201 ] || [ "$status_code" -eq 200 ]; then
        log_success "Create IAM policy endpoint returned $status_code"
    else
        log_warning "Create IAM policy endpoint returned $status_code"
    fi
}

# Test playground endpoint
test_playground_endpoint() {
    log_info "Testing playground endpoint..."

    local playground_payload='{
        "principal": "User::\"test-user\"",
        "action": "Action::\"read\"",
        "resource": "File::\"test-file\"",
        "context": {},
        "policies": ["permit(principal, action, resource);"]
    }'

    local response=$(curl -s -X POST "$API_BASE_URL/api/v1/playground/evaluate" \
        -H "Content-Type: application/json" \
        -d "$playground_payload" \
        -w "%{http_code}")

    local status_code=${response: -3}

    if [ "$status_code" -eq 200 ]; then
        log_success "Playground evaluation endpoint returned 200"
    else
        log_warning "Playground evaluation endpoint returned $status_code"
    fi
}

# Main test function
run_tests() {
    log_info "Starting Hodei Artifacts API tests..."

    # Start server
    start_server

    # Run tests
    test_health_endpoint
    test_openapi_spec
    test_swagger_ui
    test_schema_endpoints
    test_policy_endpoints
    test_iam_endpoints
    test_playground_endpoint

    log_success "All tests completed!"

    # Show summary
    echo
    log_info "=== TEST SUMMARY ==="
    log_info "API Base URL: $API_BASE_URL"
    log_info "Swagger UI: $SWAGGER_UI_URL"
    log_info "OpenAPI Spec: $OPENAPI_SPEC_URL"
    log_info "Health Check: $HEALTH_URL"
    echo
    log_info "You can now:"
    log_info "1. Open Swagger UI in your browser: $SWAGGER_UI_URL"
    log_info "2. Test endpoints interactively"
    log_info "3. View the OpenAPI specification: $OPENAPI_SPEC_URL"
    echo
    log_info "Press Ctrl+C to stop the server and exit"

    # Keep server running for interactive testing
    wait $SERVER_PID
}

# Help function
show_help() {
    echo "Hodei Artifacts API Test Script"
    echo
    echo "Usage: $0 [OPTIONS]"
    echo
    echo "Options:"
    echo "  -h, --help          Show this help message"
    echo "  -s, --server-only   Start server only (no tests)"
    echo "  -t, --tests-only    Run tests only (assumes server is running)"
    echo "  -u, --url URL       Use custom API base URL (default: $API_BASE_URL)"
    echo
    echo "Examples:"
    echo "  $0                    # Run all tests with default settings"
    echo "  $0 -s                 # Start server only"
    echo "  $0 -t                 # Run tests against running server"
    echo "  $0 -u http://localhost:8080  # Use custom URL"
}

# Parse command line arguments
parse_arguments() {
    while [[ $# -gt 0 ]]; do
        case $1 in
            -h|--help)
                show_help
                exit 0
                ;;
            -s|--server-only)
                SERVER_ONLY=true
                shift
                ;;
            -t|--tests-only)
                TESTS_ONLY=true
                shift
                ;;
            -u|--url)
                API_BASE_URL="$2"
                SWAGGER_UI_URL="$API_BASE_URL/swagger-ui"
                OPENAPI_SPEC_URL="$API_BASE_URL/api-docs/openapi.json"
                HEALTH_URL="$API_BASE_URL/health"
                shift 2
                ;;
            *)
                log_error "Unknown option: $1"
                show_help
                exit 1
                ;;
        esac
    done
}

# Main execution
main() {
    parse_arguments "$@"
    check_dependencies

    if [ "$TESTS_ONLY" = true ]; then
        log_info "Running tests against existing server..."
        run_tests_without_starting
    elif [ "$SERVER_ONLY" = true ]; then
        log_info "Starting server only..."
        start_server
        log_info "Server is running at $API_BASE_URL"
        log_info "Press Ctrl+C to stop"
        wait $SERVER_PID
    else
        run_tests
    fi
}

# Alternative test function that doesn't start server
run_tests_without_starting() {
    log_info "Testing against server at $API_BASE_URL..."

    # Verify server is running
    if ! curl -s "$HEALTH_URL" > /dev/null; then
        log_error "Server is not running at $API_BASE_URL"
        log_error "Start the server first or use the full test mode"
        exit 1
    fi

    # Run tests
    test_health_endpoint
    test_openapi_spec
    test_swagger_ui
    test_schema_endpoints
    test_policy_endpoints
    test_iam_endpoints
    test_playground_endpoint

    log_success "All tests completed against running server!"
}

# Run main function
main "$@"
