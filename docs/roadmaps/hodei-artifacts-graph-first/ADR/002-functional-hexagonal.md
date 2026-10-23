# ADR-002 — Núcleo funcional puro, ADTs y slices cohesionadas
**Estado:** ACEPTADA COMO DIRECCIÓN.
**Decisión:** reglas en funciones puras y tipos algebraicos Rust (newtypes/enum/Result), `decide/evolve` donde representen transiciones, casos de uso orquestan efectos, adaptadores TypeDB, Cedar, S3, HTTP. Contextos cohesionados por capacidad, puertos mínimos en capa de aplicación, composition root único.
**No hacer:** interpretación de pureza como prohibición de async en adapters, Haskell como copia literal en Rust, interfaces/factorías vacías para cada archivo, DTO de petición en kernel, Strings genéricos para ids, método de dominio que lee reloj.
**Consecuencias:** propiedades y tests rápidos de dominio, costos de copia/abstracción controlados, API estable por slice.
**Alternativas:** Clean Architecture ceremonial, ORM entidad mutada, traducción 1:1 Java. Rechazadas.
