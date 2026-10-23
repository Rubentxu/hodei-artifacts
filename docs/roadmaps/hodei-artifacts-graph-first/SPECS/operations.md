# SPEC O-001 — Operación TypeDB CE, cloud, backup/restore y migración Nexus

## Modelo inicial
Un proceso Rust API/registry escalable horizontalmente, TypeDB CE single-node para metadatos (no prometer HA), FS local en dev y S3/MinIO en despliegue compatible con red; workers de análisis separados cuando se justifique. Kubernetes readiness diferencia servidor levantado de DB/CAS usable; liveness no provoca loops ante caída DB.

## Backup coherente
- Quiesce escrituras o checkpoint de corte lógico documentado; export TypeDB (formato versión pinneada) y referencias a blobs; generar manifest de blobs/ETags/digests y verify sobre objetos presentes.
- Guardar en ubicación separada, cifrado y privilegios mínimos; restore nuevo entorno/DB + CAS y validar integridad/referencias y operaciones Raw/Maven/OCI; medir RPO/RTO.
- No copiar directorios RocksDB en caliente suponiendo consistencia. Export/import TypeDB 3.4+ según API documentación.
- Sin rollback de schema sin migración reversible verificada; plan backup/restore y parada controlada previo schema SCHEMA txn.

## Migración SurrealDB antigua
Inventario de tablas/usuarios/policies/secret refs antes de migrar, export respaldado, map old HRN a v2 tenant-scoped, conversion validate-only y modo `dry-run`; mantener dual-read temporal solo si contrato sin write split y fecha de retiro. Nunca tener TypeDB y SurrealDB ambos autoritativos por tiempo indefinido.

## Importación Nexus
Por repositorio y formato: consumir APIs/protocolos públicos o exports autorizados; manifest que asocia ruta externa/coordinate -> HRN+blob digest + ACL map; copiar/verificar bytes, monitorizar estado y reintentar; preservar/etiquetar diferencia semántica y datos desconocidos. No incorporar código Pro/CE oculto ni `sources.zip`.

## Seguridad y despliegue
Cedar secreto credenciales del registry, OIDC/API tokens y upstream secretos fuera DB semántica; TLS en tránsito/descifrado controlado, backup cifrado, límites payload y resource quotas. Versionado de Helm/containers, SBOM del producto, digest firmado opcional y SHA256SUMS. TypeDB CE no tiene HA multinodo probada con la documentación considerada; declarar este límite.

## Observabilidad
Trazas span `route, auth, typedb, blob, upstream, graph-projector, scanner` con correlation ID; alertas error/auth, TypeDB saturation, backup age, orphan blobs, dropped work, stale policy, scanner lag, p95. No logs de tokens ni presigned URLs.
