# Plan de Implementación MVP - Hodei Artifacts

## 1. Propósito
Establecer un plan técnico accionable para alcanzar el MVP (Fase 1: Core Artifactory) consolidando estado actual, brechas, historias priorizadas, tareas técnicas, criterios de aceptación, riesgos y métricas.

## 2. Alcance MVP (Fase 1)
Incluye:
- Upload artefactos (subida + persistencia + evento).
- Download básico (recuperación segura).
- Gestión mínima de repositorios.
- Autorización ABAC mínima (cedar-policy) + autenticación placeholder.
- Indexación y búsqueda básica (nombre / texto simple).
- Infraestructura fundamental: OpenAPI contract-first, adaptadores Mongo / S3 / Kafka, observabilidad inicial (logs estructurados + tracing base + métricas mínimas).

Excluye (futuro): SBOM completo, escaneo vulnerabilidades, advanced search facets, analytics, SSO federado, workflows de seguridad, multi-formato completo.

## 3. Snapshot Estado Actual

### Código presente
- Vertical slice Upload parcialmente implementado: [`handler.rs`](crates/artifact/src/features/upload_artifact/handler.rs:7), [`command.rs`](crates/artifact/src/features/upload_artifact/command.rs:4), lógica placeholder [`logic.rs`](crates/artifact/src/features/upload_artifact/logic.rs:4).
- Modelos dominio núcleo:
  - Artifact: [`model.rs`](crates/artifact/src/domain/model.rs:14)
  - Repository: [`model.rs`](crates/repository/src/domain/model.rs:12)
  - UserAccount: [`model.rs`](crates/iam/src/domain/model.rs:8)
  - Sbom / Vulnerability (placeholder): [`model.rs`](crates/supply-chain/src/domain/model.rs:10)
  - SearchDocument: [`model.rs`](crates/search/src/domain/model.rs:8)
- Eventos compartidos: ArtifactUploadedEvent [`event.rs`](crates/shared/src/domain/event.rs:25), ArtifactDownloadRequestedEvent [`event.rs`](crates/shared/src/domain/event.rs:44), RepositoryCreatedEvent [`event.rs`](crates/repository/src/domain/event.rs:5), ArtifactIndexed [`event.rs`](crates/search/src/domain/event.rs:5).
- Puertos definidos:
  - ArtifactRepository / ArtifactStorage / ArtifactEventPublisher: [`ports.rs`](crates/artifact/src/application/ports.rs:6)
  - RepositoryStore: [`ports.rs`](crates/repository/src/application/ports.rs:15)
  - DomainEventPublisher genérico: [`ports.rs`](crates/shared/src/application/ports.rs:5)
  - SbomRepository: [`ports.rs`](crates/supply-chain/src/application/ports.rs:7)
  - UserAccountRepository: [`ports.rs`](crates/iam/src/application/ports.rs:7)
  - SearchIndex placeholder: [`mod.rs`](crates/search/src/application/mod.rs:2)
- Features búsqueda solo esqueletos con `todo!`: basic_search [`basic_search.rs`](crates/search/src/features/basic_search.rs:35), advanced_search [`advanced_search.rs`](crates/search/src/features/advanced_search.rs:66), index_management [`index_management.rs`](crates/search/src/features/index_management.rs:69).

### Ausente
- Adaptadores reales (Mongo, S3, Kafka).
- Endpoints HTTP (Axum) y openapi.yaml.
- ABAC integrado.
- Implementación search engine + indexación.
- Download slice.
- Observabilidad integrada (solo dependencias declaradas).
- Validaciones / idempotencia / cache autorizaciones.

## 4. Brechas Críticas (Top 10)
| # | Brecha | Impacto | Urgencia |
|---|--------|---------|----------|
| 1 | Falta OpenAPI contract | Inconsistencia API | Alta |
| 2 | No adaptadores persistencia (Mongo) | No se guardan datos | Alta |
| 3 | Sin storage S3 adapter | Binarios no disponibles | Alta |
| 4 | Sin event publisher Kafka | Ecosistema event-driven bloqueado | Alta |
| 5 | ABAC no implementado | Riesgo seguridad | Alta |
| 6 | No endpoint download | Caso de uso incompleto | Alta |
| 7 | Búsqueda sin indexación | DevSecOps sin discoverability | Media |
| 8 | Sin idempotencia upload | Duplicados / integridad | Media |
| 9 | Sin observabilidad (metrics/tracing) | Operabilidad limitada | Media |
|10 | Validaciones checksum/tamaño ausentes | Riesgo integridad | Media |

