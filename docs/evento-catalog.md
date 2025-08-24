# Catálogo Extenso de Eventos - Hodei Artifacts
Estado: Draft v0.1  
Fecha: 2025-08-24  
Metodología: Event-Driven Brainstorming + Domain Storming

## 1. Introducción y Filosofía Event-Driven

Este documento cataloga todos los eventos identificados mediante **Event Storming** y **Domain-Driven Brainstorming** para Hodei Artifacts. Cada evento representa un **hecho significativo** que ocurrió en el pasado y que puede desencadenar reacciones en otros componentes del sistema.

### 1.1 Principios de Diseño de Eventos
- **Event-Carried State Transfer**: Los eventos contienen toda la información necesaria para que los consumidores actúen sin consultas adicionales.
- **Inmutabilidad**: Los eventos son hechos históricos que nunca cambian.
- **Versionado**: Cada evento tiene versión para evolución sin romper compatibilidad.
- **Correlación**: Todos los eventos mantienen trazabilidad mediante correlationId.

### 1.2 Categorización de Eventos
| Categoría | Descripción | Patrón | Ejemplos |
|-----------|-------------|---------|----------|
| **Domain Events** | Cambios de estado en agregados | `{Aggregate}{Action}` | ArtifactUploaded |
| **Integration Events** | Comunicación entre bounded contexts | `{Context}{Event}` | SecurityScanCompleted |
| **System Events** | Eventos de infraestructura | `System{Event}` | SystemHealthChanged |
| **Audit Events** | Eventos de auditoría y cumplimiento | `{Action}Audited` | AccessAttemptAudited |

## 2. Eventos por Vertical Slice / Bounded Context

### 2.1 Artifact Ingest (Ingesta de Artefactos)

#### Eventos Principales
| Evento | Trigger | Payload Clave | Consumidores | Criticidad |
|--------|---------|---------------|--------------|------------|
| **ArtifactUploadStarted** | Cliente inicia upload | `artifactId`, `repositoryPath`, `expectedSize`, `uploader`, `correlationId` | Security, Monitoring | Medium |
| **ArtifactValidationFailed** | Validación metadata falla | `artifactId`, `validationErrors[]`, `uploader` | Monitoring, Audit | High |
| **ArtifactHashCalculated** | Hash SHA-256 completado | `artifactId`, `sha256`, `algorithm`, `calculationTimeMs` | Security, Integrity | High |
| **ArtifactUploaded** | Upload completado exitosamente | `artifactId`, `repository`, `name`, `version`, `sha256`, `sizeBytes`, `mediaType`, `uploader`, `uploadTimeMs` | Search, Security, Analytics | Critical |
| **ArtifactUploadFailed** | Upload falló | `artifactId`, `repository`, `name`, `version`, `errorCode`, `errorMessage`, `uploader` | Monitoring, Retry | High |
| **DuplicateArtifactDetected** | Intento upload duplicado | `artifactId`, `existingArtifactId`, `sha256`, `uploader` | Audit, Deduplication | Medium |
| **ArtifactStorageLocationChanged** | Cambio ubicación S3 | `artifactId`, `oldLocation`, `newLocation`, `reason` | Replication, Cache | Medium |
| **ArtifactMetadataEnriched** | Metadata adicional extraída | `artifactId`, `extractedMetadata{}`, `extractorType` | Search, Analytics | Low |

#### Eventos de Error y Recuperación
| Evento | Descripción | Payload |
|--------|-------------|---------|
| **ArtifactCorruptionDetected** | Hash no coincide | `artifactId`, `expectedSha256`, `actualSha256`, `detectionMethod` |
| **StorageQuotaExceeded** | Límite almacenamiento alcanzado | `repositoryId`, `currentUsage`, `quotaLimit`, `artifactId` |
| **ArtifactUploadRetried** | Reintento automático upload | `artifactId`, `retryAttempt`, `previousError`, `backoffMs` |

### 2.2 Artifact Retrieve (Recuperación de Artefactos)

