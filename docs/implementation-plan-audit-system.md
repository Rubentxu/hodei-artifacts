# Plan de Implementación: Sistema de Auditoría con Eventos (CloudWatch-like)

## 📋 Visión General

Implementar un sistema completo de auditoría basado en eventos de dominio que capture todas las operaciones críticas del sistema, similar a AWS CloudTrail/CloudWatch Logs, permitiendo trazabilidad, debugging y compliance.

---

## 🎯 Objetivos

1. **Instrumentar todos los casos de uso críticos** con publicación de eventos de dominio
2. **Crear un EventHandler de auditoría** que almacene eventos para consulta posterior
3. **Implementar APIs de consulta** para acceder al historial de eventos
4. **Garantizar calidad** con tests comprehensivos en verde

---

## 📦 Fases de Implementación

### **Fase 1: Instrumentar Casos de Uso de IAM** (30 min)

#### 1.1 CreateGroupUseCase
- **Archivo:** `crates/hodei-iam/src/features/create_group/use_case.rs`
- **Evento:** `GroupCreated`
- **Cambios:**
  - Añadir campo `event_publisher: Option<Arc<InMemoryEventBus>>`
  - Método `with_event_publisher()`
  - Publicar evento después de `repo.save()`
  - Manejar errores de publicación con warnings

#### 1.2 AddUserToGroupUseCase
- **Archivo:** `crates/hodei-iam/src/features/add_user_to_group/use_case.rs`
- **Evento:** `UserAddedToGroup`
- **Cambios similares a 1.1**

#### 1.3 Actualizar Módulos DI
- `crates/hodei-iam/src/features/create_group/di.rs`
- `crates/hodei-iam/src/features/add_user_to_group/di.rs`
- Añadir función `make_use_case_with_events()`

#### 1.4 Actualizar build_app_state
- `src/lib.rs`
- Inyectar event_bus en los nuevos casos de uso

---

### **Fase 2: Eventos de Dominio para Organizations** (20 min)

#### 2.1 Crear archivo de eventos
- **Archivo:** `crates/hodei-organizations/src/shared/domain/events.rs`
- **Eventos a definir:**
  ```rust
  - AccountCreated { account_hrn, name, parent_hrn, created_at }
  - AccountMoved { account_hrn, from_ou_hrn, to_ou_hrn, moved_at }
  - ScpAttached { scp_hrn, target_hrn, target_type, attached_at }
  - ScpDetached { scp_hrn, target_hrn, target_type, detached_at }
  - OrganizationalUnitCreated { ou_hrn, name, parent_hrn, created_at }
  - OrganizationalUnitDeleted { ou_hrn, deleted_at }
  ```

#### 2.2 Implementar trait DomainEvent
- Cada evento implementa `DomainEvent`
- `event_type()` con nomenclatura `organizations.{entity}.{action}`
- `aggregate_id()` retorna el HRN del recurso principal

#### 2.3 Exportar desde mod.rs
- `crates/hodei-organizations/src/shared/domain/mod.rs`
- `pub mod events;`
- `pub use events::*;`

---

### **Fase 3: Instrumentar Casos de Uso de Organizations** (40 min)

#### 3.1 CreateAccountUseCase
- **Archivo:** `crates/hodei-organizations/src/features/create_account/use_case.rs`
- **Evento:** `AccountCreated`
- Añadir event_publisher
- Publicar después de persist

#### 3.2 AttachScpUseCase
- **Archivo:** `crates/hodei-organizations/src/features/attach_scp/use_case.rs`
- **Evento:** `ScpAttached`
- Determinar target_type (Account vs OU)
- Publicar evento con contexto completo

#### 3.3 MoveAccountUseCase
- **Archivo:** `crates/hodei-organizations/src/features/move_account/use_case.rs`
- **Evento:** `AccountMoved`
- Capturar from_ou y to_ou
- Publicar dentro de la transacción (después del commit)

#### 3.4 Actualizar Módulos DI
- Actualizar cada `di.rs` con función `_with_events`
- Actualizar `build_app_state()` en `src/lib.rs`

---

### **Fase 4: Sistema de Auditoría (AuditEventHandler)** (60 min)

#### 4.1 Estructura de Datos
**Archivo:** `crates/shared/src/infrastructure/audit/mod.rs`

```rust
pub struct AuditLog {
    pub id: uuid::Uuid,
    pub event_type: String,
    pub aggregate_id: Option<String>,
    pub aggregate_type: Option<String>,
    pub event_data: serde_json::Value,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub correlation_id: Option<String>,
    pub causation_id: Option<String>,
    pub metadata: std::collections::HashMap<String, String>,
}

pub struct AuditLogStore {
    logs: Arc<RwLock<Vec<AuditLog>>>,
}
```

#### 4.2 AuditEventHandler
**Archivo:** `crates/shared/src/infrastructure/audit/handler.rs`

