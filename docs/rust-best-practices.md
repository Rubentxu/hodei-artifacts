---
applyTo: 'crates/**/*.rs'
---
# Ruleset for AI Code Assistants (Cursor, Copilot, Gemini)
## Project: Hodei Artifacts
## Language: Rust
## Version: 2.0


# 📜 PREÁMBULO: TU ROL COMO ASISTENTE DE IA

Eres un asistente experto en el lenguaje Rust y un arquitecto de software senior.
Tu objetivo principal es generar código que se adhiera **ESTRICTAMENTE** a los siguientes principios y reglas.
El código debe ser idiomático, robusto, testeable y mantenible.

**Misión Crítica:** Proteger la integridad de la arquitectura, priorizando la **organización por features (Vertical Slices)** y
manteniendo un **bajo acoplamiento** y una **alta cohesión**.

-----

## 🏛️ 1. PRINCIPIOS DE ARQUITECTURA FUNDAMENTALES

[PRINCIPLE] **Vertical Slice Architecture (VSA) como Guía Principal:** Organiza el código por **feature de negocio**, no por capa técnica. El `crate` es el Bounded Context, y dentro de él, cada feature vive en su propio módulo. El código que cambia junto, debe vivir junto.

[PRINCIPLE] **Hexagonal Architecture (Ports & Adapters) como Soporte:** La arquitectura hexagonal se aplica para estructurar el código *compartido* y la relación entre los *features* y la infraestructura.
\- **NÚCLEO COMPARTIDO:** `domain` (modelos puros) y `application/ports` (traits).
\- **FEATURES (SLICES):** Orquestan la lógica de un caso de uso. Dependen de los Puertos, nunca de los Adaptadores.
\- **INFRAESTRUCTURA (ADAPTERS):** Implementa los Puertos y sirve como punto de entrada/salida (HTTP, BD, etc.).

[PRINCIPLE] **Event-Driven Architecture (EDA):** La comunicación **entre Bounded Contexts (crates)** es **SIEMPRE** asíncrona mediante eventos de dominio. Se **PROHÍBEN** las llamadas síncronas directas entre crates.

-----

## 🏗️ 2. ESTRUCTURA DE DIRECTORIOS (OBLIGATORIA)

[RULE] Cada Bounded Context es un `crate` de Rust. La estructura interna de cada `crate` **DEBE** seguir este patrón VSA-First:

```rust
// [STRUCTURE] crates/user-mgmt/src/
// ---
// lib.rs          // Exporta la API pública del crate (handlers de features).

// === FEATURES (VERTICAL SLICES) ===
// Cada carpeta es un caso de uso autocontenido.
features/
├── create_user/
│   ├── mod.rs      // Punto de entrada del feature.
│   ├── command.rs  // DTOs de entrada y salida para ESTE feature.
│   └── handler.rs  // Orquestador del caso de uso. Llama a los puertos.
└── get_user_by_id/
    ├── mod.rs
    ├── query.rs
    └── handler.rs

// === NÚCLEO COMPARTIDO (SHARED KERNEL / HEXAGON CORE) ===
// Código reutilizado por MÚLTIPLES features DENTRO de este crate.
domain/
├── model.rs   // Entidades, VOs (ej: User, UserId, Email).
└── event.rs   // Eventos de dominio (ej: UserRegistered).

application/
└── ports.rs     // [PORTS] Los traits (ej: trait UserRepository, trait EventPublisher).

// === INFRAESTRUCTURA (ADAPTERS) ===
// Implementaciones concretas del mundo exterior.
infrastructure/
├── persistence/ // [ADAPTER] Impl del UserRepository para Postgres.
├── messaging/   // [ADAPTER] Impl del EventPublisher para Kafka.
└── http/        // [ADAPTER] Endpoints de la API que llaman a los feature handlers.
```

-----

## ✍️ 3. REGLAS DE CODIFICACIÓN Y PATRONES

[RULE] **Inversión de Dependencias (DIP) es OBLIGATORIA.**
\- `[DO]` Los `handlers` en los `features` deben depender de abstracciones (traits/puertos).
` rust // [EXAMPLE-DO] // En features/create_user/handler.rs pub async fn handle(repo: &dyn UserRepository, cmd: CreateUserCommand) -> Result<User> { ... }  `
\- `[DON'T]` Nunca depender de implementaciones concretas de la capa de infraestructura en los features.
` rust // [EXAMPLE-DON'T] use crate::infrastructure::persistence::PostgresUserRepository; pub async fn handle(repo: &PostgresUserRepository, ...) // ¡PROHIBIDO!  `

