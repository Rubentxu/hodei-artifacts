# Riesgos y mitigaciones medibles

| Id / gravedad | Riesgo | Mitigación y verificación |
|---|---|---|
| R-01 P0 | Colisión HRN/Cedar intertenant | UID completo y propiedades round-trip + no alias sin alcance |
| R-02 P0 | SCP concediendo en lugar de limitar | autorizer compuesto, tests para SCP-only y deny overriding |
| R-03 P0 | contaminación estado Cedar concurrente | snapshot por evaluación, no `RwLock` global sustituido |
| R-04 P0 | publicar metadata sin blob o GC de blob referenciado | protocolo CAS-DB, crash matrix, reconciliación |
| R-05 P0 | TypeDB CE punto único de fallo | RPO/RTO honestos, restore validado, sin afirmar HA |
| R-06 P0 | leak S3/cross tenant por digest | auth antes de ruta/redirect, queries de cuenta, tests negativos |
| R-07 P1 | sobresaturación de TypeDB por análisis recursivo | limites, semáforos, paginación, profiling de selectividad y backpressure |
| R-08 P1 | fixtures/tests legacy acoplados | catálogo de garantías y promoción justificada antes de retire |
| R-09 P1 | duplicación semántica SBOM/vulnerability | unificación de afirmaciones y procedencia, no falsos match |
| R-10 P1 | deriva de compat Nexus | cliente real/version pin, matriz evidence, no '100%' sin cobertura |
| R-11 P1 | snapshots de dependencias inválidos por entorno | incluir perfiles, POM padres, plugins, mirrors, cache de cliente y digest |
| R-12 P1 | APIs externas con cuotas/error | import idempotente, backoff, snapshots de feeds y mocks contractuales |
| R-13 P1 | event jobs TypeDB convierten en broker genérico | mantener mínimo, lease/idempotencia, externalizar solo tras medición |
| R-14 P1 | contenido EPL de Nexus incorporado a MIT sin revisión | implementar protocolos independientes, revisión de procedencia/licencia |
| R-15 P1 | pruebas pesadas vuelven a crecer | <=15 min aplicable, selección por riesgo, AAT separadas, coste p95 |
| R-16 P1 | complejidad FP excesiva o sobrearquitectura de interfaces | elegir pure core/imperative shell simple; sin monadas artificiosas |
| R-17 P1 | versiones TypeDB/Cedar API incompatibles | pin de versiones y query/schema CI real; no copiar TypeQL sin compilar |
| R-18 P2 | snapshots TypeDB y CAS no consistentes | backup coordinado con manifest checkpoint/digests y restauración |
| R-19 P1 | archivos legacy a medias marcados listos | compilar por slice, inventario de deuda y estado RUN/NOT_RUN |
| R-20 P1 | sedación de alertas por VEX o heurísticas | separar assertion, observation y assessment; trazabilidad de fuentes |

**Un riesgo sin mitigación o evidencia pendiente no se marca cerrado.** Los umbrales se fijan por benchmark de baseline y hardware/entorno, nunca se presentan metas como mediciones.
