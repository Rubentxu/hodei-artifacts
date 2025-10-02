# Get Validation Rules Endpoint

## Descripción

Endpoint REST para obtener la lista de reglas de validación disponibles.

## Endpoint

```
GET /api/v1/validation/rules
```

## Parámetros

No se requieren parámetros.

## Respuestas

### 200 OK - Lista de reglas de validación

```json
{
  "rules": [
    {
      "id": "syft",
      "name": "Syft SBOM Generator",
      "description": "Generates Software Bill of Materials using Syft",
      "type": "sbom",
      "enabled": true
    },
    {
      "id": "osv-scanner",
      "name": "OSV Scanner",
      "description": "Scans for vulnerabilities using OSV Scanner",
      "type": "vulnerability",
      "enabled": true
    },
    {
      "id": "custom-rule-1",
      "name": "Custom Validation Rule 1",
      "description": "Custom validation rule for specific requirements",
      "type": "custom",
      "enabled": false
    }
  ]
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
# Obtener reglas de validación
curl -X GET http://localhost:8080/api/v1/validation/rules
```

### HTTPie

```bash
# Obtener reglas de validación
http GET http://localhost:8080/api/v1/validation/rules
```

### JavaScript (Fetch API)

```javascript
// Obtener reglas de validación
async function getValidationRules() {
  try {
    const response = await fetch('http://localhost:8080/api/v1/validation/rules');
    
    if (!response.ok) {
      const error = await response.json();
      console.error('Error:', error);
      return null;
    }
    
    const rules = await response.json();
    console.log('Validation rules:', rules);
    return rules;
  } catch (error) {
    console.error('Network error:', error);
    return null;
  }
}

// Uso
getValidationRules();
```

### Python (requests)

```python
import requests

def get_validation_rules():
    url = "http://localhost:8080/api/v1/validation/rules"
    
    try:
        response = requests.get(url)
        response.raise_for_status()
        
        rules = response.json()
        print(f"Validation rules: {rules}")
        return rules
    except requests.exceptions.HTTPError as e:
        print(f"HTTP Error: {e}")
        print(f"Response: {e.response.json()}")
        return None
    except requests.exceptions.RequestException as e:
        print(f"Error: {e}")
        return None

# Uso
get_validation_rules()
```

## Arquitectura

### Flujo de Ejecución

1. **Handler** (`src/api/validation_engine/handlers.rs::get_validation_rules`):
   - Procesa la solicitud
   - Crea `GetValidationRulesQuery`
   - Ejecuta el use case

2. **Use Case** (`crates/security/src/features/validation_rules/use_case.rs`):
   - Obtiene la lista de reglas de validación del repositorio
   - Retorna la lista de reglas o error

3. **Repository** (`crates/security/src/features/validation_rules/repository.rs`):
   - Interactúa con el almacenamiento de reglas de validación
   - Recupera la lista de reglas disponibles

4. **Storage** (`crates/security/src/features/validation_rules/storage.rs`):
   - Implementa el almacenamiento de reglas de validación
   - Proporciona interfaces para listar reglas

### Inyección de Dependencias

El use case se inicializa en `main.rs`:

```rust
// Build validation rules use case via DI
let get_validation_rules_uc = security::features::validation_rules::di::make_use_case();
```

## Documentación OpenAPI

La documentación completa de la API está disponible en:

- **Swagger UI**: http://localhost:8080/api-docs
- **OpenAPI Spec**: http://localhost:8080/api-docs/openapi.json

## Tests

Los tests del endpoint se encuentran en `tests/integration/policies_api_test.rs`:

```bash
# Ejecutar tests del endpoint
cargo test -p hodei-artifacts-api test_get_validation_rules

# Ejecutar todos los tests de integración
cargo test -p hodei-artifacts-api
```

## Métricas

El endpoint registra las siguientes métricas:

- `validation_rules_listed_total`: Contador de veces que se listan reglas de validación
- `http_requests_total`: Contador total de requests HTTP
- `http_request_duration_seconds`: Histograma de duración de requests

## Notas Técnicas

- El endpoint sigue la arquitectura VSA (Vertical Slice Architecture)
- Retorna todas las reglas de validación disponibles en el sistema
- Incluye información sobre el estado (enabled/disabled) de cada regla
- Manejo de errores tipado con `ValidationRulesError`
