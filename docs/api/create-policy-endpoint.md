# Create Policy Endpoint

## Descripción

Endpoint REST para crear una nueva política de Cedar.

## Endpoint

```
POST /api/v1/policies
```

## Parámetros

### Request Body

```json
{
  "name": "policy-name",
  "description": "Optional policy description",
  "policy_content": "permit(principal, action, resource);",
  "enabled": true
}
```

- **name** (string, requerido): Nombre de la política
- **description** (string, opcional): Descripción opcional de la política
- **policy_content** (string, requerido): Contenido de la política en formato Cedar
- **enabled** (boolean, opcional): Si la política está habilitada (por defecto: true)

## Respuestas

### 200 OK - Política creada exitosamente

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "name": "policy-name",
  "description": "Optional policy description",
  "policy_content": "permit(principal, action, resource);",
  "enabled": true,
  "created_at": "2024-01-01T12:00:00Z",
  "updated_at": "2024-01-01T12:00:00Z"
}
```

### 400 Bad Request - Solicitud inválida

```json
{
  "error": "Policy name cannot be empty"
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
# Crear una nueva política
curl -X POST http://localhost:8080/api/v1/policies \
  -H "Content-Type: application/json" \
  -d '{
  "name": "allow-read-access",
  "description": "Allows read access to resources",
  "policy_content": "permit(principal, action == Action::\"read\", resource);",
  "enabled": true
}'
```

### HTTPie

```bash
# Crear una nueva política
echo '{
  "name": "allow-read-access",
  "description": "Allows read access to resources",
  "policy_content": "permit(principal, action == Action::\"read\", resource);",
  "enabled": true
}' | http POST http://localhost:8080/api/v1/policies
```

### JavaScript (Fetch API)

```javascript
// Crear una política
async function createPolicy(policyData) {
  try {
    const response = await fetch('http://localhost:8080/api/v1/policies', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(policyData),
    });
    
    if (!response.ok) {
      const error = await response.json();
      console.error('Error:', error);
      return null;
    }
    
    const policy = await response.json();
    console.log('Policy created:', policy);
    return policy;
  } catch (error) {
    console.error('Network error:', error);
    return null;
  }
}

// Uso
const policyData = {
  name: "allow-read-access",
  description: "Allows read access to resources",
  policy_content: "permit(principal, action == Action::\"read\", resource);",
  enabled: true
};

createPolicy(policyData);
```

### Python (requests)

```python
import requests

def create_policy(policy_data):
    url = "http://localhost:8080/api/v1/policies"
    
    try:
        response = requests.post(url, json=policy_data)
        response.raise_for_status()
        
        policy = response.json()
        print(f"Policy created: {policy}")
        return policy
    except requests.exceptions.HTTPError as e:
        print(f"HTTP Error: {e}")
        print(f"Response: {e.response.json()}")
        return None
    except requests.exceptions.RequestException as e:
        print(f"Error: {e}")
        return None

# Uso
policy_data = {
    "name": "allow-read-access",
    "description": "Allows read access to resources",
    "policy_content": "permit(principal, action == Action::\"read\", resource);",
    "enabled": True
}

create_policy(policy_data)
```

## Arquitectura

### Flujo de Ejecución

1. **Handler** (`src/api/policy_handlers.rs::create_policy`):
   - Valida la solicitud
   - Crea `CreatePolicyCommand`
   - Ejecuta el use case

2. **Use Case** (`crates/policies/src/features/create_policy/use_case.rs`):
   - Valida el comando
   - Crea la política en el `PolicyStore`
   - Retorna la política creada o error

3. **PolicyStore** (`crates/policies/src/shared/application/store.rs`):
   - Llama a `PolicyStorage::create_policy`

4. **Storage** (`crates/policies/src/shared/infrastructure/surreal/`):
   - Almacena la política en SurrealDB
   - Retorna `Policy`

### Inyección de Dependencias

El use case se inicializa en `main.rs`:

```rust
// Build policies create_policy use case via DI
#[cfg(feature = "embedded")]
let (create_policy_uc, _) = policies::features::create_policy::di::embedded::make_use_case_embedded(&config.database.url)
    .await?;
#[cfg(not(feature = "embedded"))]
let (create_policy_uc, _) = policies::features::create_policy::di::make_use_case_mem()
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
cargo test -p hodei-artifacts-api test_create_policy

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
- Manejo de errores tipado con `CreatePolicyError`
