# Add Validation Rule Endpoint

## Descripción

Endpoint REST para añadir una nueva regla de validación al sistema.

## Endpoint

```
POST /api/v1/validation/rules
```

## Parámetros

### Request Body

```json
{
  "id": "new-custom-rule",
  "name": "New Custom Validation Rule",
  "description": "Description of the new custom validation rule",
  "type": "custom",
  "enabled": true,
  "configuration": {
    "rule_specific_param": "value"
  }
}
```

- **id** (string, requerido): Identificador único de la regla
- **name** (string, requerido): Nombre descriptivo de la regla
- **description** (string, opcional): Descripción detallada de la regla
- **type** (string, requerido): Tipo de regla (sbom, vulnerability, custom, etc.)
- **enabled** (boolean, opcional): Estado de la regla (por defecto: true)
- **configuration** (object, opcional): Configuración específica de la regla

## Respuestas

### 201 Created - Regla de validación añadida exitosamente

```json
{
  "id": "new-custom-rule",
  "name": "New Custom Validation Rule",
  "description": "Description of the new custom validation rule",
  "type": "custom",
  "enabled": true,
  "created_at": "2024-01-01T12:00:00Z"
}
```

### 400 Bad Request - Solicitud inválida

```json
{
  "error": "Invalid validation rule data"
}
```

### 409 Conflict - Regla de validación ya existe

```json
{
  "error": "Validation rule already exists"
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
# Añadir una regla de validación
curl -X POST http://localhost:8080/api/v1/validation/rules \
  -H "Content-Type: application/json" \
  -d '{
  "id": "new-custom-rule",
  "name": "New Custom Validation Rule",
  "description": "Description of the new custom validation rule",
  "type": "custom",
  "enabled": true
}'
```

### HTTPie

```bash
# Añadir una regla de validación
echo '{
  "id": "new-custom-rule",
  "name": "New Custom Validation Rule",
  "description": "Description of the new custom validation rule",
  "type": "custom",
  "enabled": true
}' | http POST http://localhost:8080/api/v1/validation/rules
```

### JavaScript (Fetch API)

```javascript
// Añadir una regla de validación
async function addValidationRule(ruleData) {
  try {
    const response = await fetch('http://localhost:8080/api/v1/validation/rules', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(ruleData),
    });
    
    if (!response.ok) {
      const error = await response.json();
      console.error('Error:', error);
      return null;
    }
    
    const rule = await response.json();
    console.log('Validation rule added:', rule);
    return rule;
  } catch (error) {
    console.error('Network error:', error);
    return null;
  }
}

// Uso
addValidationRule({
  id: 'new-custom-rule',
  name: 'New Custom Validation Rule',
  description: 'Description of the new custom validation rule',
  type: 'custom',
  enabled: true
});
```

### Python (requests)

```python
import requests

def add_validation_rule(rule_data):
    url = "http://localhost:8080/api/v1/validation/rules"
    
    try:
        response = requests.post(url, json=rule_data)
        response.raise_for_status()
        
        rule = response.json()
        print(f"Validation rule added: {rule}")
        return rule
    except requests.exceptions.HTTPError as e:
        print(f"HTTP Error: {e}")
        print(f"Response: {e.response.json()}")
        return None
    except requests.exceptions.RequestException as e:
        print(f"Error: {e}")
        return None

# Uso
add_validation_rule({
  'id': 'new-custom-rule',
  'name': 'New Custom Validation Rule',
  'description': 'Description of the new custom validation rule',
  'type': 'custom',
  'enabled': True
})
```

## Arquitectura

### Flujo de Ejecución

1. **Handler** (`src/api/validation_engine/handlers.rs::add_validation_rule`):
   - Procesa la solicitud
   - Valida los datos de la regla
   - Crea `AddValidationRuleCommand`
   - Ejecuta el use case

2. **Use Case** (`crates/security/src/features/validation_rules/use_case.rs`):
   - Valida el comando
   - Verifica que la regla no exista
   - Almacena la nueva regla usando el repositorio
   - Retorna la regla creada o error

3. **Repository** (`crates/security/src/features/validation_rules/repository.rs`):
   - Interactúa con el almacenamiento de reglas de validación
   - Guarda la nueva regla

4. **Storage** (`crates/security/src/features/validation_rules/storage.rs`):
   - Implementa el almacenamiento de reglas de validación
   - Proporciona interfaces para guardar reglas

### Inyección de Dependencias

El use case se inicializa en `main.rs`:

```rust
// Build validation rules use case via DI
let add_validation_rule_uc = security::features::validation_rules::di::make_use_case();
```

## Documentación OpenAPI

La documentación completa de la API está disponible en:

- **Swagger UI**: http://localhost:8080/api-docs
- **OpenAPI Spec**: http://localhost:8080/api-docs/openapi.json

## Tests

Los tests del endpoint se encuentran en `tests/integration/policies_api_test.rs`:

```bash
# Ejecutar tests del endpoint
cargo test -p hodei-artifacts-api test_add_validation_rule

# Ejecutar todos los tests de integración
cargo test -p hodei-artifacts-api
```

## Métricas

El endpoint registra las siguientes métricas:

- `validation_rules_added_total`: Contador de reglas de validación añadidas
- `http_requests_total`: Contador total de requests HTTP
- `http_request_duration_seconds`: Histograma de duración de requests

## Notas Técnicas

- El endpoint sigue la arquitectura VSA (Vertical Slice Architecture)
- Permite añadir nuevas reglas de validación al sistema
- Valida que el ID de la regla sea único
- Manejo de errores tipado con `ValidationRulesError`
- Soporta configuración específica para cada tipo de regla
