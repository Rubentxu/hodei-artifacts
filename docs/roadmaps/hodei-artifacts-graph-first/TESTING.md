# TESTING — delivery-first-testing, router y lentes, mínimo permanente

**Skill real:** https://github.com/Rubentxu/agent-skill/tree/main/skills/delivery-first-testing ; leer `SKILL.md` y `references/lens-registry.json`. No copiar una versión inventada ni considerar `planned` ejecutable.

## Router por intención
| Intención / fase | Lentes READY del router | Decisión |
|---|---|---|
| Feature H1–H9 | `change-impact` → `ephemeral-uat` → `regression-promotion` | frontera real + riesgo + protección mínima |
| Reglas repetidas | `data-driven-design` | particiones y límites con IDs; aserción de cardinalidad |
| Tests dudosos | `mutation-guidance` | mutación dirigida, no cobertura superficial |
| Limpieza suite legacy | `harness-economics` | medir p50/p95 y demostrar equivalencia antes de retirar |
| Merge | `merge-assurance` | HEAD exacto, gates seleccionados ejecutados, no pases vacíos |
| Integración | `integration-assurance` | ensamblaje de contratos y corregir rojo |
| Release | `release-assurance` | instalar y ejercitar binario/artefactos reales |
| Fallos | `failure-triage` | defecto, obsoleto, infra, flaky, stale, fixture, regresión |

## Regla de oro del harness
- `test-harness/lib/` y perfiles/fixtures base **permanecen versionados**. Sesiones de ciclo en `test-harness/.sessions` ignorado o `mktemp` fuera del repo. Nunca almacenar scripts de aceptación del ciclo bajo `tests/e2e` persistentes.
- PREPARE → EXECUTE → ASSERT → EVIDENCE → CLEANUP. `set -euo pipefail`, timeout, puertos dinámicos, identidad de binario comprobada; aserciones positivas y negativas, polling por condición, sin `sleep 10` como garantía. Sesiones aisladas por run ID; no `pkill` global.
- Si test/cargo/client no existe: `NOT_RUN`/`BLOCKED`, jamás PASS. Detectar 0 tests, `SKIP`, stale binary/artefacto, exit falso 0 y procesos huérfanos.
- Antes de cerrar Hx: capturar reporte no sensible con SHA/version/digest, matriz garantía → evidencia, decisión `PROMOTE|REPLACE|RETIRE|DEFER` y cobertura sustitutiva si necesaria. **BORRAR todos los escenarios UAT/E2E/AAT efímeros del ciclo**, fixtures temporales, contenedores y procesos registrados. Si cleanup incompleto, no cerrar PASS.
- Test que proteja aislamiento, autorización, integridad, persistencia, concurrencia y recuperación **no se retira** sin equivalencia discriminante demostrada por prueba sustitutiva económica.

## Suite permanente óptima
1. Unidad: ADTs, `decide/evolve`, transiciones y errores tipados, HRN/PURL/digest; tests data-driven + proptest/seeds reproducibles.
2. Integración mínima: TypeDB transaction/constraints, blob FS/S3, Cedar schema & PEP/PDP, parser SBOM e invalidación; fixtures pequeñas y procesos efímeros por test donde sea razonable.
3. Contrato de protocolo: datadriven sobre HTTP parser/formatter contra fixtures/versiones; al menos un smoke de consumidor binario por wave en release gate, no cientos de E2E persistentes.
4. Mutación selectiva: HRN escaping, SCP boundary, checksum, GC, race condition. `KILLED` solo si aserción relevante falla tras ejecutarse; errores compilación `INVALID`.
5. AAT: latencia/RSS, recuperación TypeDB/CAS, seguridad multi-tenant y carga mixta; temporales de bloque, benchmark reproducible versionado (método/dataset), no suite permanente cara.

## Economía y gates
- Inner loop afectado objetivo 30–120s (hipótesis); merge ~2–5min; suite automatizada aplicable <=15min como guardarraíl, no excusa para omitir riesgos críticos; largos stress/restore fuera de quick gate y verificados en RC.
- Ratio LOC tests/LOC prod <= 1 como control; si supera investigar redundancia/no recortar garantías.
- Capturar `cargo metadata` y ejecutar Cargo del workspace real, no confiar en Makefile viejo. Herramientas: `cargo fmt`, `cargo clippy`, `cargo nextest run` si instalado, `cargo test` fallback, clientes de cada protocolo y TypeDB/Podman disponibles. Comandos exactos se adaptan tras H0, no inventar targets.
- Prohibido `|| true`, retry ilimitado, `exit 0` sin aserción o marcar test skipped como pass.

## Matriz efímera por hito
Ver `ACCEPTANCE.md`: 1–3 escenarios compuestos reales con positivos y negativos por bloque. Una AAT no sustituye pruebas de integración contra persistencia real ni unitarias de invariantes.

## Registro mínimo antes de borrar una sesión
```text
block=H3 head=<sha> artifact=<digest> client=mvn <version>
guarantee=proxy cached revalidation / group ordering
selected=affected+integration+ephemeral-uat+release
observed=RUN:3 PASS:3 FAIL:0 SKIP:0; elapsed=<medido>
regression=REPLACE con test data-driven <ruta de código> y revisión verde
cleanup=complete (processes=0, containers=0)
result=PASS / BLOCKED / FAIL
```
Nunca guardar datos personales/secrets ni artefactos gigantes en Git; el test es efímero, la garantía y evidencia resumida persisten en release notes.
