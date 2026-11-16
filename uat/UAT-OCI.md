# Compatibilidad OCI — contrato de compatibilidad

**Tarea:** HSP-103 (HAR-09) · **Servicio medido:** `hodei-artifacts 0.2.0`
**Medido el:** 2026-11-16 · **Suite oficial:** `opencontainers/distribution-spec`
@ `97274622c11112caa21efb8c52acca3c6b8fa7f1` (digest de las fuentes:
`898a7fc644c5eae0accd6fc96fc019196ddd4c56eac72d1e14b0450c70a52ff5`).

## 1. Qué afirma este contrato

Afirma que el registro habla la **Distribution Spec 1.1** con los clientes y
con las categorías que aquí se nombran, y dice también lo que **no** habla.
Un contrato que solo dice «somos compatibles» no dice nada: lo que vale es
la lista de lo que se ha medido —con la versión de cada cliente— y la lista
de lo que no se ha medido, con su motivo y su acción.

Las categorías se nombran una por una, y cada nombre es el que la suite
oficial imprime. La suite oficial se ejecuta contra el registro en cada
ejecución de la CI y su resultado se archiva; el informe de esta tarea está
en `target/uat/oci-compatibility.md` y el detalle por prueba, en
`target/uat/oci-conformance/` (`results.yaml`, `junit.xml`, `report.html`).

## 2. Versiones con las que se ha medido

| Cliente | Versión | Qué se le ha pedido |
| --- | --- | --- |
| Docker (CLI) | 27.3.1 | `push`/`pull` de una imagen de una plataforma; `manifest create`/`manifest push` de un índice de dos plataformas; `load` de la imagen en el demonio |
| Podman | 6.1.3 | `build` sin red, `push`, `tag` y `pull` de una imagen; `save` de la imagen para pasarla al demonio de la CLI de Docker |
| ORAS | 1.2.0 | `push` de un artefacto con su tipo de medio; `attach` de un SBOM con `--artifact-type`; `discover` de los referrers por la API de referrers |
| Suite oficial | distribution-spec @ `9727462` | 38 categorías de API y de datos en verde; 725 pruebas en total: 436 en verde, 289 omitidas, 0 en rojo |

El demonio que hay detrás de la CLI de Docker es el `podman`, que expone la
API de Docker por un socket unix: la CLI de Docker no habla con un registro,
habla con un demonio, y es el demonio quien habla con el registro.

El registro de la prueba habla **HTTP sin TLS**, que es lo que hace un
registro de desarrollo. El demonio solo relaja la verificación del TLS
cuando el registro se llama `localhost`, así que la dirección de la prueba
es `localhost:<puerto>` y no una dirección IP.

## 3. La declaración

### 3.1 Categorías de API

| Categoría | Estado |
| --- | --- |
| Ping | `Pass` |
| Tag listing | `Pass` |
| Blob push | `Pass` |
| Blob post only | `Pass` |
| Blob post put | `Pass` |
| Blob chunked | `Pass` |
| Blob streaming | `Pass` |
| Blob mount | `Pass` |
| Blob upload cancel | `Pass` |
| Blob get | `Pass` |
| Blob head | `Pass` |
| Manifest put by digest | `Pass` |
| Manifest put by tag | `Pass` |
| Manifest put with subject | `Pass` |
| Manifest get by digest | `Pass` |
| Manifest get by tag | `Pass` |
| Manifest head by digest | `Pass` |
| Manifest head by tag | `Pass` |
| Referrers | `Pass` |

### 3.2 Categorías de datos

| Categoría | Estado |
| --- | --- |
| Image | `Pass` |
| Image Uncompressed | `Pass` |
| Image with Large Manifest | `Pass` |
| Index | `Pass` |
| Nested Index | `Pass` |
| Index with Subject | `Pass` |
| Empty Index | `Pass` |
| Artifact | `Pass` |
| Artifact Index | `Pass` |
| Artifact without Layers | `Pass` |
| Artifacts with Subject | `Pass` |
| Missing Subject | `Pass` |
| Bad Digest Image | `Pass` |
| Invalid Manifest Digest | `Pass` |
| Custom Fields | `Pass` |
| Data Field | `Pass` |
| No Layers | `Pass` |
| Non-distributable Layers | `Pass` |
| Blobs sha256 | `Pass` |