## 5. Principios de Ejecución
1. Contract-First: cada endpoint debe estar primero en `openapi.yaml`.
2. Hexagonal estricto: adaptadores implementan puertos existentes.
3. Vertical Slice cohesivo: evitar lógica transversal en `shared` salvo tipos estables.
4. Event-Carried State: eventos con payload completo listos para consumo.
5. Idempotencia y seguridad > features “nice to have”.
6. Observabilidad mínima desde primer deploy (no “add later”).
7. Fail fast en upload (validaciones previas a I/O costoso).

## 6. Epics MVP Priorizadas
| Epic | Código Relacionado | Prioridad |
|------|--------------------|-----------|
| Upload | [`handler.rs`](crates/artifact/src/features/upload_artifact/handler.rs:7) | P0 |
| Repositorios | [`model.rs`](crates/repository/src/domain/model.rs:12) | P0 |
| Download | (a crear) | P0 |
| Búsqueda Básica | [`basic_search.rs`](crates/search/src/features/basic_search.rs:35) | P0 |
| ABAC Básico | (cedar-policy integration) | P0 |
| Infra/Observabilidad | workspace / raíz | P0 |
| Idempotencia + Validaciones | Upload domain | P1 |
| Cache ABAC / optimizaciones | ABAC layer | P1 |

## 7. Historias (User Stories) P0 / P1
P0-U1 Subir artefacto (obtener artifact_id).  
P0-U2 Rechazar si repositorio no existe.  
P0-U3 Persistir binario (S3) + metadatos (Mongo).  
P0-U4 Publicar evento ArtifactUploaded.  
P0-U5 Crear repositorio (CRUD mínimo create).  
P0-U6 Descargar artefacto por id / presigned.  
P0-U7 Evaluar política ABAC en upload/download.  
P0-U8 Indexar artefacto tras evento.  
P0-U9 Búsqueda básica por nombre.  
P0-U10 OpenAPI base validada en CI.  
P1-U11 Validar checksum/tamaño archivo.  
P1-U12 Idempotencia upload (dedupe por checksum).  
P1-U13 Cache decisiones ABAC (TTL).  
P1-U14 Métricas Prometheus uploads/downloads/search.  
P1-U15 Tracing distribuido (spans en handlers + eventos).

## 8. Tareas Técnicas Detalladas (Mapeo)
### Upload (U1–U4, U11, U12)
- Definir schema OpenAPI `POST /v1/artifacts` (multipart + JSON).
- Implementar endpoint Axum -> map a `UploadArtifactCommand` [`command.rs`](crates/artifact/src/features/upload_artifact/command.rs:4).
- Validaciones (checksum hex 64 chars, size>0).
- S3Adapter: trait `ArtifactStorage` [`ports.rs`](crates/artifact/src/application/ports.rs:12) → `put_object`.
- MongoArtifactRepository implementa `ArtifactRepository` [`ports.rs`](crates/artifact/src/application/ports.rs:7).
- Idempotencia: índice único (repository_id + checksum).
- Publicador Kafka implementa `ArtifactEventPublisher` [`ports.rs`](crates/artifact/src/application/ports.rs:17).
- Transformar evento interno a ArtifactUploadedEvent [`event.rs`](crates/shared/src/domain/event.rs:25).
- Estrategia reintentos (backoff simple en publish).

### Repositorios (U2, U5)
- Endpoint `POST /v1/repositories`.
- MongoRepositoryStore implementa `RepositoryStore` [`ports.rs`](crates/repository/src/application/ports.rs:15).
- Índice único `name`.
- Validar existencia antes upload.

