# Hodei Artifacts Distribution Module

A comprehensive package distribution system supporting Maven, npm, and Docker formats with Vertical Slice Architecture (VSA) implementation.

## Overview

The Distribution module provides a unified API for managing and serving packages across multiple ecosystems:

- **Maven**: Java artifact repository (.jar, .pom, .war, .ear, .aar)
- **npm**: Node.js package registry (.tgz)
- **Docker**: Container image registry (manifests and blobs)

Built with Rust, following Clean Architecture principles and Vertical Slice Architecture (VSA) patterns for maximum modularity and testability.

## Architecture

### Vertical Slice Architecture (VSA)

Each package format is implemented as a complete vertical slice with:

```
features/
├── handle_maven_request/     # Maven artifact operations
├── handle_npm_request/       # npm package operations  
├── handle_docker_request/    # Docker image operations
├── generate_maven_metadata/  # Maven metadata generation
├── generate_npm_metadata/    # npm metadata generation
└── generate_docker_manifest/ # Docker manifest generation
```

Each feature contains:
- **Domain**: Pure business logic, no external dependencies
- **Ports**: Segregated interfaces for each operation
- **Use Cases**: Specific business operations
- **Adapters**: Infrastructure implementations
- **API**: HTTP endpoint handlers
- **DI**: Dependency injection containers

### Clean Architecture Layers

1. **Domain Layer**: Pure business logic, validation, entities
2. **Application Layer**: Use cases, orchestration
3. **Infrastructure Layer**: External integrations (S3, MongoDB, Redis)
4. **API Layer**: HTTP endpoints, request/response handling

## Features

### Maven Support
- ✅ Artifact upload/download (.jar, .pom, .war, .ear, .aar)
- ✅ Maven metadata generation (maven-metadata.xml)
- ✅ Repository layout compliance
- ✅ HTTP caching headers (ETag, Last-Modified)
- ✅ Content-type validation
- ✅ Coordinate validation (groupId, artifactId, version)

### npm Support
- ✅ Package upload/download (.tgz)
- ✅ Scoped package support (@scope/package)
- ✅ Package metadata generation
- ✅ Dist-tags management
- ✅ Version validation (semver)
- ✅ Package name validation

### Docker Support
- ✅ Registry V2 API compliance
- ✅ Manifest upload/download
- ✅ Blob upload/download
- ✅ Layer management
- ✅ Digest validation
- ✅ Content-addressable storage

### Common Features
- ✅ Comprehensive error handling
- ✅ Structured logging with tracing
- ✅ Authentication and authorization
- ✅ Rate limiting
- ✅ Caching support
- ✅ Health checks
- ✅ Metrics and observability

## Quick Start

### Prerequisites

```bash
# Rust toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Development dependencies
docker-compose up -d  # MongoDB, Redis, S3/MinIO
```

### Installation

```bash
# Clone the repository
git clone https://github.com/your-org/hodei-artifacts.git
cd hodei-artifacts

# Build the project
cargo build --release

# Run tests
cargo test
```

### Basic Usage

```rust
use distribution::{DistributionIntegration, DistributionApiState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the distribution service
    let integration = DistributionIntegration::new().await?;
    
    // Create API state for HTTP routes
    let api_state = integration.create_api_state();
    
    // Use with your favorite web framework
    let app = create_distribution_router(api_state);
    
    // Start server
    axum::Server::bind(&"0.0.0.0:8080".parse()?)
        .serve(app.into_make_service())
        .await?;
    
    Ok(())
}
```

See [`examples/simple_integration.rs`](examples/simple_integration.rs) for a complete working example.

## API Documentation

Comprehensive API specifications are available in [`docs/api-specifications.md`](docs/api-specifications.md).

### Quick API Reference

#### Maven Endpoints
```
GET    /maven/{groupId}/{artifactId}/{version}/{filename}
PUT    /maven/{groupId}/{artifactId}/{version}/{filename}
HEAD   /maven/{groupId}/{artifactId}/{version}/{filename}
GET    /maven/{groupId}/{artifactId}/maven-metadata.xml
```

#### npm Endpoints
```
GET    /npm/{package}
PUT    /npm/{package}
GET    /npm/{package}/package.json
GET    /npm/{scope}/{package}
PUT    /npm/{scope}/{package}
```

#### Docker Registry V2 Endpoints
```
GET    /v2/
GET    /v2/_catalog
GET    /v2/{name}/manifests/{reference}
PUT    /v2/{name}/manifests/{reference}
HEAD   /v2/{name}/manifests/{reference}
GET    /v2/{name}/blobs/{digest}
HEAD   /v2/{name}/blobs/{digest}
POST   /v2/{name}/blobs/uploads/
PUT    /v2/{name}/blobs/uploads/{uuid}
GET    /v2/{name}/tags/list
```

## Configuration

### Environment Variables

```bash
# Storage configuration
S3_ENDPOINT=http://localhost:9000
S3_ACCESS_KEY_ID=minioadmin
S3_SECRET_ACCESS_KEY=minioadmin
S3_BUCKET=hodei-artifacts

# Database configuration
MONGODB_URI=mongodb://localhost:27017/hodei
REDIS_URL=redis://localhost:6379

# Security configuration
CEDAR_POLICIES_PATH=./config/policies.cedar
JWT_SECRET=your-secret-key

# Performance configuration
MAX_UPLOAD_SIZE=1GB
CACHE_TTL=3600
RATE_LIMIT_PER_MINUTE=100
```

### Repository Configuration

