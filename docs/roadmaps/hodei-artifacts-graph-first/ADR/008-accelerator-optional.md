# ADR-008 — Dependency Accelerator por snapshot, sin cambiar protocolos estándar
**Estado:** ACEPTADA COMO EXPERIMENTO de H7, activación por benchmark.
**Decisión:** snapshots verificables de resolver efectivo y delta CAS; CLI opt-in para hidratar cache antes de Maven normal; shared volume en workers cuando disponible. Bundle tar solo si gana contra tráfico individual y shared cache; independencia del protocolo existente.
**Alternativas:** zip por hash `pom.xml` (rechazado por perfiles, BOM, plugins y SNAPSHOT), plugin Maven obligatorio (rechazado para MVP), cache de bytes sin permisos (rechazado).
**Gates:** cold build ahorro objetivo >=25% de fase dependencias para validar inversión; no regresiones warm, medidas bajo mismo hardware, permisos de cache y fallback correcto.
