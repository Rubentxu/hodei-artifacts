# El registro de repositorios — contrato de aceptación

**Tarea:** HSP-132 · **Servicio medido:** `hodei-artifacts 0.2.0`

## 1. Qué afirma este contrato

Afirma que `GET /api/v1/repositories` sirve **la lista que el contrato
declara** y que la lista es de la cuenta del principal verificado, y nada
más. Y dice también lo que **no** promete.

El registro de repositorios es una cosa distinta del catálogo: aquí un
repositorio es un nombre y, si alguien lo dijo, una descripción. El
repositorio del catálogo —el que tiene formato, modalidad y canal— vive bajo
`/api/v1/catalog/repositories`, y las dos superficies no se mezclan.

Las reglas, una por una:

- **La lista es de la cuenta, y la cuenta sale del token verificado.** Un
  repositorio de otra cuenta no sale de la lista **ni se cuenta** en ella:
  decir que existe es decir que es de otro. `X-Account`, `X-User` o
  `X-Scope` no se miran.
- **Cedar decide un repositorio cada vez.** Lo que deniega no sale de la
  página y no se cuenta en el `total`: un `total` que contase lo que el
  principal no puede ver diría cuánta hay. La acción es la de leer un
  repositorio, la misma que pide la lectura de uno.
- **Una decisión que no se pudo tomar no es una lista vacía.** Un
  autorizador que falla o que no llega a un veredicto responde `500` con su
  clase (`indeterminate`), y no una página sin nada: una página vacía
  afirmaría que no hay repositorios donde no se ha podido preguntar.
- **La página es la que se pidió.** `limit` (de 1 a 100, 20 por defecto) y
  `offset` (0 por defecto) cortan la página, y una página que no es una
  página —`limit=0`, `limit=101`, `limit=many`, `offset=-1`, un parámetro
  repetido— es un `400`, no una página arbitraria.
- **Lo que está guardado y no se puede leer se dice.** Una fila cuyo nombre
  canónico no se puede leer es corrupción, y la lista responde `500` con la
  clase `corruption` en vez de encogerse en silencio.
- **El `404` de una cuenta ajena es el `404` de un nombre que no existe.** El
  mismo cuerpo, el mismo estado, la misma respuesta: un `hrn` de otra
  cuenta no se evalúa, y el cuerpo no dice si el nombre existe en otro sitio.
- **El contrato y el código dicen lo mismo.** El contrato de OpenAPI
  declara el camino que el servicio sirve (`/api/v1/repositories`), los
  parámetros que la ruta lee (`hrn`, `limit`, `offset`) y la forma de la
  respuesta (`total`, `items`, `limit`, `offset`), y una prueba lo compara
  con lo que la ruta sirve de verdad.

## 2. Qué se ha medido

| Prueba | Qué comprueba |
| --- | --- |
| `tests/hsp132_registry_list.rs` | La lista de la cuenta, la página, el aislamiento por cuenta, la autorización de Cedar sobre el listado, el `404` uniforme y la comparación del contrato con lo que la ruta sirve. |
| `tests/repository_durable.rs` | La consulta a la base real: la lista es de la cuenta y sale ordenada, y las filas de otra cuenta no viajan. |
| `tests/repository_read.rs` | La lectura por `hrn`: el permiso, el `404` uniforme y el `500` de una decisión que no se pudo tomar. |

### REG-01 — la lista es de la cuenta

Sin `hrn`, la respuesta es la página de los repositorios de la cuenta del
principal verificado que puede leer. Un repositorio de otra cuenta no sale
de la página ni se cuenta en el `total`.

### REG-02 — lo que Cedar deniega no sale ni se cuenta

Con una política que deja leer un repositorio y no los demás, la página
trae el permitido y el `total` cuenta el permitido. El `total` no dice
cuántos hay en la cuenta: dice cuántos se pueden ver.

### REG-03 — la página es la que se pidió

`limit=2&offset=1` sobre tres repositorios devuelve la segunda y la tercera
fila, con `limit` y `offset` en la respuesta. Una página fuera de lo que el
contrato promete es un `400` con la clase `invalid`.

### REG-04 — una decisión que no se pudo tomar no es una lista vacía

Con un autorizador que no llega a un veredicto, la lista responde `500` con
la clase `indeterminate`.

### REG-05 — la lectura por `hrn` no cambia

Con `hrn`, la respuesta es ese repositorio, y no la lista. Un `hrn` de otra
cuenta y un `hrn` que no existen responden el mismo `404` con el mismo
cuerpo.

### REG-06 — el contrato y el código dicen lo mismo

El contrato empaquetado declara el camino, los parámetros y las
propiedades de la respuesta; la prueba compara esas declaraciones con el
cuerpo que la ruta sirve.

## 3. El contrato de OpenAPI

El camino declarado es `/api/v1/repositories`, que es donde la API cuelga.
El camino por identificador que el contrato declaraba antes
(`/v1/repositories/{id}`, con lectura, modificación y baja) se ha retirado:
el servicio no lo sirve, y prometerlo era una promesa que nadie podía
cobrar.

## 4. Lo que este contrato no promete

- **No promete modificar ni borrar** un repositorio del registro. La
  superficie es de alta y lectura, y el contrato de OpenAPI ya no promete
  más.
- **No promete** una lista del catálogo. Los repositorios que tienen
  formato, modalidad y canal son otra cosa, y se listan en
  `/api/v1/catalog/repositories`.
- **No promete** el contenido de los repositorios: ni qué hay dentro, ni su
  digest, ni su tamaño. Eso es lo que dicen la búsqueda y el detalle de un
  paquete.
- **No promete** que el `total` cuente la cuenta entera: cuenta lo que el
  principal puede leer, y por eso puede ser menor que el número de filas
  que hay.