```rust
pub struct AuditEventHandler {
    store: Arc<AuditLogStore>,
}

impl<E: DomainEvent> EventHandler<E> for AuditEventHandler {
    fn name(&self) -> &'static str {
        "AuditEventHandler"
    }
    
    async fn handle(&self, envelope: EventEnvelope<E>) -> anyhow::Result<()> {
        // Convertir evento a AuditLog
        // Almacenar en el store
        // Log con tracing::info
    }
}
```

#### 4.3 API de Consulta
**Archivo:** `crates/shared/src/infrastructure/audit/query.rs`

```rust
pub struct AuditQuery {
    pub event_type: Option<String>,
    pub aggregate_id: Option<String>,
    pub from_date: Option<chrono::DateTime<chrono::Utc>>,
    pub to_date: Option<chrono::DateTime<chrono::Utc>>,
    pub limit: usize,
}

impl AuditLogStore {
    pub async fn query(&self, query: AuditQuery) -> Vec<AuditLog>;
    pub async fn count(&self, query: AuditQuery) -> usize;
    pub async fn get_by_id(&self, id: uuid::Uuid) -> Option<AuditLog>;
}
```

#### 4.4 Integración en AppState
- Añadir `audit_store: Arc<AuditLogStore>` al `AppState`
- Crear y suscribir `AuditEventHandler` en `build_app_state()`
- Suscribirse a TODOS los eventos de dominio

---

### **Fase 5: API REST para Auditoría** (30 min)

#### 5.1 Handlers HTTP
**Archivo:** `src/api/audit/handlers.rs`

```rust
// GET /api/v1/audit/logs?event_type=...&from=...&to=...&limit=...
pub async fn list_audit_logs(
    State(state): State<Arc<AppState>>,
    Query(params): Query<AuditQueryParams>,
) -> Result<Json<AuditLogsResponse>, AppError>

// GET /api/v1/audit/logs/:id
pub async fn get_audit_log(
    State(state): State<Arc<AppState>>,
    Path(id): Path<uuid::Uuid>,
) -> Result<Json<AuditLog>, AppError>

// GET /api/v1/audit/stats
pub async fn get_audit_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AuditStatsResponse>, AppError>
```

#### 5.2 Registrar Rutas
**Archivo:** `src/api/mod.rs`
- Añadir módulo `pub mod audit;`
- Crear función `audit_routes()`

**Archivo:** `src/lib.rs`
- Añadir `.nest("/audit", api::audit_routes())` en el router

---

### **Fase 6: Tests Comprehensivos** (40 min)

#### 6.1 Tests Unitarios de Use Cases
- **Para cada caso de uso instrumentado:**
  - Test que verifica que el evento se publica
  - Test que verifica el contenido del evento
  - Test que verifica que el caso de uso no falla si el event bus falla
  - Mock del EventPublisher

#### 6.2 Tests del AuditEventHandler
**Archivo:** `crates/shared/src/infrastructure/audit/handler_test.rs`
- Test de almacenamiento correcto
- Test de múltiples eventos concurrentes
- Test de serialización/deserialización

#### 6.3 Tests del AuditLogStore (Query)
**Archivo:** `crates/shared/src/infrastructure/audit/query_test.rs`
- Test de filtrado por event_type
- Test de filtrado por rango de fechas
- Test de filtrado por aggregate_id
- Test de limit
- Test de queries combinadas

#### 6.4 Tests de Integración End-to-End
**Archivo:** `tests/audit_integration_test.rs`
- Crear un usuario → Verificar evento en audit log
- Crear un grupo y añadir usuario → Verificar ambos eventos
- Crear cuenta en organization → Verificar evento
- Consultar logs vía API REST → Verificar respuesta

---

## 📊 Estructura de Archivos Resultante

```
hodei-artifacts/
├── crates/
│   ├── hodei-iam/
│   │   ├── src/
│   │   │   ├── features/
│   │   │   │   ├── create_group/
│   │   │   │   │   ├── use_case.rs       [MODIFICADO]
│   │   │   │   │   └── di.rs              [MODIFICADO]
│   │   │   │   └── add_user_to_group/
│   │   │   │       ├── use_case.rs       [MODIFICADO]
│   │   │   │       └── di.rs              [MODIFICADO]
│   │   │   └── shared/
│   │   │       └── domain/
│   │   │           └── events.rs          [EXISTENTE]
│   │
│   ├── hodei-organizations/
│   │   ├── src/
│   │   │   ├── features/
│   │   │   │   ├── create_account/
│   │   │   │   │   ├── use_case.rs       [MODIFICADO]
│   │   │   │   │   └── di.rs              [MODIFICADO]
│   │   │   │   ├── attach_scp/
│   │   │   │   │   ├── use_case.rs       [MODIFICADO]
│   │   │   │   │   └── di.rs              [MODIFICADO]
│   │   │   │   └── move_account/
│   │   │   │       ├── use_case.rs       [MODIFICADO]
│   │   │   │       └── di.rs              [MODIFICADO]
│   │   │   └── shared/
│   │   │       └── domain/
│   │   │           ├── events.rs          [NUEVO]
│   │   │           └── mod.rs             [MODIFICADO]
│   │
│   └── shared/
│       └── src/
│           └── infrastructure/
│               └── audit/
│                   ├── mod.rs             [NUEVO]
│                   ├── handler.rs         [NUEVO]
│                   ├── handler_test.rs    [NUEVO]
│                   ├── query.rs           [NUEVO]
│                   └── query_test.rs      [NUEVO]
│
├── src/
│   ├── api/
│   │   ├── audit/
│   │   │   ├── mod.rs                     [NUEVO]
│   │   │   └── handlers.rs                [NUEVO]
│   │   └── mod.rs                         [MODIFICADO]
│   ├── app_state.rs                       [MODIFICADO]
│   └── lib.rs                             [MODIFICADO]
│
└── tests/
    └── audit_integration_test.rs          [NUEVO]
```

