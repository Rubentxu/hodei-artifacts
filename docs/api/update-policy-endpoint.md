# Update Policy Endpoint

## Descripción

Endpoint REST para actualizar una política existente identificada por su ID.

## Endpoint

```
PUT /api/v1/policies/{policy_id}
```

## Parámetros

### Path Parameters

- **policy_id** (string, requerido): Identificador único de la política

### Request Body

```json
{
  "policy_content": "permit(principal, action, resource);"
}
```

- **policy_content** (string, requerido): Contenido de la política en formato Cedar

## Respuestas

### 200 OK - Política actualizada exitosamente

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

### 400 Bad Request - Solicitud inválida

```json
{
  "error": "Policy content cannot be empty"
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
# Actualizar una política existente
curl -X PUT http://localhost:8080/api/v1/policies/policy0 \
  -H "Content-Type: application/json" \
  -d '{
  "policy_content": "permit(principal == User::\"admin\", action, resource);"
}'
```

### HTTPie

```bash
# Actualizar una política existente
echo '{
  "policy_content": "permit(principal == User::\"admin\", action, resource);"
}' | http PUT http://localhost:8080/api/v1/policies/policy0
```

### JavaScript (Fetch API)

```javascript
// Actualizar una política
async function updatePolicy(policyId, policyContent) {
  try {
    const response = await fetch(`http://localhost:8080/api/v1/policies/${policyId}`, {
      method: 'PUT',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ policy_content: policyContent }),
    });
    
    if (!response.ok) {
      const error = await response.json();
      console.error('Error:', error);
      return null;
    }
    
    const policy = await response.json();
    console.log('Policy updated:', policy);
    return policy;
  } catch (error) {
    console.error('Network error:', error);
    return null;
  }
}

// Uso
updatePolicy('policy0', 'permit(principal == User::"admin", action, resource);');
```

### Python (requests)

```python
import requests

def update_policy(policy_id, policy_content):
    url = f"http://localhost:8080/api/v1/policies/{policy_id}"
    
    try:
        response = requests.put(url, json={"policy_content": policy_content})
        response.raise_for_status()
        
        policy = response.json()
        print(f"Policy updated: {policy}")
        return policy
    except requests.exceptions.HTTPError as e:
        print(f"HTTP Error: {e}")
        print(f"Response: {e.response.json()}")
        return None
    except requests.exceptions.RequestException as e:
        print(f"Error: {e}")
        return None

# Uso
update_policy('policy0', 'permit(principal == User::"admin", action, resource);')
```

## Arquitectura

### Flujo de Ejecución

1. **Handler** (`src/api/policy_handlers.rs::update_policy`):
   - Valida el `policy_id` y el contenido de la política
   - Crea `UpdatePolicyCommand`
   - Ejecuta el use case

2. **Use Case** (`crates/policies/src/features/update_policy/use_case.rs`):
   - Valida el comando
   - Actualiza la política en el `PolicyStore`
   - Retorna la política actualizada o error

3. **PolicyStore** (`crates/policies/src/shared/application/store.rs`):
   - Llama a `PolicyStorage::update_policy`

4. **Storage** (`crates/policies/src/shared/infrastructure/surreal/`):
   - Actualiza la política en SurrealDB
   - Parsea la política de Cedar
   - Retorna `Policy`

### Inyección de Dependencias

El use case se inicializa en `main.rs`:

```rust
// Build policies update_policy use case via DI
#[cfg(feature = "embedded")]
let (update_policy_uc, _) = policies::features::update_policy::di::embedded::make_use_case_embedded(&config.database.url)
    .await?;
#[cfg(not(feature = "embedded"))]
let (update_policy_uc, _) = policies::features::update_policy::di::make_use_case_mem()
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
cargo test -p hodei-artifacts-api test_update_policy

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
- Manejo de errores tipado con `UpdatePolicyError`