#### Eventos de Acceso
| Evento | Trigger | Payload Clave | Consumidores |
|--------|---------|---------------|--------------|
| **ArtifactDownloadRequested** | Cliente solicita descarga | `artifactId`, `userId`, `userAgent`, `clientIP`, `requestedRange?` | AuthZ, Analytics |
| **ArtifactAccessGranted** | Autorización exitosa | `artifactId`, `userId`, `policyDecision`, `grantedPermissions[]` | Audit, Analytics |
| **ArtifactAccessDenied** | Acceso denegado | `artifactId`, `userId`, `denialReason`, `appliedPolicy`, `attemptDetails` | Security, Audit |
| **ArtifactDownloadStarted** | Descarga iniciada | `artifactId`, `userId`, `downloadMethod`, `presignedUrl?`, `correlationId` | Monitoring |
| **ArtifactDownloadCompleted** | Descarga completada | `artifactId`, `userId`, `bytesTransferred`, `downloadTimeMs`, `successful` | Analytics, Billing |
| **ArtifactDownloadFailed** | Descarga falló | `artifactId`, `userId`, `errorCode`, `errorMessage`, `bytesTransferred` | Monitoring, Support |
| **PresignedUrlGenerated** | URL temporal generada | `artifactId`, `userId`, `urlHash`, `expirationTime`, `permissions[]` | Security, Audit |
| **PresignedUrlExpired** | URL temporal expiró | `artifactId`, `urlHash`, `originalUserId`, `expiredAt` | Security, Cleanup |

#### Eventos de Uso y Analytics
| Evento | Descripción | Payload |
|--------|-------------|---------|
| **ArtifactPopularityChanged** | Cambio popularidad | `artifactId`, `newDownloadCount`, `popularityRank`, `timePeriod` |
| **ArtifactUsagePatternDetected** | Patrón uso detectado | `artifactId`, `patternType`, `frequency`, `userSegment` |
| **UnusualAccessPatternDetected** | Acceso sospechoso | `artifactId`, `userId`, `anomalyType`, `confidence`, `riskScore` |

### 2.3 Search & Index (Búsqueda e Indexación)

#### Eventos de Indexación
| Evento | Trigger | Payload Clave | Consumidores |
|--------|---------|---------------|--------------|
| **ArtifactIndexed** | Artefacto añadido al índice | `artifactId`, `indexedFields{}`, `indexingTimeMs`, `indexVersion` | Analytics |
| **IndexUpdateFailed** | Fallo actualización índice | `artifactId`, `errorCode`, `errorMessage`, `retryScheduled` | Monitoring |
| **BulkIndexingStarted** | Indexación masiva iniciada | `totalArtifacts`, `estimatedTimeMs`, `indexType`, `triggeredBy` | Monitoring |
| **BulkIndexingCompleted** | Indexación masiva completada | `processedArtifacts`, `successCount`, `failureCount`, `actualTimeMs` | Monitoring |
| **SearchIndexOptimized** | Índice optimizado | `optimizationType`, `performanceGain`, `timeMs` | Performance |

#### Eventos de Búsqueda
| Evento | Trigger | Payload Clave | Consumidores |
|--------|---------|---------------|--------------|
| **SearchQueryExecuted** | Usuario ejecuta búsqueda | `queryString`, `userId`, `resultCount`, `responseTimeMs`, `filters{}` | Analytics |
| **SearchQueryFailed** | Búsqueda falló | `queryString`, `userId`, `errorCode`, `errorMessage` | Monitoring |
| **SearchResultClicked** | Usuario hace clic en resultado | `queryString`, `artifactId`, `resultRank`, `userId` | Analytics, ML |
| **PopularSearchDetected** | Query popular identificada | `queryString`, `frequency`, `timePeriod`, `userCount` | Analytics |
| **SlowSearchDetected** | Búsqueda lenta detectada | `queryString`, `responseTimeMs`, `threshold`, `indexLoad` | Performance |

### 2.4 AuthZ ABAC (Autorización)

#### Eventos de Políticas
| Evento | Trigger | Payload Clave | Consumidores |
|--------|---------|---------------|--------------|
| **PolicyCreated** | Nueva política creada | `policyId`, `policyText`, `createdBy`, `scope[]`, `version` | Cache, Audit |
| **PolicyUpdated** | Política modificada | `policyId`, `oldVersion`, `newVersion`, `changes[]`, `updatedBy` | Cache, Audit |
| **PolicyDeleted** | Política eliminada | `policyId`, `deletedBy`, `deletionReason`, `affectedResources[]` | Cache, Audit |
| **PolicyActivated** | Política activada | `policyId`, `activatedBy`, `effectiveDate`, `scope[]` | Cache, Enforcement |
| **PolicyDeactivated** | Política desactivada | `policyId`, `deactivatedBy`, `reason`, `effectiveDate` | Cache, Enforcement |
| **PolicyConflictDetected** | Conflicto entre políticas | `conflictingPolicies[]`, `conflictType`, `affectedResources[]` | Governance |

