# Update Versioning Configuration Endpoint

## Descripción

Endpoint REST para actualizar la configuración de versioning para un artifact específico.

## Endpoint

```
PUT /api/v1/versioning/config/{namespace}/{name}
```

## Parámetros

### Path Parameters

- **namespace** (string, requerido): Namespace del artifact
- **name** (string, requerido): Nombre del artifact

### Request Body

```json
{
  "versioning_policy": "semver",
  "auto_increment": true,
  "allow_prerelease": false,
  "rules": [
    {
      "type": "compatibility",
      "validator": "semver-compatibility",
      "severity": "error"
    },
    {
      "type": "changelog",
      "validator": "changelog-required",
      "severity": "warning"
    }
  ]
}
```

- **versioning_policy** (string, requerido): Política de versioning a aplicar (semver, custom, etc.)
- **auto_increment** (boolean, opcional): Si se debe auto incrementar la versión (por defecto: true)
- **allow_prerelease** (boolean, opcional): Si se permiten versiones prerelease (por defecto: false)
- **rules** (array of objects, requerido): Lista de reglas de validación de versioning

## Respuestas

### 200 OK - Configuración de versioning actualizada

```json
{
  "artifact_coordinates": {
    "namespace": "com.example",
    "name": "my-artifact"
  },
  "versioning_policy": "semver",
  "auto_increment": true,
  "allow_prerelease": false,
  "rules": [
    {
      "type": "compatibility",
      "validator": "semver-compatibility",
      "severity": "error"
    },
    {
      "type": "changelog",
      "validator": "changelog-required",
      "severity": "warning"
    }
  ],
  "updated_at": "2024-01-01T12:00:00Z"
}
```

### 400 Bad Request - Solicitud inválida

```json
{
  "error": "Invalid versioning configuration data"
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
# Actualizar configuración de versioning
curl -X PUT http://localhost:8080/api/v1/versioning/config/com.example/my-artifact \
  -H "Content-Type: application/json" \
  -d '{
  "versioning_policy": "semver",
  "auto_increment": true,
  "allow_prerelease": false,
  "rules": [
    {
      "type": "compatibility",
      "validator": "semver-compatibility",
      "severity": "error"
    },
    {
      "type": "changelog",
      "validator": "changelog-required",
      "severity": "warning"
    }
  ]
}'
```

### HTTPie

```bash
# Actualizar configuración de versioning
echo '{
  "versioning_policy": "semver",
  "auto_increment": true,
  "allow_prerelease": false,
  "rules": [
    {
      "type": "compatibility",
      "validator": "semver-compatibility",
      "severity": "error"
    },
    {
      "type": "changelog",
      "validator": "changelog-required",
      "severity": "warning"
    }
  ]
}' | http PUT http://localhost:8080/api/v1/versioning/config/com.example/my-artifact
```

### JavaScript (Fetch API)

```javascript
// Actualizar configuración de versioning
async function updateVersioningConfig(namespace, name, configData) {
  try {
    const response = await fetch(`http://localhost:8080/api/v1/versioning/config/${namespace}/${name}`, {
      method: 'PUT',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(configData),
    });
    
    if (!response.ok) {
      const error = await response.json();
      console.error('Error:', error);
      return null;
    }
    
    const config = await response.json();
    console.log('Versioning configuration updated:', config);
    return config;
  } catch (error) {
    console.error('Network error:', error);
    return null;
  }
}

// Uso
updateVersioningConfig('com.example', 'my-artifact', {
  versioning_policy: 'semver',
  auto_increment: true,
  allow_prerelease: false,
  rules: [
    {
      type: 'compatibility',
      validator: 'semver-compatibility',
      severity: 'error'
    },
    {
      type: 'changelog',
      validator: 'changelog-required',
      severity: 'warning'
    }
  ]
});
```

### Python (requests)

```python
import requests

def update_versioning_config(namespace, name, config_data):
    url = f"http://localhost:8080/api/v1/versioning/config/{namespace}/{name}"
    
    try:
        response = requests.put(url, json=config_data)
        response.raise_for_status()
        
        config = response.json()
        print(f"Versioning configuration updated: {config}")
        return config
    except requests.exceptions.HTTPError as e:
        print(f"HTTP Error: {e}")
        print(f"Response: {e.response.json()}")
        return None
    except requests.exceptions.RequestException as e:
        print(f"Error: {e}")
        return None

# Uso
update_versioning_config('com.example', 'my-artifact', {
  'versioning_policy': 'semver',
  'auto_increment': True,
  'allow_prerelease': False,
  'rules': [
    {
      'type': 'compatibility',
      'validator': 'semver-compatibility',
      'severity': 'error'
    },
    {
      'type': 'changelog',
      'validator': 'changelog-required',
      'severity': 'warning'
    }
  ]
})
```

## Arquitectura

### Flujo de Ejecución

1. **Handler** (`src/api/versioning/handlers.rs::update_versioning_config`):
   - Procesa la solicitud
   - Valida las coordenadas del artifact y los datos de configuración
   - Crea `UpdateVersioningConfigCommand`
   - Ejecuta el use case

2. **Use Case** (`crates/distribution/src/features/versioning_config/use_case.rs`):
   - Valida el comando
   - Verifica que el artifact exista
   - Actualiza la configuración de versioning en el repositorio
   - Retorna la configuración actualizada o error

3. **Repository** (`crates/distribution/src/features/versioning_config/repository.rs`):
   - Interactúa con el almacenamiento de configuraciones de versioning
   - Actualiza la configuración para el artifact especificado

4. **Storage** (`crates/distribution/src/features/versioning_config/storage.rs`):
   - Implementa el almacenamiento de configuraciones de versioning
   - Proporciona interfaces para actualizar configuraciones

### Inyección de Dependencias

El use case se inicializa en `main.rs`:

```rust
// Build versioning config use case via DI
let update_versioning_config_uc = distribution::features::versioning_config::di::make_use_case();
```

## Documentación OpenAPI

La documentación completa de la API está disponible en:

- **Swagger UI**: http://localhost:8080/api-docs
- **OpenAPI Spec**: http://localhost:8080/api-docs/openapi.json

## Tests

Los tests del endpoint se encuentran en `tests/integration/policies_api_test.rs`:

```bash
# Ejecutar tests del endpoint
cargo test -p hodei-artifacts-api test_update_versioning_config

# Ejecutar todos los tests de integración
cargo test -p hodei-artifacts-api
```

## Métricas

El endpoint registra las siguientes métricas:

- `versioning_config_updates_total`: Contador de actualizaciones de configuración de versioning
- `http_requests_total`: Contador total de requests HTTP
- `http_request_duration_seconds`: Histograma de duración de requests

## Notas Técnicas

- El endpoint sigue la arquitectura VSA (Vertical Slice Architecture)
- Permite actualizar la configuración de versioning para un artifact específico
- Valida que el artifact exista antes de actualizar la configuración
- Manejo de errores tipado con `VersioningConfigError`
