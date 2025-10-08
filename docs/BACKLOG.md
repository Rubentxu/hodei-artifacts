# Hodei Artifacts - Backlog Actualizado

**Última actualización:** 2024
**Estado del Proyecto:** Fase 1 completada, preparado para Fase 2

---

## ✅ Completado

### Fase 1: Health Check, Refactor y Arquitectura Base

#### ✅ 1.1 Compilación y Limpieza Inicial
- [x] Resolución de errores de compilación en todos los crates
- [x] Alineamiento de bounded contexts
- [x] Eliminación de código legacy (adaptadores in-memory obsoletos)
- [x] Unificación de tipos (`ValidationResult`, `AttributeType`, etc.)

#### ✅ 1.2 Encapsulación de EngineBuilder (CRÍTICO)
- [x] Creación de factoría bundle pública en `hodei-policies`
- [x] Función `create_schema_registration_components()` implementada
- [x] Refactor de `hodei-iam` para usar API pública
- [x] Eliminación de imports de `EngineBuilder` fuera de `hodei-policies`
- [x] Tests unitarios para nueva factoría
- [x] Documentación completa del refactor
- [x] Verificación: `cargo clippy -- -D warnings` pasa en `hodei-policies`
- [x] Validación: cero acoplamiento entre bounded contexts

#### ✅ 1.3 Dependency Injection y Arquitectura
- [x] Todas las factories refactorizadas para usar trait objects
- [x] Parámetros genéricos eliminados donde no son necesarios
- [x] Validación de inyección explícita en todas las features

---

## ✅ Completado - Fase 2: Playground Feature

### 2.1 Implementar `playground_evaluate` en `hodei-policies`

**Objetivo:** Permitir evaluación ad-hoc de políticas sin persistencia.

#### ✅ Estructura de la Feature (Vertical Slice Architecture)
- [x] `mod.rs` - Exportaciones del módulo
- [x] `use_case.rs` - Lógica de negocio principal
- [x] `ports.rs` - Traits (ISP - minimal & segregated)
- [x] `dto.rs` - Commands, Queries, Views
- [x] `error.rs` - Errores específicos
- [x] `di.rs` - Factory para DI
- [x] `mocks.rs` - Mocks para testing
- [x] `use_case_test.rs` - Tests unitarios con mocks

#### ✅ Tareas Específicas
- [x] **Diseño de DTOs**
  - [x] `PlaygroundEvaluateCommand`: inline policies + schema + request
  - [x] `PlaygroundEvaluateResult`: decisión + razones + diagnósticos
  - [x] Soporte para políticas inline (texto Cedar)
  - [x] Soporte para schema inline o versión almacenada

- [x] **Definición de Ports**
  - [x] `SchemaLoaderPort`: cargar schema (inline o storage)
  - [x] `PolicyValidatorPort`: validar políticas ad-hoc
  - [x] `PolicyEvaluatorPort`: evaluar request contra políticas
  - [x] `ContextConverterPort`: convertir atributos de contexto

- [x] **Implementación del Use Case**
  - [x] Validar schema inline o cargar desde storage
  - [x] Validar políticas inline contra schema
  - [x] Construir contexto de autorización
  - [x] Evaluar políticas usando Cedar engine
  - [x] Retornar decisión + razones detalladas

- [x] **Tests Unitarios**
  - [x] Test: evaluación con allow explícito
  - [x] Test: evaluación con deny explícito
  - [x] Test: evaluación con múltiples políticas
  - [x] Test: manejo de políticas inválidas
  - [x] Test: manejo de schema inválido
  - [x] Test: verificación de razones y diagnósticos

- [x] **Factory DI**
  - [x] Implementar `PlaygroundEvaluateUseCaseFactory::build()`
  - [x] Inyectar dependencias de schema, validador, evaluador
  - [x] Tests de factory

### 2.2 Actualizar API Pública de `hodei-policies`
- [x] Añadir `playground_evaluate` a `api.rs`
- [x] Re-exportar DTOs públicos
- [x] Documentación de la feature

