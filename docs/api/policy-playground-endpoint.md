# Policy Playground Endpoint

## Descripción

Endpoint REST para evaluar políticas ad-hoc contra escenarios de autorización sin persistirlas.

## Endpoint

```
POST /api/v1/policies/playground
```

## Parámetros

### Request Body

```json
{
  "policies": ["permit(principal, action, resource);"],
  "schema": "entity User = { };\nentity Resource = { };\naction read, write on Resource;",
  "entities": [
    {
      "uid": "User::\"alice\"",
      "attributes": {},
      "parents": []
    }
  ],
  "authorization_requests": [
    {
      "name": "Alice read access",
      "principal": "User::\"alice\"",
      "action": "Action::\"read\"",
      "resource": "Resource::\"document1\"",
      "context": {}
    }
  ],
  "options": {
    "include_diagnostics": true
  }
}
```

- **policies** (array of strings, requerido): Políticas de Cedar a evaluar
- **schema** (string, opcional): Esquema de Cedar opcional como string
- **entities** (array of EntityDefinitionApi, requerido): Entidades disponibles para la evaluación
- **authorization_requests** (array of PlaygroundScenarioApi, requerido): Escenarios de autorización a testear
- **options** (PlaygroundOptionsApi, opcional): Opciones para la evaluación

### EntityDefinitionApi

- **uid** (string, requerido): String UID de la entidad (e.g. User::"alice")
- **attributes** (object, requerido): Atributos de la entidad como JSON
- **parents** (array of strings, requerido): UIDs de entidades padre

### PlaygroundScenarioApi

- **name** (string, requerido): Nombre del escenario
- **principal** (string, requerido): String EUID del principal (e.g. User::"alice")
- **action** (string, requerido): String EUID de la acción (e.g. Action::"view")
- **resource** (string, requerido): String EUID del recurso (e.g. Resource::"doc1")
- **context** (object, opcional): Contexto JSON

### PlaygroundOptionsApi

- **include_diagnostics** (boolean, opcional): Incluir información de diagnóstico en los resultados (por defecto: true)

## Respuestas

### 200 OK - Resultados de evaluación del playground

```json
{
  "policy_validation": {
    "is_valid": true,
    "errors": [],
    "warnings": [],
    "policies_count": 1
  },
  "schema_validation": {
    "is_valid": true,
    "errors": [],
    "entity_types_count": 2,
    "actions_count": 2
  },
  "authorization_results": [
    {
      "scenario_name": "Alice read access",
      "decision": "Allow",
      "reasons": ["policy0"]
    }
  ],
  "statistics": {
    "total_scenarios": 1,
    "allow_count": 1,
    "deny_count": 0,
    "total_evaluation_time_us": 1200,
    "average_evaluation_time_us": 1200
  }
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
# Evaluar políticas en el playground
curl -X POST http://localhost:8080/api/v1/policies/playground \
  -H "Content-Type: application/json" \
  -d '{
  "policies": ["permit(principal == User::\"alice\", action, resource);"],
  "entities": [
    {
      "uid": "User::\"alice\"",
      "attributes": {},
      "parents": []
    }
  ],
  "authorization_requests": [
    {
      "name": "Alice read access",
      "principal": "User::\"alice\"",
      "action": "Action::\"read\"",
      "resource": "Resource::\"document1\"",
      "context": {}
    }
  ]
}'
```

### HTTPie

```bash
# Evaluar políticas en el playground
echo '{
  "policies": ["permit(principal == User::\"alice\", action, resource);"],
  "entities": [
    {
      "uid": "User::\"alice\"",
      "attributes": {},
      "parents": []
    }
  ],
  "authorization_requests": [
    {
      "name": "Alice read access",
      "principal": "User::\"alice\"",
      "action": "Action::\"read\"",
      "resource": "Resource::\"document1\"",
      "context": {}
    }
  ]
}' | http POST http://localhost:8080/api/v1/policies/playground
```

### JavaScript (Fetch API)

```javascript
// Evaluar políticas en el playground
async function policyPlayground(playgroundData) {
  try {
    const response = await fetch('http://localhost:8080/api/v1/policies/playground', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(playgroundData),
    });
    
    if (!response.ok) {
      const error = await response.json();
      console.error('Error:', error);
      return null;
    }
    
    const result = await response.json();
    console.log('Playground result:', result);
    return result;
  } catch (error) {
    console.error('Network error:', error);
    return null;
  }
}

// Uso
const playgroundData = {
  policies: ["permit(principal == User::\"alice\", action, resource);"],
  entities: [
    {
      "uid": "User::\"alice\"",
      "attributes": {},
      "parents": []
    }
  ],
  authorization_requests: [
    {
      "name": "Alice read access",
      "principal": "User::\"alice\"",
      "action": "Action::\"read\"",
      "resource": "Resource::\"document1\"",
      "context": {}
    }
  ]
};

policyPlayground(playgroundData);
```

### Python (requests)

```python
import requests

def policy_playground(playground_data):
    url = "http://localhost:8080/api/v1/policies/playground"
    
    try:
        response = requests.post(url, json=playground_data)
        response.raise_for_status()
        
        result = response.json()
        print(f"Playground result: {result}")
        return result
    except requests.exceptions.HTTPError as e:
        print(f"HTTP Error: {e}")
        print(f"Response: {e.response.json()}")
        return None
    except requests.exceptions.RequestException as e:
        print(f"Error: {e}")
        return None

# Uso
playground_data = {
    "policies": ["permit(principal == User::\"alice\", action, resource);"],
    "entities": [
        {
            "uid": "User::\"alice\"",
            "attributes": {},
            "parents": []
        }
    ],
    "authorization_requests": [
        {
            "name": "Alice read access",
            "principal": "User::\"alice\"",
            "action": "Action::\"read\"",
            "resource": "Resource::\"document1\"",
            "context": {}
        }
    ]
}

policy_playground(playground_data)
```

## Arquitectura

### Flujo de Ejecución

1. **Handler** (`src/api/policy_handlers.rs::policy_playground`):
   - Mapea DTOs de API a DTOs de dominio
   - Ejecuta el use case

2. **Use Case** (`crates/policies/src/features/policy_playground/use_case.rs`):
   - Valida las políticas y el esquema
   - Evalúa los escenarios de autorización
   - Retorna resultados de evaluación o error

3. **Policy Engine** (`crates/policies/src/shared/application/engine.rs`):
   - Procesa las políticas de Cedar
   - Evalúa las solicitudes de autorización

### Inyección de Dependencias

El use case se inicializa en `main.rs`:

```rust
// Build policies policy_playground use case via DI
#[cfg(feature = "embedded")]
let (policy_playground_uc, _) = policies::features::policy_playground::di::embedded::make_use_case_embedded(&config.database.url)
    .await?;
#[cfg(not(feature = "embedded"))]
let (policy_playground_uc, _) = policies::features::policy_playground::di::make_use_case_mem()
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
cargo test -p hodei-artifacts-api test_policy_playground

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
- Utiliza Cedar Policy Engine para la evaluación de políticas
- Soporta tanto almacenamiento en memoria (`mem`) como embebido (`embedded`)
- Incluye validación exhaustiva de entrada
- Manejo de errores tipado con `PolicyPlaygroundError`
- Permite evaluar políticas sin persistirlas