#### Eventos de Decisiones de Acceso
| Evento | Trigger | Payload Clave | Consumidores |
|--------|---------|---------------|--------------|
| **AccessDecisionMade** | Decisión autorización tomada | `principal`, `action`, `resource`, `decision`, `appliedPolicies[]`, `evaluationTimeMs` | Audit, Performance |
| **AccessDecisionCached** | Decisión cacheada | `decisionKey`, `decision`, `ttl`, `cacheHit`, `evaluationTimeMs` | Performance |
| **AccessDecisionExpired** | Decisión cache expiró | `decisionKey`, `expiredAt`, `timesUsed` | Cache |
| **PolicyEvaluationFailed** | Error evaluando política | `policyId`, `principal`, `resource`, `errorCode`, `errorMessage` | Monitoring |
| **SuspiciousAccessAttempt** | Intento acceso sospechoso | `principal`, `resource`, `attemptDetails`, `riskScore`, `blockedBy` | Security |

#### Eventos de Usuarios y Grupos
| Evento | Trigger | Payload Clave | Consumidores |
|--------|---------|---------------|--------------|
| **UserCreated** | Usuario registrado | `userId`, `externalId`, `userType`, `attributes{}`, `createdBy` | AuthZ, Audit |
| **UserUpdated** | Usuario modificado | `userId`, `changedAttributes{}`, `updatedBy`, `reason` | AuthZ, Audit |
| **UserDeactivated** | Usuario desactivado | `userId`, `deactivatedBy`, `reason`, `lastActivity` | AuthZ, Cleanup |
| **GroupMembershipChanged** | Cambio membresía grupo | `groupId`, `userId`, `action`, `changedBy`, `effectiveDate` | AuthZ |
| **UserAttributesChanged** | Atributos usuario cambiaron | `userId`, `changedAttributes{}`, `source`, `changeReason` | AuthZ |

### 2.5 Repository Management (Gestión de Repositorios)

#### Eventos de Repositorios
| Evento | Trigger | Payload Clave | Consumidores |
|--------|---------|---------------|--------------|
| **RepositoryCreated** | Nuevo repositorio creado | `repositoryId`, `path`, `visibility`, `creator`, `configuration{}` | AuthZ, Analytics |
| **RepositoryUpdated** | Repositorio modificado | `repositoryId`, `changedFields{}`, `updatedBy`, `reason` | AuthZ, Search |
| **RepositoryDeleted** | Repositorio eliminado | `repositoryId`, `deletedBy`, `artifactCount`, `totalSize`, `backupLocation?` | Cleanup, Audit |
| **RepositoryArchived** | Repositorio archivado | `repositoryId`, `archivedBy`, `reason`, `retentionPolicy` | Archive, Cleanup |
| **RepositoryRestored** | Repositorio restaurado | `repositoryId`, `restoredBy`, `restoredFrom`, `reason` | Analytics |

#### Eventos de Retención y Limpieza
| Evento | Trigger | Payload Clave | Consumidores |
|--------|---------|---------------|--------------|
| **RetentionPolicyCreated** | Nueva política retención | `policyId`, `repositoryId`, `rules{}`, `createdBy` | Cleanup |
| **RetentionPolicyTriggered** | Política retención ejecutada | `policyId`, `repositoryId`, `candidateArtifacts[]`, `triggeredBy` | Cleanup |
| **ArtifactMarkedForDeletion** | Artefacto marcado borrar | `artifactId`, `deletionDate`, `retentionPolicy`, `reason` | Cleanup |
| **ArtifactPurged** | Artefacto eliminado físicamente | `artifactId`, `purgedBy`, `originalSize`, `purgeReason` | Analytics, Audit |
| **CleanupJobStarted** | Job limpieza iniciado | `jobId`, `repositoryId`, `jobType`, `estimatedArtifacts` | Monitoring |
| **CleanupJobCompleted** | Job limpieza completado | `jobId`, `processedArtifacts`, `deletedArtifacts`, `freedSpace`, `duration` | Monitoring |

