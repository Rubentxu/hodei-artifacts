# SPEC Q-001 — Benchmark y presupuestos de rendimiento

No asumir que TypeDB superará SQL ni que tar comprime JAR; optimizar con datos. **Hardware, versión TypeDB, tamaño de grafo y dataset pinneados** en cada benchmark.

## Métricas
- Fast read: GET/HEAD RPS, p50/p95/p99, per-tenant 401/403, cache hit/miss, `GraphLookup` duration, Cedar decision cost y pool saturation.
- Bytes: throughput sostenido y pico, RSS/bytes en vuelo, CPU/MB, tamaño de transfer, backpressure y límites de buffers por conexión.
- Writes: throughput `AssetPublished`, tiempo de confirmación TypeDB y CAS, contención de 1 path con N writers, recuperación de upload.
- Graph: PURL lookup, 1/3/5-hops, reverse-CVE impact en 100k/1M/10M edges de referencia, ingesta CycloneDX SPDX, memoria/p95 y límites.
- Recovery: pérdida proceso, caída DB, backup/restauración y checksum integral CAS.
- Accelerator: tiempo dependencia cold/warm, bytes, RTT, tamaño bundle y CPU.

## Baseline y SLO
H0 registra p50/p95/errores en API actual donde exista y hardware de benchmark. H2 fija un presupuesto candidato solo tras resultado baseline, no cifras inventadas. H3/H4 comparan Nexus CE de versión pinneada en misma carga/hardware o entorno equivalente reportando diferencias; ninguna mejora `2x` se proclama sin mediciones.

## Política de optimización
1. Query indexada/denormalización semántica acotada en TypeDB (`AssetLocator`), medir perfil.
2. Moka solo datos inmutables/resolución versionada; security freshness garantizada.
3. HTTP Keep-Alive, conexión pooling, streaming `Bytes`, singleflight por upstream, TTL/ETag.
4. CDN con auth scoped y digest, S3 upload multipart controlado y FS sendfile solo tras medir.
5. Throttle análisis grafo, jobs y grandes ingestas de SBOM bajo saturación de GET.
6. Benchmark produce decisión `KEEP | REWORK | REJECT` y explica ruido/confianza.

**Gate:** no meter stress/benchmark largo en suite <=15 min; AAT puntual en release candidato y evidencia trazable a digest, recursos limpiados.