---

## ✅ Checklist de Implementación

### Fase 1: IAM Use Cases
- [ ] Instrumentar CreateGroupUseCase
- [ ] Instrumentar AddUserToGroupUseCase
- [ ] Actualizar DI de create_group
- [ ] Actualizar DI de add_user_to_group
- [ ] Actualizar build_app_state con event injection

### Fase 2: Events de Organizations
- [ ] Crear events.rs en organizations
- [ ] Definir AccountCreated
- [ ] Definir AccountMoved
- [ ] Definir ScpAttached
- [ ] Definir ScpDetached
- [ ] Definir OrganizationalUnitCreated
- [ ] Definir OrganizationalUnitDeleted
- [ ] Exportar desde mod.rs

### Fase 3: Organizations Use Cases
- [ ] Instrumentar CreateAccountUseCase
- [ ] Instrumentar AttachScpUseCase
- [ ] Instrumentar MoveAccountUseCase
- [ ] Actualizar DI correspondientes
- [ ] Actualizar build_app_state

### Fase 4: Sistema de Auditoría
- [ ] Crear estructura AuditLog
- [ ] Crear AuditLogStore
- [ ] Implementar AuditEventHandler genérico
- [ ] Implementar queries (filter, limit, count)
- [ ] Integrar en AppState
- [ ] Suscribir handler a todos los eventos

### Fase 5: API REST
- [ ] Crear handlers para list_audit_logs
- [ ] Crear handler para get_audit_log
- [ ] Crear handler para get_audit_stats
- [ ] Registrar rutas en router
- [ ] Documentar con utoipa

### Fase 6: Tests
- [ ] Tests unitarios de CreateGroupUseCase
- [ ] Tests unitarios de AddUserToGroupUseCase
- [ ] Tests unitarios de CreateAccountUseCase
- [ ] Tests unitarios de AttachScpUseCase
- [ ] Tests unitarios de MoveAccountUseCase
- [ ] Tests de AuditEventHandler
- [ ] Tests de AuditLogStore queries
- [ ] Test de integración end-to-end
- [ ] Verificar que todos los tests pasan (cargo test)

---

## 🎯 Criterios de Éxito

1. ✅ Todos los casos de uso críticos publican eventos
2. ✅ AuditEventHandler captura TODOS los eventos
3. ✅ API REST permite consultar eventos con filtros
4. ✅ Tests unitarios cubren >80% del código nuevo
5. ✅ Test de integración verifica flujo end-to-end
6. ✅ `cargo test` pasa sin errores
7. ✅ `cargo clippy` sin warnings críticos
8. ✅ Documentación actualizada

---

## 📈 Métricas Esperadas

- **Eventos de Dominio Totales:** 16 (10 IAM + 6 Organizations)
- **Casos de Uso Instrumentados:** 6
- **Líneas de Código Nuevas:** ~800-1000
- **Tests Nuevos:** ~15-20
- **Endpoints REST Nuevos:** 3

---

## 🔄 Orden de Ejecución

1. **Fase 1** → Instrumentar IAM (base ya hecha con CreateUser)
2. **Fase 2** → Crear eventos Organizations
3. **Fase 3** → Instrumentar Organizations
4. **Fase 4** → Implementar sistema de auditoría (core)
5. **Fase 6** → Tests (paralelo con Fase 5)
6. **Fase 5** → API REST (última para poder testear manualmente)

---

## 🚀 Inicio de Implementación

**Comando inicial:**
```bash
# Verificar estado limpio
cargo check --all-targets
cargo test

# Crear branches
git checkout -b feature/audit-system-events
```

**Tiempo estimado total:** 3-4 horas de desarrollo enfocado

---

**Última actualización:** 2024-01-XX  
**Estado:** 📋 Plan Aprobado - Listo para Implementación