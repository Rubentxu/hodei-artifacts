# Plan Detallado de Implementación (WBS Ejecutable) - Hodei Artifacts

## 0. Objetivo
Generar un desglose accionable (work breakdown structure) directamente implementable a partir de [`plan.md`](docs/plan.md) y la arquitectura descrita en [`arquitectura-sistema.md`](docs/arquitectura-sistema.md:1). Se prioriza ruta crítica y calidad operativa temprana.

## 1. Convenciones
- Formato tareas: ID (EPIC-STORY-TN) + Tipo (CODE/INFRA/TEST/OBS/SEC/OPS).
- Sección DoD específica por historia (ref cruza con sección 9 de [`plan.md`](docs/plan.md:159)).
- Referencias a archivos existentes con líneas (cuando aplica) p.e. [`handler.rs`](crates/artifact/src/features/upload_artifact/handler.rs:7).
- Nuevos archivos sin línea inicial todavía (se crearán).

## 2. Vista Global de Epics (Ruta Crítica)
1. OpenAPI Base (E-OPENAPI)
2. Infra Persistencia + Storage (E-INFRA-PERSIST)
3. Repo Create (E-REPO) → prereq Upload
4. Upload End-to-End (E-UPLOAD)
5. Kafka Events + Publisher (E-EVENTS)
6. Index + Search Básica (E-SEARCH)
7. Download (E-DOWNLOAD)
8. ABAC Mínimo (E-ABAC)
9. Validaciones + Idempotencia (E-VALIDATION)
10. Observabilidad + Cache ABAC (E-OBS)

## 3. WBS por Epic

### 3.1 E-OPENAPI
| ID | Descripción | Tipo | Output |
|----|-------------|------|--------|
| OPENAPI-T1 | Crear `openapi.yaml` inicial endpoints: POST /v1/repositories, POST /v1/artifacts, GET /v1/artifacts/{id}, GET /v1/search | CODE | `openapi.yaml` |
| OPENAPI-T2 | Añadir esquemas Artifact, Repository, ErrorResponse estables | CODE | Sección components |
| OPENAPI-T3 | Pipeline validación drift (script compara hash) | OPS | Job CI |
| OPENAPI-T4 | Política rechazo PR si diff sin actualización | OPS | Regla CI |

### 3.2 E-INFRA-PERSIST
| ID | Descripción | Tipo | Output |
|----|-------------|------|--------|
| INFRA-T1 | Crear crate/ módulo infra Mongo (`crates/infra_mongo/`) con cliente reutilizable | CODE | `MongoClientFactory` |
| INFRA-T2 | Implementar `ArtifactRepository` [`ports.rs`](crates/artifact/src/application/ports.rs:6) → `MongoArtifactRepository` | CODE | `mongo_artifact_repository.rs` |
| INFRA-T3 | Implementar `RepositoryStore` [`ports.rs`](crates/repository/src/application/ports.rs:15) → `MongoRepositoryStore` | CODE | `mongo_repository_store.rs` |
| INFRA-T4 | Crear índices: `repositories.name` único, `artifacts.repository_id+checksum` (placeholder hasta idempotencia) | INFRA | Scripts init |
| INFRA-T5 | S3 Adapter implementa `ArtifactStorage` [`ports.rs`](crates/artifact/src/application/ports.rs:12) | CODE | `s3_artifact_storage.rs` |
| INFRA-T6 | Configuración por variables entorno (MONGO_URI, S3_ENDPOINT, S3_BUCKET) | OPS | `.env.example` |
| INFRA-T7 | Tests integración repos / artifacts (Custom Docker Compose framework) | TEST | `tests/integration_tests.rs` |

### 3.3 E-REPO (Historias U5, U2 soporte)
| ID | Descripción | Tipo | Output |
|----|-------------|------|--------|
| REPO-T1 | Definir DTO CreateRepositoryRequest/Response | CODE | `dto.rs` |
| REPO-T2 | Endpoint Axum POST /v1/repositories → usa `MongoRepositoryStore` | CODE | `handler_create_repository.rs` |
| REPO-T3 | Validación nombre (regex ^[a-z0-9._-]{3,50}$) | CODE | Validador |
| REPO-T4 | Manejo conflicto 409 | CODE | Mapeo error |
| REPO-T5 | Tests endpoint (happy / duplicado / invalid) | TEST | `tests/repository_endpoint.rs` |

