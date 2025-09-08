# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Repository Overview

Hodei Artifacts is a next-generation artifact repository built in Rust, designed for high performance, supply chain security, and scalability. It's a modular monolith using Vertical Slice Architecture (VSA), Hexagonal Architecture, and Event-Driven Architecture (EDA).

## Essential Commands

### Build and Test Commands
```bash
# Build the project
cargo build

# Run all tests (unit + integration)
cargo test

# Run only unit tests
cargo test --lib --bins

# Run only integration tests
cargo test --tests

# Run tests for a specific crate
cargo test -p <crate_name>

# Run tests with logging output
RUST_LOG=info cargo test -- --nocapture

# Run the main binary (HTTP server)
cargo run
```

### E2E Testing
```bash
# Navigate to e2e directory and run Playwright tests
cd e2e
npm ci
npx playwright install --with-deps
npx playwright test
```

### Code Quality Verification
```bash
# Verify no inline tests (enforces testing organization policy)
bash scripts/verify-no-inline-tests.sh

# Check OpenAPI drift
bash scripts/check-openapi-drift.sh
```

### Development Services
```bash
# Start development dependencies (MongoDB, Kafka, S3/MinIO, Cedar)
docker-compose up -d

# Stop services
docker-compose down
```

## Architecture Overview

### Workspace Structure
- **Monorepo**: All crates organized under `crates/` directory
- **Main binary**: `src/main.rs` bootstraps HTTP server using Axum
- **Shared types**: `crates/shared/` for cross-cutting concerns

### Key Crates (Bounded Contexts)
- `artifact/`: Artifact management (upload/download/metadata)
- `iam/`: Identity and Access Management with ABAC (Cedar policies)
- `search/`: Search and indexing with Tantivy
- `repository/`: Data access abstractions and MongoDB adapters
- `distribution/`: Distribution/CDN functionality (Maven, npm, PyPI)
- `security/`: Security scanning and supply chain verification
- `infra-mongo/`: MongoDB client and testing utilities

### Architectural Patterns

## Vertical Slice Architecture (VSA) (MUY IMPORTANTE)
### Arquitectura Multi-Crate con DDD Estricto y DI Flexible

### Estructura de Directorios y Principios

```
/
├── Cargo.toml                      # Workspace de Rust
├── crates/
│   ├── todo_management/            # Bounded Context de Gestión de Tareas
│   │   ├── src/
│   │   │   ├── lib.rs              # API pública del contexto
│   │   │   ├── domain/             # Dominio compartido dentro del contexto
│   │   │   │   ├── todo.rs         # Entidad Todo con lógica de negocio
│   │   │   │   ├── status.rs       # Value Object: Estado de tarea
│   │   │   │   ├── id.rs           # Value Object: TodoId
│   │   │   │   └── mod.rs          # Exportaciones públicas del dominio
│   │   │   ├── features/           # Funcionalidades verticalmente aisladas
│   │   │   │   ├── create_todo/    # Feature: Crear tarea
│   │   │   │   │   ├── mod.rs      # Exportación del módulo feature
│   │   │   │   │   ├── use_case.rs # Lógica específica del caso de uso
│   │   │   │   │   ├── dto.rs      # Comandos, queries y DTOs específicos
│   │   │   │   │   ├── ports.rs    # Interfaces SEGREGADAS para esta feature
│   │   │   │   │   ├── adapter.rs  # Implementaciones CONCRETAS de ports
│   │   │   │   │   ├── api.rs # Punto de entrada de la feature
│   │   │   │   │   └── di.rs       # Configuración de dependencias flexible
│   │   │   │   ├── complete_todo/  # Feature: Completar tarea
│   │   │   │   │   └── ...         # Misma estructura con sus PROPIOS ports
│   │   │   │   └── list_todos/     # Feature: Listar tareas
│   │   │   │       └── ...         # Misma estructura con sus PROPIOS ports
│   │   │   ├── error.rs            # Errores específicos del contexto
│   │   │   └── types.rs            # Tipos públicos compartidos
│   │   └── Cargo.toml              # Dependencias del crate
└── src/
    └── api_http/                   # Ejecutable HTTP principal
        ├── src/
        │   ├── main.rs             # Punto de entrada y configuración HTTP
        │   └── di_config.rs        # Configuración global de implementaciones
        └── Cargo.toml              # Dependencias del ejecutable
```

