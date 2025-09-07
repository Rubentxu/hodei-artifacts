# Guía de Organización de Tests (TEST-ORG1)

Versión: 1.3 (actualizada con ejecución paralela de tests de integración)
Cambios 1.3: implementa sistema de generación de Docker Compose con templates para ejecución paralela de tests de integración
Cambios 1.2: implementa estructura de tests unitarios en archivos separados con sufijo _test.rs y mantiene tests E2E con Playwright

Objetivo: Establecer una convención estricta, reproducible y automatizable para la localización, tipo y nivel de abstracción de las pruebas en el monorepo. Garantiza:
- Aislamiento (evitar acoplar tests a detalles internos).
- Señal temprana (rápida detección de regresiones).
- Escalabilidad (añadir bounded contexts y slices sin reabrir decisiones).
- Simplicidad de CI (jobs diferenciados sin lógica ad-hoc por crate).

---

## 1. Estructura de Archivos para Tests

### 1.1 Tests Unitarios (archivos separados en src/)
```
src/
├── lib.rs
├── main.rs
├── modules/
│   ├── mod.rs
│   ├── calculator.rs          # Código fuente
│   ├── calculator_test.rs     # Tests unitarios (mismo directorio)
│   ├── repository.rs          # Código fuente  
│   └── repository_test.rs     # Tests unitarios (mismo directorio)
```

### 1.2 Tests de Integración (directorio tests/)
```
crates/<context>/
  tests/
    it_calculator.rs           # Tests de integración
    it_repository.rs
```

### 1.2.1 Tests de Integración con `testcontainers` y Docker Compose

Para los tests de integración que requieren un entorno de servicios completo (MongoDB, RabbitMQ, S3, etc.), utilizamos la librería `testcontainers` junto con templates de Docker Compose. Este enfoque nos permite:

- **Entornos reproducibles:** Definidos en `tests/compose/docker-compose.yml`.
- **Orquestación centralizada:** El módulo `shared-test` gestiona el ciclo de vida del entorno (levantar, esperar por salud de servicios, tumbar).
- **Aislamiento y ejecución paralela:** `testcontainers` genera configuraciones Docker Compose únicas para cada test, con redes, subredes y puertos dinámicos que evitan conflictos. Esto permite la ejecución paralela segura de tests de integración.

**Uso:** Los tests de integración interactúan con una estructura `TestEnvironment` que provee clientes preconfigurados para los servicios.

### 1.2.2 Ejecución Paralela de Tests con Template Docker Compose

Para habilitar la ejecución paralela de tests de integración, se ha implementado un sistema de generación dinámica de archivos Docker Compose:

**Template Docker Compose:** `tests/compose/docker-compose.template.yml`
```yaml
services:
  mongodb:
    ports:
      - "{{MONGO_HOST_PORT}}:27017"
  rabbitmq:
    ports:
      - "{{RABBITMQ_HOST_PORT}}:5672"
  localstack:
    ports:
      - "{{S3_HOST_PORT}}:4566"
networks:
  {{NETWORK_NAME}}:
    ipam:
      config:
        - subnet: {{SUBNET}}
```

**Placeholders disponibles:**
- `{{NETWORK_NAME}}`: Nombre único de red Docker
- `{{SUBNET}}`: Subred única para evitar conflictos de IP
- `{{MONGO_HOST_PORT}}`: Puerto dinámico para MongoDB
- `{{RABBITMQ_HOST_PORT}}`: Puerto dinámico para RabbitMQ
- `{{S3_HOST_PORT}}`: Puerto dinámico para LocalStack/S3

**Detección de recursos:** El sistema detecta automáticamente los recursos disponibles (CPU, memoria) para optimizar el límite de ejecución paralela.

**Uso en tests:**
```rust
use shared_test::setup_test_environment;

#[tokio::test]
async fn test_my_feature() {
    let env = setup_test_environment(None).await;
    // Test logic using isolated environment
}
```

### 1.3 Tests E2E con Playwright (directorio separado)
```
e2e/
  tests/
  setup/
  package.json
  playwright.config.ts
```

## 2. Implementación de Tests Unitarios en Archivos Separados

**src/modules/calculator.rs:**
```rust
pub struct Calculator;

impl Calculator {
    pub fn add(a: i32, b: i32) -> i32 {
        a + b
    }

    pub fn subtract(a: i32, b: i32) -> i32 {
        a - b
    }

    // Función interna para testing
    pub(crate) fn internal_helper() -> i32 {
        42
    }
}
```

**src/modules/calculator_test.rs:**
```rust
// Importar el módulo principal
use super::calculator::Calculator;

#[test]
fn test_add() {
    assert_eq!(Calculator::add(2, 2), 4);
    assert_eq!(Calculator::add(-1, 1), 0);
}

#[test]
fn test_subtract() {
    assert_eq!(Calculator::subtract(5, 3), 2);
    assert_eq!(Calculator::subtract(10, 15), -5);
}

#[test]
fn test_internal_helper() {
    // Acceso a funciones internas del mismo módulo
    assert_eq!(Calculator::internal_helper(), 42);
}
```

