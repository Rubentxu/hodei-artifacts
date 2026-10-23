# ADR-001 — TypeDB 3 como único almacén de conocimiento
**Estado:** ACEPTADA COMO DIRECCIÓN; validación operativa pendiente en H2/H9.
**Contexto:** el diseño TypeDB + PostgreSQL + journal incrementaría sincronización, lag, duplicación y coste. SurrealDB legado no satisface estrategia seleccionada.
**Decisión:** TypeDB 3 version-pinned es la BBDD principal y única de metadatos, relaciones y estados de recursos. FS/S3 externo para bytes y Moka/HTTP cachés reconstruibles. No PostgreSQL, SurrealDB, Kafka, CDC ni Event Sourcing universal en primera arquitectura.
**Consecuencias:** consultas Hot Path deben ser TypeQL selectivas o proyecciones semánticas dentro TypeDB; validación de consistencia snapshot, riesgos de saturación y single-node CE. Backup/restauración demostrables. Se añade un read store especializado solo con benchmark reproducible + ADR explícito.
**Alternativas:** PostgreSQL CQRS (rechazado por complejidad prematura), dual-master (rechazado por consistencia), TypeDB+cache (elegida).
**Gates:** H2 transacciones y p95 GET, H9 restore y disponibilidad declarada.
