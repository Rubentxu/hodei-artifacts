# Delete Policy Endpoint

## Descripción

Endpoint REST para eliminar una política específica por su ID.

## Endpoint

```
DELETE /api/v1/policies/{policy_id}
```

## Parámetros

### Path Parameters

- **policy_id** (string, requerido): Identificador único de la política

## Respuestas

### 200 OK - Política eliminada exitosamente

```json
{
  "message": "Policy deleted successfully",
  "policy_id": "policy0",
  "timestamp": "2024-01-01T12:00:00Z"
}
```

### 400 Bad Request - ID inválido

```json
{
  "error": "Invalid policy ID"
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
# Eliminar una política existente
curl -X DELETE http://localhost:8080/api/v1/policies/policy0

# Eliminar una política que no existe
curl -X DELETE http://localhost:8080/api/v1/policies/nonexistent-id
```

### HTTPie

```bash
# Eliminar una política existente
http DELETE http://localhost:8080/api/v1/policies/policy0

# Eliminar una política que no existe
http DELETE http://localhost:8080/api/v1/policies/nonexistent-id
```

### JavaScript (Fetch API)

```javascript
// Eliminar una política
async function deletePolicy(policyId) {
  try {
    const response = await fetch(`http://localhost:8080/api/v1/policies/${policyId}`, {
      method: 'DELETE',
    });
    
    if (!response.ok) {
      const error = await response.json();
      console.error('Error:', error);
      return null;
    }
    
    const result = await response.json();
    console.log('Policy deleted:', result);
    return result;
  } catch (error) {
    console.error('Network error:', error);
    return null;
  }
}

// Uso
deletePolicy('policy0');
```

### Python (requests)

```python
import requests

def delete_policy(policy_id):
    url = f"http://localhost:8080/api/v1/policies/{policy_id}"
    
    try:
        response = requests.delete(url)
        response.raise_for_status()
        
        result = response.json()
        print(f"Policy deleted: {result}")
        return result
    except requests.exceptions.HTTPError as e:
        print(f"HTTP Error: {e}")
        print(f"Response: {e.response.json()}")
        return None
    except requests.exceptions.RequestException as e:
        print(f"Error: {e}")
        return None

# Uso
delete_policy('policy0')
```

## Arquitectura

### Flujo de Ejecución

1. **Handler** (`src/api/policy_handlers.rs::delete_policy`):
   - Valida el `policy_id`
   - Crea `DeletePolicyCommand`
   - Ejecuta el use case

2. **Use Case** (`crates/policies/src/features/delete_policy/use_case.rs`):
   - Valida el comando
   - Elimina la política del `PolicyStore`
   - Retorna éxito o error

3. **PolicyStore** (`crates/policies/src/shared/application/store.rs`):
   - Llama a `PolicyStorage::delete_policy_by_id`

4. **Storage** (`crates/policies/src/shared/infrastructure/surreal/`):
   - Elimina la política de SurrealDB
   - Retorna `Option<Policy>`

### Inyección de Dependencias

El use case se inicializa en `main.rs`:

```rust
// Build policies delete_policy use case via DI
#[cfg(feature = "embedded")]
let (delete_policy_uc, _) = policies::features::delete_policy::di::embedded::make_use_case_embedded(&config.database.url)
    .await?;
#[cfg(not(feature = "embedded"))]
let (delete_policy_uc, _) = policies::features::delete_policy::di::make_use_case_mem()
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
cargo test -p hodei-artifacts-api test_delete_policy

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
- Manejo de errores tipado con `DeletePolicyError`
