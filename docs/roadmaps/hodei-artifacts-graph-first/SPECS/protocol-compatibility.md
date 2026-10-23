# SPEC P-001 — Paridad observable Nexus Community Edition por protocolo

**Regla:** Nexus CE de referencia externa, version pinneada por ejecución. No confundir fuente `sonatype/nexus-public` Core con la distribución CE; no suponer código de ediciones Pro. Ver `SOURCES.md` matriz oficial. License review EPL-1.0 y condiciones de CE antes de copiar código; preferir reimplementación independiente por protocolo/contrato público.

## Matriz de estados
`UNPLANNED | DESIGNED | IMPLEMENTED | VERIFIED | PARTIAL | UNSUPPORTED | BLOCKED`. Marcar VERIFIED solo cuando cliente real + respuesta + versión Nexus reference + digest de fixture + resultado comparativo. Guardar matriz en `docs/roadmaps/.../compatibility-matrix.csv`, actualizar por hito; que la URL oficial anuncie formato no prueba la implementación de Hodei.

## Contrato transversal
- Endpoint y path, wire format, métodos y status HTTP, auth challenge, headers (Content-Type, Range, ETag, Last-Modified), checksum/digest, redirect (no bypass), revalidation, errores, redirecciones y timeouts, paginación, semántica de repo Hosted/Proxy/Group.
- Rutas y nombres maliciosos, UTF-8/percent-encoding, SSRF, size limits, bursts, concurrent redeploy/delete; principios `deny-by-default` y compensación ante cortes.
- Comparación diferencial de bytes/metadata, errores, orden de resolución y compat con clientes reales. Donde Nexus es ambiguo o históricamente inseguro, documentar deliberada diferencia de seguridad, no copiar bug.

## Wave H2 Raw
`PUT`, `GET`, `HEAD`, `DELETE`, path normalization, metadata, Range/conditional GET, checksum, immutable/redeploy policy, upload > RAM presupuesto. Consumidores curl + CLI; baseline Nexus Raw si está soportado por edición/version.

## Wave H3 Maven 2
GAV/classifier/extension, `pom.xml`, `maven-metadata.xml`, SHA checksums, snapshot/release, redeploy, Maven Wagon/Resolver, parent/BOM, plugins, transitive dependencies, remote TTL/negative cache, order group, authentication. Clientes `mvnw`, `gradlew`; diferencias 401/403/404 registradas. No alterar resoluciones entre builds.

## Wave H4 OCI
Registry `/v2/`, `GET/HEAD/POST/PATCH/PUT/DELETE` según subrecurso, blob upload/resume, digest/manifest/index/tags, auth Bearer challenge, media types, errors OCI, signatures/referrers si scope del hito; `podman`, `skopeo` cuando disponibles. CAS GC conserva hijos.

## Wave H8 npm/PyPI/APT
- npm: packument, tarball, scoped packages, dist-tags, integrity y tokens, `npm install` y `npm publish` donde se soporte.
- PyPI: PEP 503/691, wheel/sdist, `pip install`, `twine upload` si aplica, serialización metadata.
- APT: `Packages`, `Release/InRelease`, by-hash, GPG keys, cambios de índices atómicos; apt sandbox sin tocar host.
- Otros formatos de matriz CE: backlogs separados por coste/valor (NuGet/Helm/Go/Conan...). No prometer todos dentro de una release.

## Formato de registro por caso
`protocol,ce_version,client_version,operation,fixture_digest,expected_http,observed_http,expected_semantics,observed_semantics,status,evidence_digest,commit_sha,notes`. Identidad del binario Hodei es obligatoria.

## Conjunto de aceptación por wave
Máximo 1–3 E2E complejas, donde cada una incluye éxito + negativo discriminante; los desgloses HTTP pueden ser data-driven en unit/integ estable, no decenas de testcontainers. Ejecutar verdadero cliente binario, no mocks, en release gate.
