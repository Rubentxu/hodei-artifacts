# SPEC R-001 — TypeDB + CAS + streaming + recuperación
**Fuente**: `src/bootstrap.rs` legado Surreal, `artifact` upload, `distribution` handler `Vec<u8>`; objetivo FS primero, S3 después.

## Consistencia publicación contra dos sistemas (TypeDB y FS/S3)
No existe transacción ACID entre TypeDB y S3. Protocolo obligatorio:
1. Reservar UploadSession en TypeDB con clave idempotente, política/tenant, límites bytes y TTL; no asset público.
2. Recibir `Stream<Bytes>` a objeto temporal privado, simultáneamente hash SHA256 y size, backpressure y cuota; abortar/resumir según protocolo.
3. Finalizar blob inmutable verificado; guardar receipt que se puede verificar; fail antes de publicar si checksum incorrecto.
4. En WRITE txn TypeDB confirmar Asset → Blob, AssetKey único, estado Available, referencias y audit; gestionar carreras optimistic/version/constraints con retries finitos.
5. Responder éxito solo tras ambas condiciones. Si TypeDB commit falla, blob queda huérfano temporal y se GC tras período de gracia y reconciliación; si S3 falla, asset nunca se hace Available.
6. Borrar recurso lógico: no borrar bytes si quedan referencias vivas/sesiones/copia backup/pins; retención y grace period; protección ante concurrent publish/GC.

## Lecturas
- `HEAD/GET`: obtener `AssetLocator`, identidad/permiso, blob receipt/digest; streaming `Bytes` desde FS/S3 sin cargar todos los bytes. ETag fuerte por digest (representación concreta), Last-Modified, 304/412/416/206 según HTTP correcto, Range con límites.
- No enviar raw presigned URL que omita autorización y namespace; si se usa, TTL corto + binding permitido por policy + audit + límite de scope. Anti-traversal/SSRF/path normalization, MIME y uploads descomprimidos.
- Fetch proxy: revalidar upstream, negative TTL, request coalescing por llave, timeout y tamaño; no permitir cached shared response de un tenant sin auth.
- FS adapter usa archivos por hash; `ContentHash` distinto de `AssetId`. S3 metadata/vetag diferente de SHA256 multipart: no tratar ETag S3 como sha256.

## TypeDB operaciones
- Driver Rust async pin; pool de conexiones y transacciones cortas; no SCHEMA txn en hot path; migraciones versionadas antes del tráfico; un único punto de escritura semántica por slice.
- Audit event y `WorkItem` durable con `status, retry_after, attempt, causal_id, job_key` en misma transacción TypeDB del hecho pertinente. Lease con concurrencia probada y ACK idempotente; no usar si sin necesidad.
- Projection materializada en TypeDB solo si hay query repetida con perfil coste y owner/invalidación. Casos críticos se actualizan en misma txn cuando posible; derivados eventuales se versionan y no autorizan acceso crítico sin freshness.

## Crash matrix mínima
| Fallo | Estado esperado |
|---|---|
| Antes de finalizar blob | no Asset visible, upload expira/limpia |
| Blob durable, antes de commit TypeDB | blob huérfano no visible, candidato a GC |
| Tras TypeDB commit, antes de respuesta | reintento idempotente devuelve receipt existente |
| Durante GC vs nueva publicación | reference/pin evita borrar datos útiles |
| Mismo path publicado por dos writers | una transición autorizada, conflicto determinista |
| TypeDB indisponible | escrituras fallan cerrado, lecturas según política documentada sin acceso no autorizado |
| Proxy upstream caído | solo stale permitido según contrato repo, nunca fingir versión fresh |

## Backups
Export/import TypeDB versionado (3.4+ para API completa según documentación); snapshot compatible más manifest de blobs/digests/corte lógico; restaurar en DB nueva y verificar muestras + cobertura de referencias. RPO/RTO medidos, no HA CE. Logs/trace IDs sin credenciales.