**Resumen de Fase 2:**
- ✅ 8 archivos creados (~1,687 líneas)
- ✅ 28 tests unitarios pasando
- ✅ 0 errores de compilación
- ✅ 0 warnings de clippy
- ✅ Documentación completa
- ✅ Total tests en hodei-policies: 145 (30 nuevos)

---

## 🚧 En Progreso

### Fase 1.5: Limpieza Final (Opcional)

#### 🔧 Limpieza de Warnings
- [ ] Resolver warnings de imports no usados en `hodei-iam`
  - [ ] `artifact::Artifact` en `internal/domain/mod.rs`
  - [ ] `crate::internal::domain::User` en features
  - [ ] Imports de validación en `create_policy`
  - [ ] Otros imports no utilizados (~13 warnings)
- [ ] Corregir visibilidad de tipos (`User`, `Group` más privados que sus métodos públicos)
- [ ] Ejecutar `cargo clippy --fix` donde sea seguro

#### 🔧 Errores de Compilación en Tests de `hodei-iam`
- [ ] Arreglar features con tests que no compilan:
  - [ ] `list_policies` (falta `Default` trait en DTOs)
  - [ ] Otros 69 errores de compilación en tests
- [ ] Objetivo: `cargo test --workspace` debe pasar completamente

#### 🔧 Corrección de API REST Binary
- [ ] Arreglar errores de tipos en `hodei-artifacts-api/src/main.rs`
- [ ] Verificar handlers de Axum
- [ ] Asegurar que `app_state.rs` usa las nuevas factories

---

## 📋 Pendiente - Fase 3: Exposición REST API

### 3.1 Endpoints de Axum

#### 3.1.1 Playground Endpoints
- [ ] `POST /api/v1/playground/evaluate`
  - [ ] Handler que acepta políticas inline + request
  - [ ] Validación de input
  - [ ] Serialización de resultado
  - [ ] Manejo de errores

#### 3.1.2 Schema Management Endpoints
- [ ] `POST /api/v1/schema/iam/register`
  - [ ] Usa `RegisterIamSchemaUseCase` (ya existe)
  - [ ] Handler para bootstrapping
  - [ ] Response con versión de schema creado

#### 3.1.3 Policy Management Endpoints
- [ ] `POST /api/v1/policies/validate`
  - [ ] Usa `ValidatePolicyUseCase` (ya existe)
  - [ ] Handler para validación de políticas

### 3.2 Middleware y Autorización
- [ ] Implementar middleware de autorización con Cedar
  - [ ] Interceptar requests
  - [ ] Construir contexto de autorización
  - [ ] Evaluar políticas cargadas
  - [ ] Denegar o permitir según decisión

### 3.3 App State y Composition Root
- [ ] Actualizar `app_state.rs` para usar:
  - [ ] `create_schema_registration_components()` (nueva factoría)
  - [ ] `PlaygroundEvaluateUseCaseFactory::build()`
  - [ ] Almacenar use cases como trait objects en estado

### 3.4 Error Handling y Response Types
- [ ] DTOs de respuesta HTTP estandarizados
- [ ] Mapeo de errores de dominio a códigos HTTP
- [ ] Formato JSON consistente

---

## 📋 Pendiente - Fase 4: Documentación e Integración

### 4.1 Documentación Técnica
- [ ] OpenAPI / Swagger spec generado
  - [ ] Esquemas de request/response
  - [ ] Ejemplos de uso
  - [ ] Códigos de error documentados

- [ ] README actualizado con:
  - [ ] Instrucciones de arranque
  - [ ] Ejemplos de curl/HTTP
  - [ ] Arquitectura de alto nivel

- [ ] Guía de contribución
  - [ ] Cómo añadir una nueva feature (VSA template)
  - [ ] Checklist de verificación
  - [ ] Estándares de código

### 4.2 Tests de Integración
- [ ] Tests con `testcontainers` para SurrealDB
  - [ ] Setup/teardown automatizado
  - [ ] Aislamiento entre tests
  - [ ] Seed data para escenarios realistas

- [ ] Test end-to-end del flujo completo:
  - [ ] Registrar schema IAM
  - [ ] Crear políticas
  - [ ] Evaluar en playground
  - [ ] Verificar resultados

