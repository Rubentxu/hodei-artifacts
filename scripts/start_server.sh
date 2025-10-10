#!/bin/bash

# Hodei Artifacts API - Quick Start Script
# Starts the server and displays important URLs for testing

set -e

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${BLUE}🚀 Hodei Artifacts API - Quick Start${NC}"
echo

# Check if cargo is available
if ! command -v cargo &> /dev/null; then
    echo -e "${YELLOW}⚠️  Cargo not found. Please install Rust first.${NC}"
    exit 1
fi

# Build the project
echo -e "${BLUE}📦 Building project...${NC}"
if ! cargo build --release; then
    echo -e "${YELLOW}❌ Build failed${NC}"
    exit 1
fi

echo
echo -e "${GREEN}✅ Build successful${NC}"
echo

# Display important URLs
echo -e "${BLUE}📋 Important URLs:${NC}"
echo -e "  🌐 API Server:    ${GREEN}http://localhost:3000${NC}"
echo -e "  📊 Health Check:  ${GREEN}http://localhost:3000/health${NC}"
echo -e "  📖 Swagger UI:    ${GREEN}http://localhost:3000/swagger-ui${NC}"
echo -e "  📄 OpenAPI Spec:  ${GREEN}http://localhost:3000/api-docs/openapi.json${NC}"
echo

echo -e "${BLUE}🔧 Available Testing Scripts:${NC}"
echo -e "  🧪 Full test:     ${GREEN}./test_api.sh${NC}"
echo -e "  🔗 Curl examples: ${GREEN}./curl_examples.sh${NC}"
echo

echo -e "${BLUE}🎯 Quick Commands:${NC}"
echo -e "  Health check:     ${YELLOW}curl http://localhost:3000/health | jq${NC}"
echo -e "  OpenAPI spec:     ${YELLOW}curl http://localhost:3000/api-docs/openapi.json | jq '.info'${NC}"
echo

echo -e "${GREEN}🚀 Starting server...${NC}"
echo -e "${YELLOW}Press Ctrl+C to stop the server${NC}"
echo

# Start the server
# Use development mode for configuration compatibility
RUN_MODE=development cargo run
