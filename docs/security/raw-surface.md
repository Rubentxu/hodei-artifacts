# La superficie de contenido crudo

HSP-98 (HAR-04) monta la superficie que sirve **bytes en una ruta**: un
repositorio `raw` en modo `hosted` guarda contenido en una dirección, y la
superficie publica, descarga y describe ese contenido.

La superficie **no está bajo `/api/v1`**: la API administrativa y el
endpoint que sirve paquetes son cosas distintas, y un cliente que habla
con una no tiene por qué creer que habla con la otra.

| Método | Ruta | Qué hace |
|---|---|---|
| `PUT` | `/repository/{repositorio}/{ruta…}` | Publica el cuerpo en esa dirección. |
| `GET` | `/repository/{repositorio}/{ruta…}` | Devuelve el contenido, con `Range` y condiciones. |
| `HEAD` | `/repository/{repositorio}/{ruta…}` | Lo mismo, sin cuerpo: las cabeceras son las de `GET`. |

## El orden de las cosas

Las tres peticiones hacen los mismos tres pasos, y los hacen en el mismo
sitio:

1. **Autenticar.** Sin credencial no hay nada: la respuesta es `401` antes
   de mirar la ruta, el repositorio o cualquier otra cosa. Una petición
   sin credencial no puede convertirse en un `400` que diga algo de una
   ruta que el cliente no tiene permiso de conocer.
2. **Leer la ruta**, con las reglas de `src/raw/path.rs`. La ruta se lee
   del URI **crudo** (`OriginalUri`), no del decodificado: un `%2e%2e`
   que ya ha pasado a ser `..` no se puede ni ver ni refutar.
3. **Buscar el repositorio** en la cuenta **verificada**. Un repositorio
   de otra cuenta no existe para esta, y la respuesta es el mismo `404`
   que el de un repositorio que no existe nunca tuvo.

Y después, la decisión de Cedar con la acción que corresponde:
`Artifacts::Action::"UploadAsset"` para publicar,
`Artifacts::Action::"DownloadAsset"` para descargar. El recurso de la
decisión es el **repositorio**: una ruta concreta no es un recurso con
identidad propia en el catálogo, y una acción sobre un recurso que no
existe no se puede evaluar.

## Las reglas de la ruta

Una ruta es lo que dice el cliente, y lo que dice el cliente es lo que va
a acabar en el disco. Las reglas son estrictas y se aplican antes de
tocar el almacén de contenido:

- La ruta tiene que estar bajo `/repository/`.
- La clave del repositorio tiene que ser un **nombre**
  (`[A-Za-z0-9._-]{1,128}`, y ni `.` ni `..`), no una ruta.
- Cada segmento tiene que ser un **`pchar` de RFC 3986**: los no
  reservados (`A-Za-z0-9-._~`), los sub-delimitadores
  (`!$&'()*+,;=`) y `:` y `@`. Ni barras, ni barras invertidas, ni el
  espacio, ni `#`, ni `?`, ni `[` ni `]`, y sin ser `.` ni `..`.
- Un segmento se decodifica **una** vez, y la forma decodificada tiene
  que ser un segmento válido. Como el símbolo `%` no es un `pchar`, un
  doble codificado es una forma que todavía lleva codificación dentro, y
  una forma así no es un nombre: se rechaza. Lo que se guarda es siempre
  un nombre literal, y un nombre literal no se puede leer de dos maneras.

Y hay tres límites, que son `400` como cualquier otra ruta inválida:

| Límite | Valor |
|---|---|
| Segmentos por ruta | 64 |
| Bytes por segmento | 255 |
| Bytes por ruta | 1024 |

Lo que se acepta es exactamente lo que se puede nombrar sin ambigüedad.
Una ruta que dos sistemas interpretan de manera distinta es una ruta que
un día es un fichero y otro día es otro. Y como los segmentos son
`pchar`, la ruta es además una **referencia URI** válida: el `Location`
que devuelve la publicación apunta al recurso que se acaba de publicar,
y no a otro.

