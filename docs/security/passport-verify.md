# Verificación del pasaporte de confianza

`POST /api/v1/passports/verify?repository=<clave>` devuelve un **veredicto**
sobre un pasaporte (diseño OCI con VSA firmada) que el llamante aporta. No
concede permisos: ninguna ruta de autorización lo acepta como credencial y la
respuesta lleva `grants_conferred: false`.

## Raíces de confianza: las fija el operador

Las raíces se leen del fichero `trust.passport_verification_file` al arrancar.
Nunca vienen del documento, de la petición ni de un upstream. Sin fichero, la
ruta responde `501`; un fichero inválido detiene el arranque.

Campos (`version: 1`): `policy`, `issuer` (opcional; fija el emisor de las
raíces), `max_age_secs`, `clock_skew_secs` (tope 300), `min_slsa_level`,
`max_document_bytes` (por defecto 8 MiB, tope 64 MiB), `provenance_roots`
(`lineage`, `revision`, `key_id`, `public_point_hex` ECDSA P-256, `issuer`,
`activated_at`, `retired_at`, `revoked_at`) y `signers` (clave OpenPGP Ed25519
con `valid_from`, `valid_until`, `revoked_at`). La rotación es añadir una
revisión y retirar la anterior.

## Controles

| Control | Efecto |
| --- | --- |
| Cedar `VerifyTrustPassport` sobre el repositorio del llamante | Sin la acción, 403 auditado antes de leer el documento. |
| Audiencia | Un pasaporte de otra cuenta o repositorio da `audience-mismatch` sin veredicto de validez. |
| Algoritmos | Lista blanca (ECDSA P-256, Ed25519); nada de `none` ni confusión de clave. |
| Bytes canónicos | La firma cubre los bytes canónicos; un manifiesto reserializado se rechaza. |
| Ventana de la raíz y del firmante | Activación, retirada y revocación se comprueban a la hora del servidor. |
| `nbf`/`exp` | Skew acotado a 300 s; antigüedad máxima configurable. |
| Emisor | Si `issuer` está fijado, debe coincidir con el de la raíz. |
| Revocación | Consulta el ledger de confianza; ledger caído = `503` sin veredicto. |
| Coste | Tamaño, 512 blobs, 256 consultas al ledger, 4 verificaciones simultáneas (429) y 20 s de plazo. |
| Auditoría | Allowed/Denied/Failed fuera del await del handler; sin secretos ni documento. |

## Respuestas

`200` válido; `422` inválido con `code`; `413` demasiado grande; `400` cuerpo
no es un pasaporte; `429` saturado; `501` sin raíces; `503` sin veredicto.

## Límites conocidos

Los hashes SHA-1/SHA-224 con Ed25519 los rechaza la librería `pgp`; no hay
prueba propia de ello. La caché de revocación usa `trust.revocation_verify_secs`
(por defecto 5 s): una revocación puede tardar hasta ese plazo en verse.
