# Guía de Uso: Sistema de Auditoría de Eventos

## 📚 Introducción

Este documento proporciona una guía completa para usar el sistema de auditoría de eventos implementado en Hodei Artifacts. El sistema captura automáticamente todos los eventos de dominio para trazabilidad, debugging y compliance.

---

## 🎯 Características Principales

- ✅ **Captura automática** de todos los eventos de dominio
- ✅ **Consultas avanzadas** con múltiples filtros
- ✅ **Estadísticas agregadas** tipo CloudWatch
- ✅ **Thread-safe** y listo para concurrencia
- ✅ **Extensible** para nuevos tipos de eventos

---

## 🚀 Inicio Rápido

### 1. Acceso al AuditLogStore

El `AuditLogStore` está disponible en el `AppState` de la aplicación:

```rust
use crate::app_state::AppState;
use std::sync::Arc;

async fn example_handler(State(state): State<Arc<AppState>>) {
    let audit_store = &state.audit_store;
    
    // Ahora puedes usar el audit_store
    let all_logs = audit_store.all().await;
}
```

### 2. Consultar Todos los Eventos

```rust
// Obtener todos los eventos de auditoría
let all_logs = audit_store.all().await;

for log in all_logs {
    println!("Event: {} at {}", log.event_type, log.occurred_at);
}
```

### 3. Contar Eventos

```rust
let total = audit_store.count_all().await;
println!("Total audit events: {}", total);
```

---

## 🔍 Consultas Avanzadas

### Filtrar por Tipo de Evento

```rust
use shared::infrastructure::audit::query::AuditQuery;

// Buscar todos los eventos de creación de usuarios
let query = AuditQuery::new()
    .with_event_type("iam.user.created");

let results = audit_store.query(query).await;
```

### Filtrar por Aggregate ID

```rust
// Ver todos los eventos relacionados con un usuario específico
let query = AuditQuery::new()
    .with_aggregate_id("hrn:hodei:iam:default:user/user-123");

let user_history = audit_store.query(query).await;

println!("Usuario tiene {} eventos en su historial", user_history.len());
```

### Filtrar por Tipo de Agregado

```rust
// Ver todos los eventos de tipo "User"
let query = AuditQuery::new()
    .with_aggregate_type("User");

let user_events = audit_store.query(query).await;
```

### Filtrar por Rango de Fechas

```rust
use chrono::{Duration, Utc};

// Eventos de las últimas 24 horas
let now = Utc::now();
let yesterday = now - Duration::hours(24);

let query = AuditQuery::new()
    .with_date_range(yesterday, now);

let recent_events = audit_store.query(query).await;
```

### Consultas Combinadas

```rust
// Todos los eventos de creación de usuarios en las últimas 24 horas
let query = AuditQuery::new()
    .with_event_type("iam.user.created")
    .with_date_range(yesterday, now)
    .with_limit(100);

let results = audit_store.query(query).await;
```

### Paginación

```rust
// Primera página (50 resultados)
let page1 = AuditQuery::new()
    .with_limit(50)
    .with_offset(0);

let first_page = audit_store.query(page1).await;

// Segunda página
let page2 = AuditQuery::new()
    .with_limit(50)
    .with_offset(50);

let second_page = audit_store.query(page2).await;
```

---

## 📊 Estadísticas

### Obtener Estadísticas Globales

```rust
let stats = audit_store.stats().await;

println!("Total eventos: {}", stats.total_events);
println!("Evento más antiguo: {:?}", stats.oldest_event);
println!("Evento más reciente: {:?}", stats.newest_event);

// Eventos por tipo
for (event_type, count) in stats.events_by_type {
    println!("{}: {} eventos", event_type, count);
}

// Eventos por tipo de agregado
for (aggregate_type, count) in stats.events_by_aggregate_type {
    println!("{}: {} eventos", aggregate_type, count);
}
```

**Ejemplo de salida:**
```
Total eventos: 1523
Evento más antiguo: 2024-01-15 08:30:00 UTC
Evento más reciente: 2024-01-15 14:45:00 UTC

iam.user.created: 245 eventos
iam.group.created: 89 eventos
organizations.account.created: 156 eventos
organizations.scp.attached: 342 eventos

User: 534 eventos
Group: 289 eventos
Account: 456 eventos
```

---

## 🔎 Casos de Uso Comunes

### 1. Auditoría de Seguridad: ¿Quién creó esta cuenta?

```rust
let account_hrn = "hrn:hodei:organizations:default:account/prod-account";

let query = AuditQuery::new()
    .with_aggregate_id(account_hrn)
    .with_event_type("organizations.account.created");

let events = audit_store.query(query).await;

if let Some(event) = events.first() {
    println!("Cuenta creada el: {}", event.occurred_at);
    println!("Metadata: {:?}", event.metadata);
    println!("Event data: {}", event.event_data);
}
```

### 2. Debugging: Historial completo de un recurso

