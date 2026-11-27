# Verificación del pasaporte de confianza

`POST /api/v1/passports/verify?repository=<clave>` devuelve un **veredicto**
sobre un pasaporte (diseño OCI con VSA firmada) que el llamante aporta. No
concede permisos: ninguna ruta de autorización lo acepta como credencial y la
respuesta lleva `grants_conferred: false`.

## Raíces de confianza: las fija el operador

Las raíces se leen del fichero `trust.passport_verification_file` al arrancar.
Nunca vienen del documento, de la petición ni de un upstream. Sin fichero, la
ruta responde `501`; un fichero inválido detiene el arranque.

Campos (`version: 1`; los campos desconocidos detienen el arranque):
- `policy`: la política bajo la que se escribió el pasaporte. Debe nombrar
  **exactamente un** `trusted_builders` (la cadena espera una sola identidad de
  constructor; una lista de varios se rechaza en lugar de honrar solo el primero).
- `issuer` (opcional): fija el emisor; la raíz que verifica debe llevar ese
  `issuer` (igual que `policy.pinned_issuer`). Sin pin, `RootEntry.issuer` es
  informativo.
- `max_age_secs`, `clock_skew_secs` (tope 300), `min_slsa_level` (1 a 3).
- `max_document_bytes` (8 MiB por defecto, tope 64 MiB), `body_deadline_secs`
  (1..=60, 15 por defecto) y `ledger_deadline_secs` (1..=30, 10 por defecto).
- `provenance_roots`: `lineage`, `revision`, `key_id`, `public_point_hex`
  (ECDSA P-256 sin comprimir; se valida que esté **en la curva**),
  `activated_at`, `retired_at`, `revoked_at`, `historical_ok`.
- `signers`: `id` (etiqueta, solo aparece en errores de arranque),
  `public_key_armor` (OpenPGP Ed25519), `valid_from`, `valid_until`,
  `revoked_at`, `historical_ok`.

### Retirar no es cortar: ventanas y `historical_ok`

El instante de congelación (`timeVerified`) lo declara el firmante, no el
servidor. Por eso, **por defecto**, una raíz o un firmante debe estar dentro de
su ventana en el momento de la verificación según el reloj del servidor, además
de en el instante declarado: una clave retirada y filtrada no puede fabricar un
pasaporte fechado dentro de su ventana. Con `historical_ok: true` el operador
acepta, tras la retirada, lo congelado mientras estaba activa (rotación sin
corte). Para retirar la confianza de verdad hay que **revocar** (`revoked_at`),
que corta siempre. Un instante declarado posterior a `ahora + skew` se rechaza.

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
| Revocación | Consulta el ledger de confianza (8 lecturas a la vez, plazo global `ledger_deadline_secs`); ledger caído o lento = `503` sin veredicto. |
| Coste | Tamaño, 512 blobs, decisiones citadas como mucho `MAX_BUNDLE_ASSETS` (comprobado antes de la criptografía), 4 verificaciones simultáneas (429; el hueco se mantiene durante la fase del ledger) y 20 s de plazo para la criptografía. |
| Cuerpo | Se lee antes de tomar hueco y con plazo (`body_deadline_secs`): 408 si no llega, 400 si se rompe, 413 si excede. Un cuerpo lento no consume huecos. |
| Auditoría | Allowed/Denied/Failed fuera del await del handler; sin secretos ni documento. |

## Respuestas

`200` válido; `422` inválido con `code`; `413` demasiado grande; `400` cuerpo
no es un pasaporte; `429` saturado; `501` sin raíces; `503` sin veredicto; `408` el cuerpo no llegó a tiempo.

## Límites conocidos

La caché de revocación es la del ledger de confianza (`trust.revocation_verify_secs`, 5 s por defecto): una revocación puede tardar hasta ese plazo en verse; con 0 cada petición consulta la autoridad. Una concesión que ha **expirado** se informa con el código `revoked` (el ledger no distingue "revocada" de "ya no vigente"); la explicación lo dice.

Los hashes SHA-1/SHA-224 con Ed25519 los rechaza la librería `pgp`; no hay
prueba propia de ello. La caché de revocación usa `trust.revocation_verify_secs`
(por defecto 5 s): una revocación puede tardar hasta ese plazo en verse.