### Download (U6)
- Endpoint `GET /v1/artifacts/{id}` (opción query `?presigned=true`).
- Método adapter S3: generar presigned URL o streaming.
- Publicar ArtifactDownloadRequestedEvent [`event.rs`](crates/shared/src/domain/event.rs:44).
- Métricas downloads_total, bytes_transferred (P1-U14).

### ABAC (U7, U13)
- Definir trait `PolicyEvaluator` (nuevo).
- Adapter cedar-policy: carga policies desde directorio configurable.
- Middleware Axum (extractor) + caching (HashMap LRU) P1.
- Métrica authorization_decision_latency.

### Indexación y Búsqueda (U8, U9)
- Consumidor Kafka topic `artifact.uploaded` → crea doc (ArtifactSearchDocument) [`model.rs`](crates/search/src/domain/model.rs:8).
- Implementar `SearchIndex` trait mock a Mongo (colección `search_index`).
- Reemplazar `todo!` en `handle_basic_search` [`basic_search.rs`](crates/search/src/features/basic_search.rs:35).
- Endpoint `GET /v1/search?q=...`.

### OpenAPI & CI (U10)
- Archivo `openapi.yaml` raíz (versión semántica).
- Validación CI (stage lint -> fail si drift).
- Generación opcional de tipos (futuro).

### Observabilidad (U14, U15)
- Añadir `tracing` spans en:
  - Upload handler [`handler.rs`](crates/artifact/src/features/upload_artifact/handler.rs:7)
  - Search handler (nuevo)
  - Download handler (nuevo)
- Exponer `/metrics` (Prometheus).
- Incluir correlation id en eventos (header -> metadata).

## 9. Orden de Ejecución (Ruta Crítica)
1. OpenAPI base (endpoints vacíos) (U10).
2. Mongo + S3 adapters (U3 infra).
3. Repository create (U5) → prereq uploads.
4. Upload end-to-end (U1–U4).
5. Kafka publisher + topic + evento (U4).
6. Index consumer + búsqueda básica (U8–U9).
7. Download (U6).
8. ABAC mínimo (deny/allow) (U7).
9. Validaciones + idempotencia (U11–U12).
10. Observabilidad & cache ABAC (U13–U15).

## 10. Criterios de Aceptación (DoD Extracto)
| Historia | DoD |
|----------|-----|
| U1 | 202 Accepted / 200 OK con `{artifact_id}`; error 422 inputs inválidos |
| U2 | Upload retorna 404 si repo inexistente |
| U3 | Archivo >1MB recuperable desde S3; metadatos persistidos |
| U4 | Evento consumible (payload completo) en topic en <250ms post persist |
| U5 | 201 repo + id; nombre duplicado -> 409 |
| U6 | Descarga <50ms p99 (mock local); evento download publicado |
| U7 | Decisión ABAC registrada (allow/deny) |
| U8 | Documento index creado <200ms (test integración) |
| U9 | Búsqueda substring retorna artifact; paginación funciona |
| U11 | Checksum inválido -> 422; tamaño 0 -> 422 |
| U12 | Subida duplicada retorna mismo artifact_id sin duplicar almacenamiento |
| U14 | `/metrics` expone counters e histogramas básicos |
| U15 | Trazas con span upload->publish->index visibles |

## 11. Backlog Priorizado (Estimaciones Relativas)
| Item | Historias | Est (pts) | Prioridad |
|------|-----------|-----------|-----------|
| Upload Core | U1-U4 | 21 | P0 |
| Repo Create | U5 | 5 | P0 |
| Download | U6 | 8 | P0 |
| ABAC Mínimo | U7 | 13 | P0 |
| Index + Búsqueda | U8-U9 | 21 | P0 |
| OpenAPI Base | U10 | 5 | P0 |
| Validaciones + Idempotencia | U11-U12 | 8 | P1 |
| Cache ABAC | U13 | 5 | P1 |
| Observabilidad | U14-U15 | 8 | P1 |

