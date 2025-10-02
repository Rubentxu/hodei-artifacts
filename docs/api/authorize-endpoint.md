# Authorize Endpoint

## Descripción

Endpoint REST para procesar solicitudes de autorización usando el motor de políticas Cedar.

## Endpoint

```
POST /api/v1/auth/authorize
```

## Parámetros

### Request Body

```json
{
  "principal": "User::\"alice\"",
  "action": "Action::\"read\"",
  "resource": "Resource::\"document1\"",
  "context": {}
}
```

- **principal** (string, requerido): Principal que solicita la autorización
- **action** (string, requerido): Acción que se quiere realizar
- **resource** (string, requerido): Recurso sobre el que se quiere realizar la acción
- **context** (object, opcional): Contexto adicional para la autorización

## Respuestas

### 200 OK - Resultado de autorización

```json
{
  "decision": "Allow",
  "reasons": ["policy0"],
  "request_id": "550e8400-e29b-41d4-a716-446655440000",
  "timestamp": "2024-01-01T12:00:00Z"
}
```

### 400 Bad Request - Solicitud inválida

```json
{
  "error": "Principal cannot be empty"
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
# Procesar una solicitud de autorización
curl -X POST http://localhost:8080/api/v1/auth/authorize \
  -H "Content-Type: application/json" \
  -d '{
  "principal": "User::\"alice\"",
  "action": "Action::\"read\"",
  "resource": "Resource::\"document1\"",
  "context": {}
}'
```

### HTTPie

```bash
# Procesar una solicitud de autorización
echo '{
  "principal": "User::\"alice\"",
  "action": "Action::\"read\"",
  "resource": "Resource::\"document1\"",
  "context": {}
}' | http POST http://localhost:8080/api/v1/auth/authorize
```

### JavaScript (Fetch API)

```javascript
// Procesar una solicitud de autorización
async function authorize(requestData) {
  try {
    const response = await fetch('http://localhost:8080/api/v1/auth/authorize', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(requestData),
    });
    
    if (!response.ok) {
      const error = await response.json();
      console.error('Error:', error);
      return null;
    }
    
    const result = await response.json();
    console.log('Authorization result:', result);
    return result;
  } catch (error) {
    console.error('Network error:', error);
    return null;
  }
}

// Uso
const requestData = {
  principal: "User::\"alice\"",
  action: "Action::\"read\"",
  resource: "Resource::\"document1\"",
  context: {}
};

authorize(requestData);
```

### Python (requests)

```python
import requests

def authorize(request_data):
    url = "http://localhost:8080/api/v1/auth/authorize"
    
    try:
        response = requests.post(url, json=request_data)
        response.raise_for_status()
        
        result = response.json()
        print(f"Authorization result: {result}")
        return result
    except requests.exceptions.HTTPError as e:
        print(f"HTTP Error: {e}")
        print(f"Response: {e.response.json()}")
        return None
    except requests.exceptions.RequestException as e:
        print(f"Error: {e}")
        return None

# Uso
request_data = {
    "principal": "User::\"alice\"",
    "action": "Action::\"read\"",
    "resource": "Resource::\"document1\"",
    "context": {}
}

authorize(request_data)
```

## Arquitectura

### Flujo de Ejecución

1. **Handler** (`src/api/auth_handler.rs::authorize`):
   - Valida la solicitud de autorización
   - Procesa la autorización usando el motor de políticas
   - Retorna el resultado de autorización

2. **Authorization Engine** (`src/api/auth_handler.rs::process_authorization`):
   - Integra con el motor de políticas Cedar
   - Evalúa la solicitud de autorización
   - Retorna el resultado de la evaluación

### Inyección de Dependencias

El motor de autorización se inicializa en `main.rs`:

```rust
// Build authorization engine via DI
#[cfg(feature = "embedded")]
let (auth_engine, _) = iam::features::authorization::di::embedded::make_authorization_engine(&config.database.url)
    .await?;
#[cfg(not(feature = "embedded"))]
let (auth_engine, _) = iam::features::authorization::di::make_authorization_engine_mem()
    .await?;
```

## Documentación OpenAPI

La documentación completa de la API está disponible en:

- **Swagger UI**: http://localhost:8080/api-docs
- **OpenAPI Spec**: http://localhost:8080/api-docs/openapi.json

## Tests

Los tests del endpoint se encuentran en `src/api/auth_handler_test.rs`:

```bash
# Ejecutar tests del endpoint
cargo test -p hodei-artifacts-api test_authorize

# Ejecutar todos los tests de auth handler
cargo test -p hodei-artifacts-api auth_handler_test
```

## Métricas

El endpoint registra las siguientes métricas:

- `authorization_requests_total`: Contador de solicitudes de autorización
- `http_requests_total`: Contador total de requests HTTP
- `http_request_duration_seconds`: Histograma de duración de requests

## Notas Técnicas

- El endpoint sigue la arquitectura VSA (Vertical Slice Architecture)
- Utiliza Cedar Policy Engine para la evaluación de autorización
- Soporta tanto almacenamiento en memoria (`mem`) como embebido (`embedded`)
- Incluye validación exhaustiva de entrada
- Manejo de errores tipado con `AuthorizationError`