## La política de sustitución

Un repositorio crudo `hosted` guarda el contenido en una dirección.
Publicar en una dirección que ya tiene contenido lo **sustituye**, y
sustituir sin más es una decisión que no puede tomar el cliente: dos
clientes que publican en la misma dirección se pisan, y el que pierde no
se entera.

Por eso la decisión es del **repositorio**, y vive en su configuración
**versionada**:

```json
{ "raw": { "replace": "allow" } }
```

- `allow`: se puede reponer. El contenido anterior sigue estando: la
  versión publicada no se reescribe, lo que se mueve es el puntero.
- `deny`, o la ausencia de la clave: la dirección publica **una vez**.
  Publicar el mismo contenido otra vez es idempotente (`200`, y
  `created: false`); publicar **otro** contenido es un `409`, y la
  escritura se aborta: no queda ni un byte en el almacén.

La ausencia de la clave es una decisión: un repositorio que no dice nada
es un repositorio que no ha dicho que se pueda reponer, y lo que no se ha
dicho no se permite.

## Las condiciones y el rango

- `ETag` es el **digest** del contenido, entre comillas. Es la identidad
  del contenido, que es la única que se puede comparar sin leer los bytes.
- `If-None-Match` que coincide al **leer**: `304`, con la etiqueta para
  que el cliente sepa cuál tiene. La comparación es **débil**, como
  manda RFC 9110: un `W/` delante dice "comparte la representación", y un
  cliente que revalida contra una caché manda la etiqueta así.
- `If-Match` que no coincide al leer, o `If-None-Match` que coincide al
  **escribir**: `412`, y la petición no se hace. Ni los bytes, ni el asset.
  La comparación de `If-Match` es **fuerte**: una etiqueta `W/` no es la
  etiqueta que se generó.
- Un `If-Match` sobre una dirección vacía falla: `*` quiere decir "el
  recurso que hay", y si no hay recurso no hay nada que comparar.
- `Range` es RFC 9110: `206` con `Content-Range` para un rango válido;
  `416` con `Content-Range: bytes */<longitud>` para uno que no cabe, o
  que no es un rango. Un rango en otra unidad, o varios rangos, se
  ignoran y se sirve el contenido entero, que es lo que el RFC permite.
  La unidad no distingue mayúsculas: `Bytes=0-9` es un tramo.

El `416` lleva la longitud del contenido en la **cabecera**, no en el
cuerpo: sin ella el cliente no sabe ni cuánto tiene.

### La condición se comprueba dentro de la transacción

Una escritura condicional tiene dos momentos: el que el cliente ve, y
el que decide. El primero es una cortesía —dice pronto lo que va a
pasar—, y el segundo es el que vale.

Por eso la condición **viaja al catálogo**, y el catálogo la comprueba
**dentro de la transacción** que escribe las filas. Entre el momento en
que la frontera lee lo que hay y el momento en que la transacción
escribe, otra publicación puede cambiar la dirección: sin la
comprobación dentro de la transacción, un `If-Match` que se cumplió al
principio puede no cumplirse al escribir, y el cliente se queda con un
`200` que no es verdad.

La comprobación dentro de la transacción no es un adorno: si la
condición no se cumple, la transacción **no se confirma**, y no queda ni
una fila escrita.

Cuando la condición se cumple, la respuesta es la que sea: `201` si la
dirección no existía, `200` si ya existía. Y el cuerpo dice lo que pasó
con la fila:

| Campo | Qué dice |
|---|---|
| `created` | La dirección no existía y ahora existe. |
| `replaced` | La dirección existía y ahora apunta a otro contenido. |

## El digest que declara el cliente