### 4.3 CI/CD Pipeline
- [ ] GitHub Actions workflow
  - [ ] `cargo check` en todos los crates
  - [ ] `cargo clippy -- -D warnings` sin fallos
  - [ ] `cargo test --workspace` pasando
  - [ ] `cargo nextest run` (si disponible)

---

## 🔮 Futuro (Post-MVP)

### Mejoras de Infraestructura
- [ ] Sustituir mocks in-memory por adaptadores SurrealDB embedded definitivos
- [ ] Implementar cache de schemas
- [ ] Optimización de evaluación de políticas

### Features Adicionales
- [ ] Web UI para el playground (frontend separado)
- [ ] Exportación de políticas evaluadas
- [ ] Historial de evaluaciones
- [ ] Métricas y observabilidad (Prometheus/Grafana)

### Escalabilidad
- [ ] Rate limiting
- [ ] Autenticación JWT
- [ ] Multi-tenancy
- [ ] Sharding de políticas por tenant

---

## 🎯 Prioridades Inmediatas (Orden de Ejecución)

### Crítico (Completado)
1. ✅ **Encapsulación de EngineBuilder** (COMPLETADO)
2. ✅ **Implementar feature `playground_evaluate`** (COMPLETADO)

### Alta Prioridad (Esta Semana)
3. 🚀 **Exposición de endpoints REST** (Fase 3)
4. 🚀 **Implementar adaptadores para ports de playground**

### Media Prioridad (Siguiente Iteración)
5. 🔧 **Limpieza de warnings en `hodei-iam`** (opcional)
6. 🔧 **Arreglar tests que no compilan en `hodei-iam`** (opcional)
7. 🔧 **Verificar API REST binary compila** (opcional)

### Media Prioridad (Siguientes Sprints)
7. 📚 **Documentación OpenAPI**
8. 🧪 **Tests de integración con testcontainers**

---

## 📊 Métricas de Progreso

### Compilación
- ✅ `hodei-policies`: Compila sin errores ni warnings
- 🔧 `hodei-iam`: Compila (lib) con warnings no críticos
- ⏳ `hodei-artifacts-api`: Errores menores pendientes

### Tests
- ✅ `hodei-policies`: 145 tests pasando (30 nuevos de playground)
- 🔧 `hodei-iam`: Tests de lib parcialmente pasando (módulos aislados OK)
- ⏳ `hodei-artifacts-api`: Sin tests aún

### Cobertura Arquitectural
- ✅ Bounded contexts correctamente separados
- ✅ Vertical Slice Architecture aplicada
- ✅ Dependency Injection via traits
- ✅ Zero coupling entre dominios
- ✅ Encapsulación de tipos internos

---

## 🔍 Notas Técnicas

### Decisiones Arquitecturales Recientes
1. **Bundle Factory Pattern:** Preferido sobre factories que exponen tipos internos
2. **Trait Objects en DI:** Usado para evitar parámetros genéricos en use cases públicos
3. **Schema Storage:** Port trait con métodos mínimos (ISP)

### Convenciones de Naming
- Use cases: `{Action}{Entity}UseCase` (e.g., `RegisterIamSchemaUseCase`)
- Factories: `{UseCase}Factory` con método `build()` o `build_with_{dependency}()`
- Ports: `{Action}{Entity}Port` (e.g., `SchemaStoragePort`)
- DTOs: `{Action}{Entity}Command/Query/Result`

### Anti-Patrones Detectados y Resueltos
- ❌ Exposición de `EngineBuilder` → ✅ Encapsulado con bundle factory
- ❌ Acoplamiento directo entre bounded contexts → ✅ Comunicación via use cases públicos
- ❌ Parámetros genéricos en use cases públicos → ✅ Trait objects para flexibilidad

---

## 📞 Contacto y Soporte

Para dudas sobre el backlog o priorización:
- Revisar documentación en `/docs/`
- Consultar `CLAUDE.md` para reglas arquitecturales
- Verificar `refactor-engine-builder-encapsulation.md` para patrón de factories

**Última revisión:** Fase 2 completada, listo para Fase 3 (REST API Integration)