# Plan de Desarrollo - Sistema de Comunicación y Autorización Multi-capa

**Fecha de Análisis:** 2025-01-04  
**Estado del Proyecto:** Refactorización Completa (Commit df77c92)  
**Objetivo:** Implementar sistema de eventos y autorización multi-capa siguiendo AWS IAM + Organizations

---

## 📊 Estado Actual del Proyecto

### ✅ Componentes Ya Implementados

#### 1. Infraestructura Base
- ✅ **Workspace Multi-Crate**: Estructura modular con bounded contexts
- ✅ **Arquitectura VSA**: Vertical Slice Architecture por feature
- ✅ **Clean Architecture**: Separación domain/application/infrastructure
- ✅ **Unit of Work Pattern**: Implementado en `shared/src/application/ports/unit_of_work.rs`

#### 2. Bounded Context: hodei-authorizer
- ✅ **EvaluatePermissionsUseCase**: Caso de uso principal implementado
- ✅ **Integración IAM**: Usa `GetEffectivePoliciesForPrincipalUseCase` de hodei-iam
- ✅ **Integración SCPs**: Usa `GetEffectiveScpsUseCase` vía trait `GetEffectiveScpsPort`
- ✅ **Lógica Multi-capa**: Evalúa SCPs antes que IAM policies
- ⚠️ **Providers Legacy**: Existen `IamPolicyProvider` y `OrganizationBoundaryProvider` (deprecated)
- ✅ **DI Container**: Implementado con builder pattern
- ✅ **Aspectos Transversales**: Traits para Cache, Logger, Metrics

#### 3. Bounded Context: hodei-iam
- ✅ **GetEffectivePoliciesForPrincipalUseCase**: Implementado
- ✅ **Domain Model**: User, Group, Policy entities
- ✅ **SurrealIamPolicyProvider**: Implementación para hodei-authorizer (legacy)
- ✅ **Repositorios**: User, Group, Policy repositories

#### 4. Bounded Context: hodei-organizations
- ✅ **Features Implementadas**:
  - `attach_scp`: Con puertos segregados ✅
  - `create_account`: Estructura VSA completa
  - `create_ou`: Estructura VSA completa
  - `create_scp`: Estructura VSA completa
  - `get_effective_scps`: Retorna PolicySet de Cedar
  - `move_account`: Con UnitOfWork para atomicidad ✅
- ✅ **Domain Model**: Account, OrganizationalUnit, ServiceControlPolicy
- ✅ **Puertos Segregados**: Implementados en attach_scp

#### 5. Bounded Context: policies
- ✅ **Features Implementadas**:
  - `policy_analysis`: Análisis estático de políticas ✅
  - `create_policy`, `update_policy`, `delete_policy`
  - `get_policy`, `list_policies`
  - `validate_policy`
  - `policy_playground`: Simulación de evaluación
  - `batch_eval`: Evaluación en lote
- ✅ **AuthorizationEngine**: Motor de evaluación Cedar

#### 6. Sistema de Eventos (Parcial)
- ⚠️ **Estructuras Básicas**: `DomainEvent`, `Event`, `EventStream` en `shared/src/events.rs`
- ❌ **EventBus Traits**: NO implementados
- ❌ **InMemoryEventBus**: NO implementado
- ❌ **EventPublisher/Subscriber**: NO implementados
- ❌ **Event Handlers**: NO implementados

---

## 🎯 Análisis por Epic

### Epic 0: Infraestructura de Eventos de Dominio

**Estado:** 10% Completado

| HU | Historia | Estado | Prioridad | Estimación |
|----|----------|--------|-----------|------------|
| HU-0.1 | Definir abstracciones del bus de eventos | ⚠️ Parcial | 🔴 CRÍTICA | 4h |
| HU-0.2 | Implementar InMemoryEventBus con broadcast | ❌ Pendiente | 🔴 CRÍTICA | 8h |
| HU-0.3 | Implementar adaptador NATS | ❌ Pendiente | 🟡 Media | 16h |
| HU-0.4 | Configurar DI global del bus | ❌ Pendiente | 🔴 CRÍTICA | 4h |

