# 📊 Plan de Cobertura de Tests - Hodei Artifacts

## 🎯 Objetivo

Alcanzar **80%+ de cobertura de código** en todos los crates del proyecto, asegurando que todas las funcionalidades críticas estén probadas con tests unitarios e integración.

## 📈 Estado Actual de Cobertura

### ✅ Kernel (95%+ cobertura)
**Archivos de test:** 6 archivos, 200+ tests
- `hrn_test.rs`: 47 tests exhaustivos para HRN parsing/validation
- `attributes.rs`: 50+ tests para AttributeValue (serialización, validación, tipos complejos)
- `entity.rs`: 30+ tests para traits HodeiEntity, ActionTrait, PolicyStorage
- `value_objects.rs`: 50+ tests para ServiceName, ResourceTypeName, AttributeName
- `surrealdb_integration_test.rs`: 6 tests de integración SurrealDB
- `in_memory_event_bus.rs`: 5 tests unitarios para event bus

**Funcionalidades probadas:**
- ✅ HRN parsing, validation, serialization
- ✅ AttributeValue con todos los tipos (Bool, Long, String, Set, Record, EntityRef)
- ✅ Value Objects con validación completa
- ✅ Traits de dominio (HodeiEntity, ActionTrait)
- ✅ Event bus in-memory
- ✅ PolicyStorage con errores
- ✅ Integración SurrealDB básica

### 🟡 Policies (20% cobertura)
**Archivos de test:** 1 archivo, 4 tests
- `smoke_test.rs`: Tests básicos de compilación

**Funcionalidades NO probadas:**
- ❌ Domain models de políticas
- ❌ Application services
- ❌ Infrastructure adapters
- ❌ Parsing de políticas Cedar
- ❌ Validación de políticas

### 🟡 Hodei-IAM (75% cobertura)
**Archivos de test:** 6 archivos, 182 tests
- Tests unitarios completos para features principales
- Cobertura de casos de error
- Tests de integración

**Funcionalidades probadas:**
- ✅ create_user, create_group, add_user_to_group
- ✅ Manejo de errores específicos
- ✅ Validación de HRN
- ✅ Transacciones

### 🟡 Hodei-Organizations (40% cobertura)
**Archivos de test:** 2 archivos, ~20 tests
- `integration_create_account_test.rs`
- `smoke_test.rs`

**Funcionalidades NO probadas:**
- ❌ Domain entities (Account, OrganizationalUnit, ServiceControlPolicy)
- ❌ Repositories (AccountRepository, OuRepository, ScpRepository)
- ❌ Use cases (GetEffectiveScpsUseCase)
- ❌ Infrastructure adapters

### 🟡 Hodei-Authorizer (60% cobertura)
**Archivos de test:** 2 archivos, 11+ tests
- `organization_boundary_provider_test.rs`: 11 tests unitarios exhaustivos
- `smoke_test.rs`

**Funcionalidades probadas:**
- ✅ OrganizationBoundaryProvider con jerarquías complejas
- ✅ Detección de ciclos
- ✅ Manejo de SCPs faltantes

**Funcionalidades NO probadas:**
- ❌ Domain models de autorización
- ❌ Application services
- ❌ Otros adapters

### 🟡 Root (src/) (10% cobertura)
**Archivos de test:** 0 archivos
- Sin tests

**Funcionalidades NO probadas:**
- ❌ Main application setup
- ❌ HTTP handlers
- ❌ Dependency injection
- ❌ Application startup

## 🎯 Plan de Mejora - Ruta a 80%+ Cobertura

### Fase 1: Policies (2-3 días)

#### Tareas:
1. **Crear tests de dominio** (`policies/src/shared/domain/`)
   ```rust
   // policy.rs tests
   - Policy parsing (Cedar DSL)
   - Policy validation
   - Policy serialization
   - Policy equality/comparison
   ```

2. **Crear tests de aplicación** (`policies/src/shared/application/`)
   ```rust
   // services.rs tests
   - Policy evaluation logic
   - Policy conflict resolution
   - Policy inheritance
   ```

3. **Crear tests de infraestructura** (`policies/src/shared/infrastructure/`)
   ```rust
   // adapters.rs tests
   - Cedar policy engine integration
   - Policy storage adapters
   - Policy caching
   ```

#### Objetivo: 80% cobertura en policies

### Fase 2: Hodei-Organizations (3-4 días)

#### Tareas:
1. **Tests de dominio** (Account, OrganizationalUnit, ServiceControlPolicy)
   ```rust
   - Entity creation/validation
   - SCP attachment/detachment
   - Hierarchy traversal
   - Serialization
   ```

2. **Tests de repositorios** (SurrealDB adapters)
   ```rust
   - CRUD operations
   - Query methods
   - Error handling
   - Connection management
   ```

3. **Tests de use cases** (GetEffectiveScpsUseCase)
   ```rust
   - Business logic
   - Error scenarios
   - Edge cases
   ```

4. **Tests de integración**
   ```rust
   - Full hierarchy creation
   - SCP resolution end-to-end
   - Performance tests
   ```

