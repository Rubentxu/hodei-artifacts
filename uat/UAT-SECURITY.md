# Políticas de confianza en la distribución — contrato de seguridad

**Tarea:** HSP-105 (HAR-11) · **Servicio medido:** `hodei-artifacts 0.2.0`

## 1. Qué afirma este contrato

Afirma que **la ruta nativa no esquiva la confianza**: un artefacto que
nadie ha promovido no se sirve por ningún protocolo, ni por una etiqueta que
se movió, ni por un manifiesto alternativo, ni por un alias, ni desde otra
cuenta. Y dice también lo que **no** hace.

El reparto es deliberado: **Cedar decide quién** puede promover o revocar, y
**la política de confianza decide qué software** merece el canal trusted.
Tener permiso para promover no es el visto bueno: la decisión de confianza
puede rechazar a quien Cedar permitió.

Las reglas, una por una:

- **Dos canales por repositorio.** El ajuste `config_settings.trust.serve`
  vale `"open"` (por defecto: se sirve lo publicado, sin garantía de
  confianza; es la cuarentena abierta) o `"trusted"` (solo se sirve un
  digest con una concesión activa). Un valor que no se entiende cierra el
  canal: un ajuste de seguridad ilegible falla del lado cerrado. Lo no
  admitido responde como si no existiera (`404`, `MANIFEST_UNKNOWN`,
  `BLOB_UNKNOWN`); para quien lee no hay canal lateral que distinga «no
  existe» de «está en cuarentena», ni en el código ni en el mensaje. El
  montaje de un blob entre repositorios (`?mount=&from=`) también pasa por la
  admisión del origen: lo que no está concedido cae a una subida normal.
  Quien escribe sí puede inferir existencia (véase §4).
- **La concesión es por digest.** Promover `sha256:X` concede `X` en ese
  repositorio. Una etiqueta que se mueve a `Y` no hereda nada: `Y` no se
  sirve hasta que alguien lo promueva. Promover un manifiesto OCI concede
  también su configuración y sus capas; promover un índice, sus hijos.
- **Una sola admisión.** Raw, blobs, manifiestos (por digest y por etiqueta),
  listado de etiquetas y referrers pasan por la misma función de admisión.
  No hay una ruta nativa con otra regla.
- **Sin caché de decisiones.** Cada petición al canal trusted lee el libro de
  concesiones, así que una revocación tiene efecto en la **petición
  siguiente**. Un error del libro es un error (`503`), nunca un permiso.
- **Toda promoción evaluada deja una decisión.** Incluso un rechazo queda
  registrado. Lo que se corta antes de evaluar (petición mal formada,
  repositorio o asset inexistente, falta de permiso, digest que ya no es el
  de la ruta) no escribe nada. Cedar sin `Permit` (denegación o
  indeterminación) o una política de confianza que no avanza nunca concede.
- **Ninguna cabecera confiere cuenta ni alcance.** La cuenta sale del token
  verificado; `X-Account`, `X-User` o `X-Scope` no se miran.
- **La auditoría no lleva credenciales.** Ni la respuesta ni el log
  contienen el token.

## 2. Qué se ha medido

| Prueba | Qué comprueba |
| --- | --- |
| `crates/hodei-catalog/src/trust.rs` (unitarias) | El canal por repositorio y que un ajuste ilegible cierra. |
| `crates/hodei-catalog-storage/tests/hsp105_trust_ledger_contract.rs` | El libro de concesiones sobre el motor real: concesión por digest, revocación por digest o decisión, idempotencia, aislamiento por cuenta y repositorio, y persistencia tras reabrir. |
| `tests/hsp105_trust_e2e.rs` | El proceso real por HTTP, con tokens, Cedar real, catálogo y almacén reales (detalle abajo). |

### SEC-01 — sin credencial no se promueve

Promover y revocar sin token, o con uno inválido, responden `401`.

### SEC-02 — otra cuenta no ve ni promueve

Una identidad de otra cuenta recibe `403` tanto si el asset existe como si
no (misma respuesta: sin canal lateral), `404` ante un repositorio que su
cuenta no tiene, y nada de lo que intenta concede nada. Las cabeceras que
dicen otra cuenta o un alcance no cambian el resultado.

### SEC-03 — un upload no evaluado no sale por el canal trusted

Se puede subir a un repositorio trusted (la cuarentena no impide publicar),
pero `GET` y `HEAD` responden `404` hasta que se promueve. En un repositorio
abierto el comportamiento no cambia.

### SEC-04 — la decisión de X no se reutiliza para Y

Se promueve el manifiesto de una imagen; la etiqueta `latest` se mueve a otra
imagen. La etiqueta, el manifiesto nuevo por digest y sus capas responden
`404`; el digest viejo sigue sirviéndose. Un fichero crudo con los mismos
bytes bajo otro nombre es el mismo software y se sirve; con bytes distintos,
no.

### SEC-05 — denegación, indeterminación o política que no avanza no publican

Con una política de confianza exigente, o sin política cargada, promover
responde `422` con `{promoted: false, reasons: [...]}` y el digest sigue sin
servirse. Cedar sin `Permit` responde `403`.

### SEC-06 — revocación

`POST /api/v1/promotions/revoke` devuelve `{revoked: n}` y el digest deja de
servirse en la petición siguiente; si es un manifiesto, también su
configuración y sus capas. Una promoción nueva lo concede de nuevo
con una decisión nueva (que reevalúa la evidencia viva).

