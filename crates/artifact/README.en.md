# Artifact Crate

Crate for managing binary artifacts: upload, download, metadata, idempotency, and event publishing.

## Architecture

This crate implements:
- **Artifact Upload** with multipart/form-data
- **Physical Artifact Deduplication** based on SHA256 hash
- **Storage in S3/MinIO** for binaries
- **Metadata Persistence** in MongoDB
- **Event Publishing** via RabbitMQ
- **Hexagonal Architecture** with ports and adapters

## Structure

```
src/
  features/
    upload_artifact/        # Complete upload feature
      api.rs               # HTTP Endpoint (Axum)
      use_case.rs          # Business logic
      adapter.rs           # Real implementations (S3, MongoDB, RabbitMQ)
      test_adapter.rs      # Mocks for testing
      ports.rs             # Traits/interfaces
      dto.rs               # Request/Response DTOs
      error.rs             # Feature-specific errors
      di.rs                # Dependency Injection container
      use_case_test.rs     # Unit tests for the use case
  domain/                  # Domain entities
tests/                     # Integration tests
  it_upload_artifact.rs    # Complete end-to-end tests
  it_mongodb_isolated.rs   # Isolated MongoDB tests
  it_testcontainers_isolated.rs  # Basic container tests
```

## Detailed Architecture

### Operational Excellence
The `artifact` crate is designed to be robust and easy to operate in production:
- **Structured Error Handling**: Uses `thiserror` to define specific errors (`UploadArtifactError`, `DomainError`), allowing clear mapping to HTTP status codes (`api.rs`) and facilitating debugging.
- **Resilience**: The ports and adapters architecture allows simulating failures in external dependencies (MongoDB, S3, RabbitMQ) using mocks (`test_adapter.rs`), ensuring the system behaves correctly during interruptions.
- **Observability (Logging and Tracing)**: Integration with the `tracing` crate (`api.rs`, `use_case.rs`, `test_adapter.rs`) provides structured logging and span creation (`info_span!`). This is fundamental for real-time monitoring, request tracing, and identifying bottlenecks or errors in production environments.

### Efficiency and Performance
The crate's design prioritizes efficiency and performance:
- **Physical Artifact Deduplication**: Before storing a binary, `use_case.rs` calculates an SHA256 hash and checks if the physical artifact already exists in the repository. This prevents redundant storage of identical data, optimizing space usage and reducing upload time for duplicate content.
- **Asynchronous Operations**: Extensive use of `async/await` with `tokio` ensures non-blocking I/O operations, crucial for handling a high volume of concurrent requests and maintaining high responsiveness.
- **Optimized Storage**: Integration with S3/MinIO (`adapter.rs`) provides a scalable and high-performance object storage solution, ideal for large volumes of binary data.
- **Multipart Upload Handling**: The HTTP endpoint (`api.rs`) is optimized to handle `multipart/form-data`, allowing efficient uploads of large files.

### Security
Security is a fundamental pillar in the crate's design:
- **Content Integrity**: The calculation and use of the SHA256 hash (`use_case.rs`) not only serves for deduplication but also acts as an integrity verification, ensuring the artifact has not been altered.
- **Immutability of Physical Artifacts**: `PhysicalArtifacts` are immutable and their identity is based on their content hash, which prevents unauthorized modifications of stored binaries.
- **Attribute-Based Access Control (ABAC) with Cedar**: Integration with `cedar-policy` (`cedar_adapter.rs`) allows defining granular authorization policies. `PackageVersion` exposes attributes and hierarchical relationships for Cedar to evaluate access permissions precisely.
- **System User for Internal Operations**: Lifecycle operations are attributed to a `UserId::new_system_user()`, a good practice for auditing and segregating permissions for automated actions.

### Best Practices
The crate rigorously follows modern software design principles:
- **Vertical Slice Architecture (VSA)**: The code organization in `src/features/upload_artifact` (with `api.rs`, `use_case.rs`, `adapter.rs`, `ports.rs`, etc.) vertically encapsulates the upload functionality, improving cohesion and maintainability.
- **Hexagonal Architecture (Ports & Adapters)**: The clear separation of interfaces (`ports.rs`) and their concrete implementations (`adapter.rs`, `test_adapter.rs`) promotes low coupling, testability, and flexibility to change underlying implementations.
- **Domain-Driven Design (DDD)**: The `domain/` module contains the central entities (`PackageVersion`, `PhysicalArtifact`) and domain events (`events.rs`), ensuring that business logic is the main focus and is well-modeled.
- **Dependency Injection (DI)**: The `di.rs` module facilitates the configuration and wiring of dependencies, both for production and testing environments, simplifying complexity management and improving testability.

### Monitoring
System visibility is achieved through:
- **Tracing Integration**: Detailed logs and spans generated by `tracing` allow tracking the execution flow of requests, identifying errors, and analyzing performance. This is crucial for integrating with APM tools and centralized monitoring systems.
- **Domain Event Publishing**: Emitting `ArtifactEvent`s via RabbitMQ provides a stream of business events that can be consumed by auditing systems, data analytics, or monitoring dashboards for real-time insights into repository activity.

## Tests

This crate includes both unit tests and end-to-end integration tests, designed to ensure code quality and robustness from multiple perspectives.

### Unit Tests

Unit tests use mocks and do not require external services, making them fast and reliable:

```bash
# Run all unit tests
cargo test --lib -p artifact

# Run tests for a specific module
cargo test -p artifact use_case_test

# With detailed logs
RUST_LOG=debug cargo test --lib -p artifact -- --nocapture
```

**Location**: `src/features/upload_artifact/use_case_test.rs` and `src/features/upload_artifact/api_test.rs`

**Contribution to Architecture**:
- **Correctness and Reliability**: They verify the core business logic (`use_case.rs`) and API behavior (`api.rs`) in isolation, ensuring each component functions as expected.
- **Development Efficiency**: They provide a fast feedback loop for developers, allowing early detection and correction of errors.
- **Maintainability**: By using mocks (`test_adapter.rs`), they ensure that changes in one component do not unexpectedly break others, facilitating safe refactorings.
- **Observability Verification**: The use of `traced_test` allows verifying that logs and spans are generated correctly, ensuring that `tracing` instrumentation is effective for monitoring.

**Coverage**:
- ✅ New artifact upload (complete creation)
- ✅ Existing artifact deduplication
- ✅ Command validation
- ✅ Error handling
- ✅ HTTP API behavior (responses, status codes)
- ✅ Verification of generated logs and spans

### Integration Tests

Integration tests use real Docker containers (`testcontainers`) to simulate a near-production environment and are protected by the `integration` feature.

```bash
# Verify compilation without execution
cargo test -p artifact --features integration --no-run

# Run all integration tests (requires Docker)
cargo test -p artifact --features integration -- --ignored

# Run a specific test
cargo test -p artifact --features integration test_upload_new_artifact_integration -- --ignored

# With testcontainers logs
RUST_LOG=testcontainers=debug,artifact=info cargo test -p artifact --features integration -- --ignored --nocapture
```

**Location**: `tests/it_upload_artifact.rs`, `tests/it_mongodb_isolated.rs`, `tests/it_testcontainers_isolated.rs`

**Infrastructure**: Each test spins up Docker containers with:
- MongoDB 6.0
- MinIO (S3-compatible)
- RabbitMQ 3.13

**Contribution to Architecture**:
- **Operational Excellence**: They validate interaction with real external dependencies, ensuring the system functions correctly in an integrated environment, which reduces deployment risks.
- **Performance and Efficiency**: Concurrency and large file handling tests (`it_upload_artifact.rs`) directly validate the system's performance and scalability characteristics.
- **Resilience**: By using real containers, the system's ability to interact with and recover from potential issues with external services is verified.
- **Security**: Indirectly, they validate data flow and integrity with real storage.
- **Hexagonal Architecture Validation**: They confirm that production adapters integrate correctly with ports and the use case.

**Coverage**:
- ✅ **Basic end-to-end upload** - HTTP → MongoDB + S3 + Events
- ✅ **Deduplication** - Same content, different versions
- ✅ **HTTP Validation** - Missing metadata, missing file, invalid JSON
- ✅ **Large files** - Upload de 5MB
- ✅ **Multiple artifacts** - 3 different uploads
- ✅ **Concurrency** - 5 simultaneous uploads of the same content
- ✅ **Infrastructure Connectivity** - Isolated tests for MongoDB and generic containers.

### Infrastructure Tests

Isolated tests to verify basic connectivity:

```bash
# MongoDB connection test
cargo test -p artifact --features integration test_mongodb_isolated_connection -- --ignored

# Basic container test
cargo test -p artifact --features integration test_hello_world_container -- --ignored
```

## Docker Configuration for Tests

### Requirements

- Docker Desktop or Docker Engine
- Sufficient resources (2GB+ RAM for containers)
- Dynamic port available

### Troubleshooting on Linux

If you experience timeouts with testcontainers on distributions like Deepin/Ubuntu:

1. **Verify Docker is working**:
   ```bash
   docker ps
   docker run hello-world
   ```

2. **For systems with Podman**: Configure testcontainers to use Podman:
   ```bash
   # Enable Podman API
   podman system service --time=0 &
   
   # Create ~/.testcontainers.properties
   echo "docker.host=unix://${XDG_RUNTIME_DIR}/podman/podman.sock" > ~/.testcontainers.properties
   echo "ryuk.container.privileged=true" >> ~/.testcontainers.properties
   ```

3. **Verify permissions**:
   ```bash
   sudo usermod -aG docker $USER
   # Reiniciar sesión
   ```

## Development

### Adding new tests

1. **Unit tests**: Add in `use_case_test.rs` using mocks
2. **Integration tests**: Add in `it_upload_artifact.rs` with `#[ignore]`

### Integration test structure

```rust
#[tokio::test]
#[ignore]  // Do not run by default
async fn test_my_new_case() {
    let context = setup_test_environment().await;  // Complete infrastructure
    
    // Prepare data
    let form = Form::new()...;
    
    // Execute
    let response = context.http_client.post(...).send().await.unwrap();
    
    // Verify HTTP
    assert_eq!(response.status(), StatusCode::CREATED);
    
    // Verify MongoDB persistence
    let db = context.mongo_client.database("test_db");
    // ... DB verifications
}
```

## Dependencies

- **Runtime**: `axum`, `mongodb`, `aws-sdk-s3`, `lapin`, `bytes`, `serde`
- **Testing**: `reqwest`, `testcontainers`, `tokio-test`
- **Features**: `integration` (for integration tests)

See `Cargo.toml` for specific versions.