#### Eventos de Cuotas y Límites
| Evento | Trigger | Payload Clave | Consumidores |
|--------|---------|---------------|--------------|
| **StorageQuotaSet** | Cuota almacenamiento establecida | `repositoryId`, `quotaBytes`, `setBy`, `effectiveDate` | Monitoring |
| **StorageQuotaWarning** | Advertencia cuota almacenamiento | `repositoryId`, `currentUsage`, `quotaLimit`, `warningThreshold` | Alerts |
| **StorageQuotaExceeded** | Cuota almacenamiento excedida | `repositoryId`, `currentUsage`, `quotaLimit`, `exceededBy` | Enforcement |
| **RepositoryStatsUpdated** | Estadísticas repo actualizadas | `repositoryId`, `artifactCount`, `totalSize`, `lastActivity` | Analytics |

### 2.6 Security Scan (Escaneo de Seguridad)

#### Eventos de Escaneo
| Evento | Trigger | Payload Clave | Consumidores |
|--------|---------|---------------|--------------|
| **SecurityScanQueued** | Escaneo encolado | `scanId`, `artifactId`, `scanType`, `priority`, `estimatedDuration` | Monitoring |
| **SecurityScanStarted** | Escaneo iniciado | `scanId`, `artifactId`, `scannerType`, `scannerVersion`, `startedBy` | Monitoring |
| **SecurityScanProgress** | Progreso escaneo | `scanId`, `artifactId`, `progressPercent`, `currentPhase`, `estimatedRemaining` | Monitoring |
| **SecurityScanCompleted** | Escaneo completado | `scanId`, `artifactId`, `status`, `vulnerabilityCount{}`, `reportUrl`, `duration` | Security, Search |
| **SecurityScanFailed** | Escaneo falló | `scanId`, `artifactId`, `errorCode`, `errorMessage`, `retryScheduled` | Monitoring |
| **VulnerabilityDetected** | Vulnerabilidad encontrada | `scanId`, `artifactId`, `vulnerability{}`, `severity`, `cveId?` | Security, Alerts |
| **CriticalVulnerabilityFound** | Vulnerabilidad crítica | `scanId`, `artifactId`, `vulnerability{}`, `cveId`, `cvssScore` | Security, Incident |

#### Eventos de SBOM
| Evento | Trigger | Payload Clave | Consumidores |
|--------|---------|---------------|--------------|
| **SBOMGenerationStarted** | Generación SBOM iniciada | `sbomId`, `artifactId`, `format`, `generatorType` | Monitoring |
| **SBOMGenerated** | SBOM generado exitosamente | `sbomId`, `artifactId`, `format`, `componentCount`, `sbomUrl` | Compliance, Search |
| **SBOMGenerationFailed** | Generación SBOM falló | `sbomId`, `artifactId`, `errorCode`, `errorMessage` | Monitoring |
| **SBOMValidated** | SBOM validado | `sbomId`, `validationResult`, `validatedBy`, `validationStandard` | Compliance |
| **ComponentLicenseDetected** | Licencia componente detectada | `sbomId`, `componentId`, `license`, `confidence` | Compliance |

#### Eventos de Firma y Verificación
| Evento | Trigger | Payload Clave | Consumidores |
|--------|---------|---------------|--------------|
| **ArtifactSigningStarted** | Firma artefacto iniciada | `artifactId`, `signingMethod`, `keyId`, `initiatedBy` | Security |
| **ArtifactSigned** | Artefacto firmado | `artifactId`, `signature`, `signingMethod`, `keyId`, `signedBy` | Verification, Audit |
| **ArtifactSigningFailed** | Firma falló | `artifactId`, `errorCode`, `errorMessage`, `keyId` | Security, Monitoring |
| **SignatureVerificationStarted** | Verificación firma iniciada | `artifactId`, `signature`, `verificationMethod` | Security |
| **SignatureVerified** | Firma verificada exitosamente | `artifactId`, `verificationResult`, `verifiedBy`, `trustLevel` | Security, Compliance |
| **SignatureVerificationFailed** | Verificación firma falló | `artifactId`, `errorCode`, `errorMessage`, `riskLevel` | Security, Incident |
| **TamperedArtifactDetected** | Artefacto alterado detectado | `artifactId`, `detectionMethod`, `tamperedAreas[]`, `riskLevel` | Security, Incident |

### 2.7 System & Infrastructure (Sistema e Infraestructura)

