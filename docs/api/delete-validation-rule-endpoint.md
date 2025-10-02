# Delete Validation Rule Endpoint

## Descripción

Endpoint REST para eliminar una regla de validación del sistema.

## Endpoint

```
DELETE /api/v1/validation/rules/{rule_id}
```

## Parámetros

### Path Parameters

- **rule_id** (string, requerido): Identificador único de la regla de validación a eliminar

## Respuestas

### 200 OK - Regla de validación eliminada exitosamente

```json
{
  "message": "Validation rule deleted successfully",
  "rule_id": "custom-rule-1",
  "deleted_at": "2024-01-01T12:00:00Z"
}
```

### 404 Not Found - Regla de validación no encontrada

```json
{
  "error": "Validation rule not found"
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
# Eliminar una regla de validación
curl -X DELETE http://localhost:8080/api/v1/validation/rules/custom-rule-1
```

### HTTPie

```bash
# Eliminar una regla de validación
http DELETE http://localhost:8080/api/v1/validation/rules/custom-rule-1
```

### JavaScript (Fetch API)

```javascript
// Eliminar una regla de validación
async function deleteValidationRule(ruleId) {
  try {
    const response = await fetch(`http://localhost:8080/api/v1/validation/rules/${ruleId}`, {
      method: 'DELETE',
    });
    
    if (!response.ok) {
      const error = await response.json();
      console.error('Error:', error);
      return null;
    }
    
    const result = await response.json();
    console.log('Validation rule deleted:', result);
    return result;
  } catch (error) {
    console.error('Network error:', error);
    return null;
  }
}

// Uso
deleteValidationRule('custom-rule-1');
```

### Python (requests)

```python
import requests

def delete_validation_rule(rule_id):
    url = f"http://localhost:8080/api/v1/validation/rules/{rule_id}"
    
    try:
        response = requests.delete(url)
        response.raise_for_status()
        
        result = response.json()
        print(f"Validation rule deleted: {result}")
        return result
    except requests.exceptions.HTTPError as e:
        print(f"HTTP Error: {e}")
        print(f"Response: {e.response.json()}")
        return None
    except requests.exceptions.RequestException as e:
        print(f"Error: {e}")
        return None

# Uso
delete_validation_rule('custom-rule-1')
```

## Arquitectura

### Flujo de Ejecución

1. **Handler** (`src/api/validation_engine/handlers.rs::delete_validation_rule`):
   - Valida el `rule_id`
   - Crea `DeleteValidationRuleCommand`
   - Ejecuta el use case

2. **Use Case** (`crates/security/src/features/validation_rules/use_case.rs`):
   - Valida el comando
   - Verifica que la regla exista
   - Elimina la regla del almacenamiento
   - Retorna confirmación de eliminación o error

3. **Repository** (`crates/security/src/features/validation_rules/repository.rs`):
   - Interactúa con el almacenamiento de reglas de validación
   - Elimina la regla especificada

4. **Storage** (`crates/security/src/features/validation_rules/storage.rs`):
   - Implementa el almacenamiento de reglas de validación
   - Proporciona interfaces para eliminar reglas

### Inyección de Dependencias

El use case se inicializa en `main.rs`:

```rust
// Build validation rules use case via DI
let delete_validation_rule_uc = security::features::validation_rules::di::make_use_case();
```

## Documentación OpenAPI

La documentación completa de la API está disponible en:

- **Swagger UI**: http://localhost:8080/api-docs
- **OpenAPI Spec**: http://localhost:8080/api-docs/openapi.json

## Tests

Los tests del endpoint se encuentran en `tests/integration/policies_api_test.rs`:

```bash
# Ejecutar tests del endpoint
cargo test -p hodei-artifacts-api test_delete_validation_rule

# Ejecutar todos los tests de integración
cargo test -p hodei-artifacts-api
```

## Métricas

El endpoint registra las siguientes métricas:

- `validation_rules_deleted_total`: Contador de reglas de validación eliminadas
- `http_requests_total`: Contador total de requests HTTP
- `http_request_duration_seconds`: Histograma de duración de requests

## Notas Técnicas

- El endpoint sigue la arquitectura VSA (Vertical Slice Architecture)
- Permite eliminar reglas de validación del sistema
- Valida que la regla exista antes de eliminarla
- Manejo de errores tipado con `ValidationRulesError`
