# Unsubscribe from Upload Progress Endpoint

## Descripción

Endpoint REST para desuscribirse de las actualizaciones del progreso de un upload.

## Endpoint

```
DELETE /api/v1/uploads/{upload_id}/progress/ws
```

## Parámetros

### Path Parameters

- **upload_id** (string, requerido): Identificador único de la sesión de upload

## Respuestas

### 200 OK - Desuscripción exitosa

```json
{
  "message": "Unsubscribed from upload progress",
  "upload_id": "upload-session-123",
  "timestamp": "2024-01-01T12:05:00Z"
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
# Desuscribirse del progreso de un upload
curl -X DELETE http://localhost:8080/api/v1/uploads/upload-session-123/progress/ws
```

### HTTPie

```bash
# Desuscribirse del progreso de un upload
http DELETE http://localhost:8080/api/v1/uploads/upload-session-123/progress/ws
```

### JavaScript (Fetch API)

```javascript
// Desuscribirse del progreso de un upload
async function unsubscribeFromUploadProgress(uploadId) {
  try {
    const response = await fetch(`http://localhost:8080/api/v1/uploads/${uploadId}/progress/ws`, {
      method: 'DELETE',
    });
    
    if (!response.ok) {
      const error = await response.json();
      console.error('Error:', error);
      return null;
    }
    
    const result = await response.json();
    console.log('Unsubscribed from upload progress:', result);
    return result;
  } catch (error) {
    console.error('Network error:', error);
    return null;
  }
}

// Uso
unsubscribeFromUploadProgress('upload-session-123');
```

### Python (requests)

```python
import requests

def unsubscribe_from_upload_progress(upload_id):
    url = f"http://localhost:8080/api/v1/uploads/{upload_id}/progress/ws"
    
    try:
        response = requests.delete(url)
        response.raise_for_status()
        
        result = response.json()
        print(f"Unsubscribed from upload progress: {result}")
        return result
    except requests.exceptions.HTTPError as e:
        print(f"HTTP Error: {e}")
        print(f"Response: {e.response.json()}")
        return None
    except requests.exceptions.RequestException as e:
        print(f"Error: {e}")
        return None

# Uso
unsubscribe_from_upload_progress('upload-session-123')
```

## Arquitectura

### Flujo de Ejecución

1. **Handler** (`src/api/upload_progress/handlers.rs::unsubscribe_progress`):
   - Valida el `upload_id`
   - Verifica la autorización del usuario
   - Elimina la suscripción WebSocket
   - Retorna confirmación de desuscripción

2. **Progress Service** (`crates/artifact/src/features/upload_progress/service.rs`):
   - Gestiona las suscripciones WebSocket
   - Permite eliminar suscripciones existentes
   - Notifica al cliente de la desuscripción

3. **Progress Storage** (`crates/artifact/src/features/upload_progress/storage.rs`):
   - Mantiene el estado actual del progreso
   - Permite eliminar suscripciones del almacenamiento

### Inyección de Dependencias

El servicio de progreso se inicializa en `main.rs`:

```rust
// Build upload progress service via DI
let upload_progress_service = artifact::features::upload_progress::di::make_service();
```

## Documentación OpenAPI

La documentación completa de la API está disponible en:

- **Swagger UI**: http://localhost:8080/api-docs
- **OpenAPI Spec**: http://localhost:8080/api-docs/openapi.json

## Tests

Los tests del endpoint se encuentran en `tests/integration/policies_api_test.rs`:

```bash
# Ejecutar tests del endpoint
cargo test -p hodei-artifacts-api test_unsubscribe_progress

# Ejecutar todos los tests de integración
cargo test -p hodei-artifacts-api
```

## Métricas

El endpoint registra las siguientes métricas:

- `websocket_connections_total`: Contador de conexiones WebSocket
- `upload_sessions_total`: Contador de sesiones de upload
- `http_requests_total`: Contador total de requests HTTP
- `http_request_duration_seconds`: Histograma de duración de requests

## Notas Técnicas

- El endpoint sigue la arquitectura VSA (Vertical Slice Architecture)
- Requiere autorización para desuscribirse del progreso
- Manejo de errores tipado con `ProgressError`
- Elimina suscripciones WebSocket activas
