# 9. Getting Started Guide

## 9.1 Prerequisites

* Rust 1.75+ (with `rustup`)
* Docker and Docker Compose
* SurrealDB CLI
* Node.js 18+ (for UI development)

## 9.2 Setup Development Environment

1. Clone the repository:
   ```bash
   git clone https://github.com/hodei-artifacts/platform.git
   cd platform
   ```

2. Install Rust toolchain:
   ```bash
   rustup update
   rustup component add clippy
   cargo install cargo-nextest
   ```

3. Start dependencies with Docker:
   ```bash
   docker compose -f docker-compose.dev.yml up -d
   ```

4. Build and run the application:
   ```bash
   cargo build
   cargo run -p api-http
   ```

## 9.3 Running Tests

* Run all unit tests:
  ```bash
  cargo nextest run
  ```

* Run integration tests:
  ```bash
  cargo test -p artifact --test it_*
  ```

* Run load tests:
  ```bash
  locust -f tests/load/locustfile.py
  ```

## 9.4 Development Workflow

1. Create a new feature branch:
   ```bash
   git checkout -b feature/your-feature-name
   ```

2. Implement your feature following the structure guidelines

3. Verify your changes:
   ```bash
   cargo check
   cargo clippy
   cargo nextest run
   ```

4. Commit and push your changes:
   ```bash
   git commit -m "feat: implement your feature"
   git push origin feature/your-feature-name
   ```

5. Create a pull request with:
  - Description of changes
  - Reference to relevant PRD section
  - Test coverage report
  - Performance impact analysis
