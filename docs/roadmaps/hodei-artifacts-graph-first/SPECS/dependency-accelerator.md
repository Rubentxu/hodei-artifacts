# SPEC D-001 — Dependency Accelerator guiado por grafo

**No cachear por `pom.xml` sin más.** Maven Resolver varía por parent POM, BOM, perfiles, mirrors, repositorios, plugins, settings, JDK, goals y SNAPSHOT. Gradle tiene caché propia que no puede rellenarse copiando repos Maven indiscriminadamente.

## Tipos de dominio
- `ProjectResolutionKey`: hash de todas las entradas que alteran resolución (build files + padres + configuración + herramienta + entorno + goals + repositories + auth scope no secrets).
- `ResolvedGraph`: versiones efectivas, edges e idempotent IDs, digests y procedencia.
- `DependencySnapshot`: `(graph_digest, resolution_key, toolchain_digest, policy_rev, asset_digests, completeness, expiry/freshness)`; no afirmar completas soluciones parciales.
- `CacheInventory`: digests presentes en worker; no confiar sin verificar al hidratar.
- `CachePlan`: ADT `Standard | Prefetch | Bundle | Delta | ReuseVolume`, calculado puro por RTT/bandwidth/count/CPU/disk y semántica de autenticación.

## Protocolos
Opt-in CLI `hodei deps hydrate --project ... --tool maven --target <isolated>` seguido por `mvn -Dmaven.repo.local=... verify`. Descargas de missing siguen vía protocolo Maven normal; no requiere plugin remoto. Gradle init plugin/hidrator específico después, preservando metadatos y locks de la versión soportada.

## Semántica bundle
Bundle opcional inmutable con manifiesto (coord/path/digest/source/repository/tenant/policy_rev), sin secretos. `tar` vs `tar.zst` se decide por benchmark: JAR ya comprimido. Verificación SHA-256 al recibir, no sobrescribir objetos locales incongruentes, zip-slip/path traversal imposible y rollback parcial sin estado corrupto.

## Integración en Fabric
Exponer API de caché/prewarm para worker Podman efímero, no incorporar Fabric como dependencia runtime de Hodei. Volumen de solo lectura si misma máquina; download delta entre redes; respetar cuenta de CI. Integración futura sin bloquear CLI Maven.

## Experimento de rendimiento
Comparar Maven nativo vs shared volume vs prefetch vs bundle/delta en cold/warm, RTT bajo/alto, 100/300/1000 assets, misses/parent/plugins, builds con snapshots dinámicos. Metrificar bytes transferidos, requests, CPU/RSS, tiempo preparación, tiempo total de build, tamaño bundle, cache hit; benchmark separados del gate <=15 min. Ahorro >=25% en fase cold CI es umbral de decisión propuesto no prometido.

## Tests
POM inalterado con `-Pprod` activo invalida clave; dos tenants mismos digests no autorizan bundle cruzado; SNAPSHOT cambia; missing digest fallback; corrupt local cache rehash; plugin deps incluidas; benchmark controlado y sin falsos ganadores.
