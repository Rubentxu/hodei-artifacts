# Guía de Estilo para Features (Vertical Slice + Hexagonal)

## 1. Principios Arquitectónicos

### Vertical Slice Architecture
- **Encapsulación por feature**: cada caso de uso aislado con su command/query, lógica pura y efectos secundarios
- **Cohesión funcional**: todos los componentes de una feature agrupados en su directorio
- **Minimal coupling**: evitar dependencias cruzadas entre features

### Hexagonal Architecture  
- **Dependency Inversion**: dependencias hacia puertos (traits) en capa de aplicación
- **Adaptadores externos**: implementaciones concretas en `infrastructure`
- **Core business logic**: sin dependencias a frameworks externos

### Principios Transversales
- **Fail Fast**: validaciones antes de cualquier I/O costoso
- **Idempotencia explícita**: decisión clara antes de efectos secundarios
- **Event-Carried State**: eventos con payload completo + correlation id
- **Pure Core**: lógica y decisiones testeables sin mocks pesados

## 2. Estructura Recomendada por Slice

```
<crate>/src/features/<feature_name>/
  command.rs / query.rs        # DTOs de entrada
  logic/
    mod.rs                     # Re-exports y documentación
    validate.rs                # Validaciones puras
    dedupe.rs                  # Lógica idempotencia (si aplica)
    use_case.rs                # Orquestación principal
    build_event.rs             # Construcción eventos (si aplica)
  handler.rs                   # Adaptador HTTP/mensajería
  dto.rs                       # Mapping HTTP -> command (opcional)
```

### Reglas de Implementación
- `handler.rs`: ≤ 40 LOC, sin reglas de negocio
- `use_case.rs`: orquesta (validar → consultas lectura → plan → side-effects)
- `logic/*.rs`: sin dependencias a crates externos salvo tipos dominio + std + utilidades puras

## 3. Convenciones de Naming

| Elemento | Convención | Ejemplo |
|----------|------------|---------|
| Command | `<Action><Entity>Command` | `UploadArtifactCommand` |
| Query | `Get<Entity>Query` / `List<Entity>Query` | `GetArtifactQuery` |
| Use case struct | `<Action><Entity>UseCase` | `UploadArtifactUseCase` |
| Resultado | `<Action><Entity>Result` | `UploadArtifactResult` |
| Enum decisiones | `<Context>Decision` | `DedupeDecision` |
| Errores | `<Feature>Error` | `ArtifactError` |
| Módulos lógicos | verbo/concepto | `validate`, `dedupe`, `build_event` |
| Tests unitarios | `tests/unit/<feature>_*` | `unit_upload_validation.rs` |
| Tests integración | `tests/it/it_<feature>_*.rs` | `it_upload_artifact.rs` |

## 4. Patrón de Validaciones

### Ubicación y Estructura
- Colocar todas las validaciones en `validate.rs`
- Retornar errores de dominio específicos (no strings genéricas)
- Regex y límites configurables via constantes internas
- **No acceder** a storage o repositorios

### Ejemplo de Implementación
```rust
use once_cell::sync::Lazy;
use regex::Regex;

static CHECKSUM_REGEX: Lazy<Regex> = Lazy::new(|| 
    Regex::new("^[a-f0-9]{64}$").unwrap()
);

pub fn validate_checksum(checksum: &ArtifactChecksum) -> Result<(), ArtifactError> {
    if !CHECKSUM_REGEX.is_match(&checksum.0) {
        return Err(ArtifactError::InvalidChecksum);
    }
    Ok(())
}

pub fn validate_size(size_bytes: u64) -> Result<(), ArtifactError> {
    if size_bytes == 0 {
        return Err(ArtifactError::InvalidSize);
    }
    Ok(())
}

pub fn validate_all(cmd: &UploadArtifactCommand) -> Result<(), ArtifactError> {
    validate_checksum(&cmd.checksum)?;
    validate_size(cmd.size_bytes)?;
    // ... otras validaciones
    Ok(())
}
```