**Análisis:**
- Existe `shared/src/events.rs` con estructuras básicas pero sin traits de bus
- El enum `DomainEvent` está vacío (comentado)
- No hay implementación de publisher/subscriber
- **Decisión:** Este Epic es PREREQUISITO para Epic 6 (hodei-configurations)

**Recomendación:** Implementar HU-0.1 y HU-0.2 primero (sprint 1)

---

### Epic 1: Refactorización y Alineamiento Arquitectónico

**Estado:** 60% Completado

| HU | Historia | Estado | Notas |
|----|----------|--------|-------|
| HU-1.1 | Definir puertos segregados attach_scp | ✅ HECHO | `attach_scp/ports.rs` completo |
| HU-1.2 | Implementar adaptadores attach_scp | ✅ HECHO | `attach_scp/adapter.rs` completo |
| HU-1.3 | Refactorizar AttachScpUseCase | ✅ HECHO | Tests pasando 100% |
| HU-1.4 | Garantizar atomicidad con UnitOfWork | ✅ HECHO | Implementado en move_account |
| HU-1.5 | Account con SCPs directas | ✅ HECHO | Campo `attached_scps` existe |
| HU-1.6 | Centralizar generación de HRN | ⚠️ Revisar | Verificar create_account |

**Análisis:**
- La mayoría de refactorizaciones están completas
- HU-1.6 requiere verificación: revisar si `CreateAccountCommand` tiene campo `hrn`

**Recomendación:** Validar HU-1.6 y cerrar Epic 1

---

### Epic 2: Motor de Autorización Central

**Estado:** 80% Completado

| HU | Historia | Estado | Notas |
|----|----------|--------|-------|
| HU-2.1 | Andamiaje hodei-authorizer | ✅ HECHO | Estructura VSA completa |
| HU-2.2 | Implementar IamPolicyProvider | ✅ HECHO | `SurrealIamPolicyProvider` existe |
| HU-2.3 | Lógica de decisión IAM | ✅ HECHO | `EvaluatePermissionsUseCase` funcional |

**Análisis:**
- El motor está operacional y evaluando políticas IAM
- Usa `GetEffectivePoliciesForPrincipalUseCase` correctamente
- Los tests pasan (11/11)
- **IMPORTANTE:** Existe código legacy (`IamPolicyProvider` trait) que debe eliminarse

**Recomendación:** Epic cerrado funcionalmente, limpiar código legacy

---

### Epic 3: Integrar Límites Organizacionales (SCPs)

**Estado:** 90% Completado

| HU | Historia | Estado | Notas |
|----|----------|--------|-------|
| HU-3.1 | Implementar OrganizationBoundaryProvider | ⚠️ Legacy | Existe pero deprecated |
| HU-3.2 | Integrar evaluación de SCPs | ✅ HECHO | Evalúa SCPs antes que IAM |

**Análisis:**
- La integración funcional está completa
- `EvaluatePermissionsUseCase` evalúa SCPs correctamente usando `GetEffectiveScpsUseCase`
- Existe `OrganizationBoundaryProvider` legacy que debe eliminarse
- La lógica de prioridad (SCP Deny > IAM) está implementada

**Recomendación:** Epic cerrado funcionalmente, limpiar código legacy

---

### Epic 4: Análisis Proactivo de Políticas

**Estado:** 90% Completado

| HU | Historia | Estado | Notas |
|----|----------|--------|-------|
| HU-4.1 | Crear endpoint /policies/analyze | ❌ Pendiente | Feature existe, falta REST API |
| HU-4.2 | Implementar reglas adicionales | ⚠️ Revisar | Ver qué reglas existen |

**Análisis:**
- El crate `policies` tiene feature `policy_analysis` completamente implementada
- Falta exponer vía REST API en `src/api_http`
- La lógica de negocio está lista para consumirse

**Recomendación:** Implementar HU-4.1 (sprint 2) - baja complejidad

---

### Epic 5: Auditoría y Trazabilidad

**Estado:** 0% Completado

| HU | Historia | Estado | Prioridad | Estimación |
|----|----------|--------|-----------|------------|
| HU-5.1 | Integrar AuditLogger en EvaluatePermissions | ❌ Pendiente | 🟠 Alta | 8h |
| HU-5.2 | Implementar SurrealAuditLogger | ❌ Pendiente | 🟠 Alta | 12h |