#### Eventos de Sistema
| Evento | Trigger | Payload Clave | Consumidores |
|--------|---------|---------------|--------------|
| **SystemStarted** | Sistema iniciado | `instanceId`, `version`, `startupTime`, `configuration{}` | Monitoring |
| **SystemShutdown** | Sistema deteniéndose | `instanceId`, `reason`, `gracefulShutdown`, `activeConnections` | Monitoring |
| **HealthCheckFailed** | Health check falló | `checkType`, `component`, `errorMessage`, `severity` | Alerts, Monitoring |
| **DatabaseConnectionLost** | Conexión BD perdida | `databaseType`, `connectionId`, `errorCode`, `retryAttempt` | Monitoring |
| **DatabaseConnectionRestored** | Conexión BD restaurada | `databaseType`, `connectionId`, `downTime`, `recovery` | Monitoring |
| **CacheEvictionOccurred** | Cache evicted | `cacheType`, `evictedKeys`, `reason`, `memoryPressure` | Performance |
| **BackupStarted** | Backup iniciado | `backupId`, `backupType`, `estimatedSize`, `destination` | Operations |
| **BackupCompleted** | Backup completado | `backupId`, `actualSize`, `duration`, `location`, `verification` | Operations |

#### Eventos de Performance
| Evento | Trigger | Payload Clave | Consumidores |
|--------|---------|---------------|--------------|
| **HighLatencyDetected** | Latencia alta detectada | `operation`, `latencyMs`, `threshold`, `frequency` | Performance, Alerts |
| **MemoryPressureHigh** | Presión memoria alta | `usagePercent`, `availableMemory`, `gcFrequency` | Monitoring |
| **CPUUsageHigh** | Uso CPU alto | `usagePercent`, `duration`, `topProcesses[]` | Monitoring |
| **DiskSpaceLow** | Espacio disco bajo | `filesystem`, `usagePercent`, `availableSpace` | Alerts |
| **ConnectionPoolExhausted** | Pool conexiones agotado | `poolType`, `maxConnections`, `activeConnections`, `waitingRequests` | Performance |

#### Eventos de Seguridad Infraestructura
| Evento | Trigger | Payload Clave | Consumidores |
|--------|---------|---------------|--------------|
| **SecurityIncidentDetected** | Incidente seguridad detectado | `incidentType`, `severity`, `affectedSystems[]`, `detectionMethod` | Security, Incident |
| **SuspiciousActivityDetected** | Actividad sospechosa | `activityType`, `source`, `details{}`, `riskScore` | Security |
| **AuthenticationFailure** | Fallo autenticación | `userId`, `authMethod`, `failureReason`, `sourceIP` | Security |
| **AuthorizationBypass** | Intento bypass autorización | `userId`, `attemptedResource`, `bypassMethod`, `blocked` | Security |
| **RateLimitExceeded** | Límite tasa excedido | `clientId`, `endpoint`, `requestCount`, `timeWindow` | Security |

## 3. Patrones Cross-Cutting de Eventos

### 3.1 Eventos de Auditoría (Audit Trail)
Todos los eventos críticos generan automáticamente eventos de auditoría:

| Patrón | Ejemplo | Payload Estándar |
|--------|---------|------------------|
| **{Action}Audited** | `ArtifactUploadAudited` | `action`, `resource`, `actor`, `timestamp`, `outcome`, `details{}` |
| **{Resource}AccessAudited** | `RepositoryAccessAudited` | `resource`, `accessor`, `accessType`, `granted`, `policies[]` |
| **{Change}Audited** | `PolicyChangeAudited` | `changeType`, `resource`, `oldValue`, `newValue`, `changedBy` |

### 3.2 Eventos de Correlación
| Evento Base | Eventos Relacionados | Correlation ID |
|-------------|---------------------|----------------|
| **ArtifactUploaded** | SecurityScanStarted, ArtifactIndexed, SBOMGenerated | `uploadCorrelationId` |
| **SecurityScanStarted** | VulnerabilityDetected, SecurityScanCompleted | `scanCorrelationId` |
| **UserCreated** | PolicyEvaluated, AccessGranted | `userLifecycleId` |

### 3.3 Eventos de Métricas y Observabilidad
| Evento | Tipo Métrica | Dimensiones |
|--------|--------------|-------------|
| **MetricThresholdExceeded** | Performance | `metric`, `value`, `threshold`, `component` |
| **SLAViolated** | SLA | `slaType`, `target`, `actual`, `violationDuration` |
| **AlertTriggered** | Alert | `alertRule`, `severity`, `affectedResources[]` |

