# Metrics Endpoint

## Descripción

Endpoint REST para exponer métricas del sistema en formato Prometheus.

## Endpoint

```
GET /api/v1/metrics
```

## Parámetros

No se requieren parámetros.

## Respuestas

### 200 OK - Métricas del sistema

```
# HELP http_requests_total Total number of HTTP requests
# TYPE http_requests_total counter
http_requests_total{method="GET",endpoint="/api/v1/health"} 42
http_requests_total{method="POST",endpoint="/api/v1/policies"} 15

# HELP http_request_duration_seconds HTTP request duration in seconds
# TYPE http_request_duration_seconds histogram
http_request_duration_seconds_bucket{endpoint="/api/v1/health",le="0.005"} 40
http_request_duration_seconds_bucket{endpoint="/api/v1/health",le="0.01"} 42
http_request_duration_seconds_bucket{endpoint="/api/v1/health",le="+Inf"} 42
http_request_duration_seconds_sum{endpoint="/api/v1/health"} 0.12
http_request_duration_seconds_count{endpoint="/api/v1/health"} 42

# HELP policy_operations_total Total number of policy operations
# TYPE policy_operations_total counter
policy_operations_total 25
```

### 404 Not Found - Métricas deshabilitadas

```
Metrics disabled
```

## Ejemplos de Uso

### cURL

```bash
# Obtener métricas del sistema
curl -X GET http://localhost:8080/api/v1/metrics
```

### HTTPie

```bash
# Obtener métricas del sistema
http GET http://localhost:8080/api/v1/metrics
```

### JavaScript (Fetch API)

```javascript
// Obtener métricas del sistema
async function getMetrics() {
  try {
    const response = await fetch('http://localhost:8080/api/v1/metrics');
    
    if (!response.ok) {
      const error = await response.text();
      console.error('Metrics error:', error);
      return null;
    }
    
    const metrics = await response.text();
    console.log('System metrics:', metrics);
    return metrics;
  } catch (error) {
    console.error('Network error:', error);
    return null;
  }
}

// Uso
getMetrics();
```

### Python (requests)

```python
import requests

def get_metrics():
    url = "http://localhost:8080/api/v1/metrics"
    
    try:
        response = requests.get(url)
        response.raise_for_status()
        
        metrics = response.text
        print(f"System metrics: {metrics}")
        return metrics
    except requests.exceptions.RequestException as e:
        print(f"Error: {e}")
        return None

# Uso
get_metrics()
```

## Arquitectura

### Flujo de Ejecución

1. **Handler** (`src/api/metrics_handler.rs::metrics`):
   - Verifica si las métricas están habilitadas
   - Recopila y expone las métricas del sistema
   - Retorna las métricas en formato texto

2. **Metrics Service** (`src/app_state.rs::Metrics`):
   - Mantiene y actualiza las métricas del sistema
   - Proporciona interfaces para registrar diferentes tipos de métricas

### Inyección de Dependencias

El servicio de métricas se inicializa en `main.rs`:

```rust
// Initialize metrics
let metrics = Arc::new(Metrics::new());
```

## Documentación OpenAPI

La documentación completa de la API está disponible en:

- **Swagger UI**: http://localhost:8080/api-docs
- **OpenAPI Spec**: http://localhost:8080/api-docs/openapi.json

## Tests

Los tests del endpoint se encuentran en `src/api/metrics_handler_test.rs`:

```bash
# Ejecutar tests del endpoint
cargo test -p hodei-artifacts-api test_metrics

# Ejecutar todos los tests de metrics handler
cargo test -p hodei-artifacts-api metrics_handler_test
```

## Métricas

El endpoint expone las siguientes métricas del sistema:

- `http_requests_total`: Contador total de requests HTTP
- `http_request_duration_seconds`: Histograma de duración de requests
- `policy_operations_total`: Contador de operaciones de políticas
- `authorization_requests_total`: Contador de solicitudes de autorización
- `upload_sessions_total`: Contador de sesiones de upload
- `artifacts_uploaded_total`: Contador de artifacts subidos

## Notas Técnicas

- El endpoint sigue la arquitectura VSA (Vertical Slice Architecture)
- Expone métricas en formato Prometheus
- Puede estar deshabilitado según la configuración
- No requiere autenticación
- Retorna datos en formato texto plano
