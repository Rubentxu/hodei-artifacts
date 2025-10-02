# Validate Policy Endpoint

## Descripción

Endpoint REST para validar la sintaxis y semántica de una política de Cedar sin persistirla.

## Endpoint

```
POST /api/v1/policies/validate
```

## Parámetros

### Request Body

```json
{
  "policy_content": "permit(principal, action, resource);"
}
```

- **policy_content** (string, requerido): Contenido de la política en formato Cedar a validar

## Respuestas

### 2.00 OK - Resultado de validación

```json
{
  "is_valid": true,
  "errors": [],
  "warnings": []
}
```

### 400 Bad Request - Solicitud inválida

```json
{
  "error": "Invalid request"
}
```

## Ejemplos de Uso

### cURL

```bash
# Validar una política
curl -X POST http://localhost:8080/api/v1/policies/validate \
  -H "Content-Type: application/json" \
  -d '{
  "policy_content": "permit(principal, action, resource);"
}'
```

### HTTPie

```bash
# Validar una política
echo '{
  "policy_content": "permit(principal, action, resource);"
}' | http POST http://localhost:8080/api/v1/policies/validate
```

### JavaScript (Fetch API)

```javascript
// Validar una política
async function validatePolicy(policyContent) {
  try {
    const response = await fetch('http://localhost:8080/api/v1/policies/validate', {
      method: 'POST',
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
    
    const result = await response.json();
    console.log('Validation result:', result);
    return result;
  } catch (error) {
    console.error('Network error:', error);
    return null;
  }
}

// Uso
validatePolicy('permit(principal, action, resource);');
```

### Python (requests)

```python
import requests

def validate_policy(policy_content):
    url = "http://localhost:8080/api/v1/policies/validate"
    
    try:
        response = requests.post(url, json={"policy_content": policy_content})
        response.raise_for_status()
        
        result = response.json()
        print(f"Validation result: {result}")
        return result
    except requests.exceptions.HTTPError as e:
        print(f"HTTP Error: {e}")
        print(f"Response: {e.response.json()}")
        return None
    except requests.exceptions.RequestException as e:
        print(f"Error: {e}")
        return None

# Uso
validate_policy('permit(principal, action, resource);')
```

## Arquitectura

### Flujo de Ejecución

1. **Handler** (`src/api/policy_handlers.rs::validate_policy`):
   - Valida la solicitud
   - Crea `ValidatePolicyQuery`
   - Ejecuta el use case

2. **Use Case** (`crates/policies/src/features/validate_policy/use_case.rs`):
   - Valida el query
   - Valida la política usando el motor de Cedar
   - Retorna el resultado de validación o error

3. **Policy Engine** (`crates/policies/src/shared/application/engine.rs`):
   - Procesa la validación de la política de Cedar
   - Retorna resultados de validación

### Inyección de Dependencias

El use case se inicializa en `main.rs`:

```rust
// Build policies validate_policy use case via DI
#[cfg(feature = "embedded")]
let (validate_policy_uc, _) = policies::features::validate_policy::di::embedded::make_use_case_embedded(&config.database.url)
    .await?;
#[cfg(not(feature = "embedded"))]
let (validate_policy_uc, _) = policies::features::validate_policy::di::make_use_case_mem()
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
cargo test -p hodei-artifacts-api test_validate_policy

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
- Utiliza Cedar Policy Engine para la validación de políticas
- Soporta tanto almacenamiento en memoria (`mem`) como embebido (`embedded`)
- Incluye validación exhaustiva de entrada
- Manejo de errores tipado con `ValidatePolicyError`
- Permite validar políticas sin persistirlas
