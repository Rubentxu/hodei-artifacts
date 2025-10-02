# Get Existing Versions Endpoint

## Descripción

Endpoint REST para obtener la lista de versiones existentes para un artifact específico.

## Endpoint

```
GET /api/v1/artifacts/{namespace}/{name}/versions
```

## Parámetros

### Path Parameters

- **namespace** (string, requerido): Namespace del artifact
- **name** (string, requerido): Nombre del artifact

### Query Parameters

- **limit** (integer, opcional): Número máximo de versiones a devolver (por defecto: 100, máximo: 1000)
- **offset** (integer, opcional): Desplazamiento para paginación (por defecto: 0)

## Respuestas

### 200 OK - Lista de versiones existentes

```json
{
  "artifact_coordinates": {
    "namespace": "com.example",
    "name": "my-artifact"
  },
  "versions": [
    {
      "version": "1.0.0",
      "created_at": "2024-01-01T12:00:00Z",
      "size": 1024,
      "checksum": "a1b2c3d4e5f6..."
    },
    {
      "version": "1.0.1",
      "created_at": "2024-01-02T12:00:00Z",
      "size": 1024,
      "checksum": "g7h8i9j0k1l2..."
    }
  ],
  "total": 2,
  "limit": 100,
  "offset": 0
}
```

### 404 Not Found - Artifact no encontrado

```json
{
  "error": "Artifact not found"
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
# Obtener versiones existentes
curl -X GET http://localhost:8080/api/v1/artifacts/com.example/my-artifact/versions

# Obtener versiones existentes con paginación
curl -X GET "http://localhost:8080/api/v1/artifacts/com.example/my-artifact/versions?limit=50&offset=0"
```

### HTTPie

```bash
# Obtener versiones existentes
http GET http://localhost:8080/api/v1/artifacts/com.example/my-artifact/versions

# Obtener versiones existentes con paginación
http GET "http://localhost:8080/api/v1/artifacts/com.example/my-artifact/versions?limit=50&offset=0"
```

### JavaScript (Fetch API)

```javascript
// Obtener versiones existentes
async function getExistingVersions(namespace, name, params = {}) {
  try {
    const url = new URL(`http://localhost:8080/api/v1/artifacts/${namespace}/${name}/versions`);
    Object.keys(params).forEach(key => url.searchParams.append(key, params[key]));
    
    const response = await fetch(url);
    
    if (!response.ok) {
      const error = await response.json();
      console.error('Error:', error);
      return null;
    }
    
    const versions = await response.json();
    console.log('Existing versions:', versions);
    return versions;
  } catch (error) {
    console.error('Network error:', error);
    return null;
  }
}

// Uso
getExistingVersions('com.example', 'my-artifact', { limit: 50, offset: 0 });
```

### Python (requests)

```python
import requests

def get_existing_versions(namespace, name, params=None):
    url = f"http://localhost:8080/api/v1/artifacts/{namespace}/{name}/versions"
    
    try:
        response = requests.get(url, params=params)
        response.raise_for_status()
        
        versions = response.json()
        print(f"Existing versions: {versions}")
        return versions
    except requests.exceptions.HTTPError as e:
        print(f"HTTP Error: {e}")
        print(f"Response: {e.response.json()}")
        return None
    except requests.exceptions.RequestException as e:
        print(f"Error: {e}")
        return None

# Uso
get_existing_versions('com.example', 'my-artifact', {'limit': 50, 'offset': 0})
```

## Arquitectura

### Flujo de Ejecución

1. **Handler** (`src/api/versioning/handlers.rs::get_existing_versions`):
   - Procesa la solicitud
   - Valida las coordenadas del artifact
   - Procesa los parámetros de paginación
   - Crea `GetExistingVersionsQuery`
   - Ejecuta el use case

2. **Use Case** (`crates/distribution/src/features/existing_versions/use_case.rs`):
   - Valida la query
   - Obtiene la lista de versiones existentes del repositorio
   - Retorna la lista de versiones o error

3. **Repository** (`crates/distribution/src/features/existing_versions/repository.rs`):
   - Interactúa con el almacenamiento de artifacts
   - Recupera la lista de versiones para el artifact especificado

4. **Storage** (`crates/distribution/src/features/existing_versions/storage.rs`):
   - Implementa el almacenamiento de artifacts
   - Proporciona interfaces para listar versiones existentes

### Inyección de Dependencias

El use case se inicializa en `main.rs`:

```rust
// Build existing versions use case via DI
let get_existing_versions_uc = distribution::features::existing_versions::di::make_use_case();
```

## Documentación OpenAPI

La documentación completa de la API está disponible en:

- **Swagger UI**: http://localhost:8080/api-docs
- **OpenAPI Spec**: http://localhost:8080/api-docs/openapi.json

## Tests

Los tests del endpoint se encuentran en `tests/integration/policies_api_test.rs`:

```bash
# Ejecutar tests del endpoint
cargo test -p hodei-artifacts-api test_get_existing_versions

# Ejecutar todos los tests de integración
cargo test -p hodei-artifacts-api
```

## Métricas

El endpoint registra las siguientes métricas:

- `artifact_versions_listed_total`: Contador de veces que se listan versiones de artifacts
- `http_requests_total`: Contador total de requests HTTP
- `http_request_duration_seconds`: Histograma de duración de requests

## Notas Técnicas

- El endpoint sigue la arquitectura VSA (Vertical Slice Architecture)
- Retorna la lista de versiones existentes para un artifact específico
- Soporta paginación para manejar artifacts con muchas versiones
- Manejo de errores tipado con `ExistingVersionsError`