## Explicación Detallada de Cada Archivo en una Feature

### `crates/todo_management/src/features/create_todo/ports.rs`
```rust
// Interfaces ESPECÍFICAS y SEGREGADAS para create_todo
// Cada feature define SUS PROPIOS ports, incluso si son similares a otros

#[async_trait]
pub trait TodoCreatorRepository: Send + Sync {
    // SOLO el método que esta feature necesita
    async fn save_todo(&self, todo: Todo) -> Result<(), RepositoryError>;
}

#[async_trait]
pub trait TodoNotifier: Send + Sync {
    // SOLO el método que esta feature necesita
    async fn notify_todo_created(&self, todo: Todo) -> Result<(), NotificationError>;
}
```

### `crates/todo_management/src/features/create_todo/dto.rs`
```rust
// Estructuras de datos específicas para create_todo
pub struct CreateTodoCommand {
    pub title: String,
    pub description: Option<String>,
}

pub struct TodoCreatedResponse {
    pub id: TodoId,
    pub title: String,
    pub status: TodoStatus,
}
```

### `crates/todo_management/src/features/create_todo/use_case.rs`
```rust
// Lógica de negocio específica para crear un todo
pub struct CreateTodoUseCase {
    repository: Arc<dyn TodoCreatorRepository>,
    notifier: Arc<dyn TodoNotifier>,
}

impl CreateTodoUseCase {
    pub fn new(
        repository: Arc<dyn TodoCreatorRepository>,
        notifier: Arc<dyn TodoNotifier>,
    ) -> Self {
        Self { repository, notifier }
    }
    
    pub async fn execute(&self, command: CreateTodoCommand) -> Result<TodoCreatedResponse, TodoError> {
        // Validar comando
        let todo = Todo::new(command.title, command.description);
        
        // Persistir
        self.repository.save_todo(todo.clone()).await?;
        
        // Notificar
        self.notifier.notify_todo_created(todo.clone()).await?;
        
        // Devolver respuesta
        Ok(TodoCreatedResponse {
            id: todo.id,
            title: todo.title,
            status: todo.status,
        })
    }
}
```