La suite oficial marca estas categorías en verde **en la ejecución**, y la
prueba `the_official_conformance_suite_runs_the_declared_categories` falla si
alguna de ellas deja de estarlo: la declaración no es una lista escrita a
mano, es una lista que la CI comprueba.

## 4. Lo que este contrato **no** declara

Una categoría que no está en el apartado 3 no se declara. Estas son las que
quedan fuera, con su estado real en la suite, el motivo y la acción.

| Categoría | Estado en la suite | Motivo | Acción |
| --- | --- | --- | --- |
| Blobs sha512 | `Disabled` | El digest del contenido es `sha256` y solo `sha256`: el modelo de digest no lleva algoritmo. **El conjunto de datos no se ha ejecutado.** | Extender el modelo de digest para que lleve el algoritmo, y calcular el `sha512` en el almacén de contenido junto al `sha256`. Tarea propia: el modelo de digest es transversal (hub, catálogo, evidencia). |
| Digest Algorithm sha512 | `Disabled` | Ídem que la anterior. **No se ha ejecutado.** | Ídem. |
| Tag delete | `Skip` | El registro **no borra**: `DELETE` de una etiqueta responde `405`. La suite lo omite porque el registro no lo implementa. | Decidir si esta release borra. Si borra, implementar el `DELETE` de etiquetas y su borrado en el catálogo, en una sola transacción. |
| Tag delete atomic | `Skip` | Ídem, con la variante atómica. | Ídem. |
| Manifest delete | `Skip` | El registro no borra manifiestos: `DELETE` responde `405`. | Ídem que el borrado de etiquetas. |
| Manifest delete atomic | `Skip` | Ídem, con la variante atómica. | Ídem. |
| Blob delete | `Skip` | El registro no borra blobs: `DELETE` responde `405`. | Ídem. |
| Blob delete atomic | `Skip` | Ídem, con la variante atómica. | Ídem. |
| Blob anonymous mount | `Skip` | El montaje **anónimo** no se implementa: el registro cae a una subida normal y la suite acepta el repliegue, que es lo que permite el protocolo. | Implementar el montaje anónimo si se quiere el atajo; no es un incumplimiento del protocolo. |
| Blob get range | `Skip` | El `GET` de un blob **no** negocia rangos: devuelve el blob entero con `200` en lugar de `206`. La especificación lo **recomienda** (`SHOULD`) y no lo exige (`MUST`), así que es una desviación recomendada, no un incumplimiento. | Soportar `Range` en la lectura de blobs: es lo que piden los clientes que descargan en paralelo. |
| Manifest put with tag params | `Disabled` | El `PUT` de un manifiesto con el parámetro `?tag=` no se implementa. **No se ha ejecutado.** | Implementar el parámetro, o dejarlo fuera de la declaración de forma explícita. |
| Tag Param | `Disabled` | El listado de etiquetas con el parámetro `?tag=` no se implementa. **No se ha ejecutado.** | Ídem. |
| Tag Param sha512 | `Disabled` | Ídem, con digest `sha512`. **No se ha ejecutado.** | Ídem, y depende del modelo de digest. |
| Sparse Manifests | `Disabled` | Los manifiestos **dispersos**—con descriptores que no se han subido— no se han ejecutado. | Medirlo cuando el registro admita subir un manifiesto antes que sus descriptores. |

Lo que la suite marca `Skip` **no** es un fallo de la suite: es la forma que
tiene la suite de decir «este registro no implementa esto». El registro
responde `405` al `DELETE` **de contenido publicado**—una etiqueta, un
manifiesto, un blob—, y la suite omite la prueba. El `DELETE` de la
**sesión de subida** sí existe y responde `204`: es la categoría «Blob
upload cancel», que está en verde.

## 5. Lo que este contrato **no** certifica

- **No certifica las categorías que no se han ejecutado.** El conjunto de
  datos `sha512` no se ha ejecutado porque el registro no habla ese
  algoritmo; declararlo en verde sería propaganda.
- **No valida firmas criptográficas.** El registro transporta referrers y
  atestaciones, y el camino de los SBOM con `oras attach` está probado; lo
  que **no** se ha comprobado es que la firma de una atestación sea válida,
  ni quién la firma.
