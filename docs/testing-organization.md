# Guía de Organización de Tests (TEST-ORG1)

Versión: 1.2 (actualizada con estructura de tests unitarios en archivos separados)
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
2. `test-integration`: `cargo test --test 'it_*'` (tests de integración)
3. `test-e2e-playwright`: `cd e2e && npm test` (tests E2E con Playwright)
4. Verificación organización tests: ejecuta `bash scripts/verify-no-inline-tests.sh`

## 8. Referencias

- Estrategia original: sección 16 de [`plan.md`](docs/plan.md#L239)
- Integración testcontainers: [`test-containers.md`](docs/test-containers.md)
- Ejemplos de implementación: `crates/artifact/src/` y `crates/artifact/tests/`