**Análisis:**
- No existe ningún trait `AuditLogger` en el código actual
- No hay sistema de auditoría implementado
- `EvaluatePermissionsUseCase` no registra decisiones
- Necesita:
  1. Definir trait `AuditLogger` en shared
  2. Definir modelo `AuditEvent`
  3. Implementar `SurrealAuditLogger`
  4. Inyectar en `EvaluatePermissionsUseCase`

**Recomendación:** Implementar en sprint 2-3 (después de Epic 0)

---

### Epic 6: Servicio de Auditoría de Configuración

**Estado:** 0% Completado

| HU | Historia | Estado | Prioridad | Estimación |
|----|----------|--------|-----------|------------|
| HU-6.1 | Instrumentar publicación de eventos | ❌ Pendiente | 🔴 CRÍTICA | 16h |
| HU-6.2 | Implementar registro de cambios | ❌ Pendiente | 🔴 CRÍTICA | 20h |
| HU-6.3 | Motor de evaluación de cumplimiento | ❌ Pendiente | 🟠 Alta | 24h |
| HU-6.4 | APIs de cumplimiento | ❌ Pendiente | 🟡 Media | 16h |

**Análisis:**
- No existe el crate `hodei-configurations`
- **DEPENDE COMPLETAMENTE de Epic 0** (infraestructura de eventos)
- Requiere un bounded context nuevo completo
- Es el epic más grande y complejo

**Recomendación:** Posponer hasta completar Epic 0 (sprint 4+)

---

## 📅 Plan de Implementación Propuesto

### Sprint 1: Fundamentos de Eventos (2 semanas)

**Objetivo:** Establecer infraestructura de comunicación asíncrona

#### Semana 1
- [ ] **HU-0.1**: Definir traits del bus de eventos (4h)
  - Crear `shared/src/application/ports/event_bus.rs`
  - Traits: `EventPublisher`, `EventSubscriber`, `EventHandler`
  - Actualizar `DomainEvent` enum
  
- [ ] **HU-0.2**: Implementar InMemoryEventBus (8h)
  - Crear `shared/src/infrastructure/in_memory_event_bus.rs`
  - Usar `tokio::sync::broadcast::channel`
  - Tests unitarios completos

#### Semana 2
- [ ] **HU-0.4**: Configurar DI global (4h)
  - Modificar `src/api_http/src/di_config.rs`
  - Registrar bus como singleton
  - Conectar subscribers

- [ ] **HU-1.6**: Validar centralización de HRN (2h)
  - Revisar `CreateAccountCommand`
  - Corregir si es necesario

- [ ] **Limpieza Legacy**: Eliminar providers deprecated (4h)
  - Eliminar `IamPolicyProvider` trait en hodei-authorizer/ports
  - Eliminar `OrganizationBoundaryProvider` trait
  - Eliminar archivos legacy en hodei-iam

**Entregables:**
- ✅ Sistema de eventos funcional in-memory
- ✅ Tests: 100% cobertura del bus de eventos
- ✅ Documentación: Guía de uso del bus de eventos

---

### Sprint 2: Auditoría y API de Análisis (2 semanas)

**Objetivo:** Añadir trazabilidad y exponer análisis de políticas

#### Semana 1
- [ ] **HU-5.1**: Crear sistema de auditoría (8h)
  - Definir trait `AuditLogger` en shared
  - Definir modelo `AuditEvent`
  - Integrar en `EvaluatePermissionsUseCase`

- [ ] **HU-5.2**: Implementar persistencia (12h)
  - Crear `SurrealAuditLogger`
  - Schema de tabla `audit_log`
  - Tests de integración

#### Semana 2
- [ ] **HU-4.1**: Endpoint de análisis de políticas (6h)
  - Crear `src/api_http/src/api/policies/handlers.rs`
  - Endpoint POST `/policies/analyze`
  - Tests de integración

- [ ] **HU-4.2**: Reglas de análisis adicionales (6h)
  - Implementar detección de wildcards
  - Implementar detección de permisos amplios
  - Documentar reglas

**Entregables:**
- ✅ Sistema de auditoría completo y persistente
- ✅ API REST para análisis de políticas
- ✅ Dashboard de cumplimiento (frontend básico)