- **No certifica la promoción.** Publicar en el registro no es promover un
  artefacto a un canal de distribución: esa es otra superficie.
- **No certifica que la CLI de Docker sea un motor.** Lo que se ha medido es
  que la CLI habla con el registro a través de un demonio.
- **No certifica el borrado de contenido publicado.** El registro no borra
  etiquetas, manifiestos ni blobs: el apartado 4 lo dice. Lo que sí existe
  es el `DELETE` de la sesión de subida.
- **No certifica la disponibilidad, ni el rendimiento, ni la escala.**
- **No certifica la suite oficial entera.** Solo las categorías del
  apartado 3, con la versión del apartado 2.
- **No certifica la integridad de la capa no distribuible**, ni que las
  sesiones de subida no se agoten: los dos límites que quedan abiertos
  están en el apartado 6.

## 6. Los límites que quedan abiertos

Hay dos cosas que la prueba **enseñó** y que no se han arreglado. Se
escriben aquí porque un contrato que declara lo que promise y calla lo que
no, es un contrato incompleto: quien lo lea tiene que saber qué está
**fuera** del alcance de la garantía, no solo lo que está dentro.

### La capa extranjera: el registro no comprueba de dónde viene

Una capa **no distribuible** es la que el manifiesto declara con sus `urls`
—de dónde se baja— y con uno de los dos tipos de medio de no distribución.
El registro no la guarda y no la descarga: **le cree al cliente**.

Lo que sí comprueba:

- que el tipo de medio sea exactamente uno de los dos de no distribución
  (`application/vnd.oci.image.layer.nondistributable.v1.tar`, con o sin
  `+gzip`). Un descriptor con `urls` y **otro** tipo de medio es una capa
  normal con una pista, y esa sí se exige en el almacén: sin esta
  distinción, un manifiesto podría nombrar contenido que no existe.
- que las `urls` sean `https`. Una `http` se rechaza con
  `MANIFEST_INVALID`, y una `urls` vacía no es una capa extranjera.

Lo que **no** comprueba:

- que la `url` exista, que sirva ese contenido, ni que el contenido tenga
  el digest que el manifiesto declara. El registro apunta a la capa; el
  que la baja es el cliente, y el que la **verifica** es quien confia en
  el manifiesto. El digest queda registrado, y eso es lo que puede
  decir el registro: que el manifiesto declara un digest, no que los
  bytes de la `url` sean esos.
- quién es el servidor de la `url`: una `https` de cualquier host es
  admitida. La regla es sobre el **esquema**, no sobre el destino.

Esto es lo que dice el protocolo—la capa se baja del `url`, no del
registro—, y el contrato lo declara como lo que es: el registro **no
garantiza la integridad de la capa no distribuible**.

### Las sesiones de subida: un tope global, y de una hora

Las sesiones de subida viven en un registro **global**, no por cuenta:

- **64 sesiones** vivas a la vez (`security.oci_max_concurrent_uploads`),
- cada una **una hora** sin que nadie la toque
  (`security.oci_upload_ttl_secs`).

Las dos cosas son configurables por despliegue. El límite es real y tiene
un dueño claro: un cliente que abre sesiones y las abandona ocupa el
hueco de los demás durante una hora. Cuando el tope se alcanza, la sesión
nueva se rechaza con `BLOB_UPLOAD_INVALID` —un `400`, un error del
cliente—, y no con un `429` o un `503` que le dijera a ese cliente que
volviera más tarde.

Dos cosas que **no** se han hecho aquí, y que son la acción que queda:

- el tope debería ser **por cuenta**, no global, para que una cuenta no
  pueda agotar el pool de las demás;
- el error de «no hay hueco» debería ser un `429`/`503`, no un `400`: un
  `400` no se reintenta solo, y el cliente que lo recibe se equivoca sobre
  la causa.

Ninguna de las dos cosas rompe el protocolo: la suite oficial no las
mide, y los clientes que se han probado no las han encontrado. Se
documentan porque **son** un límite del registro, y porque el día que
se arreglen, hay que saber que estaban aquí.

## 7. Lo que este trabajo encontró y corrigió

La prueba con clientes de verdad y con la suite oficial encontró cuatro
desviaciones del protocolo y del contrato de autorización. Todas están
corregidas, y todas tenían un síntoma engañoso:

| Hallazgo | Síntoma | Corrección |
| --- | --- | --- |
| El esquema Cedar no declaraba el tipo de ancestro de `Artifacts::Repository` | Un repositorio con niveles (`team/demo`) se respondía con `403 DENIED`: el motor de autorización fallaba cerrado porque el padre que el mapeador declaraba no estaba permitido por el esquema. | Declarar `memberOfTypes` en el tipo `Repository` del esquema (`config/examples/schema.json` y el esquema del despliegue de las pruebas). |
| La negociación del `Accept` leía **solo la primera** cabecera | Un cliente que manda varias cabeceras `Accept`—la CLI de Docker manda la del índice y la de la imagen— se quedaba sin manifiesto con `MANIFEST_UNKNOWN`. | Leer **todas** las cabeceras `Accept` y todas las listas separadas por comas. |
| El `PUT` que cierra una subida no comprobaba el rango | Un trozo fuera de orden enviado en el `PUT` de cierre respondía `400` en lugar de `416`: el cliente no sabía que su desfase era el problema. | Comprobar el `Content-Range` también en el cierre, y responder `416` con el `Range` que dice por dónde seguir. |
| Las capas **extranjeras** se exigían en el almacén | Un manifiesto con una capa no distribuible—la que declara sus `urls`— se rechazaba con `MANIFEST_BLOB_UNKNOWN`, siendo un manifiesto válido. | No exigir en el almacén la capa **no distribuible**: la que su tipo de medio declara (`application/vnd.oci.image.layer.nondistributable.v1.tar`, con o sin `+gzip`) y cuyas `urls` dicen de dónde se baja. La exención es **exactamente** esa: un descriptor con `urls` y un tipo de medio cualquiera es una capa normal con una pista, y eximirla sería dejar de exigir un contenido que el manifiesto nombra. |

La revisión independiente de la pieza encontró cuatro cosas más, todas
corregidas y todas en el mismo sitio: en el **acuerdo entre lo que el
cliente dice y lo que el registro comprueba**.

| Hallazgo | Síntoma | Corrección |
| --- | --- | --- |
| El esquema Cedar no declaraba **la regla** que el cambio introduce | El `memberOfTypes` convierte «deny» en «hereda» para todo el subárbol de un repositorio, y esa regla no estaba escrita en ninguna parte: una política con el operador `in` sobre `Artifacts::Repository` cambia de comportamiento sin que nadie lo decida. | La regla está escrita aquí y en el manual de Cedar: el ancestro es la cadena de prefijos **dentro de la misma cuenta**, y el `403` de un repositorio con niveles se arregla declarando el tipo de ancestro en el esquema. **Y el arreglo es del operador**: con el esquema anterior, un repositorio con niveles sigue con `403`. |
| La negociación del `Accept` no leía el **peso** | Un `Accept: */*;q=0`—que es un «no acepto nada»— servía cualquier manifiesto: el único caso en que el registro servía un tipo que el cliente había rechazado. | Leer el peso y tratar `q=0` como un rechazo (RFC 9110, sección 12.5.1). |
| El rango del `PUT` de cierre se comprobaba **solo por su inicio** | Un trozo que declaraba `Content-Range: 0-99` y mandaba tres bytes se aceptaba: el registro confiaba en el cuerpo y tiraba el rango. | Comprobar que el rango declarado y la longitud del cuerpo cuadran, en el `PATCH` y en el `PUT` de cierre, y responder `416` si no cuadran. |
| La exención de la capa extranjera no se aplicaba a los **blobs de un artefacto** | Un artefacto OCI con un descriptor de blob no distribuible se rechazaba con `MANIFEST_BLOB_UNKNOWN`, siendo un manifiesto válido: el mismo concepto tratado de dos maneras según la forma del manifiesto. | El criterio es el mismo en las dos ramas: la capa no distribuible no se exige en el almacén. |

### La regla de los ancestros, escrita

El cambio del esquema de autorización no es un detalle: es una **regla**,
y una regla que no está escrita es una regla que nadie ha decidido.

