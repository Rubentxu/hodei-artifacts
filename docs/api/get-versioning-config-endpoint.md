# Get Versioning Configuration Endpoint

## Descripción

Endpoint REST para obtener la configuración de versioning para un artifact específico.

## Endpoint

```
GET /api/v1/versioning/config/{namespace}/{name}
```

## Parámetros

### Path Parameters

- **namespace** (string, requerido): Namespace del artifact
- **name** (string, requerido): Nombre del artifact

## Respuestas

### 200 OK - Configuración de versioning

```json
{
  "artifact_coordinates": {
    "namespace": "com.example",
    "name": "my-artifact"
  },
  "versioning_policy": "semver",
  "auto_increment": true,
  "allow_prerelease": false,
  "rules": [
    {
      "type": "compatibility",
      "validator": "semver-compatibility",
      "severity": "error"
    },
    {
      "type": "changelog",
      "validator": "changelog-required",
      "severity": "warning"
    }
  ],
  "created_at": "2024-01-01T12:00:00Z",
  "updated_at": "2024-01-01T12:00:00Z"
}
```

### 404 Not Found - Configuración de versioning no encontrada

```json
{
  "error": "Versioning configuration not found for artifact"
}
```

### 500 Internal Server Error

```json
{
  "error": "Internal server error"
}
```

## Ejemplos de Uso

### cURL

```bash
# Obtener configuración de versioning
curl -X GET http://localhost:8080/api/v1/versioning/config/com.example/my-artifact
```

### HTTPie

```bash
# Obtener configuración de versioning
http GET http://localhost:8080/api/v1/versioning/config/com.example/my-artifact
```

### JavaScript (Fetch API)

```javascript
// Obtener configuración de versioning
async function getVersioningConfig(namespace, name) {
  try {
    const response = await fetch(`http://localhost:8080/api/v1/versioning/config/${namespace}/${name}`);
    
    if (!response.ok) {
      const error = await response.json();
      console.error('Error:', error);
      return null;
    }
    
    const config = await response.json();
    console.log('Versioning configuration:', config);
    return config;
  } catch (error) {
    console.error('Network error:', error);
    return null;
  }
}

// Uso
getVersioningConfig('com.example', 'my-artifact');
```

### Python (requests)

```python
import requests

def get_versioning_config(namespace, name):
    url = f"http://localhost:8080/api/v1/versioning/config/{namespace}/{name}"
    
    try:
        response = requests.get(url)
        response.raise_for_status()
        
        config = response.json()
        print(f"Versioning configuration: {config}")
        return config
    except requests.exceptions.HTTPError as e:
        print(f"HTTP Error: {e}")
        print(f"Response: {e.response.json()}")
        return None
    except requests.exceptions.RequestException as e:
        print(f"Error: {e}")
        return None

# Uso
get_versioning_config('com.example', 'my-artifact')
```

## Arquitectura

### Flujo de Ejecución

1. **Handler** (`src/api/versioning/handlers.rs::get_versioning_config`):
   - Procesa la solicitud
   - Valida las coordenadas del artifact
   - Crea `GetVersioningConfigQuery`
   - Ejecuta el use case

2. **Use Case** (`crates/distribution/src/features/versioning_config/use_case.rs`):
   - Valida la query
   - Obtiene la configuración de versioning del repositorio
   - Retorna la configuración o error

3. **Repository** (`crates/distribution/src/features/versioning_config/repository.rs`):
   - Interactúa con el almacenamiento de configuraciones de versioning
   - Recupera la configuración para el artifact especificado

4. **Storage** (`crates/distribution/src/features/versioning_config/storage.rs`):
   - Implementa el almacenamiento de configuraciones de versioning
   - Proporciona interfaces para obtener configuraciones

### Inyección de Dependencias

El use case se inicializa en `main.rs`:

```rust
// Build versioning config use case via DI
let get_versioning_config_uc = distribution::features::versioning_config::di::make_use_case();
```

## Documentación OpenAPI

La documentación completa de la API está disponible en:

- **Swagger UI**: http://localhost:8080/api-docs
- **OpenAPI Spec**: http://localhost:8080/api-docs/openapi.json

## Tests

Los tests del endpoint se encuentran en `tests/integration/policies_api_test.rs`:

```bash
# Ejecutar tests del endpoint
cargo test -p hodei-artifacts-api test_get_versioning_config

# Ejecutar todos los tests de integración
cargo test -p hodei-artifacts-api
```

## Métricas

El endpoint registra las siguientes métricas:

- `versioning_config_requests_total`: Contador de solicitudes de configuración de versioning
- `http_requests_total`: Contador total de requests HTTP
- `http_request_duration_seconds`: Histograma de duración de requests

## Notas Técnicas

- El endpoint sigue la arquitectura VSA (Vertical Slice Architecture)
- Retorna la configuración de versioning para un artifact específico
- Manejo de errores tipado con `VersioningConfigError`