El cliente puede declarar el digest del contenido en la cabecera
`x-checksum-sha256` (con o sin el prefijo `sha256:`). Si lo declara, se
**comprueba** contra lo que llega: un cliente que declara un digest y
envía otra cosa está roto, y un servidor que lo acepta sin comprobar
está mintiendo sobre lo que tiene.

Una declaración que **no es un digest** es un `400`, no una declaración
ignorada: el cliente ha dicho algo, y lo que ha dicho no tiene sentido.
Una promesa que no se puede comprobar no se acepta en silencio.

## Los códigos de respuesta

| Código | Cuándo |
|---|---|
| `201` | La publicación creó la dirección. |
| `200` | La publicación repuso el contenido, o republicó el mismo. |
| `304` | La lectura, con un `If-None-Match` que coincide. |
| `206` | La lectura de un tramo. |
| `400` | La ruta no es válida, o el contenido no es el que el cliente declaró. |
| `401` | No hay credencial, o no es válida. |
| `403` | La decisión de Cedar no es permiso. |
| `404` | No hay nada en esa dirección, o el repositorio no existe en la cuenta verificada. |
| `405` | El método no es el de la superficie: la ruta solo admite `PUT`, `GET` y `HEAD`. |
| `409` | La dirección ya tiene otro contenido, y la política no permite reponer. |
| `412` | Una condición que no se cumple. |
| `413` | El contenido es más grande de lo que se admite. |
| `416` | El rango no se puede servir. |
| `500` | El almacén de contenido está roto: el contenido que hay no es el que su digest dice. |
| `501` | El repositorio pide un ámbito de digest que esta release no sirve. |
| `503` | La superficie no está montada, o el almacén no está disponible. |

El `404` y el `403` no se confunden: el `403` dice que el repositorio
existe y el principal no puede usarlo; el `404` dice que para esta
cuenta no hay nada. Un repositorio de otra cuenta es un `404`, porque un
`403` diría que existe en algún sitio, y esa es la existencia que no se
puede exponer.

## Configuración

```toml
[security]
raw_api_enabled = true
```

```toml
[content]
cas_path = "./data/hodei.cas"
```

- `raw_api_enabled` está **cerrado por defecto**, igual que
  `admin_api_enabled`. Un despliegue que abre la superficie tiene que
  declarar en su bundle de Cedar las acciones `UploadAsset` y
  `DownloadAsset`: el servicio **no arranca** con un bundle que no
  declare lo que autoriza con.
- `cas_path` es donde viven los bytes, por digest. No es una decisión de
  la cuenta, sino del despliegue.
- El catálogo y el almacén de contenido se montan cuando
  `repository.durable = true`.

## Verificación

`tests/har04_raw_hosted_contract.rs` (en proceso, contra SurrealDB y el
CAS de verdad):

1. La publicación, la descarga y la descripción devuelven el contenido
   intacto, con el `ETag` del digest y el tamaño correcto.
2. El rango que cabe es `206`; el que no cabe y el que no es un rango son
   `416` con `Content-Range`. Las cuatro formas de un tramo —con
   extremos, con sufijo, abierto y con otra unidad— se sirven como están
   escritas.
3. `If-None-Match` que coincide es `304`, también con la etiqueta débil;
   `If-Match` que no coincide es `412`; una escritura cuya condición no
   se cumple **no deja nada escrito**, ni en el catálogo ni en el
   almacén de contenido.
4. La ruta válida se preserva; el traversal, el doble codificado, los
   caracteres de control y las rutas ambiguas son `400`; una ruta
   rechazada no publica nada.
5. La dirección ocupada por otro contenido es `409` cuando la política no
   permite reponer, y `200` cuando sí. Un `409` **no deja ni un byte**
   en el almacén de contenido.
6. Sin credencial `401`; sin permiso `403`; el repositorio de otra cuenta
   es `404`; un repositorio de otro formato es `404`.
7. El permiso va al **repositorio**, no a la cuenta: un segundo
   repositorio de la misma cuenta que el bundle no cubre es `403`.