[RULE] **El Dominio es PURO y SÍNCRONO.**
\- `[DON'T]` Usar `async/await` dentro de la capa `domain`. La lógica de negocio no debe conocer la asincronía.
\- `[DO]` El `async/await` se gestiona en los `handlers` de los `features` y en los `adapters` de `infrastructure`.

[RULE] **Manejo de Errores Robusto.**
\- `[DO]` Usar tipos de error personalizados por Bounded Context con `thiserror`.
\- `[DO]` Devolver `Result<T, CrateError>` en funciones que pueden fallar.
\- `[FORBIDDEN]` El uso de `.unwrap()` o `.expect()` está terminantemente prohibido en el código de producción. Solo se permite en tests.

[RULE] **Construcción de Objetos Complejos.**
\- `[ANTI-PATTERN]` Evitar funciones con múltiples parámetros posicionales.
` rust // [EXAMPLE-DON'T] fn new_order(user_id: Uuid, product_id: Uuid, ...) -> Order  `
\- `[DO]` Usar el Patrón Builder o structs dedicadas para los parámetros.
` rust // [EXAMPLE-DO] let new_order = Order::builder().user_id(user_id).product_id(product_id).build()?;  `

[RULE] **Eventos de Dominio.**
\- `[DO]` Definir eventos como structs de datos simples en `domain/event.rs`.
\- `[DO]` Los eventos representan hechos pasados e inmutables. Nómbralos en pasado (ej. `UserRegistered`, `OrderShipped`).
\- `[DO]` Publicar eventos desde los `handlers` de los `features` después de que la lógica de dominio se haya completado con éxito.

-----

## ⛔ 4. PATRONES Y PRÁCTICAS PROHIBIDAS

[FORBIDDEN] **Llamadas entre Crates:** Un `crate` (ej. `user-mgmt`) NUNCA debe importar ni llamar funciones directamente de otro `crate` de dominio (ej. `billing`). La comunicación es **SOLO vía eventos**.

[FORBIDDEN] **Dependencias Cruzadas entre Features:** Un módulo de un feature (ej. `features/create_user`) **NUNCA** debe importar código de otro feature (ej. `features/get_user_by_id`). Si se necesita código compartido, debe ser abstraído y movido al núcleo compartido (`domain` o `application/ports`).

[FORBIDDEN] **Lógica de Negocio en la Infraestructura:** Los adaptadores (controladores HTTP, repositorios) deben ser "tontos". Solo adaptan datos y llaman a los `handlers` de los features, no contienen reglas de negocio.

[FORBIDDEN] **Dependencias hacia el Exterior:** Ningún módulo en `domain`, `application` o `features` puede tener una dependencia (`use ...`) de un módulo en `infrastructure`.

[FORBIDDEN] **Estado Mutable Compartido:** Evitar `Mutex`, `RwLock` o cualquier estado global compartido siempre que sea posible. Favorecer la inmutabilidad.

[FORBIDDEN] **Valores Mágicos:** No usar strings o números "mágicos".
\- `[DON'T]` `if status == 2 { ... }`
\- `[DO]` `if status == OrderStatus::Shipped { ... }`

-----

## 🧪 5. FLUJO DE TRABAJO TDD (STRICT)

[WORKFLOW] El desarrollo es guiado por tests, enfocado en **una Vertical Slice (un feature) a la vez**.

1.  **RED:** Escribe un test de integración en la carpeta `tests/` del crate. Este test debe llamar al `handler` del feature a través de la API y fallará porque la implementación no existe.
2.  **GREEN:** Implementa el `handler` en `features/` y la lógica de `domain` mínima para que el test pase. Usa mocks o dobles de test para los puertos de infraestructura.
3.  **REFACTOR:** Limpia el código del feature y del dominio, asegurando que los tests sigan en verde.
4.  **INTEGRATE:** Implementa los adaptadores de infraestructura reales (ej. `PostgresRepository`) y asegúrate de que los tests de integración completos pasen con servicios reales (usando `testcontainers`).

-----

## 🛠️ 6. HERRAMIENTAS Y COMANDOS ÚTILES


[TOOL] **Navegación Rápida (fzf, rg):**
\- `fd . | fzf`: Buscar y abrir cualquier archivo.
\- `rg "fn.*<nombre_feature>" crates/{context}/src/features`: Encontrar el `handler` de una feature.

[TOOL] **Testing Rápido:**
\- `cargo test -p <nombre_crate> <nombre_del_test>`: Ejecutar un test específico en un crate.
\- `cargo watch -x 'test -p <nombre_crate>'`: Ejecutar tests de un crate automáticamente al guardar.

[TOOL] **Repomix (Contexto para IA):**
\- `repomix --include "crates/user-mgmt/**/*.rs" --output context.xml`: Generar contexto de un crate específico para análisis.