### 3.4 E-UPLOAD (U1–U4)
| ID | Descripción | Tipo | Output |
|----|-------------|------|--------|
| UPLOAD-T1 | Completar `UploadArtifactCommand` [`command.rs`](crates/artifact/src/features/upload_artifact/command.rs:4) con campos checksum,size,mime | CODE | Actualización struct |
| UPLOAD-T2 | Endpoint POST /v1/artifacts (multipart form: file + json metadata) → map a command | CODE | `upload_handler.rs` |
| UPLOAD-T3 | Validar repo existe (usa `RepositoryStore`) antes de I/O | CODE | Lógica precondición |
| UPLOAD-T4 | Guardar binario S3 (`put_object`) | CODE | Uso adapter |
| UPLOAD-T5 | Persistir metadatos (`MongoArtifactRepository::save`) | CODE | Impl método |
| UPLOAD-T6 | Publicar evento `ArtifactUploadedEvent` [`event.rs`](crates/shared/src/domain/event.rs:25) | CODE | Publisher |
| UPLOAD-T7 | Propagar correlation-id a evento (header → metadata) | OBS | Campo extra |
| UPLOAD-T8 | Tests integración end-to-end (repo creado + upload) | TEST | `tests/upload_e2e.rs` |
| UPLOAD-T9 | Manejo errores: 422 validaciones, 404 repo, 500 storage | CODE | Mapeos Axum |

### 3.5 E-EVENTS
| ID | Descripción | Tipo | Output |
|----|-------------|------|--------|
| EVENTS-T1 | Kafka producer adapter implementa `ArtifactEventPublisher` [`ports.rs`](crates/artifact/src/application/ports.rs:17) | CODE | `kafka_event_publisher.rs` |
| EVENTS-T2 | Config topics (`artifact.uploaded`) + script creación | INFRA | `infra/kafka/topics.sh` |
| EVENTS-T3 | Serialización evento (JSON schema estable) | CODE | Serializer |
| EVENTS-T4 | Reintentos exponenciales (máx 3) + métricas | OBS | Lógica wrapper |
| EVENTS-T5 | Test publish mock (inyectar trait) | TEST | `tests/event_publish.rs` |

### 3.6 E-SEARCH (U8–U9)
| ID | Descripción | Tipo | Output |
|----|-------------|------|--------|
| SEARCH-T1 | Definir trait `SearchIndex` (si placeholder incompleto) [`mod.rs`](crates/search/src/application/mod.rs:2) | CODE | Trait completado |
| SEARCH-T2 | Implementación MongoSearchIndex (colección `search_index`) | CODE | `mongo_search_index.rs` |
| SEARCH-T3 | Consumer Kafka `artifact.uploaded` → genera documento y persiste | CODE | `artifact_uploaded_consumer.rs` |
| SEARCH-T4 | Implementar `handle_basic_search` (reemplazar `todo!`) [`basic_search.rs`](crates/search/src/features/basic_search.rs:35) | CODE | Función lista |
| SEARCH-T5 | Endpoint GET /v1/search?q= | CODE | `search_handler.rs` |
| SEARCH-T6 | Paginación (limit, offset) | CODE | Parámetros |
| SEARCH-T7 | Tests búsqueda (match parcial, paginado) | TEST | `tests/search_basic.rs` |
| SEARCH-T8 | Métricas queries / latencia | OBS | Instrumentación |

### 3.7 E-DOWNLOAD (U6)
| ID | Descripción | Tipo | Output |
|----|-------------|------|--------|
| DOWNLOAD-T1 | Crear slice `download_artifact` (estructura similar a upload) | CODE | Carpeta |
| DOWNLOAD-T2 | Query GetArtifact (metadata + stream/presigned) | CODE | `query.rs` |
| DOWNLOAD-T3 | Endpoint GET /v1/artifacts/{id}?presigned=bool | CODE | `download_handler.rs` |
| DOWNLOAD-T4 | Integrar S3 adapter: streaming o generar URL | CODE | Lógica |
| DOWNLOAD-T5 | Publicar `ArtifactDownloadRequestedEvent` [`event.rs`](crates/shared/src/domain/event.rs:44) | CODE | Publisher |
| DOWNLOAD-T6 | Tests (directo y presigned) | TEST | `tests/download.rs` |
| DOWNLOAD-T7 | Métricas bytes_transferred / downloads_total | OBS | Instrumentación |

### 3.8 E-ABAC (U7)
| ID | Descripción | Tipo | Output |
|----|-------------|------|--------|
| ABAC-T1 | Definir `PolicyEvaluator` trait | CODE | `policy_evaluator.rs` |
| ABAC-T2 | Adapter Cedar: carga políticas archivo (`policies/*.cedar`) | CODE | `cedar_policy_adapter.rs` |
| ABAC-T3 | Middleware Axum (PEP) para extraer atributos request | CODE | `auth_middleware.rs` |
| ABAC-T4 | Integrar en endpoints (upload, download, search) | CODE | Uso middleware |
| ABAC-T5 | Logging decisión (allow/deny) + métrica latency | OBS | Spans / counter |
| ABAC-T6 | Tests decisiones (deny sin policy, allow con policy) | TEST | `tests/abac.rs` |

