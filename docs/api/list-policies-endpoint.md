# List Policies Endpoint

## Descripción

Endpoint REST para listar todas las políticas existentes con soporte para paginación y filtrado.

## Endpoint

```
GET /api/v1/policies
```

## Parámetros

### Query Parameters

- **offset** (integer, opcional): Número de elementos a saltar para paginación (por defecto: 0)
- **limit** (integer, opcional): Número máximo de elementos a devolver (por defecto: sin límite, máximo: 1000)
- **filter_id** (string, opcional): Filtrar políticas por ID (coincidencia parcial)

## Respuestas

### 200 OK - Lista de políticas recuperada exitosamente

```json
{
  "policies": [
    {
      "id": "policy0",
      "name": "policy0",
      "description": null,
      "policy_content": "permit(principal, action, resource);",
      "enabled": true,
      "created_at": "2024-01-01T12:00:00Z",
      "updated_at": "2024-01-01T12:00:00Z"
    }
  ],
  "total": 1,
  "offset": 0,
  "limit": 100
}
```

### 400 Bad Request - Parámetros de consulta inválidos

```json
{
  "error": "Invalid query parameters"
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
# Listar todas las políticas
curl -X GET http://localhost:8080/api/v1/policies

# Listar políticas con paginación
curl -X GET "http://localhost:8080/api/v1/policies?offset=0&limit=10"

# Filtrar políticas por ID
curl -X GET "http://localhost:8080/api/v1/policies?filter_id=policy"
```

### HTTPie

```bash
# Listar todas las políticas
http GET http://localhost:8080/api/v1/policies

# Listar políticas con paginación
http GET "http://localhost:8080/api/v1/policies?offset=0&limit=10"

# Filtrar políticas por ID
http GET "http://localhost:8080/api/v1/policies?filter_id=policy"
```

### JavaScript (Fetch API)

```javascript
// Listar políticas
async function listPolicies(params = {}) {
  try {
    const url = new URL('http://localhost:8080/api/v1/policies');
    Object.keys(params).forEach(key => url.searchParams.append(key, params[key]));
    
    const response = await fetch(url);
    
    if (!response.ok) {
      const error = await response.json();
      console.error('Error:', error);
      return null;
    }
    
    const policies = await response.json();
    console.log('Policies:', policies);
    return policies;
  } catch (error) {
    console.error('Network error:', error);
    return null;
  }
}

// Uso
listPolicies({ offset: 0, limit: 10, filter_id: "policy" });
```

### Python (requests)

```python
import requests

def list_policies(params=None):
    url = "http://localhost:8080/api/v1/policies"
    
    try:
        response = requests.get(url, params=params)
        response.raise_for_status()
        
        policies = response.json()
        print(f"Policies: {policies}")
        return policies
    except requests.exceptions.HTTPError as e:
        print(f"HTTP Error: {e}")
        print(f"Response: {e.response.json()}")
        return None
    except requests.exceptions.RequestException as e:
        print(f"Error: {e}")
        return None

# Uso
list_policies({"offset": 0, "limit": 10, "filter_id": "policy"})
```

## Arquitectura

### Flujo de Ejecución

1. **Handler** (`src/api/policy_handlers.rs::list_policies`):
   - Procesa los parámetros de consulta
   - Crea `ListPoliciesQuery`
   - Ejecuta el use case

2. **Use Case** (`crates/policies/src/features/list_policies/use_case.rs`):
   - Valida la query
   - Obtiene la lista de políticas del `PolicyStore`
   - Retorna la lista de políticas o error

3. **PolicyStore** (`crates/policies/src/shared/application/store.rs`):
   - Llama a `PolicyStorage::list_policies`

4. **Storage** (`crates/policies/src/shared/infrastructure/surreal/`):
   - Consulta SurrealDB para obtener las políticas
   - Parsea las políticas de Cedar
   - Retorna `Vec<Policy>`

### Inyección de Dependencias

El use case se inicializa en `main.rs`:

```rust
// Build policies list_policies use case via DI
#[cfg(feature = "embedded")]
let (list_policies_uc, _) = policies::features::list_policies::di::embedded::make_use_case_embedded(&config.database.url)
    .await?;
#[cfg(not(feature = "embedded"))]
let (list_policies_uc, _) = policies::features::list_policies::di::make_use_case_mem()
    .await?;
```

## Documentación OpenAPI

La documentación completa de la API está disponible en:

- **Swagger UI**: http://localhost:8080/api-docs
- **OpenAPI Spec**: http://localhost:8080/api-docs/openapi.json

## Tests

Los tests del endpoint se encuentran en `src/api/policy_handlers_test.rs`:

```bash
# Ejecutar tests del endpoint
cargo test -p hodei-artifacts-api test_list_policies

# Ejecutar todos los tests de policy handlers
cargo test -p hodei-artifacts-api policy_handlers_test
```

## Métricas

El endpoint registra las siguientes métricas:

- `policy_operations_total`: Contador de operaciones de políticas
- `http_requests_total`: Contador total de requests HTTP
- `http_request_duration_seconds`: Histograma de duración de requests

## Notas Técnicas

- El endpoint sigue la arquitectura VSA (Vertical Slice Architecture)
- Utiliza Cedar Policy Engine para el manejo de políticas
- Soporta tanto almacenamiento en memoria (`mem`) como embebido (`embedded`)
- Incluye validación exhaustiva de parámetros de consulta
- Manejo de errores tipado con `ListPoliciesError`
- Soporta paginación y filtrado por ID