### SEC-07 — manifiesto alternativo, referrer o etiqueta mutable

Un manifiesto alternativo por digest, un adjunto (SBOM, firma) que cuelga de
un manifiesto promovido y el listado de etiquetas de lo no promovido no
aparecen hasta que cada uno se promueve. Una ruta con `..` codificado no
abre nada.

### SEC-08 — nada de credenciales en la traza

Ni el cuerpo de la respuesta de promoción ni el log del servicio contienen el
token.

## 3. La superficie

| Ruta | Acción Cedar | Qué hace |
| --- | --- | --- |
| `POST /api/v1/promotions` | `PromoteAsset` | Cuerpo `{repository, path, digest}` (`digest` es `sha256:<hex>` canónico en minúsculas). Evalúa la evidencia y la política de confianza y, si avanza, concede el digest y lo que abarca. |
| `POST /api/v1/promotions/revoke` | `RevokePromotion` | Cuerpo `{repository, reason}` más **exactamente uno** de `digest` o `decision_id`. Devuelve `{revoked}`. |

Códigos: `401` sin credencial, `403` sin permiso (y también cuando falta el
asset y no hay permiso), `404` repositorio inexistente (se responde antes de evaluar el permiso) o
asset inexistente (con permiso), `409` el asset ya no tiene el digest pedido, `422` rechazo de la
política de confianza, `501` el servicio no tiene plano de promoción
(catálogo no duradero), `503` libro o proveedor de identidad no disponibles.

### Cómo se activa

- **Política de confianza.** `[trust] policy_file` (variable
  `HODEI_TRUST__POLICY_FILE`) apunta a un JSON de `TrustPolicy`;
  `policy_id` elige la política. Sin fichero, el registro está vacío y toda
  promoción se rechaza con `PolicyUnavailable`: sin política no hay
  confianza. Un fichero declarado e ilegible impide arrancar.
- **Canal del repositorio.** Se fija con
  `CatalogWriter::update_config(account, repo_key, RepositoryConfig)` o al
  sembrar el catálogo. No existe todavía una ruta HTTP que cree
  repositorios del catálogo.
- **Bundle de autorización.** Un despliegue con catálogo duradero **debe
  declarar** `PromoteAsset` y `RevokePromotion` en su esquema Cedar; si no,
  no arranca (`config/examples/schema.json` los trae).

## 4. Lo que no se promete

- **Los hechos de riesgo son los de cero.** La decisión de promoción se
  evalúa con `RiskFacts::default()`: aún no se alimentan CVE, VEX ni
  alcanzabilidad reales. Una política que exija establecer el riesgo
  rechaza toda promoción hasta que exista esa fuente.
- **Una concesión no caduca sola.** Una CVE nueva, un VEX revocado o una
  excepción caducada **no** retiran concesiones existentes: se retiran con
  la revocación explícita, y una promoción nueva sí reevalúa la evidencia.
  No hay caché de decisiones que invalidar; la revocación lee el libro en
  cada petición.
- **La concesión es por digest y repositorio.** Un alias con los mismos
  bytes en el mismo repositorio se sirve (es el mismo software); no se
  promueve entre repositorios.
- **Revocar un manifiesto retira también lo que su promoción concedió**
  (configuración, capas, hijos de un índice). Si una capa se comparte con
  otra imagen promovida, esa imagen la pierde hasta que se vuelva a promover:
  falla del lado cerrado. La revocación por `decision_id` retira solo lo de
  esa decisión.
- **Quien puede escribir puede inferir existencia.** Un `PUT` con
  `If-Match`, un conflicto por «publicar una vez», un `201` frente a un
  `Unchanged` o la comprobación de referencias de un manifiesto distinguen
  lo que ya existe en cuarentena de lo que no. Hace falta permiso de subida
  en el repositorio y la cuenta es la misma: no cruza cuentas ni sirve
  bytes.
- **El repositorio inexistente responde `404` antes de Cedar.** Un
  principal de la cuenta sin permiso puede saber qué repositorios existen en
  su propia cuenta. No hay cruce entre cuentas.
- **La expansión de un manifiesto la decide la ruta.** Promover un asset
  bajo `manifests/` concede los digests que nombra, existan o no en el
  repositorio. Exige permiso de promover y que la política avance; un
  digest sin bytes publicados no se sirve.
- **Tope de cobertura.** Una promoción abarca como máximo 4096 digests;
  pasado el tope concede menos, nunca más.
- **El canal abierto es el de siempre.** Un repositorio sin
  `trust.serve = "trusted"` no aplica la compuerta: la confianza es opt-in
  por repositorio.
- **Tiempo de espera.** Un fallo o estado desconocido del libro responde
  `503`; nunca se traduce en permiso, pero tampoco hay reintento.
- **El borrado de concesiones no existe.** Una concesión revocada se
  conserva como historia; un digest revocado solo vuelve con una decisión
  nueva.

## 5. Cómo se ejecuta

```bash
cargo test -p hodei-catalog --lib trust
cargo test -p hodei-catalog-storage --test hsp105_trust_ledger_contract
cargo test --test hsp105_trust_e2e -- --test-threads=2
```

La pipeline registra las suites en `pipeline.kts`: el libro en la etapa de
base de datos y el proceso real en la de infraestructura.