El repositorio `team/demo` es hijo de `team`, y `team/demo/app` es hijo de
`team/demo`. La relación es la **cadena de prefijos del nombre**, dentro
de la misma cuenta: el mapeador de HRN a Cedar declara esos padres a partir
del nombre, existan o no en el catálogo. Con el tipo de ancestro declarado
en el esquema (`memberOfTypes`), una política con el operador `in` sobre
`Artifacts::Repository` concede a `team` lo que concede a `team/demo` y a
`team/demo/app`.

Lo que **no** hace la regla: nada cruza de cuenta. El ancestro se
sintetiza con la misma partición, servicio, región y cuenta, y una
petición de otra cuenta se rechaza antes de llegar al motor.

Y lo que exige: el esquema es **del operador**, y el esquema que el
servicio carga lo elige quien lo despliega
(`HODEI_SECURITY__AUTHORIZATION_SCHEMA_PATH`). El arreglo del `403` de
los repositorios con niveles está en `config/examples/schema.json` y en el
esquema del despliegue de las pruebas, pero **una instalación con el
esquema anterior sigue con el `403`**: el cambio es del esquema que
declara, y no del código que evalúa.

### Y una corrección en la forma de medir

Dos de los casos fallaban de vez en cuando, y la causa **no** estaba en el
registro: estaba en lo que se comparaba.

El **digest del demonio** y el **digest del registro** no son el mismo, y no
tienen por qué serlo: la imagen entra en el demonio por un tar--con el
manifiesto de su formato local-- y sale por otro--con el manifiesto que el
registro publicó, que es el de Docker y no el de OCI--. Los dos digests son
reales, y el demonio los anota en la misma lista, **cuyo orden no es un
contrato**. La prueba comparaba el primero de la lista, y por eso fallaba
unas veces y otras no. Ahora compara lo que sí es un contrato: el digest que
el registro publica antes y después de la ida y vuelta, y que el cliente se
anote **ese** digest, entre los que tiene.

Y el `oras` toma el nombre del fichero del argumento **tal cual**, y ese
nombre es el título del artefacto: un título con una ruta absoluta no se
puede bajar dentro de un directorio--el cliente no se deja salir de él, y
hace bien--. El artefacto se publica ahora con el nombre que tiene dentro
del directorio de trabajo. Es la protección de recorrido de rutas del
propio cliente, y no una limitación del registro.

Y una tercera, que no era del registro sino de la **máquina**: dos
`podman build` a la vez se pelean por el `fuse-overlayfs`. El
desmontaje se queda esperando a que termine la escritura, el proceso
**no termina**, y la máquina se queda con gigas de páginas sucias y con
procesos en estado irrecuperable que no se pueden matar. Las
construcciones de imagen toman ahora un cerrojo--un `flock` sobre un
fichero del `target`--, y la suite se puede correr en paralelo sin
colgar la máquina. La pipeline la corre con un solo hilo de todos modos,
por los puertos y por el almacén, pero el cerrojo hace que la suite no
dependa de que nadie la ejecute en un hilo.

## 8. Cómo se reproduce

```bash
# Las herramientas que la suite necesita, con su version.
bash scripts/oci-e2e-preflight.sh

# La suite oficial: se descarga, se verifica su digest y se compila.
bash scripts/oci-conformance.sh

# Los clientes reales y la suite oficial contra el registro.
# Un solo hilo: los casos eligen el puerto al azar y el almacen de
# imagenes del `podman` es de la maquina.
cargo test --workspace --test oci_e2e_contract -- --test-threads=1

# El informe (evidencia) y el detalle por prueba.
target/uat/oci-compatibility.md
target/uat/oci-conformance/results.yaml

# El informe tiene que estar completo: si falta un caso, o hay uno en
# rojo, el script falla. Esto es lo que la pipeline ejecuta.
bash scripts/check-oci-compatibility.sh target/uat/oci-compatibility.md
```

Las herramientas se pueden señalar por el entorno, para que la prueba use
las que se quiera: `HODEI_DOCKER_BIN`, `HODEI_PODMAN_BIN`, `HODEI_ORAS_BIN`.

El informe se escribe **por ejecución**: el binario de pruebas lleva el
informe en memoria y lo vuelca en cada caso. Una ejecución **parcial**—con
un filtro de nombre— deja el informe con un solo caso, y el script de
arriba lo dice. Por eso la pipeline borra el informe antes de correr y
comprueba que tiene los nueve.
