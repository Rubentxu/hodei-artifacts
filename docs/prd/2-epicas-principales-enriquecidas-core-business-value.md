# 2. Épicas Principales Enriquecidas (Core Business Value)

## Épica E1: 🔄 Artifact Lifecycle Management
**Objetivo**: Gestión completa del ciclo de vida de artefactos  
**Valor de Negocio**: Funcionalidad core - sin esto no hay producto  
**Complejidad**: ⭐⭐⭐ (Alta)  
**Flujo Event Storming**: Flujo 3 (Ingesta de Artefactos)  
**Eventos Clave**: ArtifactUploadStarted, ArtifactUploaded, ArtifactUploadFailed, ArtifactValidationFailed, DuplicateArtifactDetected, ArtifactHashCalculated, ArtifactMetadataEnriched  
**Investigación Técnica**: [Ver documentación técnica completa](technical-research-e1-e2.md#epic-e1-artifact-lifecycle-management)

### Features Principales con Contexto de Eventos (20 features)
| Feature ID | Nombre | Descripción | Eventos Relacionados | Use Cases | Prioridad | Estimación | Dependencies |
|------------|--------|-------------|---------------------|-----------|-----------|------------|--------------|
| E1.F01 | **Artifact Upload Core** | Upload básico con hash SHA-256 | ArtifactUploadStarted, ArtifactUploaded, ArtifactHashCalculated | Publicar Artefacto, Validar Artefacto Antes de Publicar | P0 | 13 pts | - |
| E1.F02 | **Artifact Upload Multipart** | Upload streaming para archivos >100MB | ArtifactUploadStarted, ArtifactUploaded, UploadProgressUpdated | Publicar Artefacto | P0 | 8 pts | E1.F01 |
| E1.F03 | **Artifact Metadata Extraction** | Extracción automática metadata por tipo | ArtifactMetadataEnriched | Ver Metadatos de Artefacto, Editar Metadatos de Artefacto | P1 | 5 pts | E1.F01 |
| E1.F04 | **Artifact Validation Engine** | Validación sintáctica y semántica | ArtifactValidationFailed | Validar Artefacto Antes de Publicar | P0 | 8 pts | E1.F01 |
| E1.F05 | **Duplicate Detection** | Detección y manejo duplicados | DuplicateArtifactDetected | Detectar y Gestionar Artefactos Duplicados | P1 | 5 pts | E1.F01 |
| E1.F06 | **Upload Progress Tracking** | Seguimiento progreso upload tiempo real | UploadProgressUpdated | Publicar Artefacto | P2 | 3 pts | E1.F02 |
| E1.F07 | **Batch Upload Operations** | Subida múltiples artefactos en lote | ArtifactUploadStarted, ArtifactUploaded (múltiple) | Publicar Artefacto | P2 | 8 pts | E1.F01 |
| E1.F08 | **Upload Resume Support** | Reanudar uploads interrumpidos | ArtifactUploadStarted, ArtifactUploaded | Publicar Artefacto | P2 | 5 pts | E1.F02 |
| E1.F09 | **Artifact Versioning Logic** | Lógica semantic versioning + tags | ArtifactMetadataEnriched | Publicar Artefacto | P1 | 5 pts | E1.F01 |
| E1.F10 | **Content-Type Detection** | Auto-detección tipo MIME avanzada | ArtifactMetadataEnriched | Publicar Artefacto | P2 | 3 pts | E1.F03 |
| E1.F11 | **Upload Bandwidth Throttling** | Control ancho banda uploads | UploadBandwidthLimited | Publicar Artefacto | P2 | 3 pts | E1.F02 |
| E1.F12 | **Artifact Preview Generation** | Thumbnails/previews para UI | ArtifactPreviewGenerated | Ver Metadatos de Artefacto | P3 | 5 pts | E1.F03 |
| E1.F13 | **Upload Analytics Events** | Eventos telemetría detallados | Todos eventos upload | Publicar Artefacto | P1 | 3 pts | E1.F01 |
| E1.F14 | **Multi-Repository Upload** | Upload simultáneo múltiples repos | ArtifactUploadStarted, ArtifactUploaded (múltiple) | Publicar Artefacto | P2 | 8 pts | E1.F01 |
| E1.F15 | **Upload Scheduling** | Programar uploads para horarios específicos | ArtifactUploadScheduled, ArtifactUploadStarted | Publicar Artefacto | P3 | 5 pts | E1.F01 |
| E1.F16 | **Artifact Transformation** | Conversión formatos on-the-fly | ArtifactTransformed, ArtifactMetadataEnriched | Aplicar Transformaciones a Artefactos | P3 | 13 pts | E1.F03 |
| E1.F17 | **Upload Conflict Resolution** | Estrategias resolución conflictos | UploadConflictDetected, UploadConflictResolved | Publicar Artefacto | P2 | 5 pts | E1.F05 |
| E1.F18 | **Artifact Checksums Multiple** | Soporte MD5, SHA-1, SHA-512 | ArtifactHashCalculated | Publicar Artefacto | P2 | 3 pts | E1.F01 |
| E1.F19 | **Upload Webhook Notifications** | Notificaciones externas upload | ArtifactUploaded (webhook) | Configurar Webhooks Personalizables | P2 | 3 pts | E1.F13 |
| E1.F20 | **Upload Performance Optimization** | Optimizaciones rendimiento específicas | UploadPerformanceOptimized | Publicar Artefacto | P1 | 8 pts | E1.F02 |

### Investigación Técnica Detallada

#### Dependencias Óptimas y Estado Actual

**Estado Actual Integrado:**
- **sha2 = "0.10"**: Hashing SHA-256 (ya integrado)
- **axum**: Soporte multipart habilitado (ya integrado)  
- **tokio**: Runtime async (ya integrado)
- **Deduplicación básica**: Usando solo SHA-256

**Dependencias Recomendadas:**
```toml
# Añadir a dependencias del workspace:
md5 = "0.7"          # Compatibilidad MD5
infer = "0.13"       # Detección tipo archivo  
bloom = "0.6"        # Bloom filters para deduplicación
bytes = "1.0"        # Manejo eficiente de bytes
moka = "0.12"        # Caching de alto rendimiento
zip = "0.6"          # Soporte archivos ZIP
```

**Características de Performance:**
- **Hashing**: SHA-256 ~300-500 MB/s, MD5 ~1-2 GB/s
- **Parsing multipart**: 10k-100k requests/segundo con Axum
- **Detección archivos**: <1ms por archivo con `infer`
- **Uso memoria**: Mínimo con patrones de streaming adecuados

#### Algoritmos Clave de Implementación

**1. Content-Defined Chunking (Rabin Fingerprint)**
```rust
// Algoritmo Rabin fingerprint para boundaries de chunks
fn compute_fingerprint(window: &VecDeque<u8>) -> u64 {
    let mut fingerprint = 0u64;
    for &byte in window {
        fingerprint = (fingerprint << 8) | byte as u64;
        fingerprint ^= RABIN_POLY; // Polinomio irreducible
    }
    fingerprint
}

fn is_boundary(fingerprint: u64) -> bool {
    fingerprint % TARGET_CHUNK_SIZE as u64 == 0
}
```

**2. Cómputo Paralelo de Hashes**
```rust
// Cómputo paralelo de hashes usando tasks tokio
async fn compute_all_hashes(chunk: Vec<u8>) -> HashResults {
    let chunk_arc = Arc::new(chunk);
    let (sha256, md5, sha512, blake3) = tokio::join!(
        compute_sha256(Arc::clone(&chunk_arc)),
        compute_md5(Arc::clone(&chunk_arc)), 
        compute_sha512(Arc::clone(&chunk_arc)),
        compute_blake3(Arc::clone(&chunk_arc))
    );
    HashResults { sha256, md5, sha512, blake3 }
}
```

**3. Scalable Bloom Filters**
```rust
// Scalable bloom filter para deduplicación
struct ScalableBloomFilter {
    filters: Vec<BloomFilter>,
    current_filter: usize,
    max_filters: usize,
}

impl ScalableBloomFilter {
    fn grow(&mut self) {
        if self.current_filter + 1 >= self.max_filters {
            self.compact();
            return;
        }
        
        let new_capacity = self.filters[self.current_filter].capacity() * 2;
        let new_filter = BloomFilter::with_rate(ERROR_RATE, new_capacity);
        
        self.filters.push(new_filter);
        self.current_filter += 1;
    }
}
```

#### Patrones de Integración

**Multipart Streaming Handler**
```rust
async fn handle_multipart_streaming(mut multipart: Multipart) -> Result<UploadResponse> {
    let mut sha256 = Sha256::new();
    let mut total_size = 0;
    
    while let Some(field) = multipart.next_field().await? {
        if field.name() == Some("file") {
            let mut chunk_buffer = vec![0u8; CHUNK_SIZE];
            
            while let Some(chunk) = field.chunk().await? {
                total_size += chunk.len();
                sha256.update(&chunk);
                emit_progress_event(total_size).await;
            }
        }
    }
    
    let final_hash = format!("{:x}", sha256.finalize());
    Ok(UploadResponse { checksum: final_hash })
}
```

**Memory Management con Object Pool**
```rust
struct StreamingUploadOptimizer {
    memory_pool: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl StreamingUploadOptimizer {
    async fn acquire_buffer(&self) -> Vec<u8> {
        let mut pool = self.memory_pool.lock().await;
        pool.pop().unwrap_or_else(|| vec![0u8; CHUNK_SIZE])
    }
    
    async fn release_buffer(&self, buffer: Vec<u8>) {
        let mut pool = self.memory_pool.lock().await;
        if pool.len() < MAX_BUFFERS {
            pool.push(buffer);
        }
    }
}
```

#### Prioridad de Implementación

**Orden de Prioridad E1:**
1. **P0**: Multipart streaming + hashing paralelo (E1.F02, E1.F20)
2. **P1**: Content-defined chunking + deduplicación (E1.F05, E1.F17)  
3. **P1**: Pipeline extracción metadata (E1.F03, E1.F10)
4. **P2**: Compresión avanzada + transformación (E1.F15, E1.F16)

### Políticas del Sistema Integradas
- **Política Flujo 3**: Siempre que `VersiónDePaquetePublicada` → Entonces disparar escaneo de seguridad, generación SBOM, generación de procedencia SLSA, e indexación
- **Validación Pre-upload**: Verificar permisos de escritura, configuración repositorio, y cuota almacenamiento

### Integraciones Cruzadas
- **E4 (ABAC)**: Validación de permisos antes de cualquier operación de upload
- **E6 (Security)**: Disparar escaneos de seguridad post-upload (política automática)
- **E3 (Search)**: Disparar indexación post-upload (política automática)
- **E5 (Repository)**: Verificar configuraciones de repositorio pre-upload

### Métricas de Éxito Extendidas
- **Throughput**: >500 uploads/minuto  
- **Latencia p95**: <100ms para uploads completos
- **Event Completion**: 100% de eventos emitidos correctamente
- **Policy Trigger**: 100% de políticas ejecutadas post-upload
- **Error Rate**: <1% de uploads fallidos

---

## Épica E2: 📥 Artifact Retrieval & Distribution
**Objetivo**: Descarga eficiente y distribución optimizada  
**Valor de Negocio**: Performance crítica para adopción  
**Complejidad**: ⭐⭐ (Media)  
**Flujo Event Storming**: Flujo 6 (Consumo de Artefactos)  
**Eventos Clave**: ArtifactDownloadRequested, ArtifactDownloadCompleted, PresignedUrlGenerated, ArtifactAccessGranted, ArtifactAccessDenied

### Features Principales con Contexto de Eventos (18 features)
| Feature ID | Nombre | Descripción | Eventos Relacionados | Use Cases | Prioridad | Estimación |
|------------|--------|-------------|---------------------|-----------|-----------|------------|
| E2.F01 | **Download Core Engine** | Descarga básica con autorización | ArtifactDownloadRequested, ArtifactDownloadCompleted, ArtifactAccessGranted/Denied | Descargar Artefacto, Evaluar Permiso de Lectura de Artefacto | P0 | 8 pts |
| E2.F02 | **Presigned URL Generation** | URLs temporales S3 | PresignedUrlGenerated | Descargar Artefacto, Download Link Sharing | P0 | 5 pts |
| E2.F03 | **Range Request Support** | HTTP Range para descargas parciales | ArtifactDownloadPartial, ArtifactDownloadCompleted | Descargar Artefacto | P1 | 5 pts |
| E2.F04 | **Download Resume Support** | Reanudar descargas interrumpidas | ArtifactDownloadResumed, ArtifactDownloadCompleted | Descargar Artefacto | P1 | 5 pts |
| E2.F05 | **Conditional Downloads** | ETag, If-Modified-Since, 304 responses | ArtifactDownloadConditional, ArtifactNotModified | Descargar Artefacto | P1 | 3 pts |
| E2.F06 | **Download Bandwidth Control** | Throttling dinámico | DownloadBandwidthLimited, DownloadBandwidthExceeded | Descargar Artefacto | P2 | 3 pts |
| E2.F07 | **CDN Integration** | Integración CloudFront/CloudFlare | ArtifactDownloadFromCDN, CDNCacheHit/Miss | Descargar Artefacto | P2 | 8 pts |
| E2.F08 | **Geographic Distribution** | Edge locations por región | ArtifactDownloadFromEdge, EdgeLocationSelected | Descargar Artefacto | P2 | 13 pts |
| E2.F09 | **Download Analytics** | Métricas detalladas uso | DownloadAnalyticsCollected, DownloadPatternDetected | Generar Informe de Uso de Artefactos | P1 | 3 pts |
| E2.F10 | **Batch Download API** | Descarga múltiples artefactos | ArtifactDownloadRequested (batch), ArtifactDownloadCompleted (batch) | Descargar Artefacto | P2 | 8 pts |
| E2.F11 | **Download Caching Strategy** | Cache inteligente cliente/proxy | DownloadFromCache, CacheHit/Miss | Descargar Artefacto | P1 | 5 pts |
| E2.F12 | **Download Authentication** | Múltiples métodos auth | ArtifactAccessGranted/Denied, AuthenticationMethodUsed | Descargar Artefacto | P0 | 5 pts |
| E2.F13 | **Download Rate Limiting** | Límites por usuario/IP | DownloadRateLimited, RateLimitExceeded | Descargar Artefacto | P1 | 3 pts |
| E2.F14 | **Download Virus Scanning** | Escaneo en tiempo real | DownloadVirusScanStarted, DownloadVirusScanCompleted | Download Virus Scanning | P2 | 8 pts |
| E2.F15 | **Download Compression** | Compresión on-the-fly | ArtifactDownloadCompressed, CompressionRatioCalculated | Descargar Artefacto | P2 | 5 pts |
| E2.F16 | **Download Link Sharing** | URLs compartibles con expiración | PresignedUrlGenerated, DownloadLinkShared | Download Link Sharing | P2 | 3 pts |
| E2.F17 | **Download Mirrors** | Múltiples fuentes redundantes | DownloadFromMirror, MirrorSelected | Descargar Artefacto | P3 | 8 pts |
| E2.F18 | **Download Statistics Dashboard** | Dashboard tiempo real | DownloadStatsUpdated, RealTimeDashboardRefreshed | Generar Informe de Uso de Artefactos | P2 | 5 pts |

### Decisiones Clave (Flujo 6)
- Validar que el principal tiene permisos de lectura (ABAC integration)
- Verificar que el artefacto esté en estado `Active` (no en cuarentena/baneado)
- Aplicar políticas de control de acceso basado en atributos
- Respetar decisiones de cuarentena/baneo del Flujo 4

### Integración con Políticas de Seguridad (Flujo 4)
- **Artefacto en cuarentena**: Bloquear descarga inmediatamente
- **Artefacto baneado**: Bloquear descarga y notificar seguridad  
- **Validar hash contra bloqueo global**: Prevenir descarga de artefactos maliciosos

### Use Cases Avanzados Integrados
- "Requerir Atestación para Descarga": Validar SBOM/firmas antes de permitir descarga
- "Bloquear Artefactos Inseguros": Integrar con vulnerabilidades críticas
- "Restringir Acceso por Contexto de Red": Control por IP/geolocalización

### Métricas de Éxito Extendidas
- **Latencia p99**: <50ms para decisiones de descarga
- **Disponibilidad**: 99.9% uptime para servicio de descarga
- **Seguridad**: 0% de descargas de artefactos en cuarentena/baneados
- **Throughput**: Soporte para >1000 descargas concurrentes
- **Cache Hit Rate**: >80% para contenido popular

### Investigación Técnica Detallada

#### Dependencias Óptimas y Estado Actual

**Estado Actual Integrado:**
- **aws-sdk-s3 ^1.0**: Integración básica S3 (ya integrado)
- **Presigned URLs**: Implementación básica existe
- **Core download**: Métodos directos y presigned disponibles

**Dependencias Recomendadas:**
```toml
# Añadir a dependencias del workspace:
http-range = "0.1.3"    # Parsing headers HTTP Range
etag = "0.2.0"         # Cómputo/validación ETag  
governor = "0.6.3"     # Rate limiting avanzado
brotli = "3.4.0"       # Soporte compresión Brotli
clamav-client = "0.5.0" # Escaneo virus ClamAV
async-process = "2.2.0" # Gestión async de subprocesos
maxminddb = "0.24.0"   # Geolocalización IP
url = "2.5.0"          # Manipulación URLs
```

#### Algoritmos Clave de Implementación

**1. HTTP Range Request Parsing**
```rust
fn parse_range_header(range_header: &str, total_size: u64) -> Result<Vec<ByteRange>> {
    if !range_header.starts_with("bytes=") {
        return Err(InvalidRangeHeader);
    }
    
    let ranges_str = &range_header[6..];
    let range_parts: Vec<&str> = ranges_str.split(',').collect();
    
    let mut parsed_ranges = Vec::new();
    for range_str in range_parts {
        let range = parse_single_range(range_str.trim(), total_size)?;
        parsed_ranges.push(range);
    }
    
    Ok(parsed_ranges)
}
```

**2. Token Bucket Rate Limiting**
```rust
struct TokenBucket {
    capacity: u64,        // Máximo tokens
    tokens: u64,          // Tokens actuales  
    last_refill: Instant, // Último tiempo de refill
    refill_rate: u64,     // Tokens por segundo
}

impl TokenBucket {
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill);
        let tokens_to_add = (elapsed.as_secs_f64() * self.refill_rate as f64) as u64;
        
        self.tokens = (self.tokens + tokens_to_add).min(self.capacity);
        self.last_refill = now;
    }
    
    fn consume(&mut self, tokens: u64) -> bool {
        self.refill();
        if self.tokens >= tokens {
            self.tokens -= tokens;
            true
        } else {
            false
        }
    }
}
```

**3. Conditional Header Validation**
```rust
fn validate_conditional_headers(artifact: &Artifact, headers: &Headers) -> Result<()> {
    let etag = format!("\"{}\"", artifact.checksum.0);
    let last_modified: DateTime<Utc> = artifact.created_at.0.into();
    
    // If-Match validation
    if let Some(if_match) = headers.if_match {
        if !if_match.precondition_passes(&etag) {
            return Err(PreconditionFailed);
        }
    }
    
    // If-Unmodified-Since validation  
    if let Some(if_unmodified_since) = headers.if_unmodified_since {
        if last_modified > if_unmodified_since.into() {
            return Err(PreconditionFailed);
        }
    }
    
    Ok(())
}
```

**4. On-the-fly Compression Pipeline**
```rust
async fn compress_response(response: Response, accept_encoding: &str) -> Response {
    let (mut parts, body) = response.into_parts();
    let bytes = hyper::body::to_bytes(body).await.unwrap();
    
    let (compressed_data, encoding) = if accept_encoding.contains("br") {
        let mut encoder = BrEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&bytes).unwrap();
        (encoder.finish().unwrap(), "br")
    } else if accept_encoding.contains("gzip") {
        // ... compresión gzip
    } else {
        return Response::from_parts(parts, hyper::Body::from(bytes));
    };
    
    parts.headers.insert("Content-Encoding", encoding.parse().unwrap());
    parts.headers.insert("Content-Length", compressed_data.len().into());
    
    Response::from_parts(parts, hyper::Body::from(compressed_data))
}
```

**5. Geographic Routing Algorithm**
```rust
fn find_closest_location(&self, country: Option<&str>, region: Option<&str>) -> &EdgeLocation {
    // Simple geographic matching
    if let (Some(country_code), Some(region_code)) = (country, region) {
        // Try exact match first
        if let Some(location) = self.edge_locations.iter().find(|loc| 
            loc.region == region_code && loc.country == country_code
        ) {
            return location;
        }
        
        // Fallback to country match
        if let Some(location) = self.edge_locations.iter().find(|loc| 
            loc.country == country_code
        ) {
            return location;
        }
    }
    
    // Default to first available location
    self.edge_locations.first().unwrap()
}
```

#### Patrones de Integración

**Enhanced Storage Interface**
```rust
#[async_trait]
trait ArtifactStorage {
    // Range support
    async fn get_object_range(
        &self, 
        repository_id: &RepositoryId, 
        artifact_id: &ArtifactId,
        start: u64,
        end: u64,
    ) -> Result<Vec<u8>, ArtifactError>;
    
    // ETag support  
    async fn get_object_etag(
        &self,
        repository_id: &RepositoryId,
        artifact_id: &ArtifactId,
    ) -> Result<String, ArtifactError>;
    
    // CDN integration
    async fn get_presigned_cdn_url(
        &self,
        repository_id: &RepositoryId,
        artifact_id: &ArtifactId,
        expires_in_secs: u64,
        cdn_config: &CdnConfig,
    ) -> Result<String, ArtifactError>;
}
```

**Enhanced Query Structure**
```rust
#[derive(Debug, Clone)]
struct GetArtifactQuery {
    // Existing fields...
    
    pub range_header: Option<String>,
    pub if_match: Option<String>,
    pub if_none_match: Option<String>,
    pub if_modified_since: Option<String>,
    pub accept_encoding: Option<String>,
    pub client_region: Option<String>, // For geographic routing
}
```

**Bandwidth Throttling Middleware**
```rust
async fn bandwidth_throttle_middleware(request: Request, next: Next) -> Result<Response> {
    let client_ip = extract_client_ip(&request);
    
    let mut buckets = BANDWIDTH_BUCKETS.lock().await;
    let bucket = buckets.entry(client_ip.clone())
        .or_insert_with(|| TokenBucket::new(BURST_CAPACITY, DEFAULT_RATE));
    
    let estimated_size = 1024 * 1024; // 1MB estimate
    
    if !bucket.consume(estimated_size) {
        return Err(BandwidthExceeded);
    }
    
    let response = next.run(request).await;
    
    // Update based on actual response size
    if let Some(content_length) = response.headers().get("Content-Length") {
        if let Ok(size) = content_length.to_str().and_then(|s| s.parse::<u64>().ok()) {
            let actual_tokens = size.min(BURST_CAPACITY);
            let mut buckets = BANDWIDTH_BUCKETS.lock().await;
            if let Some(bucket) = buckets.get_mut(&client_ip) {
                bucket.refill();
                if bucket.tokens < actual_tokens {
                    bucket.tokens = 0;
                } else {
                    bucket.tokens -= actual_tokens;
                }
            }
        }
    }
    
    Ok(response)
}
```

#### Estrategias de Optimización de Performance

1. **Streaming Responses**: Usar `ReaderStream` para archivos grandes para evitar carga en memoria
2. **Connection Pooling**: Aprovechar connection pooling de `reqwest` para integración CDN  
3. **Cache Optimization**: Implementar caching basado en ETag para reducir llamadas a storage
4. **Parallel Processing**: Usar tasks `tokio` para escaneo de virus y compresión concurrente
5. **Memory Reuse**: Implementar object pools para gestión de buffers
6. **Lazy Evaluation**: Procesar datos solo cuando se necesitan en el pipeline de respuesta

#### Prioridad de Implementación

**Orden de Prioridad E2:**
1. **P0**: Range requests + validación ETag (E2.F03, E2.F04, E2.F05)
2. **P1**: Bandwidth throttling + rate limiting (E2.F06, E2.F13)  
3. **P1**: Compresión on-the-fly (E2.F15)
4. **P2**: Integración CDN + virus scanning (E2.F07, E2.F14)
5. **P2**: Distribución geográfica (E2.F08)

---
