# Definición Infrastructura

### 1. Estructura Multi-Crate por Bounded Context

- Cada *bounded context* es un `crate` independiente.
- Use una estructura de *workspace* en `Cargo.toml`.

**Ejemplo:**

```toml
[workspace]
members = [
    "crates/todo_management",
    "crates/user_management",
    "crates/shared",  # Para shared kernel
]
```

### 2. Shared Kernel para Dominio Compartido

- Si varios *bounded contexts* necesitan compartir entidades de dominio, colóquelas en `crates/shared`.
- Fomente la copia particular de entidades si solo un *bounded context* las necesita, para evitar acoplamiento.

**Ejemplo en `crates/shared/src/lib.rs`:**

```text
├── crates/
│   ├── shared/    
│   │   ├── src/
│   │   │   ├── lib.rs       
│   │   │   ├── domain/      # Dominio compartido
│   │   │   │   ├── aggregate.rs     
│   │   │   │   ├── hrn.rs       
│   │   │   │   ├── id.rs    # Value Object: TodoId
│   │   │   │   └── mod.rs   # Exportaciones públicas del dominio
```

### 3. Servicios Transversales

Para funcionalidades compartidas como autenticación, feature flags, etc., se deben crear `traits` en `crates/shared`. Las implementaciones concretas se proporcionan en los adaptadores de la capa de aplicación principal, permitiendo que cada *bounded context* las use sin acoplarse a la implementación.

**Ejemplo de trait para Autenticación:**
Un caso de uso común es necesitar el identificador del usuario que realiza una operación. Este `trait` abstrae la obtención de esa información.

**Definición del Trait en `crates/shared/src/auth.rs`:**

```rust
use crate::domain::UserId; // Asumiendo que UserId está en el dominio compartido

// Error específico para la autenticación
#[derive(Debug)]
pub enum AuthError {
    NoCurrentUser,
}

// Trait que provee el usuario actual
pub trait CurrentUserProvider: Send + Sync {
    fn current_user_id(&self) -> Result<UserId, AuthError>;
}
```

### 4. Reglas de Implementación por Feature

- Cada *feature* dentro de un *bounded context* debe seguir VSA: tener sus propios *ports*, *adapters*, *use case*, etc.
- No compartir *ports* entre *features*; cada *feature* define los suyos.

**Ejemplo de ports en una feature:**

```rust
// crates/todo_management/src/features/create_todo/ports.rs
pub trait TodoPersister {
    fn save(&self, todo: Todo) -> Result<(), Error>;
}
```

### 5. Inyección de Dependencias

- Inyectar servicios (como `CurrentUserProvider`) en los *use cases* a través de DI (Inyección de Dependencias).

**Ejemplo:**

```rust
// crates/todo_management/src/features/create_todo/use_case.rs
use shared::auth::CurrentUserProvider;
use std::sync::Arc;

pub struct CreateTodoUseCase<TP: TodoPersister> {
    persister: TP,
    user_provider: Arc<dyn CurrentUserProvider>, // Dependencia inyectada
}
```

### 6. Evitar Acoplamiento

- Nunca importar entidades de dominio de un *bounded context* a otro directamente. Use el *shared kernel* o copie la entidad.
- Si se copia, asegúrese de que la entidad copiada sea específica para ese *bounded context*.

### 7. Ejemplo de Copia Particular

- Si `user_management` tiene una entidad `User` y `todo_management` necesita solo `UserInfo`, copie `UserInfo` en `todo_management` en lugar de compartirla.

**En `todo_management/src/domain/user_info.rs`:**

```rust
pub struct UserInfo {
    pub id: String,
    pub name: String,
}
```

### 8. Patrón Kernel

- En `crates/shared`, definir solo los elementos de dominio que son realmente compartidos y estables.
- Evite poner lógica de negocio en el *shared kernel*; solo datos simples y `traits`.

### 9. Estrategia de Testing

El objetivo es un testing rápido y eficiente.

