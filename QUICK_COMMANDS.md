# Hodei Artifacts API - Quick Commands Reference

## 🚀 Quick Start

### Start Server
```bash
# Start server with full output
./start_server.sh

# Or manually
cargo run --release
```

### Automated Testing
```bash
# Full test (starts server + runs all tests)
./test_api.sh

# Test against running server only
./test_api.sh -t

# Start server only
./test_api.sh -s
```

### Manual Testing
```bash
# Run all curl examples
./curl_examples.sh
```

## 🔗 Important URLs

| Service | URL |
|---------|-----|
| **API Server** | `http://localhost:3000` |
| **Health Check** | `http://localhost:3000/health` |
| **Swagger UI** | `http://localhost:3000/swagger-ui` |
| **OpenAPI Spec** | `http://localhost:3000/api-docs/openapi.json` |

## 🧪 Essential Test Commands

### Health & Documentation
```bash
# Health check
curl http://localhost:3000/health | jq

# OpenAPI specification
curl http://localhost:3000/api-docs/openapi.json | jq '.info'

# Swagger UI (check if accessible)
curl -s http://localhost:3000/swagger-ui | grep -q "swagger-ui" && echo "✅ Swagger UI OK"
```

### Schema Management
```bash
# Build schema
curl -X POST http://localhost:3000/api/v1/schemas/build \
  -H "Content-Type: application/json" \
  -d '{
    "name": "test-schema",
    "version": "1.0.0",
    "entities": {
      "User": {"attributes": {"name": "string", "email": "string"}}
    }
  }' | jq

# Load schema
curl "http://localhost:3000/api/v1/schemas/load?name=test-schema&version=1.0.0" | jq
```

### Policy Operations
```bash
# Validate policy
curl -X POST http://localhost:3000/api/v1/policies/validate \
  -H "Content-Type: application/json" \
  -d '{"policy": "permit(principal, action, resource);"}' | jq

# Evaluate policy
curl -X POST http://localhost:3000/api/v1/policies/evaluate \
  -H "Content-Type: application/json" \
  -d '{
    "principal": "User::\"alice\"",
    "action": "Action::\"read\"", 
    "resource": "File::\"report.pdf\"",
    "context": {}
  }' | jq
```

### IAM Policy Management
```bash
# List policies
curl http://localhost:3000/api/v1/iam/policies | jq

# Create policy
curl -X POST http://localhost:3000/api/v1/iam/policies \
  -H "Content-Type: application/json" \
  -d '{
    "name": "test-policy",
    "description": "Test policy",
    "policy": "permit(principal, action, resource);"
  }' | jq
```

### Playground
```bash
# Playground evaluation
curl -X POST http://localhost:3000/api/v1/playground/evaluate \
  -H "Content-Type: application/json" \
  -d '{
    "principal": "User::\"bob\"",
    "action": "Action::\"write\"",
    "resource": "File::\"document.txt\"",
    "context": {"department": "engineering"},
    "policies": ["permit(principal, action, resource) when { context.department == \"engineering\" };"]
  }' | jq
```

## 🛠️ Development Commands

### Build & Check
```bash
# Build project
cargo build --release

# Check for errors
cargo check

# Check for warnings
cargo clippy -- -D warnings

# Run tests
cargo nextest run
```

### Code Quality
```bash
# Fix auto-fixable issues
cargo fix

# Format code
cargo fmt

# Check dependencies
cargo tree
```

## 📊 Verification Commands

### Server Status
```bash
# Check if server is running
curl -s http://localhost:3000/health > /dev/null && echo "✅ Server running" || echo "❌ Server not running"

# Check process
ps aux | grep hodei-artifacts-api | grep -v grep
```

### OpenAPI Validation
```bash
# Count endpoints and schemas
curl -s http://localhost:3000/api-docs/openapi.json | jq '.paths | keys | length'
curl -s http://localhost:3000/api-docs/openapi.json | jq '.components.schemas | keys | length'
```

## 🔧 Troubleshooting

### Common Issues
```bash
# Port in use
sudo lsof -i :3000

# Kill process
pkill -f hodei-artifacts-api

# Check dependencies
which curl jq cargo
```

### Debug Mode
```bash
# Verbose curl
curl -v http://localhost:3000/health

# Server with debug logging
RUST_LOG=debug cargo run
```

## 📋 Script Summary

| Script | Purpose |
|--------|---------|
| `start_server.sh` | Quick server start with URL display |
| `test_api.sh` | Comprehensive automated testing |
| `curl_examples.sh` | Manual endpoint testing examples |
| `QUICK_COMMANDS.md` | This reference file |

---

**💡 Tip**: Use `./test_api.sh` for comprehensive testing or `./curl_examples.sh` for quick manual verification.