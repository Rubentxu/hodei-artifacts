# Definición de Infraestructura con Énfasis en Calidad de Código

## Reglas de Calidad de Código

- **Compilación Libre de Errores**: El código de cada crate y cada feature debe compilar sin errores.
- **Eliminación de Warnings**: Se deben resolver todos los warnings del compilador y herramientas de linting para mantener un código limpio.
- **Tests Obligatorios**: Todos los tests deben pasar. Esto incluye tests unitarios y de integración.

### Integración en el Flujo de Trabajo

1. **Desarrollo Local**: 
   - Ejecutar `cargo check` frecuentemente para verificar la compilación.
   - Ejecutar `cargo clippy` para identificar y corregir warnings y mejorar el código.
   - Ejecutar `cargo test` para asegurar que los tests pasan antes de commits.  
   - Ejecutar `cargo nextest run` para running rápido de tests en el CI.

## 1. Estructura Multi-Crate por Bounded Context

- Cada *bounded context* es un `crate` independiente.
- Use una estructura de *workspace* en `Cargo.toml`.

**Ejemplo:**

```toml
[workspace]
resolver = "2"
members = [
    "crates/todo_management",
    "crates/user_management",
    "crates/shared",  # Para shared kernel
]

[workspace.dependencies]
tokio = { version = "1.0", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
tracing = "0.1"
thiserror = "1.0"
```

## 2. Shared Kernel para Dominio Compartido

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
│   │   │   │   ├── events.rs    # Eventos de dominio compartidos
│   │   │   │   └── mod.rs   # Exportaciones públicas del dominio
│   │   │   ├── auth.rs      # Traits de autenticación
│   │   │   ├── logging.rs   # Traits de logging
│   │   │   └── error.rs     # Errores compartidos
```

## 3. Servicios Transversales

Para funcionalidades compartidas como autenticación, feature flags, etc., se deben crear `traits` en `crates/shared`. Las implementaciones concretas se proporcionan en los adaptadores de la capa de aplicación principal, permitiendo que cada *bounded context* las use sin acoplarse a la implementación.

**Ejemplo de trait para Autenticación:**
Un caso de uso común es necesitar el identificador del usuario que realiza una operación. Este `trait` abstrae la obtención de esa información.

**Definición del Trait en `crates/shared/src/auth.rs`:**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("No authenticated user")]
    NoCurrentUser,
    #[error("Insufficient permissions")]
    InsufficientPermissions,
    #[error("Invalid token")]
    InvalidToken,
}

// Trait para proveer información de autenticación
pub trait AuthContextProvider: Send + Sync {
    fn current_user_id(&self) -> Result<UserId, AuthError>;
    fn has_permission(&self, permission: &str) -> Result<bool, AuthError>;
    fn tenant_id(&self) -> Result<Option<TenantId>, AuthError>;
}
```

## 4. Reglas de Implementación por Feature

- Cada *feature* dentro de un *bounded context* debe seguir VSA: tener sus propios *ports*, *adapters*, *use case*, etc.
- No compartir *ports* entre *features*; cada *feature* define los suyos.

**Ejemplo de ports en una feature:**

```rust
// crates/todo_management/src/features/create_todo/ports.rs
use crate::domain::Todo;
use crate::features::create_todo::error::CreateTodoError;

pub trait TodoPersister {
    fn save(&self, todo: Todo) -> Result<(), CreateTodoError>;
}
```

## 5. Inyección de Dependencias

- Inyectar servicios (como `CurrentUserProvider`) en los *use cases* a través de DI (Inyección de Dependencias).

**Ejemplo:**

