# ADR-006 — Fast Path y Knowledge Path en el mismo monolito
**Estado:** ACEPTADA; rendimiento condicionado a medición.
**Decisión:** Fast Path = autenticación, autorización vigente, localizador de asset indexado, CAS streaming; Knowledge Path = TypeQL sobre relaciones, ingesta y análisis con límites y jobs separados. Moka/CDN datos inmutables y entradas con versiones/invalidación, sin cache de `ALLOW` insegura. Mantener TypeDB única BBDD.
**Alternativas:** query de grafo transitive por cada GET (rechazado), PostgreSQL read-model de entrada (rechazado sin benchmark), microservicios separados por módulo (rechazado por operación).
**Gates:** carga mixta de downloads + ingesta SBOM, p95/RSS, multitenant, saturación y backpressure.