```rust
let resource_hrn = "hrn:hodei:iam:default:user/john.doe";

let query = AuditQuery::new()
    .with_aggregate_id(resource_hrn);

let history = audit_store.query(query).await;

println!("=== Historial de {} ===", resource_hrn);
for event in history {
    println!("{} - {}", event.occurred_at, event.event_type);
}
```

**Ejemplo de salida:**
```
=== Historial de hrn:hodei:iam:default:user/john.doe ===
2024-01-15 08:30:00 - iam.user.created
2024-01-15 09:15:00 - iam.user.added_to_group
2024-01-15 10:45:00 - iam.policy.attached_to_user
2024-01-15 12:30:00 - iam.user.updated
```

### 3. Compliance: Reporte de actividad diaria

```rust
use chrono::NaiveDate;

async fn daily_compliance_report(
    audit_store: &AuditLogStore,
    date: NaiveDate,
) -> String {
    let start = date.and_hms_opt(0, 0, 0).unwrap().and_utc();
    let end = date.and_hms_opt(23, 59, 59).unwrap().and_utc();
    
    let query = AuditQuery::new().with_date_range(start, end);
    
    let events = audit_store.query(query).await;
    let stats = audit_store.stats().await;
    
    format!(
        "Reporte de Compliance - {}\n\
         Total eventos: {}\n\
         Usuarios creados: {}\n\
         Grupos creados: {}\n\
         Políticas adjuntadas: {}",
        date,
        events.len(),
        events.iter().filter(|e| e.event_type == "iam.user.created").count(),
        events.iter().filter(|e| e.event_type == "iam.group.created").count(),
        events.iter().filter(|e| e.event_type.contains("policy.attached")).count(),
    )
}
```

### 4. Monitoreo: Eventos en tiempo real

```rust
use std::time::Duration;
use tokio::time::sleep;

async fn monitor_new_events(audit_store: Arc<AuditLogStore>) {
    let mut last_count = audit_store.count_all().await;
    
    loop {
        sleep(Duration::from_secs(5)).await;
        
        let current_count = audit_store.count_all().await;
        
        if current_count > last_count {
            let new_events_count = current_count - last_count;
            println!("🔔 {} nuevos eventos detectados", new_events_count);
            
            // Obtener los últimos eventos
            let query = AuditQuery::new().with_limit(new_events_count);
            let new_events = audit_store.query(query).await;
            
            for event in new_events {
                println!("  → {} ({})", event.event_type, event.aggregate_id.unwrap_or_default());
            }
            
            last_count = current_count;
        }
    }
}
```

### 5. Análisis: Top eventos por tipo

```rust
async fn top_events_by_type(audit_store: &AuditLogStore, limit: usize) {
    let stats = audit_store.stats().await;
    
    let mut events: Vec<_> = stats.events_by_type.into_iter().collect();
    events.sort_by(|a, b| b.1.cmp(&a.1));
    
    println!("Top {} tipos de eventos:", limit);
    for (i, (event_type, count)) in events.iter().take(limit).enumerate() {
        println!("{}. {} - {} eventos", i + 1, event_type, count);
    }
}
```

---

## 🎨 Tipos de Eventos Disponibles

### IAM Events
- `iam.user.created` - Usuario creado
- `iam.user.updated` - Usuario actualizado
- `iam.user.deleted` - Usuario eliminado
- `iam.group.created` - Grupo creado
- `iam.group.updated` - Grupo actualizado
- `iam.group.deleted` - Grupo eliminado
- `iam.user.added_to_group` - Usuario añadido a grupo
- `iam.user.removed_from_group` - Usuario removido de grupo
- `iam.policy.attached_to_user` - Política adjuntada a usuario
- `iam.policy.detached_from_user` - Política desvinculada de usuario

### Organizations Events
- `organizations.account.created` - Cuenta creada
- `organizations.account.moved` - Cuenta movida entre OUs
- `organizations.account.deleted` - Cuenta eliminada
- `organizations.scp.attached` - SCP adjuntada
- `organizations.scp.detached` - SCP desvinculada
- `organizations.ou.created` - OU creada
- `organizations.ou.deleted` - OU eliminada
- `organizations.scp.created` - SCP creada
- `organizations.scp.updated` - SCP actualizada
- `organizations.scp.deleted` - SCP eliminada

---

## 🏗️ Estructura de un AuditLog

```rust
pub struct AuditLog {
    /// ID único del evento
    pub id: Uuid,
    
    /// Tipo del evento (ej: "iam.user.created")
    pub event_type: String,
    
    /// ID del agregado relacionado (ej: HRN del usuario)
    pub aggregate_id: Option<String>,
    
    /// Tipo del agregado (ej: "User", "Account")
    pub aggregate_type: Option<String>,
    
    /// Datos del evento en formato JSON
    pub event_data: serde_json::Value,
    
    /// Cuándo ocurrió el evento
    pub occurred_at: DateTime<Utc>,
    
    /// ID de correlación para seguir flujos
    pub correlation_id: Option<String>,
    
    /// ID de causalidad (qué causó este evento)
    pub causation_id: Option<String>,
    
    /// Metadata adicional
    pub metadata: HashMap<String, String>,
}
```