### 3.9 E-VALIDATION (U11–U12)
| ID | Descripción | Tipo | Output |
|----|-------------|------|--------|
| VALID-T1 | Validar checksum (regex 64 hex) en `UploadArtifactCommand` | CODE | Validación |
| VALID-T2 | Validar size > 0 antes I/O | CODE | Guard clause |
| VALID-T3 | Índice único repo_id+checksum (creación definitiva) | INFRA | Script |
| VALID-T4 | Lógica idempotencia: si existe retornar artifact existente | CODE | Repositorio método `find_by_repo_checksum` |
| VALID-T5 | Tests duplicado retorna mismo id | TEST | `tests/upload_idempotency.rs` |

### 3.10 E-OBS (U13–U15)
| ID | Descripción | Tipo | Output |
|----|-------------|------|--------|
| OBS-T1 | Exponer `/metrics` (Prometheus exporter) | CODE | `metrics.rs` |
| OBS-T2 | Registrar counters/histogram definidos en [`plan.md`](docs/plan.md:199) | OBS | Instrumentación |
| OBS-T3 | Añadir spans `upload_handler` [`handler.rs`](crates/artifact/src/features/upload_artifact/handler.rs:7) → publish → consumer | OBS | Trazas |
| OBS-T4 | Añadir spans search y download | OBS | Trazas |
| OBS-T5 | Cache decisiones ABAC (TTL) (LruCache) | CODE | `decision_cache.rs` |
| OBS-T6 | Métrica cache hit/miss | OBS | Counter |
| OBS-T7 | Tests métricas expuestas (scrape parse) | TEST | `tests/metrics.rs` |

### 3.11 E-TEST-ORG (Organización y Hardening de Tests)
| ID | Descripción | Tipo | Output |
|----|-------------|------|--------|
| TEST-ORG1 | Crear guía `docs/testing-organization.md` consolidando principios sección 16 plan | DOC | `docs/testing-organization.md` |
| TEST-ORG2 | Extraer tests unitarios MongoArtifactRepository a `crates/artifact/tests/unit/unit_artifact_repository.rs` | TEST | Archivo test |
| TEST-ORG3 | Eliminar módulo tests interno y ajustar visibilidades (remover pubs innecesarios) | CODE | Refactor aplicado |
| TEST-ORG4 | Script verificación ausencia `#[cfg(test)]` en `src/**` (whitelist vacía) | OPS | `scripts/verify-no-inline-tests.sh` |
| TEST-ORG5 | Normalizar nombres archivos tests (`unit_`, `it_`, `e2e_`) existentes | TEST | Renombrados |

## 4. Matriz Dependencias Clave
- Upload depende de: Repos (existencia) + Infra (Mongo/S3) + OpenAPI.
- Search depende de: Eventos (publisher) + Consumer + Index.
- Download depende de: Infra S3 + Persistencia metadata + ABAC.
- ABAC se integra tras endpoints base funcionando.
- Observabilidad se instrumenta progresivamente (no bloquear ruta crítica).

## 5. Paralelización Recomendada
- Stream A: OPENAPI → REPO → UPLOAD
- Stream B (en paralelo a parte de UPLOAD): INFRA-PERSIST (adapters), EVENTS
- Stream C (tras eventos listos): SEARCH
- Stream D: DOWNLOAD (cuando infra + repos + upload listos)
- Stream E: ABAC (tras endpoints prototipo)
- Stream F: VALIDATION + OBS (hardening final)

## 6. CI/CD Tareas
| ID | Descripción | Tipo |
|----|-------------|------|
| CI-T1 | Job lint: cargo fmt + clippy (deny warnings) | OPS |
| CI-T2 | Job test unit + integración (etiquetas) | OPS |
| CI-T3 | Job contrato OpenAPI diff (`git diff` base vs generado) | OPS |
| CI-T4 | Pipeline stage publish imagen Docker (solo main) | OPS |
| CI-T5 | Security scan (cargo audit) | SEC |
| CI-T6 | Coverage report generación (sin gate inicial) | OPS |
| CI-T7 | Verificación script no tests embebidos (`scripts/verify-no-inline-tests.sh`) | OPS |

