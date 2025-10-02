# Get Policy Endpoint

## Descripción

Endpoint REST para obtener una política específica por su ID.

## Endpoint

```
GET /api/v1/policies/{policy_id}
```

## Parámetros

### Path Parameters

- **policy_id** (string, requerido): Identificador único de la política

## Respuestas

### 200 OK - Política encontrada

```json
{
  "id": "policy0",
  "name": "policy0",
  "description": null,
  "policy_content": "permit(principal, action, resource);",
  "enabled": true,
  "created_at": "2024-01-01T12:00:00Z",
  "updated_at": "2024-01-01T12:00:00Z"
}
```

### 404 Not Found - Política no encontrada

```json
{
  "error": {
    "type": "NOT_FOUND",
    "message": "Resource not found",
    "details": "Policy with ID 'nonexistent-id' not found",
    "timestamp": "2024-01-01T12:00:00Z"
  }
}
```

### 400 Bad Request - ID inválido

```json
{
  "error": {
    "type": "BAD_REQUEST",
    "message": "Bad request",
    "details": "Policy ID cannot be empty",
    "timestamp": "2024-01-01T12:00:00Z"
  }
}
```

### 500 Internal Server Error

```json
{
  "error": {
    "type": "INTERNAL_ERROR",
    "message": "Internal server error",
    "details": "Storage error: ...",
    "timestamp": "2024-01-01T12:00:00Z"
  }
}
```

## Ejemplos de Uso

### cURL

```bash
# Obtener una política existente
curl -X GET http://localhost:8080/api/v1/policies/policy0

# Obtener una política que no existe
curl -X GET http://localhost:8080/api/v1/policies/nonexistent-id
```

### HTTPie

```bash
# Obtener una política existente
http GET http://localhost:8080/api/v1/policies/policy0

# Obtener una política que no existe
http GET http://localhost:8080/api/v1/policies/nonexistent-id
```

### JavaScript (Fetch API)

```javascript
// Obtener una política
async function getPolicy(policyId) {
  try {
    const response = await fetch(`http://localhost:8080/api/v1/policies/${policyId}`);
    
    if (!response.ok) {
      const error = await response.json();
      console.error('Error:', error);
      return null;
    }
    
    const policy = await response.json();
    console.log('Policy:', policy);
    return policy;
  } catch (error) {
    console.error('Network error:', error);
    return null;
  }
}

// Uso
getPolicy('policy0');
```

### Python (requests)

```python
import requests

def get_policy(policy_id):
    url = f"http://localhost:8080/api/v1/policies/{policy_id}"
    
    try:
        response = requests.get(url)
        response.raise_for_status()
        
        policy = response.json()
        print(f"Policy: {policy}")
        return policy
    except requests.exceptions.HTTPError as e:
        print(f"HTTP Error: {e}")
        print(f"Response: {e.response.json()}")
        return None
    except requests.exceptions.RequestException as e:
        print(f"Error: {e}")
        return None

# Uso
get_policy('policy0')
```

## Arquitectura

### Flujo de Ejecución

1. **Handler** (`src/api/policy_handlers.rs::get_policy`):
   - Valida el `policy_id`
   - Crea `GetPolicyQuery`
   - Ejecuta el use case

2. **Use Case** (`crates/policies/src/features/get_policy/use_case.rs`):
   - Valida la query
   - Obtiene la política del `PolicyStore`
   - Retorna la política o error

3. **PolicyStore** (`crates/policies/src/shared/application/store.rs`):
   - Llama a `PolicyStorage::get_policy_by_id`

4. **Storage** (`crates/policies/src/shared/infrastructure/surreal/`):
   - Consulta SurrealDB
   - Parsea la política de Cedar
   - Retorna `Option<Policy>`

### Inyección de Dependencias

El use case se inicializa en `main.rs`:

```rust
// Build policies get_policy use case via DI
#[cfg(feature = "embedded")]
let (get_policy_uc, _) = policies::features::get_policy::di::embedded::make_use_case_embedded(&config.database.url)
    .await?;
#[cfg(not(feature = "embedded"))]
let (get_policy_uc, _) = policies::features::get_policy::di::make_use_case_mem()
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
cargo test -p hodei-artifacts-api test_get_policy

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
- Incluye validación exhaustiva de entrada
- Manejo de errores tipado con `GetPolicyError`
