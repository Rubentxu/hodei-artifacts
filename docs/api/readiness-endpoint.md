# Readiness Endpoint

## Descripción

Endpoint REST para verificar si el sistema está listo para aceptar tráfico.

## Endpoint

```
GET /api/v1/readiness
```

## Parámetros

No se requieren parámetros.

## Respuestas

### 200 OK - Sistema listo

```json
{
  "status": "ready",
  "timestamp": "2024-01-01T12:00:00Z",
  "checks": {
    "database": "healthy",
    "policy_engine": "healthy"
  }
}
```

### 503 Service Unavailable - Sistema no listo

```json
{
  "status": "not_ready",
  "timestamp": "2024-01-01T12:00:00Z",
  "checks": {
    "database": "unhealthy",
    "policy_engine": "healthy"
  }
}
```

## Ejemplos de Uso

### cURL

```bash
# Verificar si el sistema está listo
curl -X GET http://localhost:8080/api/v1/readiness
```

### HTTPie

```bash
# Verificar si el sistema está listo
http GET http://localhost:8080/api/v1/readiness
```

### JavaScript (Fetch API)

```javascript
// Verificar si el sistema está listo
async function checkReadiness() {
  try {
    const response = await fetch('http://localhost:8080/api/v1/readiness');
    
    if (!response.ok) {
      console.error('Readiness check failed');
      return null;
    }
    
    const readiness = await response.json();
    console.log('System readiness:', readiness);
    return readiness;
  } catch (error) {
    console.error('Network error:', error);
    return null;
  }
}

// Uso
checkReadiness();
```

### Python (requests)

```python
import requests

def check_readiness():
    url = "http://localhost:8080/api/v1/readiness"
    
    try:
        response = requests.get(url)
        response.raise_for_status()
        
        readiness = response.json()
        print(f"System readiness: {readiness}")
        return readiness
    except requests.exceptions.RequestException as e:
        print(f"Error: {e}")
        return None

# Uso
check_readiness()
```

## Arquitectura

### Flujo de Ejecución

1. **Handler** (`src/api/health_handler.rs::readiness`):
   - Obtiene el estado de readiness del sistema
   - Retorna el estado de readiness en formato JSON

2. **Health Service** (`src/app_state.rs::HealthState`):
   - Mantiene el estado de readiness de los componentes del sistema
   - Proporciona información sobre la disponibilidad de servicios

### Inyección de Dependencias

El servicio de health se inicializa en `main.rs`:

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
cargo test -p hodei-artifacts-api test_readiness

# Ejecutar todos los tests de health handler
cargo test -p hodei-artifacts-api health_handler_test
```

## Métricas

El endpoint registra las siguientes métricas:

- `http_requests_total`: Contador total de requests HTTP
- `http_request_duration_seconds`: Histograma de duración de requests

## Notas Técnicas

- El endpoint sigue la arquitectura VSA (Vertical Slice Architecture)
- Proporciona información sobre la disponibilidad del sistema para aceptar tráfico
- Incluye detalles sobre componentes críticos
- Diferencia entre health (estado general) y readiness (disponibilidad para tráfico)
