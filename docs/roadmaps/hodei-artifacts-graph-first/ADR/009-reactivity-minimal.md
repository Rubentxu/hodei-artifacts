# ADR-009 — Eventos de dominio y trabajo durable mínimo, no Event Store universal
**Estado:** ACEPTADA COMO DIRECCIÓN.
**Decisión:** TypeDB confirma hechos + audit events + work items atómicamente cuando la vertical requiera asincronía. Workers con job idempotente, retry/correlación, estado `Pending|Leased|Completed|Failed` y lease por recurso, no colas no durables para decisiones críticas. Proyecciones calculadas dentro TypeDB solo si un consumidor real lo necesita.
**Alternativas:** Kafka + PostgreSQL + TypeDB con outbox (rechazado por complejidad); Event Sourcing completo (no aporta al MVP frente backup/export/journal selectivo); Redis como fuente de verdad (rechazado).
**Riesgos:** `snapshot isolation` puede permitir write-skew; usar control de versión por agregado y probar claims exclusivas/concurrentes. Audit trail no es equivalente a replay total; backups obligatorios.
