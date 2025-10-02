# Upload Artifact Endpoint

## Descripción

Endpoint REST para subir artifacts al sistema de almacenamiento.

## Endpoint

```
POST /api/v1/artifacts/upload
```

## Parámetros

### Request Body (multipart/form-data)

- **metadata** (JSON string, requerido): Metadatos del artifact a subir
- **file** (file, requerido): Contenido del artifact

### Metadata Fields

```json
{
  "coordinates": {
    "namespace": "com.example",
    "name": "my-artifact",
    "version": "1.0.0"
  },
  "file_name": "my-artifact.zip",
  "checksum": "a1b2c3d4e5f6...",
  "checksum_algorithm": "Sha256"
}
```

- **coordinates** (PackageCoordinates, requerido): Coordenadas del paquete
- **file_name** (string, requerido): Nombre del archivo
- **checksum** (string, opcional): Checksum del archivo
- **checksum_algorithm** (HashAlgorithm, opcional): Algoritmo de checksum (por defecto: Sha256)

## Respuestas

### 201 Created - Artifact subido exitosamente

```json
{
  "hrn": "hrn:artifact:com.example:my-artifact:1.0.0",
  "coordinates": {
    "namespace": "com.example",
    "name": "my-artifact",
    "version": "1.0.0"
  },
  "file_name": "my-artifact.zip",
  "content_length": 1024,
  "upload_timestamp": "2024-01-01T12:00:00Z"
}
```

### 400 Bad Request - Solicitud inválida

```json
{
  "error": "Missing metadata or file part"
}
```

### 409 Conflict - Artifact ya existe

```json
{
  "error": "Artifact already exists"
}
```

## Ejemplos de Uso

### cURL

```bash
# Subir un artifact
curl -X POST http://localhost:8080/api/v1/artifacts/upload \
  -F 'metadata={"coordinates":{"namespace":"com.example","name":"my-artifact","version":"1.0.0"},"file_name":"my-artifact.zip"}' \
  -F 'file=@my-artifact.zip'
```

### HTTPie (con httpie.multipart)

```bash
# Subir un artifact
http --multipart POST http://localhost:8080/api/v1/artifacts/upload \
  metadata='{"coordinates":{"namespace":"com.example","name":"my-artifact","version":"1.0.0"},"file_name":"my-artifact.zip"}' \
  file@my-artifact.zip
```

## Arquitectura

### Flujo de Ejecución

1. **Handler** (`src/api/upload_artifact/handlers.rs::upload_artifact_handler`):
   - Procesa la solicitud multipart
   - Extrae metadatos y contenido del archivo
   - Valida el checksum si se proporciona
   - Crea `UploadArtifactCommand`
   - Ejecuta el use case

2. **Use Case** (`crates/artifact/src/features/upload_artifact/use_case.rs`):
   - Valida el comando
   - Almacena el artifact usando el repositorio
   - Publica eventos de dominio
   - Retorna resultado de upload o error

3. **Repository** (`crates/artifact/src/infrastructure/repository.rs`):
   - Interactúa con el almacenamiento de objetos
   - Guarda el contenido del artifact

4. **Storage** (`crates/artifact/src/infrastructure/storage.rs`):
   - Implementa el almacenamiento de objetos usando `object_store`
   - Proporciona interfaces para guardar y recuperar artifacts

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
cargo test -p hodei-artifacts-api test_upload_artifact

# Ejecutar todos los tests de integración
cargo test -p hodei-artifacts-api
```

## Métricas

El endpoint registra las siguientes métricas:

- `artifacts_uploaded_total`: Contador de artifacts subidos
- `http_requests_total`: Contador total de requests HTTP
- `http_request_duration_seconds`: Histograma de duración de requests

## Notas Técnicas

- El endpoint sigue la arquitectura VSA (Vertical Slice Architecture)
- Acepta uploads multipart/form-data
- Valida checksums de archivos automáticamente
- Soporta diferentes algoritmos de hash
- Manejo de errores tipado con `UploadArtifactError`
- Publica eventos de dominio upon successful upload