#### Objetivo: 80% cobertura en hodei-organizations

### Fase 3: Hodei-Authorizer (2-3 días)

#### Tareas:
1. **Completar tests de dominio**
   ```rust
   - Authorization models
   - Policy evaluation
   - Principal/Resource types
   ```

2. **Tests de aplicación**
   ```rust
   - Authorization services
   - Policy engines
   - Decision making
   ```

3. **Tests de infraestructura**
   ```rust
   - Additional adapters
   - Caching layers
   - External integrations
   ```

#### Objetivo: 80% cobertura en hodei-authorizer

### Fase 4: Root Application (1-2 días)

#### Tareas:
1. **Tests de integración** (`tests/`)
   ```rust
   - Application startup
   - HTTP endpoints
   - Dependency injection
   - Configuration loading
   ```

2. **Tests de handlers HTTP**
   ```rust
   - Request/response mapping
   - Error handling
   - Authentication middleware
   ```

#### Objetivo: 70% cobertura en aplicación principal

### Fase 5: Configuración de Herramientas (1 día)

#### Tareas:
1. **Configurar cargo-tarpaulin**
   ```bash
   cargo install cargo-tarpaulin
   cargo tarpaulin --workspace --out Html
   ```

2. **Configurar CI con cobertura**
   ```yaml
   # .github/workflows/coverage.yml
   - name: Generate coverage
     run: cargo tarpaulin --workspace --out Lcov
   ```

3. **Configurar badges**
   ```markdown
   ![Coverage](https://img.shields.io/badge/coverage-85%25-brightgreen)
   ```

## 📊 Métricas de Éxito

### Cobertura por Crate (Objetivo)
- **kernel**: 95%+ ✅ (ya alcanzado)
- **policies**: 80%+ (de 20%)
- **hodei-iam**: 80%+ ✅ (ya alcanzado)
- **hodei-organizations**: 80%+ (de 40%)
- **hodei-authorizer**: 80%+ (de 60%)
- **root**: 70%+ (de 10%)

### Tipos de Tests
- **Unitarios**: 70%+ de tests
- **Integración**: 20%+ de tests
- **E2E**: 10%+ de tests

### Calidad de Tests
- ✅ Tests independientes (no dependen de orden)
- ✅ Tests rápidos (< 100ms cada uno)
- ✅ Cobertura de edge cases
- ✅ Tests de error scenarios
- ✅ Mocks apropiados

## 🚀 Implementación Priorizada

### Semana 1: Policies (Día 1-3)
```bash
# Crear estructura de tests
mkdir -p crates/policies/src/shared/domain/tests
mkdir -p crates/policies/src/shared/application/tests
mkdir -p crates/policies/src/shared/infrastructure/tests

# Implementar tests de dominio primero
# Policy parsing, validation, etc.
```

### Semana 2: Hodei-Organizations (Día 4-7)
```bash
# Tests de entidades de dominio
# Tests de repositorios
# Tests de use cases
# Tests de integración
```

### Semana 3: Hodei-Authorizer + Root (Día 8-10)
```bash
# Completar authorizer
# Tests de aplicación principal
# Configurar herramientas de cobertura
```

## 🛠 Herramientas Recomendadas

### Medición de Cobertura
```bash
# Instalar tarpaulin
cargo install cargo-tarpaulin

# Generar reporte HTML
cargo tarpaulin --workspace --out Html

# Generar LCOV para CI
cargo tarpaulin --workspace --out Lcov
```

### Configuración de CI
```yaml
# .github/workflows/coverage.yml
name: Coverage
on: [push, pull_request]

jobs:
  coverage:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: dtolnay/rust-toolchain@stable
      - name: Install tarpaulin
        run: cargo install cargo-tarpaulin
      - name: Generate coverage
        run: cargo tarpaulin --workspace --out Lcov --output-dir coverage/
      - name: Upload coverage
        uses: codecov/codecov-action@v3
        with:
          file: coverage/lcov.info
```

## 📋 Checklist de Verificación

### Por Crate
- [ ] **kernel**: 95%+ cobertura ✅
- [ ] **policies**: 80%+ cobertura
- [ ] **hodei-iam**: 80%+ cobertura ✅
- [ ] **hodei-organizations**: 80%+ cobertura
- [ ] **hodei-authorizer**: 80%+ cobertura
- [ ] **root**: 70%+ cobertura

### Calidad General
- [ ] Tests pasan en CI
- [ ] Cobertura reportada automáticamente
- [ ] Tests rápidos (< 30s total)
- [ ] Tests independientes del orden
- [ ] Cobertura de mutaciones (opcional)

## 🎯 Resultado Final Esperado

```
Coverage Report
===============

Overall coverage: 85%

Files:
  kernel: 95%
  policies: 82%
  hodei-iam: 88%
  hodei-organizations: 83%
  hodei-authorizer: 81%
  root: 72%
```

---

**Fecha de Creación**: Diciembre 2024  
**Próxima Revisión**: Después de completar Fase 1  
**Responsable**: AI Assistant  
**Estado**: Planificado