## 5. Patrón de Idempotencia

### Flujo Estándar
1. `dedupe::check(...) -> DedupeDecision`
2. Early return si `DedupeDecision::Existing(id)`
3. Continuar con `DedupeDecision::New(data)` 
4. Índice único como safety net para race conditions

### Implementación Tipo
```rust
pub enum DedupeDecision {
    Existing(ArtifactId),
    New(NewArtifactData),
}

pub async fn check_duplicate(
    repo: &dyn ArtifactRepository,
    cmd: &UploadArtifactCommand,
) -> Result<DedupeDecision, ArtifactError> {
    if let Some(existing) = repo
        .find_by_repo_and_checksum(&cmd.repository_id, &cmd.checksum)
        .await? 
    {
        return Ok(DedupeDecision::Existing(existing.id));
    }
    Ok(DedupeDecision::New(NewArtifactData::from(cmd)))
}
```

### Manejo de Race Conditions
- Capturar duplicate key error como carrera normal
- Re-consultar repositorio y retornar ID existente
- Evitar fallos por timing entre validación y persistencia

## 6. Orquestación en Use Case

### Orden Fijo de Ejecución
1. **Validaciones puras** (sin I/O)
2. **Dedupe/Precondiciones** (lecturas idempotentes)
3. **Construcción entidad + evento** (lógica pura)
4. **Side-effects secuenciales**: storage → repositorio → publisher
5. **Métricas + spans + retorno**

### Plantilla de Implementación
```rust
pub struct UploadArtifactUseCase<'a> {
    repo: &'a dyn ArtifactRepository,
    storage: &'a dyn ArtifactStorage,
    publisher: &'a dyn ArtifactEventPublisher,
}

impl<'a> UploadArtifactUseCase<'a> {
    #[tracing::instrument(skip(self, cmd))]
    pub async fn execute(
        &self, 
        cmd: UploadArtifactCommand
    ) -> Result<UploadArtifactResult, ArtifactError> {
        // 1. Validaciones puras
        validate::validate_all(&cmd)?;
        
        // 2. Idempotencia (lectura)
        match dedupe::check_duplicate(self.repo, &cmd).await? {
            DedupeDecision::Existing(id) => {
                metrics::increment_counter("uploads_total", &[("result", "duplicate")]);
                return Ok(UploadArtifactResult { artifact_id: id });
            }
            DedupeDecision::New(data) => {
                // 3. Construcción (pura)
                let artifact = Artifact::new(data)?;
                let event = build_event::create_uploaded_event(&artifact, &cmd)?;
                
                // 4. Side-effects secuenciales
                self.storage
                    .put_object(&artifact.repository_id, &artifact.id, &cmd.bytes)
                    .await?;
                
                self.repo.save(&artifact).await?;
                
                self.publisher.publish_created(&event).await?;
                
                // 5. Métricas y retorno
                metrics::increment_counter("uploads_total", &[("result", "success")]);
                Ok(UploadArtifactResult { artifact_id: artifact.id })
            }
        }
    }
}
```

### Manejo de Errores
- Mapear cada fase con variante de error clara
- Ejemplos: `ValidationError`, `PreconditionError`, `StorageError`, `PersistError`, `PublishError`
- Propagar context útil sin exponer detalles internos

## 7. Construcción de Eventos

### Principios
- Construir en `build_event.rs` antes de side-effects si no requiere IDs persistidos
- Si necesita ID de DB, construir después de persistencia
- **Incluir siempre**: correlation id, timestamps UTC, todos los datos necesarios para consumidores

### Campos Estándar por Evento
```rust
pub fn create_uploaded_event(
    artifact: &Artifact,
    cmd: &UploadArtifactCommand,
) -> Result<ArtifactUploadedEvent, ArtifactError> {
    Ok(ArtifactUploadedEvent {
        artifact_id: artifact.id.clone(),
        repository_id: artifact.repository_id.clone(),
        checksum: artifact.checksum.clone(),
        size_bytes: artifact.size_bytes,
        uploaded_by: cmd.user_id.clone(),
        uploaded_at: Utc::now(),
        correlation_id: correlation_id::current(),
        metadata: EventMetadata {
            version: "1.0".to_string(),
            source: "hodei-artifacts".to_string(),
        },
    })
}
```