---

### Sprint 3: Eventos de Dominio en Crates Existentes (2 semanas)

**Objetivo:** Instrumentar hodei-iam y hodei-organizations para publicar eventos

#### Semana 1
- [ ] **HU-6.1 (Parte 1)**: Instrumentar hodei-iam (8h)
  - Inyectar `EventPublisher` en todos los casos de uso de escritura
  - Definir `IamEvent` variants
  - Publicar eventos en: CreateUser, UpdateUser, DeleteUser, etc.

#### Semana 2
- [ ] **HU-6.1 (Parte 2)**: Instrumentar hodei-organizations (8h)
  - Inyectar `EventPublisher` en casos de uso de escritura
  - Definir `OrganizationEvent` variants
  - Publicar eventos en: CreateAccount, AttachScp, MoveAccount, etc.

- [ ] **Testing End-to-End**: Validar flujo de eventos (8h)
  - Tests que verifican publicación
  - Tests que verifican suscripción
  - Tests de integración completos

**Entregables:**
- ✅ hodei-iam publicando eventos
- ✅ hodei-organizations publicando eventos
- ✅ Tests E2E del flujo de eventos

---

### Sprint 4: Servicio de Configuración (Fase 1) (3 semanas)

**Objetivo:** Crear bounded context hodei-configurations con registro de cambios

#### Semana 1
- [ ] **Andamiaje del Crate** (8h)
  - Crear `crates/hodei-configurations`
  - Estructura VSA base
  - Domain model: `ConfigurationItem`, `ComplianceRule`

- [ ] **HU-6.2 (Parte 1)**: Feature record_configuration_change (12h)
  - Caso de uso para registrar cambios
  - Event handlers para consumir eventos
  - Repositorio de ConfigurationItem

#### Semana 2-3
- [ ] **HU-6.2 (Parte 2)**: Completar registro (16h)
  - Versionado de configuraciones
  - Schema de base de datos
  - Tests unitarios e integración

**Entregables:**
- ✅ Crate hodei-configurations funcional
- ✅ Registro de cambios de configuración
- ✅ API REST básica de consulta

---

### Sprint 5: Servicio de Configuración (Fase 2) (3 semanas)

**Objetivo:** Motor de evaluación de cumplimiento

#### Semana 1-2
- [ ] **HU-6.3**: Motor de cumplimiento (24h)
  - Feature evaluate_compliance
  - Integración con Cedar para evaluar reglas
  - Lógica de evaluación de ConfigurationItem

#### Semana 3
- [ ] **HU-6.4**: APIs de cumplimiento (16h)
  - Feature create_rule, list_rules
  - Feature get_compliance_details_for_resource
  - Dashboard de cumplimiento
  - Documentación completa

**Entregables:**
- ✅ Motor de cumplimiento completo
- ✅ APIs de gestión de reglas
- ✅ Dashboard de cumplimiento funcional

---

### Sprint 6: Producción y Escalabilidad (2 semanas)

**Objetivo:** Preparar para producción

#### Semana 1
- [ ] **HU-0.3**: Adaptador NATS (16h)
  - Crear `crates/event-bus-nats`
  - Implementar traits del bus
  - Tests de integración con NATS

#### Semana 2
- [ ] **Optimización y Hardening** (16h)
  - Performance testing
  - Optimización de queries
  - Documentación de deployment
  - Guías de operación

**Entregables:**
- ✅ Sistema listo para producción
- ✅ Documentación completa de deployment
- ✅ Guías de troubleshooting

---

## 🎯 Priorización y Dependencias

### Dependencias Críticas

```
Epic 0 (Eventos) ──┐
                   ├──> Epic 6 (Configuración)
Epic 2 (Autorización) ──┘

Epic 5 (Auditoría) ──> Epic 2 (requiere EvaluatePermissions)

Epic 4 (Análisis) ──> (Independiente, baja prioridad)
```

### Prioridad Recomendada