Repositories are configured through the system and support different types:

- **Hosted**: Stores artifacts locally
- **Proxy**: Caches artifacts from upstream repositories
- **Virtual**: Aggregates multiple repositories

## Testing

### Unit Tests
```bash
cargo test --lib
```

### Integration Tests
```bash
cargo test --tests
```

### End-to-End Tests
```bash
cd e2e
npm install
npx playwright test
```

### Test Organization

- **Unit tests**: Feature-specific tests with mocks
- **Integration tests**: Cross-feature integration with testcontainers
- **E2E tests**: Full system testing with real package managers

## Integration with Package Managers

### Maven
Configure `~/.m2/settings.xml`:
```xml
<settings>
  <mirrors>
    <mirror>
      <id>hodei-artifacts</id>
      <url>http://localhost:8080/maven</url>
      <mirrorOf>*</mirrorOf>
    </mirror>
  </mirrors>
</settings>
```

### npm
Configure `.npmrc`:
```
registry=http://localhost:8080/npm
```

### Docker
Configure Docker daemon:
```json
{
  "registry-mirrors": ["http://localhost:8080"]
}
```

## Performance

### Benchmarks

| Operation | Format | Latency | Throughput |
|-----------|--------|---------|------------|
| Upload | Maven (1MB) | ~50ms | ~20 MB/s |
| Download | Maven (1MB) | ~30ms | ~33 MB/s |
| Upload | npm (500KB) | ~40ms | ~12.5 MB/s |
| Download | npm (500KB) | ~25ms | ~20 MB/s |
| Upload | Docker (10MB) | ~200ms | ~50 MB/s |
| Download | Docker (10MB) | ~150ms | ~67 MB/s |

### Optimization Features

- **Streaming**: Large file uploads/downloads are streamed
- **Chunking**: Support for chunked uploads
- **Caching**: Multi-level caching (Redis, CDN, browser)
- **Compression**: Gzip compression for metadata
- **Connection pooling**: Efficient database connections

## Monitoring

### Metrics

Prometheus metrics are exposed at `/metrics`:

- `distribution_requests_total`: Total requests by format and status
- `distribution_request_duration_seconds`: Request latency histogram
- `distribution_upload_bytes_total`: Total bytes uploaded
- `distribution_download_bytes_total`: Total bytes downloaded
- `distribution_cache_hits_total`: Cache hit count
- `distribution_errors_total`: Error count by type

### Logging

Structured logging with correlation IDs:

```json
{
  "timestamp": "2023-12-01T12:00:00Z",
  "level": "INFO",
  "correlation_id": "abc123",
  "operation": "maven.upload",
  "repository": "maven-central",
  "artifact": "com.example:my-app:1.0.0",
  "duration_ms": 45,
  "status": "success"
}
```

### Health Checks

Health endpoint at `/health`:
```json
{
  "status": "healthy",
  "services": {
    "database": "healthy",
    "storage": "healthy",
    "cache": "healthy"
  },
  "version": "1.0.0"
}
```

## Security

### Authentication
- JWT tokens
- API keys
- Basic authentication
- OAuth 2.0 integration

### Authorization
- Cedar policy engine
- Fine-grained permissions
- Repository-level access control
- Operation-level permissions

### Security Features
- HTTPS/TLS encryption
- Input validation and sanitization
- Rate limiting and DDoS protection
- Audit logging
- Vulnerability scanning integration

## Deployment

### Docker Deployment

```dockerfile
FROM rust:1.70 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bullseye-slim
RUN apt-get update && apt-get install -y ca-certificates
COPY --from=builder /app/target/release/distribution /usr/local/bin/
EXPOSE 8080
CMD ["distribution"]
```

### Kubernetes Deployment

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: hodei-distribution
spec:
  replicas: 3
  selector:
    matchLabels:
      app: hodei-distribution
  template:
    metadata:
      labels:
        app: hodei-distribution
    spec:
      containers:
      - name: distribution
        image: hodei/distribution:latest
        ports:
        - containerPort: 8080
        env:
        - name: MONGODB_URI
          valueFrom:
            secretKeyRef:
              name: hodei-secrets
              key: mongodb-uri
```

## Contributing

See [CONTRIBUTING.md](../../CONTRIBUTING.md) for development guidelines.

### Development Setup

```bash
# Install development tools
cargo install cargo-watch cargo-audit

# Run development server with hot reload
cargo watch -x run

# Run with debug logging
RUST_LOG=debug cargo run

# Check for security vulnerabilities
cargo audit
```

## License

This project is licensed under the MIT License - see the [LICENSE](../../LICENSE) file for details.

## Support

- **Documentation**: [docs/](docs/)
- **Issues**: [GitHub Issues](https://github.com/your-org/hodei-artifacts/issues)
- **Discussions**: [GitHub Discussions](https://github.com/your-org/hodei-artifacts/discussions)
- **Security**: security@hodei-artifacts.org

## Roadmap

### Version 1.1 (Q1 2024)
- [ ] PyPI format support
- [ ] Helm chart repository support
- [ ] Advanced search capabilities
- [ ] Webhook notifications

### Version 1.2 (Q2 2024)
- [ ] Geo-replication
- [ ] Advanced caching strategies
- [ ] Performance optimizations
- [ ] Enhanced monitoring

### Version 2.0 (Q3 2024)
- [ ] GraphQL API
- [ ] Advanced analytics
- [ ] Machine learning integration
- [ ] Enterprise features

---

**Made with ❤️ by the Hodei Artifacts team**