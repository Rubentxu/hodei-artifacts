# ADR-010 — Delivery-first testing y release por bloque
**Estado:** ACEPTADA COMO PLAN.
**Decisión:** usar `delivery-first-testing` SKILL y lentes READY, pruebas afectadas + unitarias/propiedades + integración selectiva + 1–3 UAT/E2E/AAT reales efímeras por bloque. Harness Base versionado, escenarios bajo `.sessions`/mktemp y limpiados incluso en fracaso. Si una garantía crítica nueva emerge, sustituir UAT por test permanente más barato y discriminante antes de borrar script. Presupuesto <=15 min suite aplicable; performance larga fuera.
**Release:** por bloque: compilar/instalar binario empacado, ejecutar cliente real, SHA/digest, limpiar sesiones, merge y tag/release+push remoto con assets completos, manifiesto, SBOM, checksums. Prohibido `sources.zip` como asset. Sin permisos: BLOCKED, nunca ficticio.
**Alternativas:** conservar cientos de E2E (rechazado por drift), solo mocks (rechazado por no probar consumidor), borrar todos sin sustitución (rechazado por integridad y seguridad).
