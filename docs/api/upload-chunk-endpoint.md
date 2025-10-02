# Upload Chunk Endpoint

## Descripción

Endpoint REST para subir un chunk de un artifact en una sesión de upload existente.

## Endpoint

```
POST /api/v1/uploads/{upload_id}/chunks
```

## Parámetros

### Path Parameters

- **upload_id** (string, requerido): Identificador único de la sesión de upload

### Request Body (multipart/form-data)

- **chunk** (file, requerido): Contenido del chunk
- **chunk_number** (integer, requerido): Número secuencial del chunk (0-indexed)
- **total_chunks** (integer, requerido): Número total de chunks para este upload

## Respuestas

### 200 OK - Chunk subido exitosamente

```json
{
  "upload_id": "upload-session-123",
  "chunk_number": 0,
  "total_chunks": 10,
  "status": "chunk_received",
  "timestamp": "2024-01-01T12:00:00Z"
}
```

### 400 Bad Request - Solicitud inválida

```json
{
  "error": "Missing chunk file or metadata"
}
```

### 403 Forbidden - Acceso no autorizado

```json
{
  "error": "Unauthorized access to upload session"
}
```

### 404 Not Found - Sesión de upload no encontrada

```json
{
  "error": "Upload session not found"
}
```

### 409 Conflict - Chunk ya recibido

```json
{
  "error": "Chunk already received"
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
# Subir un chunk
curl -X POST http://localhost:8080/api/v1/uploads/upload-session-123/chunks \
  -F 'chunk=@chunk0.bin' \
  -F 'chunk_number=0' \
  -F 'total_chunks=10'
```

### HTTPie

```bash
# Subir un chunk
http --multipart POST http://localhost:8080/api/v1/uploads/upload-session-123/chunks \
  chunk@chunk0.bin \
  chunk_number=0 \
  total_chunks=10
```

## Arquitectura

### Flujo de Ejecución

1. **Handler** (`src/api/upload_artifact/handlers.rs::upload_chunk_handler`):
   - Valida el `upload_id` y los parámetros del chunk
   - Verifica la autorización del usuario
   - Procesa la solicitud multipart
   - Extrae el contenido del chunk
   - Crea `UploadChunkCommand`
   - Ejecuta el use case

2. **Use Case** (`crates/artifact/src/features/upload_artifact/use_case.rs`):
   - Valida el comando
   - Almacena el chunk usando el repositorio
   - Actualiza el progreso del upload
   - Retorna resultado de chunk upload o error

3. **Repository** (`crates/artifact/src/infrastructure/repository.rs`):
   - Interactúa con el almacenamiento de objetos
   - Guarda el contenido del chunk

4. **Storage** (`crates/artifact/src/infrastructure/storage.rs`):
   - Implementa el almacenamiento de objetos usando `object_store`
   - Proporciona interfaces para guardar y recuperar chunks

### Inyección de Dependencias

El use case se inicializa en `main.rs`:

```rust
// Build artifact upload use case via DI
let upload_artifact_uc = artifact::features::upload_artifact::di::make_use_case();
```

## Documentación OpenAPI

La documentación completa de la API está disponible en:

- **Swagger UI**: http://localhost:8080/api-docs
- **OpenAPI Spec**: http://localhost:8080/api-docs/openapi.json

## Tests

Los tests del endpoint se encuentran en `tests/integration/policies_api_test.rs`:

```bash
# Ejecutar tests del endpoint
cargo test -p hodei-artifacts-api test_upload_chunk

# Ejecutar todos los tests de integración
cargo test -p hodei-artifacts-api
```

## Métricas

El endpoint registra las siguientes métricas:

- `chunks_uploaded_total`: Contador de chunks subidos
- `http_requests_total`: Contador total de requests HTTP
- `http_request_duration_seconds`: Histograma de duración de requests

## Notas Técnicas

- El endpoint sigue la arquitectura VSA (Vertical Slice Architecture)
- Acepta uploads multipart/form-data
- Requiere autorización para subir chunks
- Manejo de errores tipado con `UploadArtifactError`
- Actualiza el progreso del upload al recibir cada chunk