- **Corredor de Pruebas `nextest`:** Adoptar `cargo-nextest` como el corredor de pruebas principal por su velocidad y mejor feedback.
- **Feedback Rápido:** Aprovechar su ejecución paralela para reducir drásticamente los tiempos de validación en el desarrollo local (TDD).
- **Optimización para CI:** Integrar `nextest` en el pipeline de Integración Continua para mantener los builds ágiles y fiables.
- **Prioridad Unitaria:** Foco en tests unitarios amplios sobre `use_case.rs` y `api.rs`, mockeando todas las dependencias externas. Testear también los eventos de dominio emitidos.
- **Logging con `tracing`:** No usar `println!`. Utilizar el crate `tracing` para capturar logs y spans, permitiendo crear *asserts* que verifiquen el comportamiento interno.
- **Tests de Integración con `testcontainers`:** Usar `testcontainers` y Docker Compose para levantar entornos aislados y reproducibles (BBDD, colas, etc.), evitando conflictos entre tests paralelos.
- **Ejecución Centralizada:** Usar `Makefile` para ejecutar todos los tipos de tests de forma consistente.

#### Estructura de Archivos de Test

**Tests Unitarios (dentro de `src/`)**
Se colocan en archivos `*_test.rs` junto al código que prueban para agilizar la compilación.

```text
src/
├── modules/
│   ├── calculator.rs          # Código fuente
│   └── calculator_test.rs     # Tests unitarios
```

**Tests de Integración (directorio `tests/`)**
Prueban el crate como una caja negra.

```text
crates/<context>/
  └── tests/
      ├── it_calculator.rs     # Tests de integración
      └── compose/
          └── docker-compose.yml # Entorno para testcontainers
```

### Resumen de Estructura de Directorios

```text
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
├── user_management/
│   └── ... similar ...
├── shared/
│   ├── src/
│   │   ├── auth.rs
│   │   └── domain/
│   └── ...
└── src/
    └── api_http/                   # Ejecutable HTTP principal
        ├── src/
        │   ├── main.rs             # Punto de entrada y configuración HTTP
        │   └── di_config.rs        # Configuración global de implementaciones
        └── Cargo.toml              # Dependencias del ejecutable
    
```

## Flujo de Trabajo de ejemplo para Historia de Usuario

### 📋 Análisis de Requisitos (Historia x.x)
**Detalle de la historia:**

#### Actividades preparatorias
- Revisar arquitectura del sistema e implementación existente de features similares
- Definir estructura de directorios y componentes para la nueva feature
- Crear lista de tareas detalladas para la implementación

#### 🎯 Tareas de Implementación

| Estado | Tarea | Descripción                       | Ubicación |
|--------|-------|-----------------------------------|-----------|
| ○ | Tarea 1 | Definir modelos de dominio        | `(nombre_crate)/src/domain/xxx.rs` |
| ○ | Tarea 2 | Crear estructura de directorios   | Feature-specific directory |
| ○ | Tarea 3 | Crear archivo mod.rs              | `mod.rs` para la feature |
| ○ | Tarea 4 | Definir abstracciones (puertos)   | `ports.rs` (segregación de interfaces SOLID) |
| ○ | Tarea 5 | Implementar adaptadores concretos | `adapter.rs` (SyftSbomGenerator, examplePartialRepository) |
| ○ | Tarea 6 | Desarrollar caso de uso           | `use_case.rs` |
| ○ | Tarea 7 | Conectar punto de entrada         | `api.rs` (manejador de eventos) |
| ○ | Tarea 8 | Crear DTOs                        | `dto.rs` (si es necesario) |
| ○ | Tarea 9 | Crear tests unitarios             | Para caso de uso y API |
| ○ | Tarea 10 | Crear tests de integración        | Con tests containers |
| ○ | Tarea 11 | Actualizar documentación          | Historia de usuario |

### Mensaje Final para el Agente AI

Al implementar, verifique siempre:

- ✅ Cada *bounded context* es un `crate` independiente.
- ✅ Use *shared kernel* solo para elementos verdaderamente compartidos.
- ✅ Cada *feature* tiene sus propios *ports* segregados.
- ✅ Los servicios transversales se inyectan a través de `traits`.
- ✅ Evite acoplamiento entre *bounded contexts*; copie entidades si es necesario.
- ✅ La estrategia de testing prioriza los tests unitarios rápidos y mockeados.
- 🔍 Verificar que sigue la arquitectura VSA y Clean Architecture por feature
- 🔍 Verificar la segregación de interfaces
- 🔍 Verificar que no hay servicios monolíticos
- 🔍 Verificar la estructura de directorios y módulos
- 🔍 Verificar los tests implementados
- 🔍 Identificar posibles mejoras o ajustes necesarios
- 🔍 Actualizar documentación de la historia de usuario |

Estas reglas mantendrán la arquitectura desacoplada y alineada con VSA y *Clean Architecture*.