```rust
// crates/todo_management/src/features/create_todo/use_case.rs
use shared::auth::AuthContextProvider;
use std::sync::Arc;
use crate::features::create_todo::ports::TodoPersister;
use crate::features::create_todo::error::CreateTodoError;

pub struct CreateTodoUseCase<TP: TodoPersister> {
    persister: TP,
    auth_provider: Arc<dyn AuthContextProvider>,
}

impl<TP: TodoPersister> CreateTodoUseCase<TP> {
    pub fn new(persister: TP, auth_provider: Arc<dyn AuthContextProvider>) -> Self {
        Self { persister, auth_provider }
    }
    
    pub fn execute(&self, title: String) -> Result<(), CreateTodoError> {
        let user_id = self.auth_provider.current_user_id()
            .map_err(|_| CreateTodoError::AuthenticationFailed)?;
        
        // Lógica de negocio aquí
        let todo = Todo::new(title, user_id);
        self.persister.save(todo)?;
        
        Ok(())
    }
}
```

## 6. Evitar Acoplamiento

- Nunca importar entidades de dominio de un *bounded context* a otro directamente. Use el *shared kernel* o copie la entidad.
- Si se copia, asegúrese de que la entidad copiada sea específica para ese *bounded context*.

## 7. Ejemplo de Copia Particular

- Si `user_management` tiene una entidad `User` y `todo_management` necesita solo `UserInfo`, copie `UserInfo` en `todo_management` en lugar de compartirla.

**En `todo_management/src/domain/user_info.rs`:**

```rust
#[derive(Debug, Clone)]
pub struct UserInfo {
    pub id: String,
    pub name: String,
}
```

## 8. Patrón Kernel

- En `crates/shared`, definir solo los elementos de dominio que son realmente compartidos y estables.
- Evite poner lógica de negocio en el *shared kernel*; solo datos simples y `traits`.

## 9. Estrategia de Testing

El objetivo es un testing rápido y eficiente.

- **Corredor de Pruebas `nextest`:** Adoptar `cargo-nextest` como el corredor de pruebas principal por su velocidad y mejor feedback.
- **Feedback Rápido:** Aprovechar su ejecución paralela para reducir drásticamente los tiempos de validación en el desarrollo local (TDD).
- **Optimización para CI:** Integrar `nextest` en el pipeline de Integración Continua para mantener los builds ágiles y fiables.
- **Prioridad Unitaria:** Foco en tests unitarios amplios sobre `use_case.rs` , mockeando todas las dependencias externas. Testear también los eventos de dominio emitidos.
- **Logging con `tracing`:** No usar `println!`. Utilizar el crate `tracing` para capturar logs y spans, permitiendo crear *asserts* que verifiquen el comportamiento interno.
- **Tests de Integración con `testcontainers`:** Usar `testcontainers` y Docker Compose para levantar entornos aislados y reproducibles (BBDD, colas, etc.), evitando conflictos entre tests paralelos.
- **Ejecución Centralizada:** Usar `Makefile` para ejecutar todos los tipos de tests de forma consistente.

#### Estructura de Archivos de Test

**Tests Unitarios (dentro de Ejemplo `src/features/create_todo/`)** 
Se colocan en archivos `*_test.rs` junto al código que prueban para agilizar la compilación.
Siempre se va a testear use_case.rs que son los ficheros que tienen toda la lógica de negocio.

```text
features/
├── todo_management/
│   ├── use_case.rs          # Código fuente│   
│   └── use_case_test.rs     # Tests unitarios
```

**Tests de Integración (directorio `tests/`)**
Prueban el crate como una caja negra.

```text
crates/<context>/
  └── tests/
      ├── it_use_case_test.rs     # Tests de integración
      └── compose/
          └── docker-compose.yml # Entorno para testcontainers
```

