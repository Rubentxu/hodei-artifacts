# Recibos de ejecución (`POST /api/v1/execution-receipts`)

HSP-37 / HSP-323 tramo 8. Este documento fija **qué prueba** un recibo de
ejecución y qué no, para que nadie lo lea como más de lo que es.

## Modelo de confianza

- **Lo que se comprueba:** el digest que declara el recibo coincide con los
  bytes que Hodei guarda para el artefacto. El servicio recalcula el sha256 de
  los bytes almacenados (en trozos, con tope de tamaño) y rechaza el recibo con
  `422 E_RECEIPT_DIGEST_MISMATCH` si difieren; en ese caso no se escribe nada.
- **Lo que NO se comprueba:** el productor **no está autenticado**. Quien posea
  la acción Cedar `IngestExecutionReceipt` sobre el repositorio puede escribir
  un recibo con cualquier `run_id` y cualquier `source_revision`. Los eventos de
  PipelineK no traen el digest ni la revisión: los declara el exportador.
- **Consecuencia:** el recibo se registra como evidencia `BuildMetadata` con
  verificación **Unverified** y método `exporter-declared`. **Nunca** deja la
  dimensión `builder_trust` en `Verified`, y por tanto una política de
  promoción que exija `builder_trust` verificado sigue denegando (lo demuestra
  `a_receipt_never_makes_the_builder_verified`).
- **Para llegar a `Verified`** hace falta un recibo firmado por una clave fijada
  del productor. Es el siguiente tramo y es una decisión de producto.

## Contrato v1

Esquema cerrado (`hodei.execution-receipt/1`, sin campos libres): productor
`pipelinek` en la versión fijada exacta (`0.97.0`), `run_id`, `outcome` (solo
`success` se enlaza), `source_revision` (hex de 40 o 64), `subject_digest`
(`sha256:`), `finished_at` (RFC 3339, no posterior a ahora + 5 min ni anterior a
2024-01-01). Ningún valor del recibo se repite en errores ni en la auditoría.

## Identidad, idempotencia y conflicto

La identidad externa es `{cuenta}/{run_id}/{digest}`:

- el mismo recibo repetido es idempotente (`201` y luego `200 already_accepted`);
- dos ejecuciones con el mismo digest son dos referencias independientes;
- el mismo run y digest con **contenido distinto** es `409 E_RECEIPT_CONFLICT`:
  no se reemplaza en silencio.

## Límites y operación

- Artefactos mayores que el tope (`ReceiptLimits`, 1 GiB por defecto) dan
  `413 E_ARTIFACT_TOO_LARGE`, decidido con `length` antes de leer un byte.
- Un semáforo acota las admisiones simultáneas (4 por defecto); sin hueco,
  `429 E_RECEIPT_BUSY`.
- El contenido del recibo se retiene frente al GC con un hold
  `receipt-<sha256>`; si la admisión falla, el hold que esa llamada colocó se
  libera (uno preexistente de un recibo idéntico no se toca).
- Cada llamada abre el ledger de eventos de la cuenta (alter idempotente del
  esquema). No se ha medido su coste ni se cachea; es una limitación conocida.
- Toda salida posterior a Cedar se audita: `Allowed`; `Denied` para
  422/409/413/429; `Failed` para 404 y 5xx.

## Límites conocidos (carreras)

- **A5, evento duplicado en carrera:** dos entregas idénticas simultáneas pueden
  superar la comprobación "ya existe" y la ingesta de evidencia puede añadir dos
  eventos `EvidenceAccepted` para el mismo registro. El registro es uno solo
  (identidad derivada), pero el ledger puede tener el evento repetido.
- **Conflicto en carrera:** si dos recibos distintos de la misma identidad
  llegan a la vez, ambos pueden pasar la comprobación previa; el segundo se
  informa como `409` pero su revisión ya está escrita (la historia se conserva).
- Solo repositorios Raw alojados.