## 12. Riesgos y Mitigaciones
| Riesgo | Mitigación |
|--------|-----------|
| Falta de contract antes de código | Gate CI que falla si openapi.yaml no actualizado |
| Duplicados por reintentos cliente | Índice único (repo_id+checksum) + devolver id existente |
| Bloqueo por error Kafka | Publicar en background + fallback log + reintentos + DLQ futura |
| Fuga lógica a `shared` | Revisión PR + regla “Regla de Tres” |
| Seguridad laxa inicial | ABAC mínimo early + default deny si política no carga |
| Observabilidad tardía | Incluir /metrics y tracing en primer merge |

## 13. Métricas Clave Iniciales
- `uploads_total{result}` counter
- `upload_duration_seconds` histogram (p95 objetivo <0.1s sin I/O remoto)
- `downloads_total`
- `authz_decision_latency_ms` (p95 <2ms)
- `search_queries_total`, `search_query_duration_ms`
- `events_published_total`, `event_publish_failures_total`

## 14. Próximos Pasos Sprint 0
1. Crear `openapi.yaml` (repos, artifacts upload/download, search).
2. Scaffold server Axum (main) con routing modular.
3. Implement adapters Mongo (ArtifactRepository, RepositoryStore).
4. Implement adapter S3 (ArtifactStorage).
5. Endpoint POST /v1/repositories (U5).
6. Endpoint POST /v1/artifacts + lógica (U1-U3).
7. Kafka publisher + publicar ArtifactUploaded (U4).
8. Consumer indexación + creación SearchIndex (U8).
9. GET /v1/search (U9).
10. GET /v1/artifacts/{id} (U6).
11. Integrar cedar-policy (carga estática) (U7).
12. Métricas y tracing base (U14-U15).

## 15. Anexos (Referencias Código Actual)
| Elemento | Archivo |
|----------|---------|
| Handler Upload | [`handler.rs`](crates/artifact/src/features/upload_artifact/handler.rs:7) |
| Command Upload | [`command.rs`](crates/artifact/src/features/upload_artifact/command.rs:4) |
| Artifact Domain | [`model.rs`](crates/artifact/src/domain/model.rs:14) |
| Repository Domain | [`model.rs`](crates/repository/src/domain/model.rs:12) |
| Search Basic Handler (todo) | [`basic_search.rs`](crates/search/src/features/basic_search.rs:35) |
| Artifact Events | [`event.rs`](crates/shared/src/domain/event.rs:25) |
| Ports Artifact | [`ports.rs`](crates/artifact/src/application/ports.rs:6) |
| Ports Repository | [`ports.rs`](crates/repository/src/application/ports.rs:15) |
| DomainEventPublisher | [`ports.rs`](crates/shared/src/application/ports.rs:5) |
| SearchIndex Placeholder | [`mod.rs`](crates/search/src/application/mod.rs:2) |

---

Este plan queda listo para ejecución inmediata. Ajustes posteriores deberán versionarse y enlazarse a métricas reales tras Sprint 0.
## 16. Estrategia Organización de Tests (Buenas Prácticas Adoptadas)

Objetivo: asegurar mantenibilidad, aislamiento y señal rápida de fallos sin filtrar detalles internos al API público.

### 16.1 Principios
1. Separación física estricta: ningún test en ficheros de código de negocio (`src/**`). Evitar módulos `#[cfg(test)]` embebidos salvo micro‐invariantes críticas (política default: prohibido).
2. Pirámide balanceada:
   - Unit: rápidas, sin I/O, ejercen lógica pura (dominio, mapeos doc&lt;-&gt;entidad, validaciones).
   - Integration: ejercen adapters reales (Mongo, S3, Kafka) usando testcontainers / servicios CI.
   - E2E / Slice: flujos verticales (upload, search, download) sobre la API HTTP o comando completo.
3. Tests deterministas: uso de datos generados (UUID, timestamps) encapsulado en builders que permiten fijar valores cuando se necesite aserción exacta.
4. Aislamiento estado: cada test de integración usa DB lógica única (sufijo aleatorio); contenedor Mongo compartido (startup amortizado).
5. No exponer internals sólo “para test”: preferir cubrir por puerto público. Si se requiere simular error raro → introducir trait/fábrica inyectable testeable vía mock en carpeta `tests/support/`.
6. Convención nombres:
   - Unit: `tests/unit/<context>_*.rs` (o `unit_<context>_*.rs` si mismo directorio).
   - Integration Adapter: `tests/it_<adapter>_*.rs`.
   - E2E vertical slice: `tests/e2e_<slice>_*.rs`.
   - Soporte común: `tests/support/mod.rs` (+ submódulos).
