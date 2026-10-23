# ADR-004 — CAS externo con publicación TypeDB transaccional y reparación
**Estado:** ACEPTADA COMO DIRECCIÓN.
**Decisión:** escribir stream temporal/validar digest/asegurar durability CAS, luego confirmar TypeDB `Asset Available` y referencias; reintentos idempotentes. Garbage collector con grace period, referencias vivas y reconciliación bidireccional. No hay 2PC TypeDB/S3, por tanto reconocer ventanas de inconsistencia y compensarlas.
**Alternativas:** guardar bytes en TypeDB (rechazado por distribución), commit TypeDB antes de blob (rechazado asset visible roto), borrar automáticamente al fallar (peligroso ante referencias concurrentes).
**Gates:** pruebas crash matrix, concurrent replace vs GC, restore manifest y no pérdida de bytes.
