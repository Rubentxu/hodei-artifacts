# Upload Progress Endpoint

## Descripción

Endpoint REST para obtener el progreso de un upload de artifact en curso.

## Endpoint

```
GET /api/v1/uploads/{upload_id}/progress
```

## Parámetros

### Path Parameters

- **upload_id** (string, requerido): Identificador único de la sesión de upload

## Respuestas

### 200 OK - Progreso del upload

```json
{
  "progress": {
    "upload_id": "upload-session-123",
    "total_size": 1048576,
    "uploaded_size": 524288,
    "percentage": 50,
    "status": "in_progress",
    "start_time": "2024-01-01T12:00:00Z",
    "last_update": "2024-01-01T12:05:00Z"
  },
  "poll_url": "/uploads/upload-session-123/progress",
  "websocket_url": "ws://localhost:3000/uploads/upload-session-123/progress/ws"
}
```

### 403 Forbidden - Acceso no autorizado

```json
{
  "error": "Unauthorized access to upload progress"
}
```

### 404 Not Found - Sesión de upload no encontrada

```json
{
  "error": "Upload session not found"
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
# Obtener el progreso de un upload
curl -X GET http://localhost:8080/api/v1/uploads/upload-session-123/progress
```

### HTTPie

```bash
# Obtener el progreso de un upload
http GET http://localhost:8080/api/v1/uploads/upload-session-123/progress
```

### JavaScript (Fetch API)

```javascript
// Obtener el progreso de un upload
async function getUploadProgress(uploadId) {
  try {
    const response = await fetch(`http://localhost:8080/api/v1/uploads/${uploadId}/progress`);
    
    if (!response.ok) {
      const error = await response.json();
      console.error('Error:', error);
      return null;
    }
    
    const progress = await response.json();
    console.log('Upload progress:', progress);
    return progress;
  } catch (error) {
    console.error('Network error:', error);
    return null;
  }
}

// Uso
getUploadProgress('upload-session-123');
```

### Python (requests)

```python
import requests

def get_upload_progress(upload_id):
    url = f"http://localhost:8080/api/v1/uploads/{upload_id}/progress"
    
    try:
        response = requests.get(url)
        response.raise_for_status()
        
        progress = response.json()
        print(f"Upload progress: {progress}")
        return progress
    except requests.exceptions.HTTPError as e:
        print(f"HTTP Error: {e}")
        print(f"Response: {e.response.json()}")
        return None
    except requests.exceptions.RequestException as e:
        print(f"Error: {e}")
        return None

# Uso
get_upload_progress('upload-session-123')
```

## Arquitectura

### Flujo de Ejecución

1. **Handler** (`src/api/upload_progress/handlers.rs::get_progress`):
   - Valida el `upload_id`
   - Verifica la autorización del usuario
   - Crea `GetProgressQuery`
   - Ejecuta el use case

2. **Use Case** (`crates/artifact/src/features/upload_progress/use_case.rs`):
   - Obtiene el progreso del upload del almacenamiento
   - Retorna el progreso o error

3. **Progress Storage** (`crates/artifact/src/features/upload_progress/storage.rs`):
   - Mantiene el estado de progreso de los uploads
   - Proporciona interfaces para consultar y actualizar progreso

### Inyección de Dependencias

El use case se inicializa en `main.rs`:

```rust
// Build upload progress use case via DI
let upload_progress_uc = artifact::features::upload_progress::di::make_use_case();
```

## Documentación OpenAPI

La documentación completa de la API está disponible en:

- **Swagger UI**: http://localhost:8080/api-docs
- **OpenAPI Spec**: http://localhost:8080/api-docs/openapi.json

## Tests

Los tests del endpoint se encuentran en `tests/integration/policies_api_test.rs`:

```bash
# Ejecutar tests del endpoint
cargo test -p hodei-artifacts-api test_get_progress

# Ejecutar todos los tests de integración
cargo test -p hodei-artifacts-api
```

## Métricas

El endpoint registra las siguientes métricas:

- `upload_sessions_total`: Contador de sesiones de upload
- `http_requests_total`: Contador total de requests HTTP
- `http_request_duration_seconds`: Histograma de duración de requests

## Notas Técnicas

- El endpoint sigue la arquitectura VSA (Vertical Slice Architecture)
- Requiere autorización para acceder al progreso
- Proporciona URLs para polling y websockets
- Manejo de errores tipado con `ProgressError`