7. Features de compilación para costos altos: `integration-mongo`, `integration-kafka`, etc. Activadas en pipeline específica.
8. Reutilización helper infraestructura: un único helper por tipo de servicio (actual: Mongo) expuesto desde crate infra bajo feature `test-util`.
9. Fail Fast: si infraestructura externa no disponible y no hay variables de entorno preconfiguradas → fallo explícito (no skip silencioso).
10. Métricas de calidad: cobertura mínima slice crítica (upload, repository) ≥ 70% líneas dominio + adaptadores; clippy sin warnings; tiempos p95 unit tests &lt; 50ms.

### 16.2 Estructura Objetivo (Ejemplo)
```
crates/
  artifact/
    src/...
    tests/
      unit/
        unit_artifact_repository_conversion.rs
      it/
        it_artifact_repository_mongo.rs
      e2e/
        e2e_upload.rs
      support/
        builders.rs
  repository/
    tests/
      unit/
      it/
tests/            # (opcional) e2e cross-crate (full service HTTP)
```

### 16.3 Estrategia de Mocks / Fakes
- Mocks sólo en capa de puertos (no mockear tipos de dominio puros).
- Evitar mocks en exceso: preferir tests de integración ligeros para adapters reales.
- Para errores difíciles de inducir (p.ej. duplicate key) → detector / trait inyectable (patrón aplicado en ArtifactRepository) con mock local fuera de `src`.

### 16.4 Integración CI
- Jobs separados:
  - `test-unit` (sin features, sin contenedores).
  - `test-integration` (activa features `integration-*`, levanta servicios o usa testcontainers).
  - `test-e2e` (cuando exista servidor HTTP bootstrap).
- Gate adicional (futuro CI-T7): script que rechaza PR si detecta `#[cfg(test)]` dentro de archivos `src/**` salvo whitelist (lista vacía inicial).

### 16.5 Métricas / Observabilidad de Tests (Futuro)
- Reporte cobertura (CI-T6) publicado como artifact.
- Tiempo por categoría (unit vs integration) para detectar regresiones de performance.
- Conteo de flakiness (re‐ejecuciones automáticas planificadas posterior a MVP).

### 16.6 Roadmap de Refactor (TEST-ORG*)
1. TEST-ORG1 Crear guía dedicada `docs/testing-organization.md`.
2. TEST-ORG2 Extraer tests unitarios embebidos (ArtifactRepository) a `crates/artifact/tests/unit/unit_artifact_repository.rs`.
3. TEST-ORG3 Eliminar módulo de tests interno en `mongo_artifact_repository` y limpiar visibilidades revertidas.
4. TEST-ORG4 Añadir verificación CI (script) que escanee `src/**` para `#[cfg(test)]` y falle si encuentra (except whitelist).
5. TEST-ORG5 Homogeneizar prefijos `unit_`, `it_`, `e2e_` en archivos existentes.

### 16.7 Justificación Beneficios
| Beneficio | Mecanismo |
|-----------|-----------|
| Menor ruido de diffs | Tests fuera de archivos productivos |
| Revisión más focalizada | PR puede filtrar carpetas tests |
| Escalabilidad adaptadores | Helpers centralizados reutilizables |
| Menor riesgo exposición interna | No se hacen `pub` tipos sólo para tests |
| Tiempos previsibles | Separación pipelines (unit rápidos) |

### 16.8 Acciones Inmediatas
- Añadir documento guía (TEST-ORG1).
- Planificar refactor extracción tests Artifact (TEST-ORG2/3) antes de introducir nuevos endpoints upload.
- Actualizar WBS (implementation-tasks.md) con nuevas tasks TEST-ORG* y CI-T7.