## 8. Instrumentación (Métricas + Tracing)

### Estructura de Spans
- **Span raíz**: `<feature>.execute` (ej: `upload_artifact.execute`)
- **Sub-spans opcionales**: `validate`, `dedupe`, `persist`, `publish`
- **Contexto**: incluir IDs relevantes como fields

### Métricas Mínimas por Feature
```rust
// Counters
metrics::increment_counter("<feature>_total", &[("result", "success|error|duplicate")]);

// Histograms  
let start = Instant::now();
// ... ejecución ...
metrics::record_histogram("<feature>_duration_seconds", start.elapsed().as_secs_f64());
```

### Reglas de Instrumentación
- **No instrumentar** funciones puras (validaciones, construcción entidades)
- **Sí instrumentar**: handlers, use cases, calls a adaptadores externos
- **Correlation ID**: propagar desde header HTTP a eventos y logs

## 9. Gestión de Errores

### Estructura por Feature
```rust
#[derive(Debug, thiserror::Error)]
pub enum ArtifactError {
    #[error("Invalid checksum format")]
    InvalidChecksum,
    #[error("Invalid file size: {0}")]
    InvalidSize(u64),
    #[error("Repository not found: {0}")]
    RepositoryNotFound(RepositoryId),
    #[error("Storage operation failed: {0}")]
    StorageError(String),
    #[error("Persistence failed: {0}")]
    PersistError(String),
    #[error("Event publishing failed: {0}")]
    PublishError(String),
}
```

### Conversión a HTTP (en handler)
```rust
impl From<ArtifactError> for StatusCode {
    fn from(err: ArtifactError) -> Self {
        match err {
            ArtifactError::InvalidChecksum | ArtifactError::InvalidSize(_) => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            ArtifactError::RepositoryNotFound(_) => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}
```

## 10. Estrategia de Testing

### Distribución por Tipo

| Tipo | Cubre | Herramientas | Ubicación |
|------|-------|--------------|-----------|
| **Unit** | Validaciones, lógica pura, construcción eventos | Sin mocks pesados | `tests/unit/<feature>_*.rs` |
| **Integration** | Use case con adaptadores reales/fakes | testcontainers, fakes ligeros | `tests/it/it_<feature>_*.rs` |
| **E2E** | Flujo HTTP completo | Playwright, HTTP client | `e2e/tests/<feature>*.spec.ts` |
| **Contract** | Drift OpenAPI | Scripts CI | Pipeline |

### Reglas de Testing
- **Un test por camino de error** relevante (no micro-variantes triviales)
- **Tests de idempotencia**: verificar que duplicados retornan mismo ID
- **Tests de race conditions**: simular duplicate key si es posible
- **Coverage mínimo**: ≥ 85% en módulos `logic/`

### Ejemplo Test Unitario
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_checksum_rejects_invalid_format() {
        let invalid_checksum = ArtifactChecksum("invalid".to_string());
        
        let result = validate::validate_checksum(&invalid_checksum);
        
        assert!(matches!(result, Err(ArtifactError::InvalidChecksum)));
    }

    #[test]
    fn validate_size_rejects_zero() {
        let result = validate::validate_size(0);
        
        assert!(matches!(result, Err(ArtifactError::InvalidSize(0))));
    }
}
```

## 11. Dependencias y Aislamiento

### Permitidas por Capa

| Capa | Dependencias Permitidas |
|------|------------------------|
| **Handler** | Axum, HTTP types, serde, tracing |
| **Use Case** | Traits de dominio, shared types, async-trait |
| **Logic** | std, regex, semver, chrono, uuid, serde |
| **Domain** | std, serde, uuid, chrono |

### Prohibiciones
- **Logic** no puede depender de adaptadores concretos
- **Domain** no puede depender de frameworks web
- **Features** no pueden tener dependencias circulares entre ellas

## 12. Evolución y Extensibilidad

### Añadir Nueva Fase al Flujo
```rust
// Si es consulta (antes de persistencia)
let scan_result = scan::analyze_content(&cmd.bytes)?;

