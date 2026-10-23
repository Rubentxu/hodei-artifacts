# ACCEPTANCE — pruebas UAT/E2E/AAT temporales por hito

**Oráculo:** comportamiento real del consumidor, no detalles internos. Ejecutar en sesión aislada con binario actual (HEAD/digest). Los scripts que se creen para cada aceptación se generan en `test-harness/.sessions/<run-id>` y se ELIMINAN al final del hito. Esta especificación permanece, no los scripts. Los casos below son escenarios planeados (NO EJECUTADOS).

| Hito | UAT / E2E efímera de la sesión | Negativo discriminante | AAT efímera específica | Garantía permanente sustituta |
|---|---|---|---|---|
| H0 | instalar binario/health actual + config válida | config inválida falla no exit-0 | ausencia procesos huérfanos | smoke health/config |
| H1 | principal de dos tenants y policies reales | `alice` A no lee B, SCP-only permit no da acceso | concurrencia de policy requests | propiedad HRN + test Cedar deny |
| H2 | curl PUT/HEAD/GET bytes Raw largos | digest malo, upload cortado, path traversal, cross tenant | RSS bounded, crash TypeDB/CAS | TypeDB txn, CAS checksum, GC |
| H3 | `mvn deploy` y `mvn verify` contra Hodei | redeploy bloqueado, proxy stale, 403/404 correcto | tiempo warm/cold (medido) | maven metadata/checksum/proxy integration |
| H4 | `podman push/pull` e índice multiarch si implementado | digest inválido, bearer error, upload resumido | backup+restore CAS/TypeDB | OCI digest/GC/manifest |
| H5 | importar SBOM CycloneDX/SPDX y consultar reverse deps | reingesta dup, PURL spoof, grafo cross tenant | query depth/time bounded | parser+TypeQL invariants |
| H6 | feed OSV fixture + scanner JSON → impacto UI/API | alias falso y VEX incorrecto no borran Finding | lag feed/impact | correlación/decision Cedar |
| H7 | CLI hydrate y Maven cold/warm | POM igual con perfil cambiado, corrupt cache, ACL | comparación A/B cold, warm, shared volume | plan pure, validación digest/key |
| H8 | npm publish/install + pip install + apt update cuando alcance | paquete scoped/private, metadata/digest/401 | throughput multiprotocol | contrato de cada formato |
| H9 | deploy/upgrade/restore+migración Nexus representativa | datos parciales y cancelación no dan falso éxito | RPO/RTO y carga mixta hasta presupuesto | restore contract/manifest |

## Detalle oráculo por categoría
- **Autorización:** misma identidad visual en dos cuentas mantiene HRN y Cedar UID distintos; listar 100 recursos de B desde A produce cero leaks; revocación se refleja en decisión de acceso según garantía acordada.
- **Integridad:** checksum de stream descargado igual al subido; fault injection no produce asset visible sin blob, ni borra blob vivo.
- **Protocolo:** comparar rutas/métodos/status/headers/checksum/metadata con baseline Nexus CE pinneada; anotar diferencia intencional por seguridad.
- **Conocimiento:** exactitud de `PURL`, `CVE aliases`, `SbomEvidence`; impacto query no confunde observación con afectación confirmada.
- **Rendimiento:** AAT con dataset reproducible, hardware y baseline; si escasea evidencia `INCONCLUSIVE`, no PASS.

## Lifecycle y borrado
PREPARE crea temp y registra PID+identidad, contenedor, imagen y red únicos. EXECUTE con timeouts y clientes oficiales. ASSERT positivo/negativo. EVIDENCE con SHA/artefacto/versión/resultado en recepción no sensible. CLEANUP en `EXIT/INT/TERM`, elimina escenarios, contenedores, redes, temporales, PID propios verificados. En error de cleanup estado no PASS.

- Cuando una UAT demuestra bug crítico no cubierto, `PROMOTE` una prueba más barata que falla con mutante relevante; retira UAT solo tras verificar sustitución.
- No borrar test legacy solo porque esté en `tests/e2e`: inventario + sustitución equivalente primero. Retirar tests efímeros generados en **todos** los bloques sin excepción.
