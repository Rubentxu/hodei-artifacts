# Health Endpoint

## Descripción

Endpoint REST para verificar el estado de salud del sistema.

## Endpoint

```
GET /api/v1/health
```

## Parámetros

No se requieren parámetros.

## Respuestas

### 200 OK - Estado de salud del sistema

```json
{
  "status": "healthy",
  "timestamp": "2024-01-01T12:00:00Z",
  "uptime_seconds": 3600,
  "components": {
    "database": {
      "status": "healthy"
    },
    "policy_engine": {
      "status": "healthy"
    }
  },
  "version": "0.1.0"
}
```

### 503 Service Unavailable - Sistema no saludable

```json
{
  "status": "unhealthy",
  "timestamp": "2024-01-01T12:00:00Z",
  "uptime_seconds": 3600,
  "components": {
    "database": {
      "status": "unhealthy",
      "reason": "Connection failed"
    },
    "policy_engine": {
      "status": "healthy"
    }
  },
  "version": "0.1.0"
}
```

## Ejemplos de Uso

### cURL

```bash
# Verificar el estado de salud del sistema
curl -X GET http://localhost:8080/api/v1/health
```

### HTTPie

```bash
# Verificar el estado de salud del sistema
http GET http://localhost:8080/api/v1/health
```

### JavaScript (Fetch API)

```javascript
// Verificar el estado de salud del sistema
async function checkHealth() {
  try {
    const response = await fetch('http://localhost:8080/api/v1/health');
    
    if (!response.ok) {
      console.error('Health check failed');
      return null;
    }
    
    const health = await response.json();
    console.log('System health:', health);
    return health;
  } catch (error) {
    console.error('Network error:', error);
    return null;
  }
}

// Uso
checkHealth();
```

### Python (requests)

```python
import requests

def check_health():
    url = "http://localhost:8080/api/v1/health"
    
    try:
        response = requests.get(url)
        response.raise_for_status()
        
        health = response.json()
        print(f"System health: {health}")
        return health
    except requests.exceptions.RequestException as e:
        print(f"Error: {e}")
        return None

# Uso
check_health()
```

## Arquitectura

### Flujo de Ejecución

1. **Handler** (`src/api/health_handler.rs::health`):
   - Obtiene el estado de salud del sistema
   - Retorna el estado de salud en formato JSON

2. **Health Service** (`src/app_state.rs::HealthState`):
   - Mantiene el estado de salud de los componentes del sistema
   - Proporciona información sobre el tiempo de actividad

### Inyección de Dependencias

El servicio de salud se inicializa en `main.rs`:

```rust
// Initialize health state
let health_state = Arc::new(RwLock::new(HealthState::new()));
```

## Documentación OpenAPI

La documentación completa de la API está disponible en:

- **Swagger UI**: http://localhost:8080/api-docs
- **OpenAPI Spec**: http://localhost:8080/api-docs/openapi.json

## Tests

Los tests del endpoint se encuentran en `src/api/health_handler_test.rs`:

```bash
# Ejecutar tests del endpoint
cargo test -p hodei-artifacts-api test_health

# Ejecutar todos los tests de health handler
cargo test -p hodei-artifacts-api health_handler_test
```

## Métricas

El endpoint registra las siguientes métricas:

- `http_requests_total`: Contador total de requests HTTP
- `http_request_duration_seconds`: Histograma de duración de requests

## Notas Técnicas

- El endpoint sigue la arquitectura VSA (Vertical Slice Architecture)
- Proporciona información sobre el estado de salud del sistema
- Incluye detalles sobre componentes individuales
- Retorna el tiempo de actividad del sistema
- Proporciona la versión actual de la aplicación