// Si necesita artefacto físico (después de storage)
let scan_result = scan::analyze_stored_artifact(&artifact_path).await?;
```

### Paralelización de Side-Effects
```rust
// Si se requieren >2 efectos paralelizables
pub struct ExecutionPlan {
    artifact: Artifact,
    event: ArtifactUploadedEvent,
    storage_task: StorageTask,
    index_task: IndexTask,
}

// Ejecutar con join
let (storage_result, repo_result) = tokio::join!(
    self.storage.put_object(...),
    self.repo.save(&plan.artifact)
);
```

## 13. Checklist para PRs de Features

### Implementación
- [ ] Command/Query definido y documentado
- [ ] Validaciones puras con tests unitarios
- [ ] UseCase ejecuta orden canónico (validate → dedupe → side-effects)
- [ ] Handler sin lógica de negocio (≤ 40 LOC)
- [ ] Idempotencia implementada y testada (si aplica)

### Eventos y Observabilidad
- [ ] Evento con correlation id y payload completo
- [ ] Métricas básicas (counter + histogram)
- [ ] Spans de tracing configurados
- [ ] Errores mapeados exhaustivamente

### Calidad
- [ ] Sin `#[cfg(test)]` en `src/**`
- [ ] OpenAPI actualizado con nuevos endpoints
- [ ] Tests: unitarios (validaciones) + integración (flujo completo)
- [ ] Coverage ≥ 85% en módulos logic/

### Documentación
- [ ] Docstrings en use case público
- [ ] Comentarios en lógica compleja
- [ ] Ejemplo de uso en módulo raíz

## 14. Anti-Patrones (Evitar)

### ❌ Violaciones Arquitectónicas
- Lógica de negocio en handlers
- Acceso directo a adaptadores desde validaciones
- Dependencias circulares entre features
- Side-effects mezclados con validaciones

### ❌ Malas Prácticas de Eventos
- Eventos parciales que requieren enriquecimiento posterior
- Falta de correlation ID en eventos
- Eventos sin timestamps o con formatos inconsistentes

### ❌ Testing Anti-Patrones
- `pub` en campos solo para facilitar tests
- Tests que dependen de orden de ejecución
- Mocks complejos para lógica pura
- Tests sin assertions específicas

### ❌ Error Handling
- Usar `String` genérico para errores de dominio
- Propagar errores de librerías externas sin wrapping
- Error messages sin contexto útil

## 15. Migración de Features Existentes

### Estrategia Incremental
1. **Extraer validaciones** a `validate.rs` (no rompe API)
2. **Crear use case** con feature flag (comparar comportamiento)
3. **Refactor handler** para delegar (switch definitivo)
4. **Añadir observabilidad** (métricas + spans)
5. **Cleanup** código legacy

### Compatibilidad
- Mantener API pública estable durante migración
- Feature flags para rollback rápido
- Tests de regresión antes de cada paso

## 16. Referencias y Recursos

### Documentos Relacionados
- [`docs/arquitectura-sistema.md`](arquitectura-sistema.md) - Principios arquitectónicos generales
- [`docs/testing-organization.md`](testing-organization.md) - Estrategia de testing detallada
- [`docs/plan.md`](plan.md) - Plan de implementación MVP

### Herramientas Recomendadas
- **Validation**: `regex`, `semver`, custom validators
- **Error Handling**: `thiserror`, custom error enums
- **Testing**: `testcontainers`, fake implementations
- **Metrics**: `metrics` crate, Prometheus exporter
- **Tracing**: `tracing`, `tracing-subscriber`

---

Esta guía evoluciona con el proyecto. Sugerencias de mejora via PRs o discusión en issues.
