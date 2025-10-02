# Validate Artifact Endpoint

## Descripción

Endpoint REST para validar un artifact subido usando reglas de validación configuradas.

## Endpoint

```
POST /api/v1/artifacts/{hrn}/validate
```

## Parámetros

### Path Parameters

- **hrn** (string, requerido): HRN (Hierarchical Resource Name) del artifact a validar

### Request Body

```json
{
  "validation_rules": ["syft", "osv-scanner", "custom-rule-1"],
  "context": {
    "organization_id": "org-123",
    "project_id": "proj-456"
  }
}
```

- **validation_rules** (array of strings, opcional): Lista de reglas de validación a aplicar
- **context** (object, opcional): Contexto adicional para la validación

## Respuestas

### 200 OK - Resultado de validación

```json
{
  "hrn": "hrn:artifact:com.example:my-artifact:1.0.0",
  "validation_result": "passed",
  "validation_timestamp": "2024-01-01T12:00:00Z",
  "applied_rules": ["syft", "osv-scanner"],
  "findings": []
}
```

### 400 Bad Request - Solicitud inválida

```json
{
  "error": "Invalid HRN format"
}
```

### 404 Not Found - Artifact no encontrado

```json
{
  "error": "Artifact not found"
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
# Validar un artifact
curl -X POST http://localhost:8080/api/v1/artifacts/hrn:artifact:com.example:my-artifact:1.0.0/validate \
  -H "Content-Type: application/json" \
  -d '{
  "validation_rules": ["syft", "osv-scanner"]
}'
```

### HTTPie

```bash
# Validar un artifact
echo '{
  "validation_rules": ["syft", "osv-scanner"]
}' | http POST http://localhost:8080/api/v1/artifacts/hrn:artifact:com.example:my-artifact:1.0.0/validate
```

### JavaScript (Fetch API)

```javascript
// Validar un artifact
async function validateArtifact(hrn, validationRules) {
  try {
    const response = await fetch(`http://localhost:8080/api/v1/artifacts/${hrn}/validate`, {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ validation_rules: validationRules }),
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
validateArtifact('hrn:artifact:com.example:my-artifact:1.0.0', ['syft', 'osv-scanner']);
```

### Python (requests)

```python
import requests

def validate_artifact(hrn, validation_rules):
    url = f"http://localhost:8080/api/v1/artifacts/{hrn}/validate"
    
    try:
        response = requests.post(url, json={"validation_rules": validation_rules})
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
validate_artifact('hrn:artifact:com.example:my-artifact:1.0.0', ['syft', 'osv-scanner'])
```

## Arquitectura

### Flujo de Ejecución

1. **Handler** (`src/api/validation_engine/handlers.rs::validate_artifact`):
   - Valida el HRN del artifact
   - Procesa las reglas de validación solicitadas
   - Crea `ValidateArtifactCommand`
   - Ejecuta el use case

2. **Use Case** (`crates/security/src/features/artifact_validation/use_case.rs`):
   - Valida el comando
   - Obtiene el artifact del almacenamiento
   - Aplica las reglas de validación
   - Retorna resultado de validación o error

3. **Validation Service** (`crates/security/src/features/artifact_validation/service.rs`):
   - Gestiona las reglas de validación
   - Ejecuta los validadores configurados
   - Agrega los resultados de validación

4. **Validators** (`crates/security/src/features/artifact_validation/validators/`):
   - Implementaciones individuales de reglas de validación
   - SyftValidator, OsvScannerValidator, CustomRuleValidator, etc.

### Inyección de Dependencias

El use case se inicializa en `main.rs`:

```rust
// Build artifact validation use case via DI
let validate_artifact_uc = security::features::artifact_validation::di::make_use_case();
```

## Documentación OpenAPI

La documentación completa de la API está disponible en:

- **Swagger UI**: http://localhost:8080/api-docs
- **OpenAPI Spec**: http://localhost:8080/api-docs/openapi.json

## Tests

Los tests del endpoint se encuentran en `tests/integration/policies_api_test.rs`:

```bash
# Ejecutar tests del endpoint
cargo test -p hodei-artifacts-api test_validate_artifact

# Ejecutar todos los tests de integración
cargo test -p hodei-artifacts-api
```

## Métricas

El endpoint registra las siguientes métricas:

- `artifacts_validated_total`: Contador de artifacts validados
- `validation_findings_total`: Contador de findings de validación
- `http_requests_total`: Contador total de requests HTTP
- `http_request_duration_seconds`: Histograma de duración de requests

## Notas Técnicas

- El endpoint sigue la arquitectura VSA (Vertical Slice Architecture)
- Utiliza HRN (Hierarchical Resource Name) para identificar artifacts
- Soporta aplicación de múltiples reglas de validación
- Manejo de errores tipado con `ArtifactValidationError`
- Integración con herramientas de validación externas (Syft, OSV Scanner)