**src/modules/mod.rs:**
```rust
pub mod calculator;

// Incluir tests solo durante compilación de tests
#[cfg(test)]
mod calculator_test;
```

## 3. Tests E2E con Playwright

### 3.1 Estructura del Directorio E2E
```
e2e/
├── package.json                    # Dependencias Node.js y scripts
├── playwright.config.ts           # Configuración Playwright
├── setup/
│   ├── global-setup.ts           # Setup global (verificar servicios)
│   └── global-teardown.ts        # Cleanup global
└── tests/
    └── *.spec.ts                 # Tests E2E por funcionalidad
```

### 3.2 Configuración y Ejecución
- **Instalación**: `cd e2e && npm install`
- **Ejecución**: `npm test` (todos los tests) o `npm run test:headed` (con UI)
- **Debug**: `npm run debug` para modo debug interactivo
- **Reportes**: `npm run report` para abrir reporte HTML

### 3.3 Características Implementadas
- **Multi-navegador**: Chrome, Firefox, Safari, Edge
- **Dispositivos móviles**: Pixel 5, iPhone 12 Pro
- **Paralelización**: Tests ejecutados en paralelo por defecto
- **Retry automático**: 2 reintentos en CI, 0 en local
- **Screenshots**: Captura automática en fallos
- **Videos**: Grabación en fallos para debugging
- **Integración servidor**: Setup automático del servidor Rust

## 4. Ejecución de Tests

**Tests unitarios**:
```bash
cargo test --lib
```

**Tests de integración**:
```bash
cargo test --test 'it_*'
```

**Tests E2E Playwright**:
```bash
cd e2e && npm test
```

## 5. Política "Cero Inline Tests" Actualizada

**Tests unitarios permitidos en archivos separados** en el mismo directorio:
```rust
#[cfg(test)]
mod calculator_test;  // Importa src/modules/calculator_test.rs
```

**Prohibido**:
- Módulos `#[cfg(test)]` inline en archivos de producción
- Tests dentro de funciones en `src/**`

**Enforcement**: Script `verify-no-inline-tests.sh` verifica:
- Ausencia de módulos `#[cfg(test)]` inline en `src/**`
- Uso correcto de archivos separados para tests

## 6. Ventajas de Este Enfoque

1. **Organización clara**: Tests unitarios junto al código que prueban
2. **Acceso a internals**: Pueden acceder a funciones `pub(crate)` del mismo módulo
3. **Mantenimiento simplificado**: Cambios en código y tests van juntos
4. **Compatibilidad con convenciones Rust**: Usa el sistema de módulos estándar
5. **Separación limpia**: Tests E2E completamente separados con Playwright

## 7. CI / Pipeline

Jobs planificados:
1. `test-unit`: `cargo test --lib` (tests unitarios)
2. `test-integration`: `cargo test --test 'it_*'` (tests de integración en paralelo)
3. `test-e2e-playwright`: `cd e2e && npm test` (tests E2E con Playwright)
4. Verificación organización tests: ejecuta `bash scripts/verify-no-inline-tests.sh`

**Ejecución paralela optimizada:** Los tests de integración se ejecutan automáticamente en paralelo utilizando el número óptimo de workers basado en los recursos del sistema:
```bash
# Ejecutar tests con paralelismo automático
cargo test --test 'it_*'

# Forzar número específico de tests paralelos
TEST_PARALLEL_JOBS=4 cargo test --test 'it_*'
```

## 8. Referencias

- Estrategia original: sección 16 de [`plan.md`](docs/plan.md#L239)

- Template Docker Compose: `tests/compose/docker-compose.template.yml`
- Sistema de detección de recursos: `crates/shared-test/src/resource_detector.rs`
- Generador de compose dinámico: `crates/shared-test/src/dynamic_compose.rs`
- Renderizador de templates: `crates/shared-test/src/template_renderer.rs`
- Ejemplos de implementación: `crates/artifact/src/` y `crates/artifact/tests/`

## IMPORTANTE: Testing
- facilitar el testing rapido.
- no recompilar el código que no se toca.
- no usar println para logear usar el crate tracing.
-  buscar una solución en el testing para que el crate de tracing se use para hacer recuperar y crear asserts que comprueben logs y spans.
- antes de centrarse en los test de integración, hay que hacer test amplios unitarios sobre los casos de uso use_case.rs y el uso del api endpoint apir.rs, mockeando todos los servicios necesarios.
- aprovechar el crate de tracing para crear asserts en los tests que comprueben logs, span etc
- también testear los eventos producidos en las features.
- usar los scripts de makefile para ejecutar los tests