### `crates/todo_management/src/features/create_todo/adapter.rs`
```rust
// Implementaciones CONCRETAS de los ports
// Cada feature tiene sus PROPIAS implementaciones

// Adaptador para producción - PostgreSQL
pub struct PostgresTodoCreatorRepository {
    pool: Arc<sqlx::PgPool>,
}

impl PostgresTodoCreatorRepository {
    pub fn new(pool: Arc<sqlx::PgPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TodoCreatorRepository for PostgresTodoCreatorRepository {
    async fn save_todo(&self, todo: Todo) -> Result<(), RepositoryError> {
        // Implementación real con PostgreSQL
        sqlx::query!(
            "INSERT INTO todos (id, title, description, status) VALUES ($1, $2, $3, $4)",
            todo.id.value(),
            todo.title,
            todo.description,
            todo.status.to_string()
        )
        .execute(&*self.pool)
        .await?;
        
        Ok(())
    }
}

// Adaptador para producción - HTTP Notifier
pub struct HttpTodoNotifier {
    client: reqwest::Client,
    base_url: String,
}

impl HttpTodoNotifier {
    pub fn new(base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url,
        }
    }
}

#[async_trait]
impl TodoNotifier for HttpTodoNotifier {
    async fn notify_todo_created(&self, todo: Todo) -> Result<(), NotificationError> {
        self.client
            .post(&format!("{}/notifications/todo-created", self.base_url))
            .json(&todo)
            .send()
            .await?;
            
        Ok(())
    }
}

// Adaptadores para testing (solo compilan en tests)
#[cfg(test)]
pub mod test {
    use super::*;
    use std::sync::Mutex;
    
    pub struct MockTodoCreatorRepository {
        pub saved_todos: Mutex<Vec<Todo>>,
    }
    
    impl MockTodoCreatorRepository {
        pub fn new() -> Self {
            Self { saved_todos: Mutex::new(Vec::new()) }
        }
    }
    
    #[async_trait]
    impl TodoCreatorRepository for MockTodoCreatorRepository {
        async fn save_todo(&self, todo: Todo) -> Result<(), RepositoryError> {
            self.saved_todos.lock().unwrap().push(todo);
            Ok(())
        }
    }
    
    pub struct MockTodoNotifier {
        pub notified_todos: Mutex<Vec<Todo>>,
    }
    
    impl MockTodoNotifier {
        pub fn new() -> Self {
            Self { notified_todos: Mutex::new(Vec::new()) }
        }
    }
    
    #[async_trait]
    impl TodoNotifier for MockTodoNotifier {
        async fn notify_todo_created(&self, todo: Todo) -> Result<(), NotificationError> {
            self.notified_todos.lock().unwrap().push(todo);
            Ok(())
        }
    }
}
```

### `crates/todo_management/src/features/create_todo/api.rs`
```rust
// Punto de entrada de la feature
pub struct CreateTodoEndpoint {
    use_case: CreateTodoUseCase,
}

impl CreateTodoEndpoint {
    pub fn new(use_case: CreateTodoUseCase) -> Self {
        Self { use_case }
    }
    
    pub async fn create_todo(
        &self,
        command: CreateTodoCommand,
    ) -> Result<TodoCreatedResponse, TodoError> {
        // Logging, métricas, etc.
        tracing::info!("Creating todo: {}", command.title);
        
        let result = self.use_case.execute(command).await;
        
        // Transformación de errores si es necesario
        result.map_err(|e| {
            tracing::error!("Failed to create todo: {}", e);
            e
        })
    }
}
```

### `crates/todo_management/src/features/create_todo/di.rs`
```rust
// Configuración de inyección de dependencias FLEXIBLE
// Acepta cualquier implementación de los ports
use super::ports::{TodoCreatorRepository, TodoNotifier};
use super::use_case::CreateTodoUseCase;
use super::api::CreateTodoEndpoint;
use std::sync::Arc;

pub struct CreateTodoDIContainer {
    pub endpoint: CreateTodoEndpoint,
}

impl CreateTodoDIContainer {
    // Constructor flexible que acepta cualquier implementación de los ports
    pub fn new(
        repository: Arc<dyn TodoCreatorRepository>,
        notifier: Arc<dyn TodoNotifier>,
    ) -> Self {
        let use_case = CreateTodoUseCase::new(repository, notifier);
        let endpoint = CreateTodoEndpoint::new(use_case);
        
        Self { endpoint }
    }
    
    // Método de conveniencia para producción
    pub fn for_production(
        db_pool: Arc<sqlx::PgPool>,
        notification_url: String,
    ) -> Self {
        let repository: Arc<dyn TodoCreatorRepository> = 
            Arc::new(PostgresTodoCreatorRepository::new(db_pool));
        
        let notifier: Arc<dyn TodoNotifier> = 
            Arc::new(HttpTodoNotifier::new(notification_url));
        
        Self::new(repository, notifier)
    }
    
    // Método de conveniencia para testing
    #[cfg(test)]
    pub fn for_testing() -> Self {
        use super::adapter::test::{MockTodoCreatorRepository, MockTodoNotifier};
        
        let repository: Arc<dyn TodoCreatorRepository> = 
            Arc::new(MockTodoCreatorRepository::new());
        
        let notifier: Arc<dyn TodoNotifier> = 
            Arc::new(MockTodoNotifier::new());
        
        Self::new(repository, notifier)
    }
}
```

