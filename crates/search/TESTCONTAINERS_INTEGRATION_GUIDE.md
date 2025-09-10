# Testcontainers Integration Guide

## Overview

This guide provides instructions on how to use Testcontainers for integration testing in the Hodei Artifacts search module. Testcontainers allows us to run real services (MongoDB, Tantivy, etc.) in Docker containers during testing, providing a more realistic testing environment.

## Prerequisites

Before using Testcontainers, ensure you have:

1. Docker installed and running on your system
2. Proper permissions to run Docker commands
3. Internet access to pull Docker images

## Setting Up Testcontainers

### 1. Cargo.toml Configuration

Add the following dependencies to your `Cargo.toml`:

```toml
[dev-dependencies]
testcontainers = { workspace = true }
testcontainers-modules = { workspace = true, features = ["mongo"] }
```

### 2. Basic Testcontainers Usage

Here's a basic example of how to use Testcontainers in your tests:

```rust
use testcontainers::{core::WaitFor, runners::AsyncRunner, GenericImage};

#[tokio::test]
async fn test_with_mongodb_container() {
    // Start MongoDB container
    let mongo_image = GenericImage::new("mongo", "7.0")
        .with_wait_for(WaitFor::message_on_stderr("Waiting for connections"));
    let _mongo_container = mongo_image.start().await.unwrap();
    
    // Your test code here
    assert!(true);
}
```

## Available Testcontainers Modules

### MongoDB Container

For testing with MongoDB:

```rust
use testcontainers_modules::mongo::Mongo;

#[tokio::test]
async fn test_with_mongodb_module() {
    let mongo = Mongo::default();
    let _container = mongo.start().await.unwrap();
    
    // Your test code here
}
```

### Custom Containers

For services not available as specific modules, use `GenericImage`:

```rust
use testcontainers::{core::WaitFor, runners::AsyncRunner, GenericImage};

#[tokio::test]
async fn test_with_custom_container() {
    let image = GenericImage::new("alpine", "latest")
        .with_wait_for(WaitFor::seconds(1))
        .with_env_var("TEST_VAR", "test_value");
    let _container = image.start().await.unwrap();
    
    // Your test code here
}
```

## Best Practices

### 1. Use `#[ignore]` for Testcontainers Tests

Since Testcontainers tests require Docker and can be slow, mark them with `#[ignore]`:

```rust
#[tokio::test]
#[ignore]
async fn test_with_testcontainers() {
    // Your testcontainers code here
}
```

Run ignored tests with:
```bash
cargo test --test your_test_file -- --ignored
```

### 2. Clean Up Resources

Always ensure containers are properly cleaned up:

```rust
#[tokio::test]
#[ignore]
async fn test_with_cleanup() {
    let container = GenericImage::new("mongo", "7.0")
        .start()
        .await
        .unwrap();
    
    // Your test code here
    
    // Container is automatically stopped when it goes out of scope
}
```

### 3. Use Environment Variables for Configuration

Make tests configurable:

```rust
use std::env;

#[tokio::test]
#[ignore]
async fn test_with_configurable_image() {
    let mongo_version = env::var("MONGO_VERSION").unwrap_or("7.0".to_string());
    let image = GenericImage::new("mongo", &mongo_version);
    let _container = image.start().await.unwrap();
    
    // Your test code here
}
```

## Running Testcontainers Tests

### 1. Run All Tests (Skipping Testcontainers)

```bash
cargo test -p search
```

### 2. Run Only Testcontainers Tests

```bash
cargo test -p search --test your_test_file -- --ignored
```

### 3. Run Specific Testcontainers Test

```bash
cargo test -p search --test your_test_file test_name -- --ignored
```

## Common Test Scenarios

### 1. Full Stack Integration Test

```rust
#[tokio::test]
#[ignore]
async fn test_full_stack_integration() {
    // Start all required services
    let mongo = GenericImage::new("mongo", "7.0")
        .with_wait_for(WaitFor::message_on_stderr("Waiting for connections"));
    let _mongo_container = mongo.start().await.unwrap();
    
    let rabbitmq = GenericImage::new("rabbitmq", "3.12")
        .with_wait_for(WaitFor::message_on_stderr("Server startup complete"));
    let _rabbitmq_container = rabbitmq.start().await.unwrap();
    
    // Your integration test code here
}
```

### 2. Network Isolation Test

```rust
#[tokio::test]
#[ignore]
async fn test_network_isolation() {
    // Create containers on the same network
    let service1 = GenericImage::new("alpine", "latest");
    let _container1 = service1.start().await.unwrap();
    
    let service2 = GenericImage::new("alpine", "latest");
    let _container2 = service2.start().await.unwrap();
    
    // Test network connectivity between containers
}
```

### 3. Volume Mounting Test

```rust
#[tokio::test]
#[ignore]
async fn test_volume_mounting() {
    let image = GenericImage::new("alpine", "latest")
        .with_volume("/host/path", "/container/path");
    let _container = image.start().await.unwrap();
    
    // Test persistent data storage
}
```

## Troubleshooting

### 1. Docker Not Running

Error: `Cannot connect to the Docker daemon`

Solution: Start Docker service
```bash
sudo systemctl start docker
# or on macOS
open -a Docker
```

### 2. Permission Denied

Error: `permission denied while trying to connect to the Docker daemon socket`

Solution: Add user to docker group
```bash
sudo usermod -aG docker $USER
```

### 3. Image Pull Failed

Error: `pull access denied for image`

Solution: Check image name and tag, ensure internet access

### 4. Container Startup Timeout

Error: Container fails to start within timeout

Solution: Increase wait time or check container logs
```rust
let image = GenericImage::new("mongo", "7.0")
    .with_wait_for(WaitFor::seconds(30)); // Increase timeout
```

## Future Improvements

1. **CI/CD Integration**: Configure GitHub Actions to run Testcontainers tests
2. **Test Matrix**: Test against multiple versions of services
3. **Resource Management**: Implement resource pooling for faster test execution
4. **Monitoring**: Add container resource usage monitoring
5. **Parallel Execution**: Optimize parallel test execution with Testcontainers

## Conclusion

Testcontainers provides a powerful way to test your application with real services in a controlled environment. By following these guidelines, you can create reliable and realistic integration tests that closely mimic production conditions.