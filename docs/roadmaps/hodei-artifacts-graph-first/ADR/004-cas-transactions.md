# ADR-004 — CAS externo con publicación TypeDB transaccional y reparación
**Estado:** ACEPTADA COMO DIRECCIÓN.
**Decisión:** escribir stream temporal/validar digest/asegurar durability CAS, luego confirmar TypeDB `Asset Available` y referencias; reintentos idempotentes. Garbage collector con grace period, referencias vivas y reconciliación bidireccional. No hay 2PC TypeDB/S3, por tanto reconocer ventanas de inconsistencia y compensarlas.
**Alternativas:** guardar bytes en TypeDB (rechazado por distribución), commit TypeDB antes de blob (rechazado asset visible roto), borrar automáticamente al fallar (peligroso ante referencias concurrentes).
**Gates:** pruebas crash matrix, concurrent replace vs GC, restore manifest y no pérdida de bytes.
**Implementación (HSP-104):** el garbage collector existe: raíces leídas de las fuentes reales en cada pasada (sin contadores propios), simulación por defecto, gracia por fecha de contenido con barrera bajo el cerrojo del almacén, marca y barrido durables y reanudables, retenciones con caducidad y tombstone auditable por borrado. Un estado desconocido no borra. Contrato y límites: `uat/UAT-OPERATIONS.md`.
