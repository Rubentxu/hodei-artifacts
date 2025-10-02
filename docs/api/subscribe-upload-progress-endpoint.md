# Subscribe to Upload Progress Endpoint

## Descripción

Endpoint WebSocket para suscribirse a actualizaciones en tiempo real del progreso de un upload.

## Endpoint

```
GET /api/v1/uploads/{upload_id}/progress/ws
```

## Parámetros

### Path Parameters

- **upload_id** (string, requerido): Identificador único de la sesión de upload

## Respuestas

### 101 Switching Protocols - Conexión WebSocket establecida

```
Connection: upgrade
Upgrade: websocket
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

## Mensajes WebSocket

### Progreso del Upload (enviado por el servidor)

```json
{
  "type": "progress_update",
  "data": {
    "upload_id": "upload-session-123",
    "total_size": 1048576,
    "uploaded_size": 524288,
    "percentage": 50,
    "status": "in_progress",
    "timestamp": "2024-01-01T12:05:00Z"
  }
}
```

### Upload Completado (enviado por el servidor)

```json
{
  "type": "upload_completed",
  "data": {
    "upload_id": "upload-session-123",
    "artifact_hrn": "hrn:artifact:com.example:my-artifact:1.0.0",
    "status": "completed",
    "timestamp": "2024-01-01T12:10:00Z"
  }
}
```

### Upload Fallido (enviado por el servidor)

```json
{
  "type": "upload_failed",
  "data": {
    "upload_id": "upload-session-123",
    "error": "Checksum validation failed",
    "status": "failed",
    "timestamp": "2024-01-01T12:05:00Z"
  }
}
```

## Ejemplos de Uso

### JavaScript (WebSocket nativo)

```javascript
// Suscribirse a progreso de upload
function subscribeToUploadProgress(uploadId) {
  const ws = new WebSocket(`ws://localhost:8080/api/v1/uploads/${uploadId}/progress/ws`);
  
  ws.onopen = () => {
    console.log('Connected to upload progress WebSocket');
  };
  
  ws.onmessage = (event) => {
    const message = JSON.parse(event.data);
    
    switch (message.type) {
      case 'progress_update':
        console.log('Progress update:', message.data);
        updateProgressBar(message.data.percentage);
        break;
      case 'upload_completed':
        console.log('Upload completed:', message.data);
        showSuccessMessage();
        break;
      case 'upload_failed':
        console.log('Upload failed:', message.data);
        showErrorMessage(message.data.error);
        break;
    }
  };
  
  ws.onerror = (error) => {
    console.error('WebSocket error:', error);
  };
  
  ws.onclose = () => {
    console.log('Disconnected from upload progress WebSocket');
  };
  
  return ws;
}

// Uso
const websocket = subscribeToUploadProgress('upload-session-123');
```

### Python (websockets library)

```python
import asyncio
import websockets
import json

async def subscribe_to_upload_progress(upload_id):
    uri = f"ws://localhost:8080/api/v1/uploads/{upload_id}/progress/ws"
    
    try:
        async with websockets.connect(uri) as websocket:
            print("Connected to upload progress WebSocket")
            
            async for message in websocket:
                data = json.loads(message)
                
                if data['type'] == 'progress_update':
                    print(f"Progress update: {data['data']}")
                    update_progress_bar(data['data']['percentage'])
                elif data['type'] == 'upload_completed':
                    print(f"Upload completed: {data['data']}")
                    show_success_message()
                    break
                elif data['type'] == 'upload_failed':
                    print(f"Upload failed: {data['data']}")
                    show_error_message(data['data']['error'])
                    break
                    
    except websockets.exceptions.ConnectionClosed:
        print("Disconnected from upload progress WebSocket")
    except Exception as e:
        print(f"Error: {e}")

# Uso
asyncio.run(subscribe_to_upload_progress('upload-session-123'))
```

## Arquitectura

### Flujo de Ejecución

1. **Handler** (`src/api/upload_progress/handlers.rs::subscribe_progress`):
   - Valida el `upload_id`
   - Verifica la autorización del usuario
   - Establece la conexión WebSocket
   - Envía actualizaciones de progreso al cliente

2. **Progress Service** (`crates/artifact/src/features/upload_progress/service.rs`):
   - Gestiona las suscripciones WebSocket
   - Proporciona actualizaciones en tiempo real del progreso
   - Notifica a los clientes cuando el upload se completa o falla

3. **Progress Storage** (`crates/artifact/src/features/upload_progress/storage.rs`):
   - Mantiene el estado actual del progreso
   - Permite consultar el progreso para enviarlo a los suscriptores

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
# Ejecutar tests del endpoint WebSocket
cargo test -p hodei-artifacts-api test_subscribe_progress

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
- Requiere autorización para suscribirse al progreso
- Proporciona actualizaciones en tiempo real del progreso de upload
- Manejo de errores tipado con `ProgressError`
- Soporta múltiples suscriptores por sesión de upload