### `crates/todo_management/src/features/create_todo/mod.rs`
```rust
// Exportaciones públicas de la feature
mod use_case;
mod dto;
mod ports;
mod adapter;
mod api;
mod di;

// Solo exponer lo necesario al exterior
pub use dto::{CreateTodoCommand, TodoCreatedResponse};
pub use api::CreateTodoEndpoint;
pub use di::CreateTodoDIContainer;
```

## Configuración Global en el Ejecutable
Depende del framework a usar.

## Beneficios de Esta Arquitectura

1. **Aislamiento total**: Cada feature es independiente con sus propias interfaces
2. **Sustitución fácil**: Implementaciones intercambiables para diferentes entornos
3. **Testing simplificado**: Mocks específicos para cada feature
4. **Mantenibilidad**: Cambios en una feature no afectan a otras
5. **Escalabilidad**: Nuevas features se añaden sin afectar las existentes
6. **Principios SOLID**: Segregación de interfaces y inversión de dependencias

Esta arquitectura permite un desarrollo ágil con testing robusto y despliegues flexibles para diferentes entornos, manteniendo un código limpio y mantenible.

## Development Practices

### Feature Implementation Order
1. **Validate** (pure functions, no I/O)
2. **Check idempotency** (read-only operations)
3. **Pure logic** (domain model construction)
4. **Side effects** (storage → repository → events)

### Testing Strategy
- **Unit tests**: Separate `_test.rs` files alongside source code
- **Integration tests**: `tests/it_*.rs` with testcontainers
- **E2E tests**: Playwright tests in `e2e/` directory
- **No inline tests**: Enforced by CI script

### Error Handling
- Feature-specific error enums with `thiserror`
- Proper error propagation and HTTP status mapping
- Meaningful error messages with context

### Code Organization Rules
- No `#[cfg(test)]` modules in `src/` files
- Unit tests in separate `*_test.rs` files
- Integration tests use testcontainers (no external services required)
- All public APIs must be documented with doc comments

## OpenAPI Contract
- **Source of truth**: `docs/openapi/openapi.yaml`
- **Modular structure**: Components split across `docs/openapi/` subdirectories
- **Contract-first**: API implementations must match OpenAPI specification
- **Drift detection**: Automated checks in CI

## Key Configuration Files
- `Cargo.toml`: Workspace configuration with shared dependencies
- `docker-compose.yml`: Development services (MongoDB, Kafka, LocalStack S3, Cedar)
- `docs/feature-style-guide.md`: Detailed implementation patterns
- `docs/testing-organization.md`: Testing conventions and structure

## Important Notes
- **Rust Edition 2024**: All crates use the latest Rust edition
- **Async runtime**: Tokio with full features
- **Observability**: Tracing, metrics, and structured logging
- **Security**: ABAC with Cedar policies, SBOM generation planned
- **Package formats**: Maven, npm, PyPI compatibility

## IMPORTANTE: Testing
- facilitar el testing rapido. 
- no recompilar el código que no se toca.
- no usar println para logear usar el crate tracing.
-  buscar una solución en el testing para que el crate de tracing se use para hacer recuperar y crear asserts que comprueben logs y spans.
- antes de centrarse en los test de integración, hay que hacer test amplios unitarios sobre los casos de uso use_case.rs y el uso del api endpoint apir.rs, mockeando todos los servicios necesarios.
- aprovechar el crate de tracing para crear asserts en los tests que comprueben logs, span etc                                                            
- también testear los eventos producidos en las features.
- usar los scripts de makefile para ejecutar los tests

## IMPORTANTE: git Conventional commit
- Crear mensajes con patron de conventional commit
- PROHIBIDO poner en los mensajes autores y coautores solo datos de los cambios
- No commitear sin supervision. 