1. 🔴 **Sprint 1**: Epic 0 - Infraestructura de Eventos (CRÍTICO)
2. 🟠 **Sprint 2**: Epic 5 - Auditoría + Epic 4 - Análisis API
3. 🟠 **Sprint 3**: Epic 6 (Parte 1) - Instrumentación de Eventos
4. 🟡 **Sprint 4-5**: Epic 6 (Parte 2-3) - Servicio de Configuración
5. 🟢 **Sprint 6**: Epic 0 (NATS) - Producción

---

## 📊 Métricas de Progreso

### Estado Global por Epic

| Epic | Progreso | Riesgo | Prioridad | Esfuerzo Restante |
|------|----------|--------|-----------|-------------------|
| Epic 0 | 10% | 🔴 Alto | CRÍTICA | 32h |
| Epic 1 | 90% | 🟢 Bajo | Alta | 2h |
| Epic 2 | 80% | 🟢 Bajo | Media | 4h |
| Epic 3 | 90% | 🟢 Bajo | Media | 4h |
| Epic 4 | 90% | 🟢 Bajo | Baja | 12h |
| Epic 5 | 0% | 🟠 Medio | Alta | 20h |
| Epic 6 | 0% | 🔴 Alto | CRÍTICA | 76h |

**Total Esfuerzo Restante:** ~150 horas (6 sprints de 2 semanas)

---

## ⚠️ Riesgos Identificados

### Riesgo 1: Complejidad del Sistema de Eventos
- **Probabilidad:** Media
- **Impacto:** Alto
- **Mitigación:** 
  - Implementar InMemoryEventBus primero (sin NATS)
  - Tests exhaustivos antes de NATS
  - Documentación clara de contratos

### Riesgo 2: Performance de Evaluación Multi-capa
- **Probabilidad:** Media
- **Impacto:** Alto
- **Mitigación:**
  - Implementar cache agresivo
  - Benchmarks desde Sprint 2
  - Optimización de queries SurrealDB

### Riesgo 3: Integración NATS en Producción
- **Probabilidad:** Baja
- **Impacto:** Alto
- **Mitigación:**
  - Posponer a Sprint 6
  - Abstraer bien los traits
  - Plan B: usar InMemoryEventBus + polling

---

## 📝 Notas Importantes

### Código Legacy a Eliminar

1. **hodei-authorizer/src/ports.rs** - Eliminar completamente
2. **hodei-authorizer/src/features/evaluate_permissions/ports.rs**:
   - Eliminar trait `IamPolicyProvider`
   - Eliminar trait `OrganizationBoundaryProvider`
3. **hodei-iam/src/shared/infrastructure/surreal/iam_policy_provider.rs** - Eliminar
4. **hodei-authorizer/src/authorizer.rs** - Eliminar (usa providers legacy)

### Testing

- **Cobertura Actual:** 67 tests, 100% pasando
- **Objetivo Sprint 1:** 90+ tests
- **Objetivo Final:** 200+ tests con cobertura >80%

### Documentación

Crear en cada sprint:
- ✅ README de cada nueva feature
- ✅ Diagramas de arquitectura actualizados
- ✅ Guías de uso para desarrolladores
- ✅ ADRs (Architecture Decision Records)

---

## 🚀 Recomendación Ejecutiva

### Para Comenzar Inmediatamente

1. **Sprint 1 - Semana 1** (Próximos 5 días):
   - Implementar HU-0.1 y HU-0.2 (sistema de eventos in-memory)
   - 1 desarrollador full-time
   - Entregable: Bus de eventos funcional con tests

2. **Validación Rápida** (1 día):
   - HU-1.6: Verificar centralización de HRN
   - Limpiar código legacy (providers)

3. **Documentación de Arquitectura** (2 días):
   - Actualizar diagramas con sistema de eventos
   - Documentar flujos de comunicación
   - Crear ADR para decisiones de diseño

### Criterios de Éxito del Proyecto

- ✅ 100% de tests pasando en cada sprint
- ✅ Sistema de eventos robusto y testeable
- ✅ Trazabilidad completa de decisiones de autorización
- ✅ Servicio de configuración operacional
- ✅ APIs documentadas y versionadas
- ✅ Performance: <100ms evaluación de permisos p95
- ✅ Escalabilidad: Soportar 1000 req/s

---

**Próximo Paso:** Comenzar HU-0.1 - Definir abstracciones del bus de eventos