### Ejemplo de event_data

Para `iam.user.created`:
```json
{
  "user_hrn": "hrn:hodei:iam:default:user/john.doe",
  "username": "john.doe",
  "email": "john.doe@example.com",
  "created_at": "2024-01-15T08:30:00Z"
}
```

---

## 🔧 Integración en Casos de Uso

Si estás creando un nuevo caso de uso y quieres que publique eventos:

### Paso 1: Añadir event_publisher al Use Case

```rust
use shared::infrastructure::in_memory_event_bus::InMemoryEventBus;
use std::sync::Arc;

pub struct MyUseCase {
    // ... otros campos
    event_publisher: Option<Arc<InMemoryEventBus>>,
}

impl MyUseCase {
    pub fn new(/* ... */) -> Self {
        Self {
            // ... otros campos
            event_publisher: None,
        }
    }
    
    pub fn with_event_publisher(mut self, publisher: Arc<InMemoryEventBus>) -> Self {
        self.event_publisher = Some(publisher);
        self
    }
}
```

### Paso 2: Publicar el evento

```rust
use shared::application::ports::event_bus::EventEnvelope;

// Después de la operación exitosa
if let Some(publisher) = &self.event_publisher {
    let event = MyDomainEvent {
        // ... datos del evento
    };
    
    let envelope = EventEnvelope::new(event)
        .with_metadata("aggregate_type".to_string(), "MyType".to_string());
    
    if let Err(e) = publisher.publish_with_envelope(envelope).await {
        tracing::warn!("Failed to publish event: {}", e);
        // No fallar el caso de uso
    }
}
```

### Paso 3: Actualizar el DI

```rust
pub fn make_use_case_with_events(
    // ... repos
    event_bus: Arc<InMemoryEventBus>,
) -> MyUseCase {
    MyUseCase::new(/* ... */).with_event_publisher(event_bus)
}
```

---

## ⚠️ Mejores Prácticas

### 1. ✅ No fallar el caso de uso si falla la publicación

```rust
// ✅ CORRECTO - Solo advertencia
if let Err(e) = publisher.publish_with_envelope(envelope).await {
    tracing::warn!("Failed to publish event: {}", e);
}

// ❌ INCORRECTO - No propaguen el error
publisher.publish_with_envelope(envelope).await?;
```

### 2. ✅ Incluir metadata útil

```rust
let envelope = EventEnvelope::new(event)
    .with_metadata("aggregate_type".to_string(), "User".to_string())
    .with_metadata("tenant_id".to_string(), tenant_id)
    .with_metadata("actor".to_string(), current_user_id);
```

### 3. ✅ Usar correlation_id para flujos relacionados

```rust
let correlation_id = uuid::Uuid::new_v4().to_string();

let envelope = EventEnvelope::with_correlation(event, correlation_id);
```

### 4. ✅ Nombrar eventos consistentemente

```
{service}.{entity}.{action}

Ejemplos:
- iam.user.created
- organizations.account.moved
- policies.policy.updated
```

---

## 🚨 Troubleshooting

### Problema: No veo eventos en el audit store

**Solución:**
1. Verificar que el caso de uso tenga `event_publisher` inyectado
2. Verificar que se está usando `make_use_case_with_events()` en el DI
3. Verificar que el `AuditEventHandler` está suscrito al tipo de evento
4. Revisar logs con `tracing::info!` para ver si los eventos se publican

### Problema: Los eventos no tienen aggregate_type

**Solución:**
Añadir metadata al envelope:
```rust
let envelope = EventEnvelope::new(event)
    .with_metadata("aggregate_type".to_string(), "User".to_string());
```

### Problema: Consultas lentas con muchos eventos

**Solución:**
- Usar filtros específicos para reducir el conjunto de datos
- Usar `limit` para paginar resultados
- Considerar migrar a persistencia con índices (SurrealDB)

---

## 📚 Recursos Adicionales

- **Documentación completa:** `docs/implementation-complete-audit-system.md`
- **Plan de implementación:** `docs/implementation-plan-audit-system.md`
- **Tests de ejemplo:** `crates/shared/src/infrastructure/audit/query.rs` (módulo tests)
- **Código fuente:** `crates/shared/src/infrastructure/audit/`

---

## 🎯 Conclusión

El sistema de auditoría proporciona una forma robusta y flexible de rastrear todos los eventos del sistema. Usa esta guía como referencia para integrar auditoría en nuevas funcionalidades y consultar el historial de eventos para debugging, compliance y análisis.

**¿Preguntas?** Revisa los tests en `crates/shared/src/infrastructure/audit/query.rs` para ver más ejemplos de uso.