### 3.4 Eventos de Business Intelligence
| Evento | Propósito | Payload |
|--------|-----------|---------|
| **UsagePatternIdentified** | Analytics | `patternType`, `confidence`, `timeframe`, `userSegment` |
| **TrendDetected** | BI | `trendType`, `metric`, `direction`, `significance` |
| **AnomalyDetected** | ML | `anomalyType`, `confidence`, `expectedValue`, `actualValue` |

## 4. Schema de Eventos (Estándar)

### 4.1 Envelope Estándar
```json
{
  "eventType": "ArtifactUploaded.v1",
  "eventId": "uuid",
  "correlationId": "uuid",
  "causationId": "uuid",
  "timestamp": "2025-08-24T10:30:00Z",
  "version": "1.0",
  "source": "hodei-artifacts.artifact-ingest",
  "data": { /* payload específico del evento */ },
  "metadata": {
    "traceId": "trace-uuid",
    "spanId": "span-uuid",
    "userId": "user-id",
    "tenantId": "tenant-id"
  }
}
```

### 4.2 Versionado de Eventos
| Versión | Cambio | Compatibilidad |
|---------|--------|----------------|
| **v1.0** | Schema inicial | N/A |
| **v1.1** | Campos opcionales añadidos | Backward compatible |
| **v2.0** | Campos obligatorios cambiados | Breaking change |

## 5. Event Sourcing Considerations

### 5.1 Eventos como Fuente de Verdad
- Todos los eventos se almacenan en **Event Store** (Kafka + MongoDB para replay).
- Los agregados se reconstruyen desde eventos.
- Snapshots para optimización de rendimiento.

### 5.2 Proyecciones y Read Models
| Proyección | Eventos Fuente | Propósito |
|------------|----------------|-----------|
| **ArtifactCatalog** | ArtifactUploaded, ArtifactIndexed | Búsqueda rápida |
| **SecurityDashboard** | SecurityScan*, Vulnerability* | Monitoreo seguridad |
| **UsageAnalytics** | *Downloaded, *Accessed | Business intelligence |
| **AuditTrail** | *Audited | Cumplimiento |

## 6. Estrategias de Procesamiento

### 6.1 Immediate vs Eventual Consistency
| Evento | Tipo Consistencia | Justificación |
|--------|------------------|---------------|
| **ArtifactUploaded** | Immediate | Crítico para integridad |
| **ArtifactIndexed** | Eventual | Optimización búsqueda |
| **SecurityScanCompleted** | Eventual | Enriquecimiento |

### 6.2 Error Handling
| Estrategia | Eventos | Acción |
|------------|---------|--------|
| **Retry** | Transient failures | 3 reintentos con backoff |
| **Dead Letter Queue** | Permanent failures | Manual intervention |
| **Compensation** | Business failures | Evento compensatorio |

## 7. Métricas de Eventos

### 7.1 KPIs por Categoría
| Categoría | Métrica | Objetivo |
|-----------|---------|----------|
| **Volumen** | Eventos/segundo | <1000/s |
| **Latencia** | Tiempo procesamiento | <100ms p95 |
| **Fiabilidad** | % eventos procesados | >99.9% |
| **Orden** | % eventos en orden | >99% |

### 7.2 Alertas Críticas
- **EventProcessingLag** > 5 segundos
- **EventProcessingFailureRate** > 1%
- **DuplicateEventDetected**
- **EventSchemaValidationFailed**

## 8. Roadmap de Implementación

### 8.1 Fase 1 (MVP)
- Eventos core: ArtifactUploaded, SecurityScanCompleted, AccessDecision
- Event bus básico (Kafka)
- Procesamiento síncrono

### 8.2 Fase 2 (Extensión)
- Eventos analytics y BI
- Event sourcing completo
- Procesamiento asíncrono avanzado

### 8.3 Fase 3 (Optimización)
- Event streaming real-time
- ML-driven event analysis
- Predictive events

---

**Total de Eventos Identificados: 120+**  
**Bounded Contexts Cubiertos: 7**  
**Patrones de Evento: 15+**

Este catálogo servirá como base para la implementación del sistema event-driven de Hodei Artifacts, garantizando una arquitectura robusta, observable y escalable.
