# Versioning Validation Endpoint

## Descripción

Endpoint REST para validar la compatibilidad de versiones de artifacts según las políticas de versioning configuradas.

## Endpoint

```
POST /api/v1/versioning/validate
```

## Parámetros

### Request Body

```json
{
  "artifact_coordinates": {
    "namespace": "com.example",
    "name": "my-artifact",
    "version": "1.0.0"
  },
  "target_version": "2.0.0",
  "validation_rules": ["semver-compatibility", "breaking-changes-policy"]
}
```

- **artifact_coordinates** (PackageCoordinates, requerido): Coordenadas del artifact a validar
- **target_version** (string, requerido): Versión objetivo para validar compatibilidad
- **validation_rules** (array of strings, opcional): Lista de reglas de validación de versioning a aplicar

## Respuestas

### 200 OK - Resultado de validación de versioning

```json
{
  "artifact_coordinates": {
    "namespace": "com.example",
    "name": "my-artifact",
    "version": "1.0.0"
  },
  "target_version": "2.0.0",
  "validation_result": "failed",
  "validation_timestamp": "2024-01-01T12:00:00Z",
  "applied_rules": ["semver-compatibility", "breaking-changes-policy"],
  "findings": [
    {
      "rule_id": "semver-compatibility",
      "severity": "error",
      "message": "Major version change detected without proper justification"
    }
  ]
}
```

### 400 Bad Request - Solicitud inválida

```json
{
  "error": "Invalid artifact coordinates or target version"
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
# Validar compatibilidad de versiones
curl -X POST http://localhost:8080/api/v1/versioning/validate \
  -H "Content-Type: application/json" \
  -d '{
  "artifact_coordinates": {
    "namespace": "com.example",
    "name": "my-artifact",
    "version": "1.0.0"
  },
  "target_version": "2.0.0",
  "validation_rules": ["semver-compatibility", "breaking-changes-policy"]
}'
```

### HTTPie

```bash
# Validar compatibilidad de versiones
echo '{
  "artifact_coordinates": {
    "namespace": "com.example",
    "name": "my-artifact",
    "version": "1.0.0"
  },
  "target_version": "2.0.0",
  "validation_rules": ["semver-compatibility", "breaking-changes-policy"]
}' | http POST http://localhost:8080/api/v1/versioning/validate
```

### JavaScript (Fetch API)

```javascript
// Validar compatibilidad de versiones
async function validateVersioning(artifactCoordinates, targetVersion, validationRules) {
  try {
    const response = await fetch('http://localhost:8080/api/v1/versioning/validate', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({
        artifact_coordinates: artifactCoordinates,
        target_version: targetVersion,
        validation_rules: validationRules
      }),
    });
    
    if (!response.ok) {
      const error = await response.json();
      console.error('Error:', error);
      return null;
    }
    
    const result = await response.json();
    console.log('Versioning validation result:', result);
    return result;
  } catch (error) {
    console.error('Network error:', error);
    return null;
  }
}

// Uso
validateVersioning(
  { namespace: 'com.example', name: 'my-artifact', version: '1.0.0' },
  '2.0.0',
  ['semver-compatibility', 'breaking-changes-policy']
);
```

### Python (requests)

```python
import requests

def validate_versioning(artifact_coordinates, target_version, validation_rules):
    url = "http://localhost:8080/api/v1/versioning/validate"
    
    try:
        response = requests.post(url, json={
            "artifact_coordinates": artifact_coordinates,
            "target_version": target_version,
            "validation_rules": validation_rules
        })
        response.raise_for_status()
        
        result = response.json()
        print(f"Versioning validation result: {result}")
        return result
    except requests.exceptions.HTTPError as e:
        print(f"HTTP Error: {e}")
        print(f"Response: {e.response.json()}")
        return None
    except requests.exceptions.RequestException as e:
        print(f"Error: {e}")
        return None

# Uso
validate_versioning(
    {"namespace": "com.example", "name": "my-artifact", "version": "1.0.0"},
    "2.0.0",
    ["semver-compatibility", "breaking-changes-policy"]
)
```

## Arquitectura

### Flujo de Ejecución

1. **Handler** (`src/api/versioning/handlers.rs::validate_versioning`):
   - Procesa la solicitud
   - Valida las coordenadas del artifact y la versión objetivo
   - Crea `ValidateVersioningCommand`
   - Ejecuta el use case

2. **Use Case** (`crates/distribution/src/features/versioning_validation/use_case.rs`):
   - Valida el comando
   - Obtiene las políticas de versioning del repositorio
   - Aplica las reglas de validación de versioning
   - Retorna resultado de validación o error

3. **Versioning Service** (`crates/distribution/src/features/versioning_validation/service.rs`):
   - Gestiona las reglas de validación de versioning
   - Ejecuta los validadores configurados
   - Agrega los resultados de validación

4. **Validators** (`crates/distribution/src/features/versioning_validation/validators/`):
   - Implementaciones individuales de reglas de validación de versioning
   - SemverCompatibilityValidator, BreakingChangesPolicyValidator, etc.

### Inyección de Dependencias

El use case se inicializa en `main.rs`:

```rust
// Build versioning validation use case via DI
let validate_versioning_uc = distribution::features::versioning_validation::di::make_use_case();
```

## Documentación OpenAPI

La documentación completa de la API está disponible en:

- **Swagger UI**: http://localhost:8080/api-docs
- **OpenAPI Spec**: http://localhost:8080/api-docs/openapi.json

## Tests

Los tests del endpoint se encuentran en `tests/integration/policies_api_test.rs`:

```bash
# Ejecutar tests del endpoint
cargo test -p hodei-artifacts-api test_validate_versioning

# Ejecutar todos los tests de integración
cargo test -p hodei-artifacts-api
```

## Métricas

El endpoint registra las siguientes métricas:

- `versioning_validations_total`: Contador de validaciones de versioning
- `versioning_findings_total`: Contador de findings de validación de versioning
- `http_requests_total`: Contador total de requests HTTP
- `http_request_duration_seconds`: Histograma de duración de requests

## Notas Técnicas

- El endpoint sigue la arquitectura VSA (Vertical Slice Architecture)
- Valida la compatibilidad de versiones según políticas definidas
- Soporta aplicación de múltiples reglas de validación de versioning
- Manejo de errores tipado con `VersioningValidationError`