## Estructura Actualizada por Feature (ejemplo) estrictamente obligada
```text
crates/todo_management/src/features/create_todo/
├── mod.rs
├── use_case.rs              # Punto de entrada principal - contiene la lógica de negocio
├── ports.rs                 # Interfaces para servicios externos (ej: TodoPersister)
├── error.rs                 # Errores específicos de la feature
├── dto.rs                   # Estructuras de datos de transferencia
├── use_case_test.rs         # Tests unitarios del caso de uso
├── event_handler.rs         # Manejador de eventos de dominio (si es necesario)
├── di.rs                    # Configuración del contenedor DI
├── mocks.rs                 # Mocks para tests
├── event_handler_test.rs    # Tests para handler de eventos
└── adapter.rs               # Tests de integración (único archivo)
```
- 1. Caso de Uso como Punto de Entrada (use_case.rs)
- 2. Puertos (Interfaces para repositorios o servicios externos con principios de segregación de interfaces SOLID) (ports.rs) 
- 3. Implementaciones de puertos (adapter.rs) 
- 4. Errores específicos de la feature (error.rs)
- 5. DTOs (dto.rs) Comandos, queries y DTOs específicos
- 6. Eventos de dominio (event_handler.rs) punto de entrada de eventos de dominio para el caso de uso.
- 7. Tests unitarios del caso de uso (use_case_test.rs)
- 8. Inyección de dependencias (di.rs) para inyectar todas las dependencias del caso de uso.
- 9. Mocks (mocks.rs) para mockear todas las dependencias del caso de uso.
- 10. Tests de eventos de dominio (event_handler_test.rs) para testear la integración con los servicios externos.
- 11. Disponibilizar la featur con mod.rs



### Resumen de Estructura de Directorios prototipo de obligado cumplimiento

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
│   │   │   │   ├── events.rs       # Eventos de dominio
│   │   │   │   └── mod.rs          # Exportaciones públicas del dominio
│   │   │   ├── features/           # Funcionalidades verticalmente aisladas
│   │   │   │   ├── create_todo/    # Feature: Crear tarea
│   │   │   │   │   ├── mod.rs      # Exportación del módulo feature
│   │   │   │   │   ├── use_case.rs # Lógica específica del caso de uso
│   │   │   │   │   ├── dto.rs      # Comandos, queries y DTOs específicos
│   │   │   │   │   ├── error.rs    # Errores personalizados de la feature
│   │   │   │   │   ├── ports.rs    # Interfaces SEGREGADAS de los servicios necesarios para esta feature
│   │   │   │   │   ├── adapter.rs  # Implementaciones CONCRETAS de los servicios definidos en ports.rs
│   │   │   │   │   ├── event_handler.rs  # Manejador de eventos de dominio
│   │   │   │   │   ├── di.rs       # Configuración del contenedor DI
│   │   │   │   │   ├── mocks.rs    # Mocks para tests
│   │   │   │   │   ├── use_case_test.rs  # Tests unitarios para caso de uso
│   │   │   │   │   └── event_handler_test.rs   # Tests para handler de eventos
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


# Directrices para Exponer Features vía REST API en el Proyecto Raíz

1. **En el directorio `src` del proyecto raíz**, crea controladores (handlers) para cada feature que necesite exponerse via REST.  
2. **Organiza los controladores por feature** en módulos bajo `src/api/` (ej: `src/api/todos/` para features de todos).  
3. **Cada handler debe inyectar el caso de uso** de la feature y transformar requests HTTP en comandos del caso de uso.  
4. **Configura las rutas en `src/main.rs`** usando Axum, importando los handlers desde los módulos de la API.  
5. **Mantén los controladores simples** solo con lógica de transporte HTTP, delegando lógica de negocio al caso de uso.  

**Convención para controladores:**  
- Usa una estructura de directorios como `src/api/{nombre_feature}/handlers.rs`  
- Nombra los handlers con el patrón `{accion}_{entidad}_handler` (ej: `create_todo_handler`)  
- Agrupa handlers relacionados en el mismo módulo para mantener la cohesión


## IMPORTANTE FEATURE VSA
- Se debe respetar en nombre del tipo de ficheros Clean Architecture dentro de cada feature.
- Si se detecta que una feature está teniendo más funcionalidades de las necesarias, se valora por principio SOLID de responsabilidad única la opción de crear otro feature que complete la funcionalidad aparte.
- **MUY IMPORTANTE** Respetar los nombres de los ficheros segun Clean architecture, nada de crear service.rs o cotroller.rs o handler.rs o cualquier otra cosa que imcumpla las especifiaciones anteriores.
- Siempre se crean los tests de use_case.rs con todo los mocks necesarios, segun se ha especificado.

## Flujo de Trabajo de ejemplo para Historia de Usuario