## 7. Riesgos por Tarea (Hotspots)
- EVENTS-T1/T4: Retrasos Kafka → fallback log; marcar TODO DLQ.
- VALID-T3: Migraciones índice en colección grande futuro → script idempotente.
- SEARCH-T3: Backpressure si consumo lento → configurable poll interval.
- ABAC-T2: Fallo carga políticas → default deny rápido.

## 8. Métricas Mapeadas a Tareas
| Métrica | Tareas que la exponen |
|---------|-----------------------|
| uploads_total | UPLOAD-T2/T8, OBS-T2 |
| upload_duration_seconds | UPLOAD-T2, OBS-T2 |
| downloads_total | DOWNLOAD-T3/T6, OBS-T2 |
| bytes_transferred | DOWNLOAD-T4, OBS-T2 |
| search_queries_total | SEARCH-T5/T7, OBS-T2 |
| search_query_duration_ms | SEARCH-T5, OBS-T2 |
| authz_decision_latency_ms | ABAC-T5, OBS-T2 |
| events_published_total | UPLOAD-T6, EVENTS-T4 |
| event_publish_failures_total | EVENTS-T4 |

## 9. DoD Extendida por Historia
- U1/U2/U3/U4: Tareas UPLOAD-T1..T9 completadas + EVENTS-T1..T3 + métricas básicas.
- U5: REPO-T1..T5 + Índice nombre único (INFRA-T4 parcial).
- U6: DOWNLOAD-T1..T6 + métricas (DOWNLOAD-T7).
- U7: ABAC-T1..T5 (cache diferido U13).
- U8/U9: SEARCH-T1..T7 + consumer funcionando + métrica queries.
- U10: OPENAPI-T1..T4 + CI-T3.
- U11/U12: VALID-T1..T5 + índice único definitivo.
- U14/U15: OBS-T1..T4 + trazas visibles + counters/histogramas.

## 10. Checklist Ejecución Sprint 0 (Secuencia)
1. OPENAPI-T1,T2
2. INFRA-T1,T5,T6
3. REPO-T1..T4
4. UPLOAD-T1..T5
5. EVENTS-T1..T3
6. UPLOAD-T6..T9
7. SEARCH-T1..T5
8. DOWNLOAD-T1..T5
9. ABAC-T1..T4
10. VALID-T1..T4
11. SEARCH-T6..T8
12. DOWNLOAD-T6,T7
13. ABAC-T5
14. OBS-T1..T4
15. VALID-T5
16. OBS-T5..T7
17. CI-T1..T5 (refinamiento final)

## 11. Tablero (Sug. Kanban Columnas)
Backlog → Ready → In Progress → Code Review → QA (Tests & Perf) → Done

## 12. Indicadores de Listo para Release (Gate)
- 100% historias P0 DoD cumplido.
- Latencias p95 (upload <100ms sin I/O remoto; download local <50ms).
- Cobertura tests integración slices críticos ≥ 70% líneas relevantes (excl infra generada).
- Sonar (o equivalente) sin code smells críticos.

## 13. Epic E-INTEGRATION-TESTS (Framework Docker Compose - Actualizado 2025-08-28)

### Estado del Framework de Testing
- **Framework Custom**: `shared-test` crate con orchestración Docker Compose robusta
- **TestEnvironment**: Struct centralizada con clientes preconfigurados (MongoDB, S3, Kafka, Cedar)
- **Auto-cleanup**: Drop trait garantiza limpieza automática de contenedores
- **Health checks**: Implementados para todos los servicios con timeouts

| ID | Descripción | Tipo | Output |
|----|-------------|------|--------|
| IT-T1 | Completar tests end-to-end missing (upload→index→search flow) | TEST | `it_complete_upload_flow.rs` |
| IT-T2 | Tests event-driven architecture (producer→consumer→side effects) | TEST | `it_kafka_event_flow.rs` |
| IT-T3 | Tests Maven full lifecycle con mvn CLI compatibility | TEST | `it_maven_full_lifecycle.rs` |
| IT-T4 | Tests npm full lifecycle con npm CLI compatibility | TEST | `it_npm_full_lifecycle.rs` |
| IT-T5 | Tests authorization integration en flujos completos | TEST | `it_authorization_integration.rs` |
| IT-T6 | Tests event ordering y failure recovery | TEST | `it_event_ordering.rs` |
| IT-T7 | Tests Tantivy indexing con datos reales | TEST | `it_tantivy_indexing.rs` |
| IT-T8 | Tests performance search engine | TEST | `it_search_performance.rs` |
| IT-T9 | Tests distribution format validation | TEST | `it_format_validation.rs` |
| IT-T10 | Setup test parallelization framework | INFRA | Configuración CI |