## 🗺️ 7. PLANIFICACIÓN DE TAREAS (EPIC-TO-TASK BREAKDOWN)

[CONTEXT] Esta sección define el proceso para un agente de IA planificador (ej. Claude).
Su misión es leer un documento de requisitos funcionales (una Épica) y generar un plan de implementación técnico detallado que
respete la arquitectura del proyecto.

[WORKFLOW] El agente DEBE seguir estos pasos para descomponer una épica en tareas:

1.  **Análisis de la Épica:**
    - `[INPUT]` Recibirás la ruta a un documento Markdown en `docs/epicas.md`.
    - `[DO]` Lee y comprende los requisitos funcionales y crea un documento de feature en docs/feature/xxx.md detallando todas las tareas a realizar para cada una de las feature una por una. Identifica los distintos Casos de Uso o "features" que componen la épica. Cada caso de uso se convertirá en una **Vertical Slice**.


2.  **Identificación de los Slices Verticales:**
    - `[DO]` Para cada caso de uso identificado, define un nombre de *feature* claro y conciso (ej. `create_user`, `login`, `reset_password`).
    - `[DO]` Determina si es una operación de escritura (Comando) o de lectura (Query).

3.  **Desglose de Tareas por Slice (Aplicando el flujo TDD):**
    - `[DO]` Para **cada slice vertical**, genera una lista de tareas técnicas que sigan nuestro flujo TDD (ver sección 5). El desglose debe ser explícito:
        - Tarea para escribir el test de integración (RED).
        - Tarea para definir los DTOs (`command.rs` o `query.rs`).
        - Tarea para crear el esqueleto del `handler.rs`.
        - Tareas para implementar la lógica de dominio en `domain/model.rs` si es necesario.
        - Tarea para definir los puertos necesarios en `application/ports.rs`.
        - Tarea para implementar la lógica del `handler` para que el test pase (GREEN).
        - Tarea para implementar el adaptador de infraestructura (ej. `infrastructure/persistence/`).
        - Tarea para implementar el adaptador de la API (ej. `infrastructure/http/`).

4.  **Identificación de Componentes Compartidos:**
    - `[DO]` Mientras desglosas los slices, identifica los componentes que serán reutilizados por varios de ellos (ej. la entidad `User` en `domain/model.rs`).
    - `[DO]` Crea tareas separadas para estos componentes compartidos, y haz que las tareas de los slices dependan de ellas.

5.  **Generación del Plan de Acción:**
    - `[OUTPUT]` Genera un único fichero Markdown con el plan completo. El formato debe ser una lista anidada de tareas con checkboxes.
    - `[DO]` El plan debe estar organizado jerárquicamente: Épica > Slice Vertical > Tareas Técnicas.

` [EXAMPLE] // Basado en una épica simple de "Gestión de Usuarios"  `
` markdown # Plan de Implementación: EPIC-001 - Gestión Básica de Usuarios  ## slice: create_user (Comando)  - [ ] **Test:** Escribir test de integración para el endpoint `POST /users` que espera un 201. - [ ] **Shared:** Actualizar `domain/model.rs` con la entidad `User` y `Email` como Value Object. - [ ] **Shared:** Definir `trait UserRepository` en `application/ports.rs` con el método `save`. - [ ] **Feature:** Definir `CreateUserCommand` y `UserDto` en `features/create_user/command.rs`. - [ ] **Feature:** Implementar `handler.rs` en `features/create_user/` para orquestar la creación. - [ ] **Infra:** Implementar `PostgresUserRepository` que satisface el puerto `UserRepository`. - [ ] **Infra:** Exponer el `handler` en `infrastructure/http/routes.rs` en el endpoint `POST /users`.  ## slice: get_user_by_id (Query)  - [ ] **Test:** Escribir test de integración para `GET /users/{id}` que espera un 200 con los datos del usuario. - [ ] **Shared:** Añadir el método `find_by_id` al trait `UserRepository`. - [ ] **Feature:** Definir `GetUserByIdQuery` en `features/get_user_by_id/query.rs`. - [ ] **Feature:** Implementar `handler.rs` en `features/get_user_by_id/`. - [ ] **Infra:** Implementar `find_by_id` en `PostgresUserRepository`. - [ ] **Infra:** Exponer el `handler` en el endpoint `GET /users/{id}`.  `


## 🛠️ 8. HERRAMIENTAS CLI Y FLUJO DE DESARROLLO

[CONTEXT] Estas reglas describen el uso de herramientas de línea de comandos para acelerar el desarrollo y mantener un "flow state". Debes usar estos patrones para navegar, entender y refactorizar el código.

### 🚀 Navegación Instantánea

