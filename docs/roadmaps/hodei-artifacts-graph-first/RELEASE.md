# RELEASE — gate y entrega remota de CADA bloque

## Precondiciones
- Hito entrega función consumible. Trabajo en rama con diff claro, contrataciones/semver/versiones y documentación pertinente; el agente tiene autorización Git y release. El repo leído en octubre 2026 no mostraba releases por API y su versión Cargo raíz era 0.1.0: **no inventar que v0.2.0 está libre**; consultar tags/releases justo antes de etiquetar.
- Localizar autoridad CI del repo y wrappers reales. Corregir rutas de workflows/tests del H0, no instalar pipeline alternativo.

## Puerta
1. `git status --porcelain` sin cambios no previstos; `git fetch` + comprobar rama remota base, árbol limpio, no commits ajenos sin revisar.
2. Cargo fmt/check/clippy y suite selectiva + integración correspondiente **ejecutada**; registrar salida y tests no ejecutados sin ocultar.
3. Instalar o extraer archivo de distribución **empacado** y probar CLI/servidor reales; 1–3 UAT/E2E/AAT efímeras según riesgo, con SHA binario; no basta `cargo test`.
4. Seguridad: secretos fuera artefacto, tenant isolation/digests verificables, DB migrations/mig rollback cuando aplica; actualizar matriz compat con `PASS/PARTIAL/UNSUPPORTED/NOT_RUN`.
5. Revisión de PROMOTE/REPLACE/RETIRE/DEFER; retirar escenarios temporales tras registrar evidencia y limpiar procesos/containers/sesiones hasta 0; harness base versionado permanece.
6. Construir distribución adecuada (binarios Rust Linux, imagen OCI, CLI, helm chart si habilitado), SBOM de release, `SHA256SUMS`, manifest JSON, changelog. **No adjuntar `sources.zip`** como asset personalizado; GitHub puede generar automáticamente enlaces 'Source code' y no se deben confundir con asset publicado.
7. Merge remoto a main (o PR aprobado según branch protection) y verificar HEAD, árbol limpio, reejecutar gate sobre árbol final/candidato release, tag SemVer no reutilizado, GitHub Release con assets completos, checksums y links; verificar que remote tag apunta a main. No marcar RELEASED si no se observa realmente publicación.
8. Documentar `version, source sha, binary sha, typeql schema version, cedar schema rev, tests run/pass/fail/skips, clients/protocols, UAT deleted, risk, release URL`.

## Contrato de manifest de release (ejemplo conceptual)
```json
{
  "version": "<semver>",
  "source_commit": "<sha>",
  "schema": {"typedb": "<version>", "cedar": "<revision>"},
  "artifacts": [
    {"filename": "<dist>", "sha256": "<hex>", "kind": "binary"},
    {"filename": "SBOM.cdx.json", "sha256": "<hex>", "kind": "sbom"},
    {"filename": "SHA256SUMS", "kind": "checksum"}
  ],
  "verification": {"result": "PASS", "tests": 0, "skipped": 0, "uat_cleaned": true}
}
```
Estos son placeholders, no valores probados. `0 tests` NO es puerta verde; el agente debe rellenar valores reales y demostrar cobertura.