8. Con la superficie cerrada, la ruta no existe.

`tests/raw_hosted_restart_contract.rs` (proceso real, puerto real):

1. El servicio arranca en un puerto de verdad, se publica un fichero con
   el `curl` de verdad, se descarga con el `curl` de verdad y los dos
   ficheros se comparan con el `sha256sum` de verdad.
2. El proceso se para, se vuelve a arrancar sobre el mismo almacén, y el
   contenido sigue ahí y sigue siendo **los mismos bytes**.
3. Un traversal enviado con `curl --path-as-is` es `400`, y no publica
   nada.

## El rastro

La publicación deja un evento **duradero** en el outbox del catálogo, en
la misma transacción que escribe las filas: o se escribe el asset y su
evento, o no se escribe ninguno. Los rechazos también dejan rastro: un
conflicto, una precondición que no se cumple y un modo que esta release
no sirve son hechos que el rastro conserva.

La descarga deja una línea en el `audit_sink` de la aplicación, y **no**
un evento en el outbox. La asimetría es real y conviene decirla: leer es
mucho más frecuente que publicar, y un evento durable por cada lectura
llenaría el outbox de una forma que nadie pidió.

Lo que sí se registra de la descarga es **todo** lo que se sabe: lo que
se sirvió y lo que no se pudo servir. Un `403` de Cedar, un `404` y un
`412` se registran con su resultado, no se pierden.

## Límites

- **El catálogo no vuelve a decidir.** La decisión de Cedar se toma en la
  frontera HTTP, donde se conoce el HRN canónico del repositorio. El
  puerto de autorización del catálogo no lleva la región, y el HRN sí la
  lleva: una segunda decisión caería sobre un recurso que puede no ser el
  mismo que evaluó la primera, y dos decisiones sobre recursos
  distintos no son una decisión más fuerte. Lo que el catálogo **sí**
  garantiza es el aislamiento por cuenta, y no por cortesía: lo lleva la
  clave de cada fila.
- **El repositorio tiene que existir en el catálogo, y el catálogo es
  otra cosa que la API administrativa.** Esta superficie lee la tabla
  `catalog_repository`; la API administrativa de alta de repositorios
  escribe en la tabla `repository`, que es el modelo anterior. Los dos no
  son lo mismo: el HRN del catálogo es
  `hrn:hodei:artifacts:<región>:<cuenta>:repository/<clave>` y el de la
  API administrativa es
  `hrn:hodei:repository:<región>:<organización>:repository/<nombre>`, y
  la cuenta no es la organización.

  Así que un repositorio creado por la API administrativa **no** es
  visible para esta superficie, y esta superficie no crea repositorios.
  El alta de un repositorio `raw`/`hosted` en el catálogo no tiene
  endpoint HTTP todavía, y unirlos es un trabajo de modelo, no de
  cableado: los dos modelos no se unen sin decidir cuál es el bueno.
- **Solo `raw` y `hosted`.** Los modos `proxy` y `group` no los sirve esta
  release, y ni siquiera se pueden crear.
- **Solo el ámbito global.** Un repositorio que pida el ámbito
  particionado por cuenta se responde con `501`, no con un `404`: un `404`
  ahí sería mentir.
- **El límite de cuerpo** es `server.max_body_size` (10 MiB por defecto).
  Pasarse es un `413`, y la escritura se aborta.
- **El `ETag` que se genera es fuerte.** Una etiqueta `W/` no coincide con
  un `If-Match`, porque la comparación de `If-Match` es fuerte. En un
  `If-None-Match` sí coincide, porque ahí la comparación es débil, que es
  lo que dice RFC 9110.
- **La condición se comprueba dos veces.** En la frontera, para no
  escribir lo que no se va a escribir, y en el catálogo, dentro de la
  transacción, que es la que vale. La segunda es la que cierra la
  carrera; la primera solo evita trabajo.