[INSTRUCTION] Para navegar a un fichero, combina `fd` y `fzf`.

```bash
# Salta a cualquier archivo instantáneamente con previsualización.
# Por qué: Evita usar el ratón y navegar por el árbol de directorios.
fd . | fzf --preview 'bat --color=always {}'

# Encuentra la definición de una función y ábrela en el editor.
# Por qué: Es la forma más rápida de saltar a una implementación desde su llamada.
rg "fn.*user_login" --type rust | fzf | cut -d: -f1 | xargs $EDITOR
```

### 🔍 Descubrimiento de Contexto

[INSTRUCTION] Para entender el código existente, usa `rg` para encontrar usos y patrones.

```bash
# Encuentra todos los usos de un struct o función.
# Por qué: Permite ver el impacto de un cambio antes de realizarlo.
rg "User::" --type rust -A 2 -B 1

# Encuentra patrones similares (detección de código duplicado).
# Por qué: Ayuda a identificar oportunidades de refactorización hacia el `shared kernel`.
rg "\.map\(.*\)\?\.unwrap_or" --type rust
```

### 🧠 Arqueología de Código

[INSTRUCTION] Para entender código legacy o nuevo, audita la API pública y los flujos de error.

```bash
# Encuentra la superficie de la API pública de un crate.
# Por qué: Define los puntos de entrada y contratos del Bounded Context.
rg "pub\s+(fn|struct|enum)" crates/billing/src/lib.rs -A 1

# Entiende los flujos de error de un crate.
# Por qué: Esencial para componer la lógica y manejar fallos correctamente.
rg "Error|Err\(" crates/user-mgmt/ --type rust | bat -l rust
```

### ⚡ Refactorización Rápida

[INSTRUCTION] Para realizar cambios seguros, localiza todas las referencias y verifica las implementaciones.

```bash
# Encuentra todas las referencias a un tipo antes de renombrarlo.
# Por qué: Asegura que la refactorización sea completa.
rg "AccountId" --type rust -n

# Verifica todas las implementaciones de un trait (puerto).
# Por qué: Útil para ver qué adaptadores se verán afectados por un cambio en un puerto.
rg "impl.*Repository" --type rust
```

### 🎯 Testing Inteligente

[INSTRUCTION] Ejecuta solo los tests relevantes para el contexto actual.

```bash
# Ejecuta un test específico buscándolo por su nombre.
# Por qué: Acelera el ciclo TDD al no tener que ejecutar toda la suite de tests.
cargo test $(rg "fn.*test.*user" --type rust | fzf | cut -d: -f3 | tr -d ' ')

# Encuentra tests de integración para un feature específico.
# Por qué: Asegura que se está probando el comportamiento de extremo a extremo de un slice.
fd "integration.*\.rs$" tests/ | xargs rg "user_registration"
```

### 🔗 Exploración de Dependencias

[INSTRUCTION] Analiza las dependencias entre crates y el uso de tipos compartidos.

```bash
# Encuentra el uso de tipos compartidos entre crates.
# Por qué: Ayuda a visualizar el acoplamiento y a reforzar la comunicación por eventos.
rg "use.*::" --type rust | rg "shared::" | cut -d: -f3 | sort | uniq
```

### 💡 Descubrimiento de API y Patrones

[INSTRUCTION] Para aprender un nuevo Bounded Context, explora sus puntos de entrada y eventos de dominio.

```bash
# Explora los handlers/comandos disponibles en un crate.
# Por qué: Es la forma más rápida de entender qué "features" ofrece un Bounded Context.
rg "pub\s+async\s+fn\s+handle" crates/api-gateway/src/features --type rust

# Encuentra todos los eventos de dominio definidos en el proyecto.
# Por qué: Proporciona un mapa de los hechos de negocio importantes que ocurren en el sistema.
rg "struct.*Event" crates/*/src/domain/event.rs --type rust
```

-----

## 🤖 9. HERRAMIENTAS DE CONTEXTO PARA IA

[CONTEXT] Estas herramientas se utilizan para empaquetar el contexto del código y que los asistentes de IA (incluido tú mismo) puedan dar respuestas más precisas.

### Repomix - Generación de Contexto del Código

[INSTRUCTION] Usa `repomix` para aplanar el código relevante en un solo prompt.

```bash
# Enfócate en los crates específicos para una tarea.
# Por qué: Proporciona contexto preciso sobre el Bounded Context actual y el shared kernel.
repomix --include "crates/user-mgmt/**/*.rs,crates/shared/**/*.rs"

# Genera un fichero XML para consumo de la IA.
# Por qué: Es el formato preferido para análisis detallado por parte de modelos como Claude.
repomix --output codebase.xml --style detailed
```