## 14. Epic E-SEARCH-COMPLETE (Implementaciones Pendientes)

### Estado Actual: Basic search funcional, Advanced search y Tantivy con TODO
| ID | Descripción | Tipo | Estado Actual |
|----|-------------|------|---------------|
| SEARCH-T9 | Implementar TantivySearchIndex completo (reemplazar todo!) | CODE | `tantivy_search.rs:21` |
| SEARCH-T10 | Advanced search con filtros y facets | CODE | `advanced_search.rs` placeholder |
| SEARCH-T11 | Index management API endpoints | CODE | `index_management.rs` estructura básica |
| SEARCH-T12 | Performance tuning y optimización queries | CODE | Métricas latencia |

## 15. Epic E-SUPPLY-CHAIN (Nueva Épica - DTOs listos)

### Estado Actual: DTOs definidos, handlers con todo!()
| ID | Descripción | Tipo | Output |
|----|-------------|------|--------|
| SUPPLY-T1 | Implementar generación SBOM (SPDX/CycloneDX) | CODE | `generate_sbom_handler` |
| SUPPLY-T2 | Vulnerability scanning integration | CODE | `get_vulnerability_report_handler` |
| SUPPLY-T3 | Supply chain health metrics | OBS | Métricas específicas |
| SUPPLY-T4 | Compliance reporting | CODE | Endpoints reporting |

## 16. Epic E-OPENAPI-UPDATE (Actualización Contratos)

### Estado Actual: OpenAPI v2.1.0 completo base, faltan nuevos endpoints
| ID | Descripción | Tipo | Output |
|----|-------------|------|--------|
| OPENAPI-T5 | Añadir search endpoints (basic/advanced) | CODE | `paths/search.yaml` update |
| OPENAPI-T6 | Añadir supply chain endpoints | CODE | `paths/supply-chain.yaml` |
| OPENAPI-T7 | Event webhooks API specification | CODE | `webhooks.yaml` update |
| OPENAPI-T8 | Metrics endpoint documentation | CODE | `paths/metrics.yaml` |
| OPENAPI-T9 | Health checks endpoints | CODE | `paths/health.yaml` |
| OPENAPI-T10 | Contract testing pipeline setup | OPS | CI job validación |

## 17. Matriz de Tests Existentes (20 archivos it_*.rs)

### Coverage por Bounded Context:
- **artifact**: 10 tests ✅ (upload, download, storage, events, idempotency)
- **iam**: 4 tests ✅ (authentication, users, policies, attachments)  
- **repository**: 2 tests ✅ (creation, store integration)
- **search**: 1 test ⚠️ (basic search only)
- **integration**: 5 tests ⚠️ (maven, npm, search, repository, error conditions)
- **distribution**: 0 tests ❌ (implementado pero sin integration tests)
- **supply-chain**: 0 tests ❌ (solo placeholders)
- **analytics**: 0 tests ❌ (no implementado)
- **security**: 0 tests ❌ (no implementado)

## 18. Acciones Post-MVP (Futuras Registradas)
- Extract advanced search (facets) – SEARCH-T10 implementa.
- SBOM ingestion pipelines – SUPPLY-T1 cubre.
- Vulnerability scanning + eventos security – SUPPLY-T2 cubre.
- Multi-formato almacenamiento (OCI layers, packages) – Future epic.
- BFF / GraphQL consolidación queries transversales – Future epic.

## 19. Orden de Ejecución Actualizado

### Fase 1: Completar Tests Base (Sprint 1-2)
1. IT-T1..T5 (tests end-to-end críticos)
2. SEARCH-T9 (Tantivy implementation)
3. IT-T6..T8 (event architecture tests)

### Fase 2: Feature Implementation (Sprint 3-4)
1. SEARCH-T10..T12 (advanced search completo)
2. SUPPLY-T1..T2 (supply chain básico)
3. IT-T9..T10 (performance y parallelization)

### Fase 3: Contracts & Ops (Sprint 5-6)
1. OPENAPI-T5..T10 (contratos actualizados)
2. SUPPLY-T3..T4 (compliance completo)
3. CI/CD hardening final

## 20. Resumen Rápido (TL;DR Actualizado)
Framework Docker Compose funcionando. Completar tests de integración faltantes, implementar Tantivy search engine, supply chain básico, actualizar OpenAPI, y configurar parallelization. Base sólida existente permite desarrollo incremental sin blockers críticos.

---

**Última actualización**: 28 agosto 2025 - Estado post-análisis código actual
**Framework de testing**: Docker Compose custom implementado y funcional