### 📋 Análisis de Requisitos (Historia x.x)
**Detalle de la historia:**

#### Actividades preparatorias
- Revisar arquitectura del sistema e implementación existente de features similares
- Definir estructura de directorios y componentes para la nueva feature
- Crear lista de tareas detalladas para la implementación

#### 🎯 Tareas de Implementación

| Estado | Tarea    | Descripción                       | Ubicación |
|--------|----------|-----------------------------------|-----------|
| ○ | Tarea 1  | Definir modelos de dominio        | `(nombre_crate)/src/domain/xxx.rs` |
| ○ | Tarea 2  | Crear estructura de directorios   | Feature-specific directory |
| ○ | Tarea 3  | Crear archivo mod.rs              | `mod.rs` para la feature |
| ○ | Tarea 4  | Definir abstracciones (puertos)   | `ports.rs` (segregación de interfaces SOLID) |
| ○ | Tarea 5  | Implementar adaptadores concretos | `adapter.rs` (SyftSbomGenerator, examplePartialRepository) |
| ○ | Tarea 6  | Desarrollar caso de uso           | `use_case.rs` |
| ○ | Tarea 7  | Crear DTOs                        | `dto.rs` (si es necesario) |
| ○ | Tarea 8  | Crear tests unitarios             | Para caso de uso y API |
| ○ | Tarea 9  | Crear tests de integración        | Con tests containers |
| ○ | Tarea 10 | Actualizar documentación          | Historia de usuario |

## Checklist de Verificación para el Agente AI

Al implementar cualquier feature, verificar:

- [ ] El código compila sin errores (`cargo check`)
- [ ] No hay warnings (`cargo clippy`)
- [ ] Todos los tests pasan (`cargo test` o `cargo nextest run`)
- [ ] El bounded context está en su propio crate
- [ ] La feature tiene todos los archivos requeridos
- [ ] Los ports están segregados y son específicos para la feature
- [ ] Las dependencias se inyectan via traits
- [ ] No hay acoplamiento directo con otros bounded contexts
- [ ] Los tests unitarios están implementados con mocks
- [ ] Se usa tracing para logging en lugar de println!
- [ ] Los nombres de archivos siguen el estándar Clean Architecture
- [ ] El shared kernel solo contiene elementos verdaderamente compartidos
- [ ] Los eventos de dominio se verifican en los tests

### Mensaje Final para el Agente AI

Al implementar, verifique siempre:

- ✅ Cada *bounded context* es un `crate` independiente.
- ✅ Use *shared kernel* solo para elementos verdaderamente compartidos.
- ✅ Cada *feature* tiene sus propios *ports* segregados.
- ✅ Los servicios transversales se inyectan a través de `traits`.
- ✅ Evite acoplamiento entre *bounded contexts*; copie entidades si es necesario.
- ✅ La estrategia de testing prioriza los tests unitarios rápidos y mockeados.
- ✅ El código compila sin errores y sin warnings.
- ✅ Todos los tests pasan.
- 🔍 Verificar que sigue la arquitectura VSA y Clean Architecture por feature
- 🔍 Verificar la segregación de interfaces
- 🔍 Verificar que no hay servicios monolíticos
- 🔍 Verificar la estructura de directorios y módulos
- 🔍 Verificar los tests implementados
- 🔍 Identificar posibles mejoras o ajustes necesarios
- 🔍 Actualizar documentación de la historia de usuario

## 5. Arquitectura y Stack Tecnológico
- Lenguaje y Runtime: Rust (última versión estable) con el runtime asíncrono Tokio.
- Framework Web: Axum.
- Base de Datos y Bus de Eventos: SurrealDB como la única fuente de verdad, aprovechando sus capacidades de base de datos de grafos, búsqueda de texto completo y eventos en tiempo real.
- Almacenamiento de Objetos: Se utilizará la crate object_store de Rust para la abstracción del almacenamiento, permitiendo el soporte de S3, Azure Blob Storage, Google Cloud Storage, etc.
- Motor de Autorización: Cedar, integrado de forma nativa en cada endpoint de la API para la aplicación de políticas.