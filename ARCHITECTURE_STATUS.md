# Estado de la arquitectura

**Hito vigente:** M0 — Convergencia (del roadmap `hodei-artifacts-trustgraph-v3`).
**Generación de documento que manda:** PRD v3 y la UAT de convergencia que lo acompaña.
**Fecha:** 2026-11-16.

Este documento dice la verdad del repositorio: qué se construye hoy, qué es
material heredado, y qué capacidades existen de verdad. Reemplaza a las
generaciones anteriores de material de arquitectura, que describen un producto
que este repositorio ya no es.

La regla que lo gobierna es simple: **una capacidad no está completa porque
esté documentada, sino porque existe su ruta real y su prueba.** Todo lo que
no llega a ese listón está etiquetado, y la etiqueta dice en qué estado está.

## 1. Etiquetas

| Etiqueta | Significado |
| --- | --- |
| `implementada` | Existe la ruta, existe la prueba, y la prueba se ejecuta en la CI. |
| `experimental` | Existe y funciona, pero no cumple todavia el contrato de seguridad o de producto de un hito completo. |
| `planificada` | Especificada en el roadmap, sin implementar. |
| `legacy` | Heredado de una generación anterior. No se construye, no se despliega, no se promete. |
| `deprecada` | Existió y su reemplazo está en marcha. |
| `referencia` | Material de estudio o de diseño. No es código. |

## 2. Crates

### 2.1 Miembros del workspace — `implementada` salvo donde se indica

| Crate | Papel | Estado |
| --- | --- | --- |
| `crates/kernel` | Núcleo compartido: identidad HRN v2, tipos de dominio, puertos. | `implementada` |
| `crates/hodei-iam` | IAM: usuarios, grupos, políticas; persistencia en SurrealDB. | `implementada` |
| `crates/hodei-policies` | Motor Cedar: schema, validación, evaluación, registro. | `implementada` |
| `crates/hodei-evidence` | Contexto de evidencia y registro de eventos (M1) y los normalizadores de los estandares (M2): los contratos, sin adapter. | `implementada` |
| `crates/hodei-evidence-storage` | Los adaptadores de la evidencia sobre el almacen que ya existe. | `implementada` |
| `crates/hodei-hub` | El almacen de contenido direccionado por su digest (HAR-01), con la cuarentena y el canal de confianza. | `implementada` |
| `crates/hodei-catalog` | El catálogo durable de repositorios, assets, versiones y referencias (HAR-02): los contratos, sin adapter. | `implementada` |
| `crates/hodei-catalog-storage` | El adaptador del catálogo sobre SurrealDB, con la transacción de publicación. | `implementada` |
| `crates/hodei-audit` | El contrato de auditoría: el evento, el sink y el outbox (HAR-03). Sin adapter y sin motor de base de datos. | `implementada` |
| `.` (`hodei-artifacts-api`) | API HTTP: composición, arranque, rutas, auditoría. | `implementada` |

Esta tabla no es el inventario completo del workspace: los contextos de M5 a
M8 (confianza, riesgo, alcance, reactivo, simulacion, investigacion) tienen
su propia seccion mas abajo, con su motivo.

El kernel no depende de ningún mecanismo concreto: ni SurrealDB, ni Axum, ni
HTTP. Eso no es una aspiración, lo comprueba una comprobación de forma
arquitectónica. Lo mismo se comprueba ahora del **dominio del catalogo**:
`crates/hodei-catalog` no puede nombrar un motor de base de datos, porque no
es su dependencia. Y del **contrato de auditoría**: `crates/hodei-audit`
define el evento, el sink y el outbox sin nombrar el motor donde se
guardan, porque es un contrato que usan dos contextos con Stores
distintos.

### 2.2 Crates en el árbol que **no** son miembros — material heredado

Ninguno se compila, ninguno se despliega, ninguno se enlaza. Todos están
excluidos del workspace de forma explícita (`exclude` en el manifiesto raíz),
para que nadie los confunda con código vivo.

| Crate | Líneas | Clasificación | Para qué sirve |
| --- | --- | --- | --- |
| `crates/distribution` | 24 455 | `legacy` | OCI, Maven, npm, Docker. La base de M9, si M9 llega. |
| `crates/search` | 15 850 | `legacy` | Búsqueda de texto completo. |
| `crates/artifact` | 14 188 | `legacy` | Subida por trozos, metadatos, versionado. Material para M1 y M9. |
| `crates/hodei-organizations` | 7 924 | `legacy` | SCP y organizaciones. Sustituido por el modelo de cuenta por defecto. |
| `crates/security` | 5 546 | `legacy` | SBOM, vulnerabilidades, firma. Material para M2 y M4. |
| `crates/repository` | 5 113 | `legacy` | CRUD de repositorios. |
| `crates/hodei-authorizer` | 3 077 | `legacy` | Servicio de autorización anterior, superado por el gate Cedar. |
| `crates/policies` | 3 315 | `legacy` | Crate de políticas anterior. No compila contra el kernel actual. |
| `crates/supply-chain` | 962 | `legacy` | SBOM de cadena de suministro. Material para M2. |

**Decisión:** no se borran todavía. Son la única fuente de las ideas de M1,
M2, M4 y M9, y borrarlas tiraría ese material. Cuando una idea se promueva,
el código se trae de aquí y el crate viejo se elimina con su línea.

## 3. Capacidades

### 3.1 Implementadas — con su ruta y su prueba

| Capacidad | Ruta | Dónde se demuestra |
| --- | --- | --- |
| Salud | `GET /health`, `/health/ready`, `/health/live` | `tests/repository_read.rs` |
| Repositorios: alta y lectura | `GET|POST /api/v1/repositories` | `tests/repository_create.rs`, `tests/repository_read.rs` |
| Políticas IAM: alta, listado, lectura, modificación, baja | `/api/v1/iam/policies*` | `tests/iam_policy_timestamps.rs`, `tests/iam_audit.rs` |
| Evaluación de políticas IAM | `POST /api/v1/iam/evaluate` | `tests/policy_circuits.rs` |
| Validación y evaluación de políticas | `/api/v1/policies/validate`, `/api/v1/policies/evaluate` | `tests/policy_circuits.rs` |
| Schema: construir, cargar, registrar IAM | `/api/v1/schemas/*` | `tests/policy_circuits.rs` |
| Playground de políticas | `POST /api/v1/playground/evaluate` | `crates/hodei-policies` |
| Autenticación: secreto compartido y JWKS/RS256 | toda ruta protegida | `tests/production_auth.rs`, `tests/repository_restart_contract.rs` |
| Autorización Cedar con bundle declarado | superficie de repositorios | `tests/policy_circuits.rs` |
| Auditoría de seguridad | eventos de las mutaciones | `tests/audit_events.rs`, `tests/iam_audit.rs` |
| Outbox de auditoría | frontera única de commit y entrega | `crates/hodei-catalog-storage/tests/har03_outbox_contract.rs` |
| Comando operador del índice de políticas | `ensure-iam-index` | `tests/iam_account_index.rs` |
| Contenido crudo: publicar, descargar y describir | `PUT|GET|HEAD /repository/{*target}` | `tests/raw_conformity_uat.rs` |
| OCI: pull de blobs y manifiestos | `GET|HEAD /v2/{*target}` | `tests/oci_pull_contract.rs` |
| OCI: push de blobs y manifiestos | `POST|PATCH|PUT /v2/{*target}` | `tests/oci_push_contract.rs` |
| OCI: índice, etiquetas y referrers | `GET /v2/<name>/tags/list`, `GET /v2/<name>/referrers/<digest>` | `tests/oci_index_contract.rs` |
| OCI: compatibilidad con clientes reales | la suite oficial de la distribution-spec | `tests/oci_e2e_contract.rs` · `uat/UAT-OCI.md` |

### 3.2 Experimentales

| Capacidad | Por qué es experimental |
| --- | --- |
| Frontend (`frontend/`) | Tiene componentes, páginas y 37 pruebas de componente que ahora sí se ejecutan, pero llama a un contrato OpenAPI que describe superficies que el backend actual no expone: usuarios, búsqueda, artefactos y tokens. Su navegación no está alineada con la API viva. |
| Playground de políticas | Admite varias gramáticas de acción y está por detrás de la superficie administrativa. |

### 3.3 Planificadas — en el roadmap, sin implementar

Evidence Graph (M1), Trust Capsule (M2), Trust Gate (M3), Risk Graph (M4),
Reachability (M5), Reactive Security (M6), Simulation (M7),
AI Investigator (M8), Trusted Artifact Hub (M9).

### 3.4 Lo que este repositorio **no** es

La generación anterior de material lo describía como un "registry de
artefactos multi-formato con control de acceso" y una "plataforma de gestión
de organizaciones". Hoy es lo primero **a medias** --un registro de
artefactos OCI con control de acceso, no multi-formato-- y no lo segundo.

- No hay Maven ni npm. La superficie de OCI **sí** existe --pull, push,
  índice, etiquetas y referrers, con su contrato de compatibilidad en
  `uat/UAT-OCI.md`--, pero vive en `src/oci/` y `src/handlers/oci.rs`, no en
  el material de `crates/distribution`, que sigue sin integrar.
- No hay organizaciones ni SCP. El modelo vivo es la cuenta por defecto.
- No hay grafo, ni evidencia, ni decisión de promoción. Son M1, M2 y M3.

## 4. Documentación

| Documento | Clasificación |
| --- | --- |
| Este documento | vigente |
| `docs/security/*.md` | vigente: describe el comportamiento comprobado |
| `docs/ci-strategy.md` | vigente |
| `docs/cedar-policies-manual.md` | vigente |
| `uat/UAT-OCI.md` | vigente: el contrato de compatibilidad OCI --con qué clientes y con qué versión se ha medido, y qué categorías se declaran-- |
| `docs/prd/` (10 documentos) | `deprecada`: el PRD v3 lo sustituye |
| `docs/prd.md` | `deprecada`: el PRD v3 lo sustituye |
| `docs/arquitectura/` (10 documentos) | `legacy`: siete de ellos se llaman "final", "definitiva" o "corregida" y se contradicen entre sí. Este documento los sustituye a todos |
| `docs/architecture.md` | `legacy` |
| `docs/historias-usuario.md`, `docs/DESIGN_REST_API_AND_PLAYGROUND.md`, `docs/PLAYGROUND_EXAMPLES.md` | `referencia` |
| `docs/api/`, `docs/openapi/`, `docs/postman/` | `referencia`: artefactos generados |
| `docs/testing-organization.md` | `deprecada`: describe una organización de pruebas que ya no es la de este repositorio |

## 5. Costuras del tiempo y de la identidad

Un momento y un identificador no son entradas: son hechos sobre la maquina.
Cuando el codigo los lee directamente, el resultado depende de cuando se
ejecuto, y dos ejecuciones de la misma operacion no se pueden comparar. Eso
importa donde el resultado es evidencia: una traza cuyo momento no se puede
reproducir no se puede comprobar.

Por eso el reloj y el generador de identificadores son **costuras**
(`kernel::application::ports::clock`): el cableado de produccion lee la
maquina, y una prueba los fija para que la traza que compara sea un hecho y
no un recuerdo. La traza de auditoria los usa ya para su identificador, su
momento y su correlacion.

## 6. El contexto de evidencia

`crates/hodei-evidence` es el primer contexto de M1. Contiene los contratos
y nada mas: no hay adapter, ni almacen, ni HTTP, ni una sola dependencia de
un motor de base de datos (una comprobacion de forma lo verifica ✓).

Dos reglas dan forma a los contratos:

- **El contenido se referencia, no se copia.** Un registro de evidencia
  guarda el digest y la localizacion de su contenido, y no el contenido. Eso
  hace que el manejo de secretos sea estructural y no una promesa: un
  registro no tiene donde llevar un token.
- **La envoltura de evento es inmutable por construccion.** Los campos son
  privados y no hay manera de cambiarlos; una correccion es otro evento, no
  una edicion. Un doctest `compile_fail` lo demuestra.

La clave de idempotencia es **estructurada**, no un texto que haya que
desmontar: sus cuatro hechos --productor, identidad externa, digest del
contenido y version mayor del normalizador-- son campos ✓. Por eso puede
distinguir una entrega repetida de una revision nueva, y por eso la revision
no sobrescribe la historia.

## 7. El ledger de eventos

`crates/hodei-evidence-storage` implementa el puerto del ledger sobre el
almacen que el despliegue ya usa. El detalle que lo define: **el
identificador de un registro del ledger es la posicion que ocupa en su
corriente** (`event_ledger:corriente~posicion`).

De ahi salen tres cosas a la vez:

- **La unicidad de la posicion es atomica**: es la clave primaria, asi que
  dos escritores no pueden tomar la misma posicion.
- **No hace falta ningun indice**, y eso importa mas de lo que parece:
  definir un indice en el motor local mantiene bloqueado el directorio de
  la base de datos por el proceso que lo define, de modo que un servicio que
  declarara un indice en su camino no podria reiniciar nunca. Con la
  posicion como identificador, el servicio no declara nada.
- **El orden de una corriente es el orden de sus identificadores**.

El precio se dice con su nombre: leer una corriente es un recorrido de la
tabla, no una busqueda por indice. A la escala en la que trabaja M1 es el
canje correcto, y la medida a escala queda para la UAT de M1 en vez de
darse por supuesta.

### Lo que la medida a escala cambio (SPIKE-012)

La medida de la escala de referencia (HSP-70) **midio** ese canje y dio el
numero. Sobre la carga de referencia, con el grafo escrito sin indice:

| Consulta | p50 sin indice | p95 sin indice |
| --- | --- | --- |
| el linaje de un artefacto | 585 ms | 596 ms |
| los despliegues de un artefacto | 136 ms | 147 ms |
| de una construccion a sus artefactos | 137 ms | 141 ms |
| de un repositorio a sus artefactos | 1509 ms | 1553 ms |

La conclusion del SPIKE-012 es que **la indexacion actual no aguanta, y lo
que hace falta no es particionar**. Las cuatro consultas son de uno a tres
saltos sobre un grafo de 22 020 nodos y 7340 relaciones, y sus numeros
crecen con el tamano de la tabla: el grafo respondia a una pregunta que
tocaba diez relaciones leyendo todas las relaciones. Particionar por
cuenta habria repartido el coste sin quitarlo, y habria multiplicado el
almacen por el numero de cuentas.

La respuesta son **indices, declarados por un comando del operador**:
`ensure-graph-index` declara los indices de las tablas del grafo desde su
propio proceso, con el mismo argumento que ya existe para
`ensure-iam-index` (definir un indice en el motor local mantiene bloqueado
el directorio de la base de datos por el proceso que lo define, de modo que
el servicio no puede declararlos en su propio camino). El grafo **no** los
declara: los dice, y el comando los deja.

Con los indices en el sitio, las mismas consultas sobre la misma carga:

| Consulta | p50 con indice | p95 con indice |
| --- | --- | --- |
| el linaje de un artefacto | 2.66 ms | 3.29 ms |
| los despliegues de un artefacto | 1.30 ms | 1.67 ms |
| de una construccion a sus artefactos | 0.43 ms | 0.55 ms |
| de un repositorio a sus artefactos | 0.44 ms | 0.55 ms |

La comparacion es de 90 veces a 3400 veces, segun la consulta. Es el mismo
grafo, los mismos datos y las mismas preguntas: lo unico que cambio es que
las tablas tienen el indice que las preguntas necesitan.

### La envolvente medida, y como crece

La carga de referencia son **107 148 piezas de evidencia** sobre 321 444
nodos y 107 148 relaciones, y la medida de tres tamanos dice como crece:

| Tamano | Piezas | Admision p50 | Proyeccion | Linaje p50 | Linaje p95 | Reconstruccion |
| --- | --- | --- | --- | --- | --- | --- |
| pequeno | 7 340 | 1.70 ms | 10.9 s | 2.66 ms | 3.29 ms | 37.9 s |
| medio | 33 801 | 1.79 ms | 47.8 s | 3.09 ms | 3.81 ms | 193.8 s |
| **referencia** | **107 148** | **1.81 ms** | **164.9 s** | **3.32 ms** | **3.68 ms** | **656.6 s** |

Las consultas **crecen casi poco**: de 7 340 a 107 148 piezas --catorce
veces mas carga-- el linaje pasa de 2.66 ms a 3.32 ms, y el p95 de 3.29 ms
a 3.68 ms. Esa es la forma que hace falta: **el coste de una consulta
depende de la respuesta y no del almacen**. Sin los indices, el mismo
crecimiento multiplicaba el linaje por diez.

La admision tambien es plana por pieza (1.70 ms -> 1.81 ms), porque la
admision pregunta "en que posicion esta la corriente" y la respuesta se
busca por la identidad de la cabeza, y no recorriendo la tabla.

La proyeccion y la reconstruccion **si** crecen con el volumen, y con
razon: la proyeccion escribe un evento por evento (164.9 s para 107 148
eventos son 1.5 ms por evento) y la reconstruccion reconstruye el grafo
entero (656.6 s). Son trabajos de escritura, no de lectura, y su coste es
proporcional a lo que escriben. Lo que importa es que las **lecturas** --la
parte que el usuario siente-- no crecen con el almacen.

### Los cuatro defectos que la medida encontro

La medida no solo midio: encontro. Los cuatro estaban en el camino de la
carga, y los cuatro estan corregidos con una prueba que los defiende.

1. **La admision era cuadratica en la longitud de la corriente.** El
   adaptador preguntaba al registro "en que posicion esta la corriente"
   recorriendo la tabla de eventos y tomando la ultima fila. Ahora hay una
   tabla de cabezas (`event_ledger_head`), y la pregunta se responde por la
   identidad de la cabeza, en la misma transaccion que escribe el evento.
2. **La busqueda de evidencia externa recorria la tabla entera.** Ahora hay
   una tabla de indices externos (`evidence_external`) y la busqueda son
   dos lecturas por identidad.
3. **El grafo encontraba un nodo recorriendo la tabla.** Las consultas del
   linaje pedian "los nodos de tipo artefacto" y elegian de entre ellos el
   buscado. La identidad de un nodo **es** la identidad de su registro, asi
   que ahora se lee por su identidad.
4. **La proyeccion leia la corriente recorriendo la tabla.** La pagina se
   pedia con "cuales son los eventos de esta corriente despues de esta
   posicion", y eso leia la tabla entera y la ordenaba, de modo que el
   coste de la proyeccion crecia mas rapido que la corriente: de 7 340
   piezas a 33 801 --4.6 veces-- el tiempo de proyeccion paso de 18.1 s a
   211.9 s, once veces mas. Ahora la pagina se pide **por las identidades
   de sus eventos**, que es lo que el propio diseno del registro ya
   permitia: la identidad de un evento **es** su posicion, y las posiciones
   de una corriente son contiguas porque el escritor que espera la
   posicion `p` escribe en la `p + 1`. La misma pagina tardo 47.8 s en vez
   de 211.9 s, y el tiempo ya no crece con el almacen: 43 ms con 5 000
   filas en la tabla y 47 ms con 100 000.

Ademas, el vaciado de una proyeccion **quitaba la tabla**, y quitar la
tabla se lleva los indices por delante: el grafo volvia a recorrer la tabla
entera en cada pregunta. Ahora el vaciado borra las filas y deja la tabla,
y una prueba (`g8`) lo defiende.

### La forma de la carga de referencia

El modelo de escala (`crates/hodei-evidence-storage/src/scale.rs`) no es un
numero: es una forma. Las cuentas se reparten entre un minimo y un maximo
con una progresion geometrica, y cada nivel de la forma se reparte asi
mismo: los repositorios por cuenta, las revisiones por repositorio, las
construcciones por revision. Una carga en la que todo cuelga de un solo
repositorio no se parece a un grafo real, y por eso la forma se reparte.

El modelo tiene **una sola fuente de verdad**: la cuenta de eventos que se
esperan es la suma de los viajes, y el viaje de cada cuenta sale de la
misma forma que la cuenta. La prueba `the_count_of_the_events_is_the_size_of_the_journeys`
lo demuestra para el modelo pequeno y para el de referencia.

La medida encontro tambien un defecto del modelo: dos piezas del mismo viaje
llevaban la misma identidad, y dos cuentas distintas llevaban identidades
que se pisaban. El almacen toma la segunda por una repeticion y guarda un
solo hecho, asi que 858 piezas de la carga de referencia nunca llegaron a
ser hechos y la carga era menor de lo que el modelo decia. Las identidades
llevan ahora la cuenta, la construccion y el artefacto que nombran, y dos
pruebas nuevas (`no_two_pieces_of_a_journey_carry_the_same_identity` y
`no_two_accounts_carry_the_same_identity`) defienden el modelo de esa
misma clase de error.


## 8. La admision de la evidencia

La canalizacion del plan, con sus pasos y lo que cada uno garantiza:

```
receive -> authenticate producer -> validate envelope/schema ->
calculate content digest -> deduplicate/idempotency check ->
preserve raw evidence -> normalize deterministically ->
validate canonical identities -> accept/reject -> append accepted event
```

Dos reglas la atraviesan:

- **Lo que no se admite no muta nada.** Un rechazo escribe su evento --un
  rechazo es historia-- y deja intactos el almacen de evidencia y el grafo.
- **La historia no se sobrescribe.** La misma entrega dos veces devuelve el
  resultado que ya existe; la misma identidad externa con contenido distinto
  es una revision nueva que conserva la anterior.

El sobre de entrada lleva una firma sobre su **forma canonica**: una
secuencia de bytes con longitud por delante de cada campo, de modo que dos
sobres distintos no pueden producir los mismos bytes. La firma se comprueba
ANTES de admitir nada.

El digest del contenido es el hash de sus bytes, y lo calcula el contrato
(`ContentDigest::of_bytes`), de modo que el productor, Hodei y quien compruebe
la evidencia mas tarde llegan al mismo valor.

## 9. El grafo

El grafo es una **proyeccion** del registro de eventos, no una segunda
verdad: el registro es la historia y el grafo se puede tirar y
reconstruir. Por eso nadie escribe una relacion: el proyector lee los
eventos admitidos y dice lo que significan, y el escritor aplica lo que el
proyector dijo.

Los nodos de M1 son repositorio, revision, construccion, artefacto,
despliegue y evidencia. Las relaciones son tipadas y no enlaces anonimos,
porque la pregunta que el grafo responde depende del tipo de la relacion y
no solo de sus extremos. Una relacion lleva **la evidencia que la sostiene**
y el momento en que se observo.

Las consultas son de cuenta: una consulta nunca cruza de una cuenta a otra.

### La reconstruccion

La reconstruccion relee el registro por el mismo proyector y construye una
proyeccion **aparte** de la que esta en uso. La comparacion es semantica:
dos grafos son el mismo cuando tienen los mismos nodos y las mismas
relaciones con el mismo apoyo, no cuando sus bytes coinciden. El grafo en
uso no se vacia nunca antes de que la reconstruccion haya pasado la
comprobacion, y la reconstruccion no altera el registro.

### Lo que NO se afirma

- **El contenido de la evidencia vive en memoria.** El SPIKE-009 decidira
  donde viven los contenidos grandes.
- **La firma es HMAC con un secreto por productor.** La verificacion con
  firma de un tercero es de un hito posterior.
- **El aislamiento entre cuentas es logico, no fisico.** Las cuentas
  comparten las tablas del grafo y del registro, y la separacion la hace la
  consulta. La medida lo confirma: el coste de una consulta crece con el
  volumen **total** del almacen y no con el de la cuenta, porque la
  pregunta se responde con el indice y el indice es comun. El
  particionado por cuenta queda para cuando la cuenta mas grande lo
  pida, y la medida dice que todavia no lo pide.

## 10. La UAT de M1

Los ocho escenarios de `uat/M1-EVIDENCE-GRAPH-UAT.md`, ejecutados sobre
fixtures locales y servicios efimeros, sin integraciones obligatorias con
otros productos. El comando:

```
cargo test -p hodei-evidence-storage --test m1_uat -- --nocapture
```

| Escenario | Que demuestra | Resultado |
|---|---|---|
| M1-UAT-01 | el linaje de la fuente al despliegue, con la evidencia en las relaciones criticas | PASSED |
| M1-UAT-02 | la entrega repetida diez veces y a la vez: un solo registro y un solo conjunto de relaciones | PASSED |
| M1-UAT-03 | la revision de un registro externo conserva el historico | PASSED |
| M1-UAT-04 | la evidencia invalida se rechaza con un motivo y no deja nodos en el grafo | PASSED |
| M1-UAT-05 | la reconstruccion dice lo mismo que el grafo en uso, en las cuatro consultas criticas | PASSED |
| M1-UAT-06 | el fallo de la proyeccion no mueve el checkpoint, y al recuperarlo aplica cada evento una vez | PASSED |
| M1-UAT-07 | el aislamiento entre cuentas, con los mismos nombres externos en las dos | PASSED |
| M1-UAT-08 | la linea base del rendimiento de referencia | PASSED (SPIKE-012: la envolvente esta medida y la conclusion dice que la indexacion actual no aguanta y que lo que hace falta son indices, no particionar) |

### Lo que la UAT dejo abierto

- **Las consultas dependien de que el operador haya declarado los
  indices.** El grafo no los declara --un indice declarado por el servicio
  bloquearia el directorio de la base de datos-- y el comando
  `ensure-graph-index` los declara desde su propio proceso. Un despliegue
  que no lo ha ejecutado sigue respondiendo bien, pero recorre las tablas:
  la UAT lo midio en unos 10 ms por consulta de uno a tres saltos. El grafo
  **dice** si los indices estan (`indexes_initialized`), de modo que el
  estado es observable en vez de supuesto.
- **El contenido de la evidencia vive en memoria.** El SPIKE-009 decidira
  donde viven los contenidos grandes.
- **La firma es HMAC con un secreto por productor.** La verificacion con
  firma de un tercero es de un hito posterior.

## 10-bis. Los estandares que Hodei lee (M2)

Los estandares entran por **normalizadores locales y fixtures de
conformidad**, antes de que se encargue cualquier conector de proveedor.
Eso lo dice el plan y esto es donde se cumple.

### El modelo propio, y no la forma del estandar

Hay **un** modelo de inventario, y es de Hodei. CycloneDX 1.7 y SPDX 3.0
son dos normalizadores del mismo modelo: el camino de compatibilidad con
SPDX no es un segundo modelo ni un segundo formato de cable, es un segundo
normalizador que produce los mismos hechos. La prueba lo dice: los dos
documentos de la suite dicen lo mismo y salen en los mismos hechos.

| En el documento | CycloneDX 1.7 | SPDX 3.0 | En el modelo |
|---|---|---|---|
| el sujeto | `metadata.component` | el paquete al que apunta `DESCRIBES` | `SbomSubject`, y ademas un componente |
| los componentes | `components[]` | `packages[]` | `SbomComponent` |
| el PURL | `components[].purl` | `externalRefs[referenceType=purl]` | identidad `purl`, confianza `confirmed` |
| el digest | `components[].hashes[]` | `externalRefs[referenceType=sha256]` | identidad `content_digest`, confianza `confirmed` |
| las dependencias | `dependencies[].dependsOn` | `relationships[DEPENDS_ON]` | `SbomDependency` |
| el momento | `metadata.timestamp` | `creationInfo.created` | `document_timestamp` |

Dos reglas que la tabla hizo visibles:

- **El sujeto es tambien un componente.** CycloneDX lo nombra aparte y
  SPDX lo nombra entre los paquetes, y el grafo de dependencias arranca en
  el. El modelo tiene una sola regla para los dos, y un normalizador que
  lo encuentre solo como sujeto lo anade a los componentes.
- **Solo se guardan las aristas que existen.** CycloneDX escribe una
  entrada por cada componente. Una entrada vacia dice que ese componente no
  depende de nada, y eso ya lo dice su ausencia.

### La incertidumbre se ve

Una identidad externa lleva su espacio de nombres, su valor, su evidencia
de origen, su confianza, su vigencia y la version de su canonicalizacion.
La confianza no es una escala de "que tan buena" sino de "que se puede
hacer con ella":

| Grado | Que significa | Veredicto |
|---|---|---|
| `confirmed` | el documento lo dice | si |
| `deterministic` | lo produjo una regla versionada | si |
| `unconfirmed` | una pista: se parece, nadie lo confirmo | **no** |
| `ambiguous` | mas de una cosa podria ser la | **no** |

Es la linea del modelo de amenazas: "un mapeo ambiguo queda sin resolver o
con confianza baja" y "ninguna fusion heuristica puede satisfacer en
silencio una politica de alta garantia".

### La atrestacion se verifica, no se copia

Una atrestacion sin verificar es una declaracion, y el dominio distingue
las dos cosas: `Verified`, `Invalid` con su motivo, y `Unverified` con su
motivo. La verificacion es **real**: la envoltura DSSE se comprueba contra
la clave de su emisor, sobre la codificacion PAE --que incluye el tipo
del payload-- y no sobre los bytes del payload.

Las causas del rechazo son un conjunto cerrado de siete, cada una con lo que
hay que mirar: envoltura malformada, payload ilegible, sin firma, clave
desconocida, firma malformada, la firma no corresponde al payload, y el
sujeto no es el que se ofrecio. Una clave que el despliegue no conoce **no**
se reporta como una firma mala: es un fallo de la confianza y no de la
firma, y las dos cosas llevan a reparaciones distintas.

La atrestacion se ata al artefacto: una procedencia que habla de otro
digest no es evidencia sobre este artefacto, y el motivo lo dice.

### Lo que no se hace, y por que

La verificacion en linea --el registro de transparencia de Sigstore y la
renovacion de la raiz de confianza-- es otro hito. Aqui se verifica el
formato de Sigstore contra la clave que se le da al despliegue, y la
limitacion queda escrita en el codigo.

### Donde vive el contenido (SPIKE-009)

La regla que sale de la medida **no es una regla de tamano sino de uso**:
los **hechos** van en linea, porque el grafo responde de ellos, y el
documento crudo va **siempre** por referencia, porque solo se inspecciona.
El documento crudo no va nunca dentro del registro de evidencia, sea del
tamano que sea: un registro lleva una `ContentRef` --digest y `object_ref`
opaco-- y no el contenido, y por eso no tiene donde meter un secreto. El
`object_ref` es opaco a proposito, y por eso el contrato no filtra el
almacen al dominio.

Medido: un componente del modelo nativo son 296 bytes, de modo que 10 000
componentes son 2.9 MiB de hechos. El umbral propuesto es 10 000
componentes: por debajo, los hechos caben de sobra en linea; por encima,
los hechos se mueven tambien al almacen direccionado por contenido y la
base de datos se queda con el indice de identidades y las aristas.

## 10-ter. La capsula de confianza y el perfil (M2)

Una capsula de confianza es **el pasaporte verificable de una release**: una
foto del estado de la confianza en un instante dado, con su hash y su
version, de modo que dos capsulas de la misma release se puedan comparar y
una capsula se pueda volver a verificar.

### La distincion que lo sostiene

**Presente no es verificado.** Y una dimension sin evidencia no se rellena
con una suposicion: es `MISSING`, y `MISSING` no es un estado favorable. El
modelo de amenazas lo dice --"la evidencia que falta no es lo mismo que la
evidencia que pasa"-- y el perfil lo dice en un valor que una politica puede
leer.

Los estados son explicitos y no son booleanos, porque un booleano no puede
decir "la evidencia esta ahi pero es vieja" aparte de "la evidencia esta ahi
y sostiene":

| Estado | Que dice | Veredicto |
|---|---|---|
| `VERIFIED` | la evidencia esta ahi, se comprobó y sostiene | si |
| `UNVERIFIED` | la evidencia esta ahi y nadie la comprobó | **no** |
| `INVALID` | la evidencia esta ahi, se comprobó y no sostiene | **no** |
| `CONTRADICTORY` | la evidencia esta ahi y dice mas de una cosa | **no** |
| `STALE` | la evidencia esta ahi y es mas vieja que el umbral | **no** |
| `FRESH` | la evidencia esta ahi y es actual | si |
| `MISSING` | no hay evidencia | **no** |
| `UNKNOWN` | hay evidencia y no zanja la pregunta | **no** |
| `COMPLETE` / `PARTIAL` | lo que un inventario dice de si mismo | si / no |
| `NOT_APPLICABLE` | la dimension no aplica a este sujeto | -- |

Las cuatro que la UAT de M2 exige que sean distinguibles --la ausente, la
caducada, la invalida y la contradictoria-- lo son, y ninguna cuenta como
buena.

### El hash cubre las entradas, no el momento

Eso es todo el sentido del hash. Una decision que apunta a una capsula
apunta a la evidencia exacta de la que se hizo la lectura, y un lector
posterior puede comprobar que la capsula de ese hash sigue diciendo lo que
decia. Si el momento del ensamblaje fuera parte del hash, dos lectores de la
misma evidencia tendrian dos capsulas distintas y no se podrian comparar. Y
la **version** del ensamblaje si es parte del hash: una capsula ensamblada
por otra version es otra lectura.

### Lo que la dimension dice

Cada dimension dice la identidad de su evidencia, o dice que no hay ninguna,
y el enlace lleva lo que un lector necesita para ir a mirar: la identidad de
la evidencia, hasta donde llega su verificacion, y la version del
normalizador que la leyo --porque el mismo documento leido por dos
normalizadores son dos piezas de evidencia y no una.

### La API

`GET /api/v1/capsules?hrn=...` devuelve la capsula y su perfil, con la
evidencia enlazada. Es una lectura, tiene su propia accion de Cedar
(`Artifacts::Action::"ReadCapsule"`, sobre el tipo `Artifacts::Artifact`), y
no cruza de cuenta: la capsula de otra cuenta se esconde tras el mismo 404
que la que no existe.

## 10-quater. La politica de confianza, y por que no es Cedar (M3)

### La conclusion del SPIKE-004

**La politica de confianza NO se expresa en Cedar.** No es una conclusion
sobre lo bueno que es Cedar: es una conclusion sobre la forma de los dos
lenguajes, y se puede escribir entera.

Cedar evalua **una peticion cada vez**, contra una tripleta `(principal,
accion, recurso)`, y responde si o no con un motivo para el no. Una politica
de confianza no es ninguna de esas cosas:

- Es un **conjunto de requisitos** sobre un **contexto precalculado**: la
  capsula dice trece cosas del artefacto y la politica dice cuales tienen
  que cumplirse. Cedar no tiene agregados, de modo que "el inventario esta
  completo *y* la procedencia esta verificada *y* no hay una vulnerabilidad
  critica alcanzable" son tres politicas que no se conocen entre si, y la
  regla que dice "gana la peor" no existe en Cedar.
- Tiene que producir **codigos de motivo** como salida. Los motivos de
  Cedar son texto de diagnostico sobre una peticion; los codigos de motivo
  de una decision de confianza son un conjunto cerrado que un informe, una
  politica y una auditoria pueden leer todos, y son por requisito y no por
  peticion.
- Es **versionada y se activa con historial**. El conjunto de Cedar es el
  que se ha cargado; no hay nocion de un conjunto que se activo en un
  momento y bajo el que una decision de hace seis meses se pueda leer
  todavia.

Lo que Cedar **si** hace, y mejor que nadie, es decidir **quien puede hacer
que**. Esa es la mitad de la autorizacion, se queda en Cedar, y es
exactamente para lo que su modelo esta hecho.

La frontera queda asi:

```text
Cedar                    ->  quien puede promover esto
el motor de confianza    ->  si el artefacto cumple
```

Y la puerta que junta las dos mitades dice **cual de las dos decidio**, de
modo que un rechazo nunca es un acertijo.

Las computaciones propias de la confianza --que es alcanzable, que es
critico, que esta expuesto-- ocurren **antes** de la evaluacion y llegan
como hechos en el contexto. Una politica que consultara un grafo seria una
politica cuya decision depende de cuando se pregunto, que es lo contrario
de reproducible.

### El contexto, versionado y determinista

El contexto es un **valor**, no un mango: una vez construido no cambia, y
dos contextos con el mismo contenido son el mismo contexto. Lleva la version
del constructor que lo armo, y la decision la nombra, de modo que una
decision de hace meses se puede leer con las reglas que la produjo.

### La decision, inmutable y con codigos

La decision dice el resultado (`ALLOW`, `DENY`, `WARN`, `REQUIRE_APPROVAL`),
los codigos de motivo, que le paso a cada requisito, la version del
contexto, la del conjunto de politicas y la de su esquema, el hash de la
capsula y la evidencia que se uso. `WARN` no es un `ALLOW` encubierto, y
la politica decide si lo que falta es una negacion o una peticion de
aprobacion --el motor no lo decide, porque eso es una cuestion de la
organizacion y no del software.

### La activacion con historial

Un conjunto de politicas no es un fichero que se lee cuando hace falta: se
**activa**. Activarlo dice que a partir de ahora decide esta version, y
deja constancia. Cambiar la politica no reescribe el pasado: las decisiones
nombran la version con la que se tomaron, y esa version sigue ahi.

## 10-quinquies. La promocion, con su decision duradera (M3)

### La decision se escribe, y se queda escrita

Una promocion es una **peticion** y una **decision**, y la decision se
escribe. Dice quien decidio, con que politica, sobre que evidencia, y con
que resultado -- y lo sigue diciendo despues de que la promocion ocurriera,
porque un flag que dice "promovido" sin historia no es una decision sino
una anecdota.

El almacen no ofrece forma de **cambiar** una decision: una decision que se
puede editar no es una decision. Y la decision viaja entera -- la politica,
el contexto, las entradas y las razones -- en un solo documento, porque
partirla en columnas haria posible leer media decision.

### La ausencia no es un refusal

Hay tres fallos que se parecen y que no son lo mismo, y el sistema los
distingue:

| Lo que pasa | Como se dice |
|---|---|
| los criterios no se cumplen | `DENY` |
| los criterios no se pudieron mirar porque faltaba una entrada obligatoria | `INDETERMINATE` |
| no hay distribucion a la que promover | `not supported` |

El segundo importa porque el primero espera una aprobacion y el segundo
espera la evidencia. El tercero no es una decision: no hay a donde ir, y
la respuesta honesta es que la operacion no esta soportada, nunca un
exito que no ocurrio. Y la decision se escribe igual, con lo que paso.

### La excepcion tiene tres partes

Una excepcion no es un interruptor: es **algo que alguien pidio**, que
**otro alguien aprobo** y que **deja de funcionar en un momento que se
escribio al aprobarla**. La caducidad es obligatoria y el codigo lo
comprueba: una excepcion sin caducidad no se aprueba. Y el que pide no
puede aprobar la suya.

Una excepcion caducada no vale nada en la decision, y solo la excepcion
**nombrada** waivea el requisito: una excepcion viva que no es la que la
politica nombro no waivea nada aunque este activa.

## 10-sexies. El grafo de riesgo, y la regla que lo sostiene (M4)

### La regla que lo sostiene

**LA AUSENCIA DE EVIDENCIA NO ES EVIDENCIA DE AUSENCIA.**

Un artefacto sin inventario tiene estado **desconocido**, nunca "no
afectado". Una version que no se puede comparar es desconocida, nunca "no
afectada". Un VEX que nadie verifico no silencia un hallazgo.

Un sistema que no puede decir "nadie lo sabe" acaba diciendo que un
despliegue con cuatrocientas dependencias esta bien, y asi es como la
gente deja de leer los informes.

### El estado tiene TRES valores

| Estado | Que dice |
|---|---|
| `affected` | el artefacto contiene algo de lo que habla el aviso |
| `unaffected` | la evidencia dice que no |
| `unknown` | nadie lo sabe, y "nadie lo sabe" no es "no afectado" |

El tercero es el que pesa, y es el que un sistema con dos valores no puede
expresar.

### El emparejamiento de versiones

Resuelve la version exacta, el rango y -- lo que importa -- la version
**desconocida**. Un rango de OSV se lee por sus eventos (`introduced`,
`fixed`, `last_affected`), y una version que no se puede ordenar deja la
respuesta en desconocido. Un rango de tipo `GIT` es sobre commits y no
sobre versiones, y tambien deja la respuesta en desconocido.

### El VEX es una DECLARACION, y se dice de quien

Una declaracion verificada es un hecho sobre el producto; una declaracion
que nadie verifico es una declaracion que alguien escribio. La diferencia
es todo el punto: un VEX **no verificado** que dice "esta corregido" no
cierra el hallazgo, y el hallazgo se queda a la vista con la declaracion al
lado. Y solo la declaracion **nombrada** para el componente lo mueve: una
declaracion viva que no es la del componente no mueve nada aunque este
activa.

La comparacion del producto con el componente entiende las dos formas de
escribir el mismo paquete: un VEX lo nombra por su PURL y el hallazgo por
sus coordenadas.

### El radio de impacto y la PROFUNDIDAD

El recorrido es `vulnerabilidad -> componente -> dependiente -> artefacto ->
despliegue`, y lo interesante no es el recorrido sino **la profundidad**. Una
dependencia que el artefacto tiene directamente es otra cosa que una que
tiene a traves de otras tres, y un informe que dice "afectado" sin decir
cual es un informe que la gente deja de leer.

La profundidad es una propiedad del **camino**, no de la arista: la arista
de un componente al del que depende no sabe si la dependencia es directa o
transitiva, y el recorrido es lo que las distingue.

### El caso de seguridad

Un hallazgo es un hecho, no una tarea. El caso de seguridad es la tarea:
dice que lo abrio, que esta afectado, hasta donde llega el daño, quien lo
tiene y **que probaria que esta corregido**.

El caso tiene una **linea del tiempo**, y la linea del tiempo es la razon
de que el caso sea un objeto y no una fila que se sobrescribe: el estado
se deriva de la evidencia mas nueva, pero el disparador que lo abrio sigue
ahi, y tambien cada cambio.

Y la remediacion verificada **necesita la evidencia del despliegue**: un
problema cerrado sin la evidencia de que el artefacto afectado dejo de
correr es una esperanza, y una esperanza no cierra un caso.

## 10-septies. El grafo de riesgo en el grafo de confianza (M4)

### Los tipos del grafo, leidos desde el dominio

El grafo de riesgo **no es un grafo aparte**: es el grafo de confianza con
tres tipos de nodo y tres tipos de arista mas.

```text
artefacto  --contains-->   componente
componente --depends_on-> componente
componente --affected_by-> vulnerabilidad
```

Y hay una decision de diseño que se pago cara: **la lectura de los tipos
vive en el dominio**, no en el almacen. El almacen tenia su propia tabla
de tipos, y cuando se anadio un tipo nuevo --el componente-- el almacen
seguia leyendolo como "desconocido" y lo tiraba. Un tipo que se anade al
dominio tiene que leerse en todas partes, y por eso la lectura es una
funcion del dominio y no una copia.

### El sujeto no es un componente de si mismo

El documento de inventario lista al sujeto entre sus componentes, porque
sus dependencias tienen que apuntar a algo. La proyeccion lo lee como lo
que es: el sujeto es el artefacto, y no un componente del artefacto.

### Las aristas del sujeto no se escriben dos veces

Las aristas que arrancan en el sujeto son las que el artefacto sostiene
sus componentes con. Escribirlas tambien como dependencias contaria la
dependencia **dos veces** en el recorrido, y la profundidad --la distancia
que dice si la dependencia es directa o transitiva-- dejaria de ser la
distancia.

### La proyeccion es una funcion pura

No consulta nada y no decide: dice lo que el grafo deberia tener, y el
grafo se escribe entero o no se escribe. Y toda relacion dice la evidencia
que la sostiene, de modo que el grafo no acepta una relacion cuya evidencia
no esta --que es lo correcto, porque una relacion sin evidencia es una
afirmacion.

## 10-octies. La alcanzabilidad, con su contrato de evidencia (M5)

### La conclusion del SPIKE-007

**Hodei no construye un analizador de codigo universal.** Lo que construye
es un contrato de evidencia de alcanzabilidad que **entiende y usa** la
evidencia que otro produjo, con su cobertura y sus limites declarados.

La razon no es que el analisis de codigo sea dificil: es que un
`UNREACHABLE` sin cobertura y sin limites es una **apuesta**, y una
apuesta no reduce la incertidumbre de un hallazgo. Lo que si la reduce es
un `REACHABLE` con su camino, porque ahi hay algo que mirar.

El camino que se elige es el de **adaptar un analizador**, no el de
construir uno: el puerto es lo que deja entrar a cualquier analizador, y
el proveedor local que vive aqui es **acotado a proposito** -- lee un
programa que se le describio y produce la evidencia de esa descripcion.
Demuestra que la cadena funciona y no presume ser mas.

### LA REGLA: LO QUE NO SE PUEDE RESOLVER ES UNKNOWN, NUNCA UNREACHABLE

Una llamada que se resuelve en tiempo de ejecucion y que el analizador no
siguio **no es una prueba** de que el codigo no se alcanza: es una prueba
de que nadie lo sabe. Y un analisis que no cubrio todo el codigo tampoco
prueba nada.

El orden de las reglas es fijo, y importa:

1. Si hay un camino desde un punto de entrada, el simbolo es
   **REACHABLE**, y el camino va en la respuesta.
2. Si no hay camino, pero el analisis no cubrio todo o encontro algo que no
   pudo resolver, el simbolo es **UNKNOWN**.
3. Solo cuando no hay camino, la cobertura fue completa y no hubo
   limitaciones, el simbolo es **UNREACHABLE**.

La primera gana sobre la segunda y la segunda sobre la tercera: un simbolo
alcanzable es alcanzable aunque el analisis encontro otra cosa que no pudo
resolver, y un simbolo cuyo analisis fue incompleto es desconocido aunque
no haya camino.

### La identidad del codigo

Un simbolo se identifica con **tres** cosas: el modulo, el fichero y el
simbolo. No una y no dos. Un nombre solo no es una identidad --dos modulos
pueden tener una funcion `parse`--; un fichero solo tampoco --una
construccion puede producir el mismo fichero dos veces--; y un modulo solo
tampoco --el mismo modulo se puede construir para dos objetivos--.

Y la identidad es lo que permite **emparejar un simbolo vulnerable** con lo
que Hodei analizo, sin que un "alcanzable" falso sea facil de producir por
casualidad.

### Las versiones del analizador se conservan

El mismo codigo analizado por dos versiones de un analizador puede dar dos
respuestas, y las dos se guardan. La mas nueva decide el estado, y el
estado **dice que analizador y que version** lo produjo, porque una
alcanzabilidad que no nombra su version no se puede reproducir ni discutir.

## 10-nonies. El SPIKE-003: la proyeccion analitica NO se anade (M5)

### Que se midio, y con que datos

La comparacion es del **mismo dataset** y del **mismo recorrido**: el
binario de la medicion construye el grafo en SurrealDB, lo mide y lo
exporta; el prototipo importa ese mismo dataset en LadybugDB y hace la
misma pregunta.

La medicion del grafo transaccional, a la carga de referencia del SPIKE-012
(107 148 piezas, 331 664 nodos, 211 373 relaciones):

| | radio p50 | radio p95 | radio max | linaje p95 |
|---|---|---|---|---|
| el grafo transaccional | 579 ms | **650 ms** | 795 ms | 4 ms |
| la proyeccion analitica (escala pequena) | 18 ms | 23 ms | 31 ms | n/a |
| el grafo transaccional (escala pequena) | 2 ms | 3 ms | 3 ms | 1 ms |

Y el coste de la proyeccion, que es la otra mitad de la pregunta:

| | coste |
|---|---|
| reconstruir el grafo transaccional | 656.6 s (11 min) |
| importar el mismo dataset en LadybugDB | **> 55 min y 5 GB** |

### La conclusion

**No se anade una proyeccion analitica secundaria.** Y ahora por un motivo
bueno y no por una medicion del vacio:

1. El grafo transaccional **no llega** al objetivo de 50 ms: responde en
   650 ms. La diferencia esta medida: este almacen devuelve **cada fila a
   ~1 ms** (502 ms con seis columnas, 454 ms con una, 1014 ms con una
   consulta por fila) y el recorrido lee ~1044 relaciones.
2. La proyeccion analitica, medida con el SDK de Rust, **no es mas rapida**:
   23 ms contra 3 ms en el mismo recorrido a escala pequena, porque su API
   pregunta de un nodo en un nodo y el grafo transaccional pregunta los
   nodos de un nivel juntos.
3. Y su **coste de proyeccion** dwarfs la pregunta que viene a resolver:
   mas de 55 minutos y 5 GB a la escala de referencia, contra los 11
   minutos de reconstruccion del almacen transaccional. Se pagarian 55
   minutos de reconstruccion por cada vez que la verdad cambia para
   ahorrar milisegundos en una consulta.

### Y la salida que queda

La forma de alcanzar el objetivo **dentro del mismo almacen**: una **vista
materializada del impacto**, que la proyeccion escribe cuando llega una
vulnerabilidad y que la consulta lee en **una** fila en vez de ~1044. Es
la respuesta de CQRS, no necesita un segundo proceso ni una segunda
verdad, y es el camino que queda por medir.

La decision esta escrita en `ADR-012-no-secondary-analytic-projection.md`.

### Dos cosas que el prototipo dijo de la integracion

1. **Los dos motores no conviven en el mismo binario**: la libreria
   precompilada de LadybugDB trae su propio zstd y choca con el de
   SurrealDB. Una proyeccion secundaria es un proceso aparte, y el
   prototipo tiene que parecerse a lo que seria.
2. **El motor analitico necesita OpenSSL a mano**: su cliente HTTP con TLS
   viene compilado con TLS y el enlazador no lo resuelve solo.

### Lo que quedo sin medir, y por que esta escrito

La vista materializada del impacto **no esta medida todavia**: su coste de
mantenimiento y el p95 que daria es lo que decide si el objetivo se
alcanza sin un segundo proceso. Es el trabajo que sigue, y su medicion
pertenece al grafo de riesgo (M4/M5).

## 10-decies. M6: el runtime de las reacciones (SPIKE-006)

### La conclusion del spike, y por que

El SPIKE-006 preguntaba: "¿hace falta un runtime minimo, o un manejador de
eventos ordinario basta?" Y la exit gate del hito lo pide literal:
**observable, acotado, idempotente y reconstruible**.

La respuesta es que un manejador ordinario **no basta**, y se ve en las
cuatro propiedades:

| Lo que pide la exit gate | Lo que tiene un manejador de eventos |
|---|---|
| idempotente | nada: es una llamada a una funcion y repetirla repite el efecto |
| acotado | nada: nada frena un bucle, y un bucle que dispara efectos es un incidente |
| observable | el log, que no es una traza con causalidad |
| reconstruible | nada: no hay estado durable que leer |

Asi que el runtime **si** hace falta, pero el minimo: no hay lenguaje de
workflows, no hay plugins dinamicos, no hay planificador distribuido y no
hay DSL. El runtime hace **cuatro cosas** y nada mas:

- **Suscribir**: un registro de comportamientos, cada uno con lo que le
  interesa.
- **Guardar**: la condicion se comprueba; si no se cumple, la reaccion no
  corre y **el motivo queda en la traza**.
- **Registrar**: la ejecucion es durable, con su cadena de causalidad, su
  numero de intentos y su traza.
- **Acotar**: un presupuesto corta la cadena cuando se excede, y **el corte
  queda en la traza**.

Las reacciones de verdad --la del CVE y la de la caducidad de una
excepcion-- son **codigo**, no reglas: se registran en el runtime y el
runtime las llama. El runtime no sabe que es un caso de seguridad ni que
es una decision de confianza: eso lo saben las reacciones, a traves de
puertos.

### Lo que el crate hace

`hodei-reactive` son cuatro modulos y un registro:

- `events`: el evento, la suscripcion y el predicado del payload. El
  predicado es un conjunto **cerrado** (`FieldEquals`, `FieldExists`,
  `FieldAbsent`): un predicado que pudiera decir cualquier cosa seria un
  lenguaje de consultas, y un lenguaje de consultas aqui seria un motor de
  workflows con otro nombre.
- `behavior`: el comportamiento, la guarda, el efecto y la traza.
- `execution`: el registro durable, el presupuesto y el corte.
- `runtime`: el despacho, el registro de comportamientos y el almacen de las
  ejecuciones.

### La semantica que se promise, y que esta probada

| Lo prometido | Donde se prueba |
|---|---|
| un comportamiento se registra, se suscribe y corre cuando el evento encaja | `a_behaviour_runs_when_the_event_matches` |
| la traza es durable y un fallo reintenta con la semantica declarada | `the_trace_is_durable_and_a_failure_retries` |
| el presupuesto corta la cadena y el corte queda en la traza | `the_budget_cuts_the_chain_and_the_cut_is_in_the_trace` |
| los efectos son idempotentes y repetir la reaccion no duplica el efecto | `the_effects_are_idempotent_and_the_repetition_does_not_duplicate_them` |
| un CVE nuevo dispara la reevaluacion y, si procede, el caso | `a_new_vulnerability_triggers_the_reevaluation_and_the_case` |
| una vulnerabilidad que no toca nada no reevalua nada | `a_vulnerability_that_touches_nothing_reevaluates_nothing` |
| la caducidad de una excepcion dispara su reaccion | `the_expiry_of_an_exception_triggers_its_reaction` |
| la reconstruccion no repite los efectos externos | `the_replay_does_not_repeat_the_external_effect` |

### Dos decisiones que la implementacion tomo y que conviene saber

1. **La clave de la idempotencia es la del EFECTO, no la del evento.** Un
   evento repetido no vuelve a correr el comportamiento, y un evento
   DISTINTO que produce el mismo efecto lo suprime en vez de aplicarlo dos
   veces. Con la clave del evento, dos eventos distintos queJmueven el mismo
   efecto lo habrian hecho dos veces.
2. **La reconstruccion no ejecuta nada.** `replay` lee los registros
   durables y **usa el resultado que quedo grabado**; el efecto externo no
   se repite, ni siquiera cuando la reaccion se reconstruye despues de un
   reinicio. Esa es la propiedad de "reconstruible" de la exit gate.

### Lo que NO se ha construido, y por que

El SPIKE-006 era explicito sobre lo que no hay que construir, y no se ha
construido: ni motor de workflows generico, ni plugins dinamicos, ni
planificador distribuido, ni DSL de workflows, ni interfaz de usuario. El
runtime es un registro, una traza y un presupuesto.

## 10-undecies. M7: la simulacion es una superposicion, no una copia

### La conclusion del SPIKE-008, y su razon

La pregunta era **como** se crea el estado aislado de un "que pasaria si",
sin que la traza ni el diff dejen de ser deterministas. Se compararon
cuatro mecanismos:

| Mecanismo | Coste por simulacion | Aislamiento | Determinismo |
|---|---|---|---|
| Reproducir en un namespace aislado | 156 s (o 656.6 s reconstruyendo) | Total, y carisimo | Perfecto |
| **Superposicion sobre la base** | **Lo que cuestan los hechos hipoteticos** | **Total por construccion** | Perfecto |
| Snapshot o rama nativa del almacen | Una copia del directorio | Depende del motor | Perfecto |
| Checkpoint mas superposicion | Lo que cuestan los hechos hipoteticos | Total por construccion | Perfecto |

**Se elige la superposicion**, y la razon es una cuenta: una copia cuesta
lo que cuesta el estado, y el estado a la escala de referencia son 156 s de
proyeccion y 656.6 s de reconstruccion. Eso es por simulacion, cada vez
que alguien pregunta. Una superposicion cuesta lo que cuestan los hechos
hipoteticos, que son pocos.

Y el aislamiento deja de ser una promesa y pasa a ser una **construccion**:

- El puerto del grafo **no tiene un metodo de escritura**. No se puede
  escribir en el estado real desde una simulacion porque no existe por
  donde hacerlo.
- El servicio **comprueba** el numero de relaciones antes y despues de
  simular, y si no coinciden lo dice.
- Los hechos hipoteticos llevan una **etiqueta** con el identificador de la
  simulacion, y por eso las consultas normales del grafo no los ven nunca.

### El diff se parece al diff de git

| Lo que dice | Como |
|---|---|
| que relaciones **aparecen** | la identidad de la relacion es el tipo y los dos extremos |
| que relaciones **desaparecen** | las que el fork sustituye, comparadas por identidad |
| que **decisiones cambian** | la decision de antes con la politica de antes, y la de despues con la del fork, con **las dos versiones de la politica** |

Y una decision que sigue igual **no es un cambio**: un diff lleno de ruido
no es un diff.

### Los limites, declarados y comprobados antes de simular

- cuantos hechos hipoteticos: 100 por defecto;
- cuantos sujetos afectados: 1000 por defecto;
- cuanto vive la simulacion: 24 horas por defecto.

Una simulacion que no cabe **no se ejecuta**, y una simulacion caducada
tampoco.

La decision esta escrita en
`ADR-013-simulation-forks-are-overlays-not-copies.md`, que es el mecanismo
que acepta el SPIKE-008.

## 11. El nucleo no depende de mecanismos concretos

El kernel no depende de SurrealDB, ni de Axum, ni de ningun cliente HTTP.
La persistencia real vive en `crates/hodei-iam/src/infrastructure/surreal/`.

El kernel tenia un adaptador de SurrealDB de 403 lineas que era un
**placeholder**: su `connect()` no conectaba, devolvia `Ok`, y seis tests
"de integracion" solo comprobaban su propia contabilidad. No lo usaba nadie
y arrastraba la dependencia concreta al nucleo protegido, asi que se
retiro. La cobertura real de la base de datos esta en los tres tests de
integracion de `hodei-iam`, que si hablan con SurrealDB.

## 7. Costuras de la configuracion

La validacion de la configuracion es **pura**: mira la configuracion y nada
mas. No toca el sistema de ficheros ni la red, de modo que la misma
configuracion valida siempre igual y validarla no deja directorios atras.

Si la maquina puede honrar la configuracion es otra pregunta, y la contesta
el **preflight de almacenamiento**, que se invoca aparte al arrancar y
reporta los fallos del almacenamiento como lo que son: un problema de la
maquina, no de la configuracion.

## 13. Las comprobaciones de forma

Una arquitectura que solo existe en un documento se deshace en el primer
commit que nadie revisa con cuidado. `tests/architecture_fitness.rs` lee el
repositorio y falla cuando aparece una forma que el diseño prohibe:

| Comprobacion | Que prohibe |
| --- | --- |
| `the_active_code_does_not_downcast` | Bajar a una implementacion concreta por downcast. |
| `a_protected_crate_does_not_depend_on_a_concrete_mechanism` | Que el nucleo protegido o los contratos de evidencia nombren un motor de base de datos o un framework de HTTP. |
| `the_active_code_does_not_import_another_context_internals` | Importar lo interno de otro contexto. |
| `there_is_no_success_of_filler` | Un `todo!()`, o un fichero de pruebas que se declara marcador de posicion sin pruebas dentro. |

La comprobacion del downcast trae consigo la prueba de que ella misma
falla cuando encuentra un downcast, y la de las dependencias, la de que
reconoce una dependencia prohibida y no confunde un comentario con una. Una
comprobacion que no puede fallar no es una comprobacion.

`tests/ci_suites.rs` hace lo mismo con la segmentacion de la CI: cada target
de prueba tiene que estar en exactamente una suite, ninguna suite puede
nombrar un target que no existe, y el inventario de lo descubierto se
imprime en el log del job.

## 14. Lo que este documento no dice

No dice qué viene después en detalle: eso está en el roadmap v3 y en el
backlog de implementación que lo acompaña. No dice que el repositorio esté
bien: dice qué está vivo y qué no, que es lo que un revisor necesita para no
tomarse por bueno un documento heredado.

## 10-docecies — M9: el hub de artefactos de confianza

El hito donde Hodei deja de ser un catalogo y pasa a ser el que guarda y distribuye. Y donde se decide, **antes de invertir**, si Hodei debe hablar OCI/Maven/npm o integrarse con los registros que ya existen (el SPIKE-010).

**La conclusion del SPIKE-010, y el motivo por el que M9 no se reduce ni se cancela:** un registro externo guarda y sirve bytes, y lo hace bien; Hodei no compite como almacen. Lo que Hodei si puede es **imponer**: un registro externo no sabe si un artefacto es de fiar, porque la decision de confianza viene del grafo de evidencia y no existe fuera de Hodei. El hub es, por tanto, un **punto de imposicion**, y el subconjunto de OCI es el minimo verificable.

**Lo que hay en el codigo:**

- `hodei-hub`: el almacen de contenido direccionado por su digest (en disco, con escritura atomica, y en memoria), la cuarentena y el canal de confianza, la promocion gobernada por la decision de confianza con su traza, y el subconjunto de OCI con la lista de lo **no** soportado escrita en el codigo.
- Las tres reglas del hito: sin decision de confianza no hay promocion; promover no muta los bytes; el mismo contenido no se guarda dos veces.
- La coexistencia: un artefacto de un registro externo se referencia y se cita sin migrar sus bytes, y sin que Hodei afirme tenerlos.
- `ADR-015`: la conclusion del SPIKE-010, con sus no-objetivos explicitos (Maven y npm no, hasta que haya demanda, spike de protocolo y UAT nueva).

**La limitacion, escrita:** el subconjunto de OCI es parcial (el manifiesto de imagen, su configuracion y sus capas; las firmas no se validan), la promocion cambia el canal sin mover bytes porque no hay bytes que muevan entre canales, y laReferences y el GC segura no estanTodavia. El indice multi-plataforma y los referrers llegaron despues, con HAR-08 (10-sieteundecies).

## 10-tresdecies — M10: el CAS de producción

La ampliación del hub hacia un repository manager real empieza por la base: el almacén de contenido tiene que poder recibir los bytes de un cliente sin que eso sea un problema. El CAS de M9 era correcto en su garantía central — el mismo contenido no se guarda dos veces, y lo que sale por el digest es lo que entró — pero tenía tres yeltones que lo hacían inoperable con artefactos de cliente.

**Lo que faltaba y ahora hay:**

- **El staging era un nombre por digest.** `path.with_extension("tmp")` daba a cada digest **un** temporal, y dos productores concurrentes del mismo contenido escribían en el **mismo** fichero: el renombrado del uno podía publicar los bytes del otro. Ahora cada escritura en curso tiene su **propio** fichero en `<root>/staging/<pid>-<nanos>-<contador>.part`.
- **El contenido se bufferizaba entero.** `put(&[u8])` recibía el artefacto completo en memoria, y no había forma de subir uno que no cupiera. Ahora hay `BlobWriteSession`: `append` por trozos, `commit` atómico, `abort`.
- **El commit no validaba nada.** Publicaba lo que hubiera, y si el cliente declaraba un digest que no era el suyo, se publicaba igual. Ahora el commit valida **algoritmo, longitud y digest** contra lo declarado (`CommitExpectation`); si algo no cuadra, el staging se elimina y **no queda ninguna referencia visible**.
- **Los fallos se confundían.** Cualquier error de lectura salía como `NotFound`: un disco lleno y una corrupción eran lo mismo. Ahora `NotFound`, `Corrupted`, `LengthMismatch`, `DigestMismatch`, `UnsupportedAlgorithm` y `Unavailable` son fallos distintos.
- **No había limpieza.** Una subida que se caía dejaba su staging en disco para siempre. Ahora el staging caducado se limpia (`clean_expired_staging`) y el vivo no se toca —lo que lo protege es el **bloqueo** del fichero mientras se escribe, no su fecha—; y una sesión abandonada borra el suyo al soltarse.

**La garantía que no cambia.** El mismo contenido no se guarda dos veces, y lo que sale por el digest es lo que entró. Lo que cambia es **cómo** se llega ahí: por trozos, con staging propio, y sin publicar nada que no sea exactamente lo declarado.

**Las dos fronteras, escritas:**

- `exists` dice si el contenido **está**, no si está **bien**. La integridad se comprueba con `verify` o con una lectura que verifica (`ReadPolicy::Verify` es la política por defecto; `Trust` existe para contenido ya verificado y no comprueba nada).
- El commit de un contenido que ya está **no** revalida el blob entero: comprueba su longitud, que es barato, y deja la verificación profunda al scrub (HAR-21). Rehashear un artefacto de 1 GiB en cada subida duplicada no es un coste que un registry pueda pagar.

**Lo que sigue sin estar:** las referencias y la garbage collection segura (HAR-10), el servidor OCI (HAR-06), y el resto del protocolo. Esto es solo el suelo sobre el que se construye lo demás.

## 10-cuartovieces — M10: el catálogo durable

El CAS de M10 sabe guardar bytes y nombrarlos por su digest. Faltaba
responder a la pregunta de al lado: **qué hay dónde**, y quién puede
verlo. Un registro que no responde a eso no es un registro: es una carpeta
con bytes.

**El catálogo como contexto acotado, no como un campo más del hub.** La
separación sigue el patrón que ya usan la evidencia y la confianza: el
dominio en `hodei-catalog` (que **no puede** nombrar un motor de base de
datos, porque no es su dependencia) y el adaptador en
`hodei-catalog-storage`. El compilador comprueba la frontera.

**Lo que hay en el código:**

- **La identidad del repositorio**: cuenta, clave, HRN v2 canónico, formato,
  modo (`hosted`/`proxy`/`group`) y configuración **versionada** (cada
  cambio sube la versión; el cliente no la pone). Un modo que esta release
  no sirve se rechaza con un error tipado (`ModeNotImplemented`), no con un
  repositorio creado que luego no va a funcionar. El HRN tiene que ser uno
  que la plataforma sepa **leer** (se valida contra el contrato canónico
  `hodei-hrn`): un repositorio con un nombre que no se puede parsear es un
  repositorio que el resto de la plataforma no puede nombrar.
- **La política de ámbito del digest** (`DigestScope`): `global` comparte
  el blob entre cuentas; `account_partitioned` da a cada cuenta el suyo.
  Es una decisión de la cuenta, guardada con el repositorio, y de ella sale
  la clave con la que el CAS guarda el blob. El ámbito **no cambia** en un
  repositorio que ya tiene contenido: los bytes se publicaron bajo un
  ámbito, y ese es el ámbito en el que siguen viviendo.
- **El asset** (`cuenta` + `repositorio` + `ruta`) apunta a una **versión**
  (nombre + versión, inmutable) y a su digest. La **referencia** al blob es
  una fila aparte: es lo que impide que el blob se recoja.
- **La etiqueta mutable** es una fila más. Moverla escribe esa fila y
  nada más: la versión que queda atrás conserva su digest y sus bytes. Por
  eso el digest sigue siendo la identidad del contenido.

**La transaccionalidad.** Publicar son cuatro cosas —versión, asset,
referencia y etiquetas— y van en una sola transacción (`BEGIN`..`COMMIT` en
una sola query), así que no queda un asset que no resuelve ni una versión
sin asset. Si una de las cuatro falla, no queda ninguna. Eso no es una
promesa del catálogo sino del motor, y se comprobó en el motor que se usa:
en SurrealDB 2.7 el núcleo ejecuta la transacción con control optimista y,
cuando una sentencia del lote falla, **cancela** la transacción y no
ejecuta el `COMMIT` que venía detrás
(`surrealdb-core-2.7.0/src/dbs/executor.rs`); y el `check()` de la respuesta
recorre **todos** los resultados del lote, no solo el último
(`surrealdb-2.7.0/src/api/method/query.rs`), de modo que el fallo de la
tercera sentencia no se esconde detrás del `COMMIT`. El adaptador se
apoya en las dos cosas, y por eso fija la versión 2.7 como suelo.

**El conflicto, y por qué no hay sobrescritura silenciosa.** Publicar en una
ruta ocupada, o cambiar el contenido de una versión publicada, recibe un
`Conflict`: el camino ya dice algo, y moverlo es una operación explícita
(`move_asset`, con su etiqueta `move_tag`), no un efecto colateral. Y la
republicación idéntica es idempotente, no un error.

**Escribir no es leer, y el catálogo no lo confunde.** Una escritura se
autoriza una vez, con la acción que le corresponde, y **no** se relee por
el camino público al terminar: releer por ahí exigiría un permiso de
lectura que el que escribe no tiene por qué tener, y devolvería un
`Denied` sobre un cambio que ya estaba escrito. Lo que se relee al
terminar se relee por dentro, y por eso un `Denied` nombra **la clase de
cosa** que se pidió (`repository`, `version`, `asset`, `tag`) y no siempre
"asset".

**La configuración versionada es un compare-and-set.** El cambio de
configuración dice en qué versión parte y solo se aplica si la versión que
hay en la base es esa. Sin esa condición, dos reconfiguraciones
simultáneas podrían quedarse las dos con el mismo número y ajustes
distintos, y el número dejaría de decir qué configuración es la que hay.
Cuando el cambio se queda fuera, se dice que hubo un conflicto y no que el
almacenamiento no está disponible.

**Los identificadores no son aleatorios, y esa es la decisión que sostiene
la garantía.** Dos clientes que publican lo mismo en el mismo sitio
escriben **el mismo registro**, y el motor detecta el conflicto de
escritura sobre él. Se midió: el motor **persistente** rechaza al que
pierde con un "read or write conflict", mientras que el motor **en
memoria** acepta las dos escrituras concurrentes del mismo registro y la
última gana en silencio. Por eso las pruebas de este catálogo van contra el
motor que se despliega: una garantía que solo se cumple en un motor no es
una garantía del catálogo.

**Y los identificadores son inyectivos.** Cada parte de un identificador
compuesto se escapa por separado y después se unen con un separador que
ninguna parte puede contener. Escapar la cadena ya unida no bastaba: el
separador era indistinguible de un separador dentro de una parte, y
`("pkg|1", "0")` y `("pkg", "1|0")` acababan en el mismo registro, con una
publicación pisando a la otra en silencio.

**Los tres estados de lectura que no se confunden.** `Missing` (no hay
nada ahí), `Denied` (hay algo ahí y este principal no puede verlo; lo
decide el puerto de autorización **antes** de mirar la base, para no
filtrar la existencia de lo ajeno) y `Corrupt` (hay algo ahí y no cuadra:
el asset y la versión dicen digests distintos, la referencia no está o no
dice lo mismo, o la longitud real del blob no es la que el catálogo
guarda). Confundirlos es peor que no responder: un `Missing` donde había un
`Denied` hace repetir al cliente; un `Corrupt` donde había un `Missing` hace
borrar al cliente.

**La sonda de integridad pregunta por la ubicación, no por el digest.** El
mismo digest puede estar en la ranura global o en la particionada de una
cuenta, y preguntar por el digest a secas solo encontraría la primera: un
blob particionado sano parecería roto. La pregunta lleva la cuenta y el
ámbito, y de ahí sale la clave con la que el CAS guarda los bytes. Lo que
la sonda comprueba es la **longitud**, no el digest: rehashear un
artefacto en cada lectura es un coste que un registro no puede pagar, y la
verificación profunda es del scrub (HAR-21).

**El esquema no se re-declara encima de uno más nuevo.** Declarar el
esquema es idempotente, pero **declarar el de esta build encima del de una
build más nueva** no devuelve el almacén a la versión nueva: las
definiciones que ya están no se rehacen, así que el almacén se queda con
un esquema mixto. Por eso `initialize` mira primero qué versión dice el
almacén, y si es más nueva que la que entiende esta build se niega a
tocarlo (`SchemaMismatch`) en vez de escribir su número encima y dejar que
el número mienta. El número de versión solo describe el esquema cuando
esta build lo escribió.

**Lo que el resultado dice es lo que hay.** Republicar lo mismo no escribe
nada, y entonces no cambia la fecha del asset ni de la referencia: el
resultado devuelve el instante que hay en la base, no "ahora". Un
resultado que dijera "actualizado ahora" sobre un registro que no se tocó
haría que el que se fiara del resultado creyera que el contenido se acaba
de publicar.

**Lo que sigue sin estar:** el servidor HTTP (HAR-04), las referencias
persistentes y la garbage collection segura (HAR-10), y el resto de
formatos. El catálogo guarda y dice; todavía no sirve bytes por el cable.
Para HAR-10 queda dicho: un blob publicado lo protege la fila de su
**versión** (que es inmutable y conserva digest y longitud) y, mientras la
ruta lo sirva, la **referencia**; la recolección tiene que tener en cuenta
las dos. El commit de publicación y su outbox atómico son la sección
siguiente.

## 10-cinquies — M10: el commit de publicación y su outbox de auditoría

Un catálogo que publica sin dejar rastro es un catálogo que nadie puede
auditar. Y un rastro que se escribe **después** del commit es un rastro
que puede no existir: el proceso se cae entre las dos cosas y el asset
quedó publicado sin que nadie supiera quién lo publicó.

**El contrato de auditoría es un crate, no un tipo repetido.** El evento
vive en `hodei-audit` (`AuditEvent`, `AuditSink`, `AuditOutbox`), que lo
usan el catálogo y la aplicación HTTP sin duplicarlo: un evento con dos
definiciones es un evento que puede tener dos formas. El crate no nombra
ningún motor de base de datos, así que el dominio y la aplicación siguen
pudiendo depender de él sin cruzar la frontera que comprueba
`tests/architecture_fitness.rs`.

**El evento dice lo que pasó y no dice lo que no pasó.** Lleva actor,
cuenta **verificada**, acción, recurso, digest (cuando la operación trata
de contenido), resultado (`allowed`, `denied`, `conflict`, `failed`),
correlación e instante. La cuenta es la del principal verificado, no la
que declara el llamante: un evento que dice la cuenta que el llamante
pide no es un evento, es un eco. Y el evento no lleva el bearer ni las
credenciales: el token se consume al verificar el principal y no llega ni
al evento ni al sink. Del **actor** hay que hablar con precisión: en la
capa HTTP sale del principal verificado, y en el catálogo lo declara
quien publica, porque el catálogo verifica la **cuenta** contra el
autorizador y no el *subject*. Por eso la palabra "verificado" no se le
pone al actor, y quien publica tiene que poner el actor que de verdad
actuó. El texto que el que llama controla —actor, cuenta, correlación,
acción, recurso y digest— pasa por `sanitize_text`, que quita los
caracteres de control y lo acota: el que llama no decide cuánto ocupa el
evento.

**El outbox vive en la misma base que el catálogo, y a propósito.** La fila
del evento se escribe **dentro de la misma transacción** que escribe el
asset: una transacción que toca dos bases no es una transacción. Así que el
asset y su evento tienen **una sola frontera de commit**. Si la fila del
evento no se puede escribir, la transacción falla y **el asset no se
publica**: no queda un hecho sin su rastro. El blob al que apunta puede
quedarse *staged*, y eso es reconciliable; un asset publicado sin evento
no lo es.

**La entrega es un problema aparte, y se trata como tal.** El
`AuditDispatcher` reclama un evento pendiente (un compare-and-set: dos
workers, uno gana), lo entrega al sink y **solo entonces** lo marca como
entregado. **Solo el worker que lo tiene puede marcarlo**: un lease que
expiró ya no es un permiso para decir nada de ese evento. Sin esa
condición, un worker lento podía devolver el evento al mercado mientras el
otro lo estaba entregando, y un solo evento producía dos efectos lógicos.
Si el sink falla, el evento vuelve a `pending` con su contador de intentos
y su último error: lo que no se entregó se puede **mirar**. Nunca se marca
entregado lo que no se entregó; y si la marca falla después de una entrega
que sí ocurrió, el resultado es `uncertain`, no `delivered`: no se afirma un
hecho que no se sabe.

**La reentrega es el mismo evento.** El `event_id` es la clave de
idempotencia, y el contrato del sink exige tratarlo como tal. Una caída
entre la entrega y la marca vuelve a entregar el mismo evento, no crea
otro. Pero la verdad es más corta que la promesa fácil: el outbox
entrega **al menos una vez**, y el **efecto lógico de un evento es del
almacén de auditoría**, que es el que guarda por ese `event_id`. El
outbox pone la condición para que pueda haber uno solo —la misma clave
y la misma fila—; el que lo asegura es el que guarda. El
`InMemoryAuditSink` deduplica por `event_id` precisamente para que eso
sea comprobable, y el sink por defecto (`tracing`) no deduplica, porque
un log es append-only.

**Lo que no se dice, no se escribe.** Una publicación idempotente —el
mismo contenido en el mismo sitio— no escribe un segundo evento: no hubo
un segundo hecho. Un rechazo (`denied`) y un conflicto (`conflict`) sí
dejan evento, pero **fuera** de la transacción del catálogo, porque no hay
mutación que atar: el rechazo no escribió nada que pueda dejar a medias.

**El límite de la auditoría también está escrito.** Una petición **mal
formada** —un repositorio vacío, una ruta, un nombre o una versión
vacíos, un digest mal formado— se rechaza antes de la frontera de efecto
y **no deja evento**: no es un hecho, es ruido. Una petición **bien
formada a la que el sistema se niega a servir sí deja evento** (`failed`):
un repositorio que no existe, un error de lectura, un modo no servido o
una sonda del blob que falla son hechos que ocurrieron. Y si el mismo
`event_id` llega con un contenido distinto, el outbox no conserva la fila
vieja en silencio: lo dice, porque una contradicción tragada es una
mentira que no se puede corregir después.

**El límite que se dice en voz alta.** El outbox es durable; el sink por
defecto (`tracing`) no: la fila sobrevive al reinicio, el log no. La
entrega durable necesita un `AuditSink` que escriba en el almacén de
auditoría (Audit Trail), que todavía no existe. Y las mutaciones de la
aplicación HTTP —alta de repositorios, mutaciones de políticas IAM— siguen
siendo best-effort: su evento se emite en el proceso y un sink caído lo
pierde sin cambiar la respuesta. La frontera única de commit es, de
momento, la de la publicación del catálogo. Y el outbox entrega **al menos
una vez**: el que garantiza el efecto lógico único es el almacén de
auditoría, que es el que guarda por `event_id`.

**Lo que sigue sin estar:** las referencias persistentes y la garbage
collection segura (HAR-10), y el almacén de auditoría que convertiría el
outbox en entrega durable.

## 10-seisdecies — M10: la superficie de contenido crudo

El catálogo sabe **qué hay dónde**. Faltaba lo de al lado: **servirlo**.
Un usuario que quiere usar Hodei como servidor binario de verdad no
quiere un catálogo, quiere publicar un fichero y volver a bajarlo.

**La superficie es pequeña a propósito.** `PUT` publica, `GET` descarga y
`HEAD` describe, y las tres hablarían con un repositorio `raw` en modo
`hosted`. Un repositorio crudo no tiene paquetes, ni versiones, ni
formatos: tiene bytes en una ruta, y por eso no comparte semántica con
OCI, Maven ni npm. La superficie **no está bajo `/api/v1`**: la API
administrativa y el endpoint que sirve paquetes son cosas distintas, y un
cliente que habla con una no tiene por qué creer que habla con la otra.

**El orden de las cosas es el mismo en las tres peticiones**, y por eso
está escrito en un sitio: autenticar, leer la ruta, buscar el
repositorio en la cuenta verificada y **después** pedir la decisión de
Cedar. La ruta se lee del URI **crudo** (`OriginalUri`), no del
decodificado: un `%2e%2e` que ya ha pasado a ser `..` no se puede ni ver
ni refutar. Y el recurso de la decisión es el **repositorio**, porque una
ruta concreta no es un recurso con identidad propia en el catálogo.

**Las reglas de la ruta son estrictas, y se comprueban antes de tocar el
almacén.** Cada segmento se decodifica una vez y tiene que ser un
`pchar` de RFC 3986: ni barras, ni barras invertidas, ni el espacio, ni
los delimitadores que le cambian el sentido a la referencia, y sin ser
`.` ni `..`. Como el símbolo `%` no es un `pchar`, un doble codificado es
una forma que todavía lleva codificación dentro, y por eso no es un
nombre: se rechaza. Eso no es un filtro estricto por gusto: es lo que
hace que la ruta sea **la misma** para quien la escribe y para quien la
guarda. Un traversal que se acepta en silencio es un fichero que un día
está en un sitio y otro en otro.

**La sustitución la decide el repositorio, no el cliente.** Publicar en
una dirección que ya tiene contenido lo sustituye, y dos clientes que
publican en la misma dirección se pisan sin que ninguno se entere. Por eso
la política vive en la configuración **versionada** del repositorio
(`raw.replace`), y por defecto la dirección publica **una vez**:
republicar el mismo contenido es idempotente, y publicar otro es un
`409` cuya escritura se aborta —no queda ni un byte en el almacén. El
catálogo, que ya tenía el `Conflict` para el caso general, gana aquí la
opción explícita de reponer (`Publication.overwrite`), que solo se usa
cuando la política lo permite.

**El `416` lleva la longitud del contenido en la cabecera**, no en el
cuerpo: RFC 9110 lo dice así, y sin `Content-Range` el cliente no sabe
ni cuánto tiene.

**La frontera de autorización, y su límite.** La decisión de Cedar se
toma en la frontera HTTP, con la acción que corresponde
(`UploadAsset`, `DownloadAsset`) y el HRN canónico del repositorio. El
catálogo **no vuelve a decidir**: su puerto de autorización no lleva la
región, y el HRN sí la lleva, así que una segunda decisión caería sobre un
recurso que puede no ser el mismo que evaluó la primera —y dos decisiones
sobre recursos distintos no son una decisión más fuerte. Lo que el
catálogo **sí** garantiza es el aislamiento por cuenta, y no por cortesía:
lo lleva la clave de cada fila, no el decisor.

**La prueba del proceso real.** El contrato en proceso (`oneshot`) prueba
el enrutado, las reglas y los códigos; no prueba que los bytes estén en
disco. Para eso está `tests/raw_hosted_restart_contract.rs`: el binario
de verdad en un puerto de verdad, un `curl` de verdad publicando y
descargando, un `sha256sum` de verdad comparando los dos ficheros, el
proceso parado y vuelto a arrancar sobre el mismo almacén, y el contenido
sigue ahí y sigue siendo **los mismos bytes**.

**La conformidad, de caja negra.** `tests/raw_conformity_uat.rs` no usa
mocks: es el binario de verdad en un puerto de verdad, con `curl` de
verdad y `sha256sum` de verdad. Lo que mide es lo que un cliente ve —que
lo publicado vuelve intacto, que un proceso muerto a mitad de una subida
no deja una ruta publicada a medias y que al reiniciar el servicio
recoge lo caducado y respeta lo vivo, que las subidas simultáneas no se
mezclan y que dos cuentas con tokens reales no se ven entre sí— y lo
deja **escrito** en un informe con la versión, el comando real de cada
caso, el resultado medido y los límites. El informe dice también lo que
**no** certifica, porque un informe que solo dice lo que sí es
propaganda, y un caso que falla sale en él como `FALLO`, no como un
caso que no aparece.

**La recuperación de lo que un proceso muerto deja.** Una subida
interrumpida deja su fichero de staging en el almacén de contenido: no se
publica nunca —la transacción del catálogo no llegó a ocurrir—, pero era
un fichero que se acumulaba sin que nadie lo quitara. El arranque limpia
el staging caducado y dice cuántos ha quitado; lo vivo se queda, y lo que
lo protege no es su fecha sino el **bloqueo**: mientras la escritura
vive, su fichero está bloqueado por el proceso que escribe, y un fichero
bloqueado no se borra aunque la suba lleve horas sin escribir. Un staging
que no se puede borrar no impide arrancar.

**Lo que sigue sin estar:** el alta del repositorio en el catálogo por
HTTP (esta superficie sirve los repositorios que ya están), el ámbito de
digest particionado por cuenta (un `501` que lo dice claro), los modos
`proxy` y `group`, y la recolección de blobs sin referencia.


## 10-sieteundecies — M10: el indice, las etiquetas y los referrers

El pull y el push ya hablaban el protocolo de OCI Distribution 1.1, pero
faltaban las dos **preguntas** que el protocolo también tiene: qué
etiquetas tiene el repositorio, y qué cuelga de este manifiesto. Un
registro que sólo sube y baja no es un registro incompleto: es un
almacén con una puerta, y un cliente de artefactos --el `oras`, el
`cosign`-- no encuentra en el nada donde está la firma de una imagen.

**El índice es un manifiesto de manifiestos.** Sus hijos son
manifiestos, no blobs, y por eso la validación de lo que el manifiesto
nombra depende de su clase y no de su forma: el manifiesto de imagen
exige su configuración y sus capas, el índice exige sus hijos, y el
manifiesto de artefacto no exige nada. Lo que el manifiesto nombra tiene
que estar en el repositorio; lo que el manifiesto **cuelga** no, porque
el protocolo dice que un manifiesto con un `subject` que todavía no está
se acepta, y un cliente puede subir el referrer antes que el sujeto.

**El `subject` se escribe con la publicación, en la misma transacción.**
Un referrer visible a medias sería un referrer que existe cuando no
existe, y la lista de referrers dejaría de ser la verdad. Por eso el
`subject` viaja en la `Publication` y no en una segunda operación: la
misma frontera de commit que escribe el manifiesto escribe de qué cuelga.

**Las dos respuestas son las que el protocolo dice, y las dos tienen una
regla que parece un detalle: no dicen 404.** Un `subject` que no tiene
referrers es una lista **vacía**, y un repositorio sin etiquetas es una
lista vacía. Un 404 haría que un cliente que busca la firma de un
manifiesto tuviera que distinguir entre "no hay firma" y "no hay
manifiesto", y el protocolo dice que esa distinción no existe. Lo que sí
se dice con 404 es lo que no existe: el repositorio. La lista de
etiquetas va en orden lexicográfico --alfanumérico sin distinguir
mayúsculas, que es como lo define el protocolo-- y pagina con `n` y
`last`, con el `Link` que dice si hay más. La de referrers es un índice
de imágenes, con el `artifactType` y las anotaciones de cada manifiesto,
y el filtro por tipo de artefacto dice en la cabecera que se ha
aplicado.

**Un referrer es una relación entre dos manifiestos, y nada más.** El
registro **no** valida firmas: un `oras attach` de una firma cuelga un
manifiesto que dice que cuelga de otro, y el registro lo registra como
eso. Y un referrer **no** promueve nada: lo único que escribe el canal
de confianza es la admisión, y escribe cuarentena; promover es una
decisión del motor de confianza (HAR-11), y la presencia de un fichero no
es una decisión. Por eso el informe de esta conformidad dice, en su
cabeza, que no certifica la validación criptográfica.

**El descriptor vacío no se exige en el almacén.** El descriptor que
dice "esta imagen no tiene configuración" trae sus bytes en línea, y un
cliente puede no subirlos nunca. Un registro que los exigiera
rechazaría un manifiesto que el protocolo dice que es válido, y el
cliente real que lo hace --el `oras`-- dejaría de poder adjuntar nada.

**La prueba del proceso real, y de un cliente real.**
`tests/oci_index_contract.rs` no usa mocks: es el binario de verdad en
un puerto de verdad, con `curl` de verdad, y con el `oras` de verdad
cuelga un SBOM de un manifiesto y lo vuelve a encontrar. Lo que mide es
lo que un cliente ve --que el índice se sirve byte a byte con sus dos
plataformas, que las etiquetas van en orden y paginan, que los referrers
se listan con su `artifactType` y que su filtro dice que se ha
aplicado-- y lo deja escrito en `target/uat/oci-index-conformity.md`,
con su lista de lo que **no** certifica.

**Lo que la revisión independiente encontró, y lo que se corrigió.**
La pieza se revisó con tres revisores que no conocían la solución: uno
contra el protocolo, uno contra la arquitectura y uno contra los modos
de fallo. Los tres marcaron lo mismo en el punto más grave --el digest
del `subject`-- y los tres tenían razón: el push aceptaba un digest a
medio escribir, lo guardaba en el catálogo, y el manifiesto quedaba
publicado con un 201 y **ilegible** para siempre, porque el catálogo lo
relee con la regla de un digest. Ahora el digest que nombra un
manifiesto --config, capa, hijo, `subject`-- tiene que ser un digest de
verdad, y un `subject` que no lo es es un 400.

Los otros tres que se corrigieron: el descriptor vacío es **uno** (el
que dice el tipo de medio y el digest a la vez; con sólo una de las dos
cosas, un manifiesto podía nombrar un contenido que no existía y el
registro no lo exigía); el `n` que no es un número es un **400** con el
código `PAGINATION_NUMBER_INVALID` que usa el registro de referencia, y
no un 405 que diría que el registro no sabe atender la petición que sí
sabe; y el `move_tag` **espera** entre intentos, porque sin espera los
cuatro intentos caían en la misma ventana de conflicto y los cuatro
fallaban.

**Las dos superficies comparten el espacio de rutas del catálogo.** Un
repositorio puede servirse por la superficie cruda y por la de OCI, y
las dos escriben en la misma tabla: la de OCI guarda sus manifiestos en
`manifests/<digest>` y sus blobs en `blobs/<digest>`, y la cruda guarda
lo que el cliente publica en la ruta que pide. Un contenido publicado por
la cruda en `manifests/<digest>` **sustituye** el asset del manifiesto de
OCI que estaba ahí: el manifiesto deja de servirse --con un 404 honesto,
porque el pull comprueba que la fila referencia el digest pedido y no
sirve nunca otros bytes-- y su relación de referrers desaparece con él,
que es lo que corresponde: el manifiesto ya no está en el repositorio.
Namespacear las superficies, que cada una tenga su espacio, es trabajo de
otra pieza.

**Los límites, escritos.** La paginación de las etiquetas se hace en la
capa de protocolo: el catálogo devuelve la lista completa y el corte lo
hace quien habla el dialecto, que es quien sabe lo que el cliente ha
visto ya. El coste es que cada petición de la lista trae todas las
etiquetas del repositorio, y un repositorio con muchas etiquetas paga
esa lectura en cada página. Los referrers **no** se paginan: la lista
se devuelve entera, y un repositorio con muchísimos referrers tendría
un índice grande. Y un referrer cuyo manifiesto ya no está en el
almacén **tumba la lista entera** con un 503: el catálogo y el almacén
dicen cosas distintas y eso no se disimula con una lista a medias.
Los descriptores de los hijos de un índice **no** se comprueban contra
el manifiesto real en su `mediaType` y su tamaño: se comprueba que el
hijo **esté**, y el hijo se sirve con su propio tipo. Y un movimiento de
etiqueta **no** deja evento de auditoría: el rastro dice que se
publicó, no a qué etiqueta se colgó.

## 10-ochoidecies — M10: la compatibilidad OCI, con clientes de verdad

Las tres piezas anteriores hicieron que el registro **hablara** el protocolo
de OCI Distribution 1.1: el pull, el push y el índice. Eso no es lo mismo
que ser compatible. La compatibilidad es una afirmación **medible**: dice
con qué clientes, con qué versión y sobre qué categorías. Y medible
significa también **acotada**: un contrato que solo dice «somos
compatibles» no dice nada, porque no se puede comprobar ni desmentir. Por
eso el contrato vive en `uat/UAT-OCI.md`, y por eso nombra una por una
las 38 categorías de la suite oficial que se han ejecutado --las de API y
las de datos-- con el estado que cada una tuvo **en la ejecución**.

**La lista de categorías no está escrita a mano: la CI la comprueba.** La
suite oficial de la distribution-spec se descarga, se verifica por el
digest de sus fuentes, se compila y se ejecuta contra el registro en cada
carrera de la pipeline; la prueba
`the_official_conformance_suite_runs_the_declared_categories` falla si
alguna de las categorías declaradas deja de estar en verde. Una
declaración que se puede comprobar es una declaración; una declaración
en un documento es un deseo. Y el otro lado de la misma regla: una
categoría que no se ha ejecutado **no** se declara. El conjunto de datos
en `sha512` no se ha ejecutado porque el registro no habla ese
algoritmo, y los parámetros `?tag=` tampoco, y los manifiestos dispersos
tampoco; declararlos en verde sería propaganda, así que el contrato los
nombra con su estado real --`Disabled`--, con el motivo y con la acción
concreta que los dejaría en verde.

**Y el informe de la ejecución también se comprueba.** El binario de
pruebas lleva el informe en memoria y lo vuelca en cada caso, así que el
informe de disco es el de la **última** ejecución: una ejecución parcial
--con un filtro de nombre-- deja un informe con un solo caso. Por eso
`scripts/check-oci-compatibility.sh` exige los nueve casos y que ninguno
esté en rojo, y la etapa de la pipeline lo ejecuta después de archivar el
informe: un informe que existe con menos casos es el informe de una suite
que no es la que la etapa cree, y eso es peor que un informe que falta.

**El detalle que hizo el trabajo posible: el demonio de Docker no es el
registro.** La CLI de Docker no habla con un registro: habla con un
demonio, y el demonio es quien habla con el registro. Por eso el caso de
la CLI levanta `podman system service` --un demonio que expone la API de
Docker por un socket unix-- y por eso la imagen que la CLI publica se
carga antes con `podman save` y `docker load`: lo que la CLI empuja es lo
que el **demonio** tiene, no lo que el cliente tiene. El registro de la
prueba habla HTTP sin TLS, como un registro de desarrollo, y el demonio
solo relaja la verificación del TLS cuando el registro se llama
`localhost`: por eso la dirección de la prueba es `localhost:<puerto>` y
no una dirección IP, y por eso la configuración va en el fichero
`registries.conf` del usuario --la del sistema no manda--. Son tres
detalles que no son del registro, y que sin ellos la prueba mide el
entorno y no el protocolo.

**El digest que se compara es el del registro, no el del cliente.** Podman
puede añadir la anotación `org.opencontainers.image.created` al empujar, así
que el digest de la imagen local **puede no ser** el digest publicado. Los
casos comparan tres cosas: el digest que dice el cliente (`--digestfile`),
el digest que vuelve al tirar de la imagen, y el `Docker-Content-Digest` que
devuelve el propio registro. Cuando los tres coinciden, la ida y la vuelta
es real y no una casualidad del formato.

**Y hay un digest más, que es el del demonio, y que no es el del registro.**
La imagen entra en el demonio de Docker por un tar--con el manifiesto de su
formato local-- y sale por otro--con el manifiesto que el registro publicó,
que es el de Docker y no el de OCI--. Los dos digests son reales y el
demonio los anota en la misma lista, **cuyo orden no es un contrato**: una
prueba que compara el primero de la lista falla unas veces y otras no. Lo
que sí es un contrato es el digest que el registro publica antes y después
de la ida y vuelta, y que el cliente se anote **ese** entre los que tiene.
La prueba de la imagen de una plataforma hace eso, y la del índice de dos
compara el digest publicado antes y después de la vuelta.

**Y la suite toma un cerrojo para las construcciones de imagen.** Dos
`podman build` a la vez en la misma máquina se pelean por el
`fuse-overlayfs`: el desmontaje se queda esperando a que termine la
escritura, el proceso no termina, y la máquina se queda con gigas de
páginas sucias y con procesos en estado irrecuperable. El cerrojo--un
`flock` sobre un fichero del `target`-- serializa las construcciones, y la
suite se puede correr en paralelo sin colgar la máquina.

**Lo que se comprueba con clientes de verdad, y con un servidor de
verdad.** El `oras` empuja un artefacto con su tipo de medio, adjunta un
SBOM con su `subject` y lo vuelve a encontrar por la API de referrers; la
CLI de Docker publica y recupera una imagen de una plataforma y publica y
recupera un índice de dos plataformas; el `podman` publica, etiqueta y
vuelve a tirar; una cuenta de otra cuenta **no** puede publicar en el
repositorio de la primera; un reintento del cliente deja **el mismo**
digest; y el contenido publicado sobrevive a que el servicio se pare y
vuelva a levantar en el mismo puerto. La suite oficial cubre lo demás: la
subida por trozos, la subida en un solo `POST`, la subida en streaming, el
`HEAD` y el `GET` de blobs, el manifiesto por digest y por etiqueta, el
manifiesto con `subject`, los referrers, y los datos --imagen, índice,
índice anidado, artefacto, artefacto sin capas, capa no distribuible,
digest inválido.

**Lo que este trabajo encontró, y lo que se corrigió.** Cuatro
desviaciones, y las cuatro tenían un síntoma engañoso --que es lo que hace
una prueba con clientes de verdad más valiosa que una prueba con el mismo
cliente de siempre. La primera: el esquema Cedar no declaraba el tipo de
ancestro de `Artifacts::Repository`, así que un repositorio con niveles
(`team/demo`) se respondía con **403 DENIED**; el motor de autorización
fallaba cerrado porque el padre que el mapeador declara no estaba
permitido por el esquema: el 403 no era una decisión, era un fallo del
esquema. La segunda: la negociación del `Accept` leía **solo la
primera** cabecera, y un cliente que manda varias --la CLI de Docker
manda la del índice y la de la imagen-- se quedaba sin manifiesto con
`MANIFEST_UNKNOWN`; la prueba leía lo que el primer cliente mandaba, y
el protocolo manda leer lo que el cliente mandaba. La tercera: el `PUT`
que cierra una subida no comprobaba el rango, así que un trozo fuera de
orden respondía `400` en lugar de `416`, y el cliente no sabía que su
desfase era el problema --el `416` es lo que lleva el `Range` que dice por
dónde seguir. Y la cuarta: las capas **extranjeras** --las que declaran
sus `urls`-- se exigían en el almacén, con lo que un manifiesto válido se
rechazaba con `MANIFEST_BLOB_UNKNOWN`; el cliente baja esa capa de donde
el manifiesto dice, no del registro.

**La revisión independiente pasó dos rondas, y la segunda encontró cuatro
cosas más.** La primera --un revisor contra el protocolo, otro contra la
fiabilidad y otro contra el riesgo-- dejó cuatro desviaciones, todas
corregidas. La segunda, sobre el diff ya corregido, dejó otras cuatro, y
todas eran del mismo tipo: **acuerdos entre lo que el cliente dice y lo
que el registro comprueba**. El `Accept` no leía el **peso**: un
`Accept: */*;q=0`--un «no acepto nada»--servía cualquier manifiesto, y
era el único caso en que el registro servía un tipo que el cliente había
rechazado; ahora `q=0` es un rechazo (RFC 9110, sección 12.5.1). El
rango del `PUT` de cierre se comprobaba **solo por su inicio**: un trozo
que declaraba `Content-Range: 0-99` y mandaba tres bytes se aceptaba, y
ahora el rango declarado y la longitud del cuerpo tienen que cuadrar. La
exención de la capa extranjera era **más ancha de lo que la
especificación necesita**: eximía cualquier descriptor con `urls`, y la
capa no distribuible es la que su **tipo de medio** declara, que es una
cosa más estrecha--un descriptor con `urls` y un tipo de medio cualquiera
es una capa normal con una pista, y eximirla era dejar de exigir un
contenido que el manifiesto nombra. Y la exención no se aplicaba a los
**blobs de un artefacto**, con lo que el mismo concepto se trataba de dos
maneras según la forma del manifiesto.

**Y el esquema Cedar introduce una regla, y la regla está escrita.** El
cambio del tipo de ancestro no es un detalle: convierte «deny» en
«hereda» para todo el subárbol de un repositorio, y eso es una decisión
de autorización que tiene que estar en un sitio donde alguien la pueda
leer. La regla está en el manual de Cedar y en el contrato de
compatibilidad: el ancestro es la **cadena de prefijos del nombre**,
dentro de la misma cuenta, y **el arreglo es del operador**--el esquema
que el servicio carga lo elige quien lo despliega, así que una
instalación con el esquema anterior sigue con el `403`--. Es un límite
que se escribe, no un defecto que se esconde.

**Los límites, escritos.** El registro **no borra contenido publicado**:
el `DELETE` de una etiqueta, de un manifiesto o de un blob responde `405`,
y la suite lo marca `Skip`, que es su forma de decir «este registro no
implementa esto». El `DELETE` de la **sesión de subida** sí existe, y
responde `204`. El `GET` de un blob **no** negocia rangos: devuelve el
blob entero con `200` en lugar de `206`; la especificación lo
**recomienda** (`SHOULD`) y no lo exige, así que es una desviación
recomendada y no un incumplimiento, y es lo que echan de menos los
clientes que descargan en paralelo. El montaje anónimo de un
blob **no** está implementado: el registro cae a una subida normal, que es
el repliegue que el protocolo contempla. Y el modelo de digest **no**
lleva algoritmo: el contenido es `sha256` y solo `sha256`, y por eso las
tres categorías de `sha512` no se han ejecutado. Extender el modelo de
digest es tarea propia y transversal --lo tocarían el hub, el catálogo y
la evidencia--, no un remiendo de esta pieza. Por encima de todo esto, el
contrato **no** certifica la validación criptográfica de las firmas: el
registro transporta referrers y atestaciones y sabe contarlos, y la
presencia de una firma no concede nada, porque la decisión la toma el
motor de confianza. Y **no** certifica la promoción: publicar en el
registro no es promover un artefacto a un canal de distribución.

**Y hay dos límites que la prueba enseñó y que no se han arreglado.** La
capa **no distribuible** la baja el cliente de donde el manifiesto dice, y
el registro **no comprueba** que esa `url` exista, que sirva ese contenido
ni que los bytes tengan el digest declarado: el registro apunta, y quien
confía en el manifiesto verifica. Lo que sí comprueba es que la `url` sea
`https` y que el tipo de medio sea exactamente uno de los dos de no
distribución --un descriptor con `urls` y cualquier otro tipo de medio es
una capa normal, y esa sí se exige en el almacén. Y las **sesiones de
subida** viven en un registro **global**: 64 a la vez, una hora cada una,
configurables por despliegue. El tope no es por cuenta, así que una
cuenta que abre sesiones y las abandona ocupa el hueco de las demás
durante una hora, y cuando el hueco se acaba la sesión nueva se rechaza
con un `400`--un error del cliente-- en lugar de un `429`. Las dos cosas
están escritas en el contrato de compatibilidad con su acción concreta,
porque un límite que no está escrito es un límite que nadie ha decidido.

## 10-novendecies — M10: la recolección segura del almacén de contenido (HAR-10)

Hasta aquí el almacén de contenido solo crecía: nada borraba bytes, y las
secciones anteriores dejaron escrito que «la recolección de blobs sin
referencia» no existía. Ahora existe, y es la **única** superficie que
borra bytes. Su contrato, sus pruebas y sus límites están en
[`uat/UAT-OPERATIONS.md`](uat/UAT-OPERATIONS.md); aquí se dice qué hay y
dónde vive.

**Qué hay.** `hodei_hub::BlobStore` sabe listar sus blobs con su fecha y
borrar uno solo si sigue sin tocarse pasada la gracia, bajo el mismo
cerrojo que la subida; una subida que deduplica contra un blob existente
lo toca. `hodei_catalog` define el libro de la recolección (corridas,
tombstones y retenciones) y `hodei-catalog-storage` lo guarda en el motor
(esquema 5). El servicio vive en `src/gc/`: un **plan puro** (`plan.rs`)
que, dadas las raíces, las retenciones y los blobs, da un veredicto y sus
razones por blob, y un **servicio** (`service.rs`) que hace la pasada de
marca y barrido, reanudable e idempotente. La superficie HTTP son cinco
rutas administrativas bajo `/api/v1/gc/` (plan, barrido, y alta, listado y
baja de retenciones), cada una con su propio recurso para la política.

**Cómo decide.** Las raíces se leen en cada pasada de las fuentes reales
--el catálogo y los manifiestos del propio almacén--, sin contadores
propios. El borrado exige que el blob no tenga raíces, que ninguna
retención lo cubra, que haya pasado la gracia
(`content.gc_grace_secs`, un día por defecto) y que dos censos seguidos
coincidan; cualquier duda es un estado desconocido y no se borra. Por
defecto la superficie solo simula.

**Lo que sigue sin estar.** El registro no borra assets ni manifiestos, así
que hoy lo reclamable son los huérfanos que deja un fallo de publicación;
las retenciones de repositorio y de cuenta cobrarán sentido cuando haya
retiros de referencias. El ámbito de digest particionado por cuenta no se
recorre. La lista completa de límites, con su acción, está en el contrato.

## 10-vicies — M10: las políticas de confianza en la distribución (HAR-11)

La ruta nativa ya no esquiva la confianza. El contrato, las pruebas y los
límites están en [`uat/UAT-SECURITY.md`](uat/UAT-SECURITY.md); aquí se dice
qué hay y dónde vive.

**Qué hay.** `hodei_catalog::trust` define el canal de servicio de un
repositorio (`config_settings.trust.serve`: `open` o `trusted`) y el libro
de concesiones por digest (`TrustLedger`), que `hodei-catalog-storage`
guarda en el motor (esquema 6). `src/trust/` conecta el plano de promoción
de `hodei-trust-policy`: la admisión única que usan raw, blobs, manifiestos,
etiquetas y referrers, y el cableado del arranque (la política de confianza
sale de `[trust] policy_file`). `src/handlers/promotions.rs` expone
`POST /api/v1/promotions` y `POST /api/v1/promotions/revoke`.

**Cómo decide.** Cedar decide **quién** (`PromoteAsset`, `RevokePromotion`);
la política de confianza decide **qué software**. Promover siempre escribe
una decisión; solo una promoción que avanza concede, y concede el digest y
lo que abarca (la configuración y las capas de un manifiesto, los hijos de
un índice). No hay caché: el canal trusted lee el libro en cada petición,
así que una revocación vale en la siguiente, y un error del libro es un
`503`, nunca un permiso.

**Lo que sigue sin estar.** Los hechos de riesgo de la promoción son los de
cero (`RiskFacts::default()`): no se alimentan CVE ni VEX reales, y una
concesión no caduca sola ante una CVE nueva; se retira con la revocación.
No hay una ruta HTTP que cree repositorios del catálogo, así que el canal
trusted se fija al sembrar o con `update_config`. Un despliegue con
catálogo duradero tiene que declarar las dos acciones nuevas en su esquema
Cedar.

## 10-unvicies — M10: el repository proxy remoto (HAR-12)

Un repositorio en modo `proxy` resuelve contra su upstream la primera vez que
se le pide una ruta. El contrato, las pruebas y los límites están en
[`uat/UAT-PROXY-VIRTUAL.md`](uat/UAT-PROXY-VIRTUAL.md); aquí se dice qué hay y
dónde vive.

**Qué hay.** `hodei_catalog::proxy` define la configuración versionada del
upstream (`config_settings.proxy.upstream`), la referencia de credencial
(`env:NOMBRE`) y el recibo de procedencia con su libro (`ProvenanceLedger`),
que `hodei-catalog-storage` guarda en el motor (esquema 7). `src/proxy/` es el
plano de salida: `egress` (la política de hosts y la clasificación de
direcciones), `fetch` (un cliente por salto con la IP fijada, redirects
manuales y presupuesto de bytes) y `resolution` (sesión de escritura no
visible, verificación, `commit` y recibo). `src/handlers/raw.rs` lo conecta a
`GET`/`HEAD` y a `GET /api/v1/provenance`; `PUT` a un proxy responde `405`.

**Cómo decide.** El repositorio dice a qué upstream habla; `[proxy]
allowed_hosts` dice a qué hosts puede llegar el servicio, y por defecto es
ninguno. El loopback y las redes privadas solo se abren con `private` en la
entrada. Lo que se baja pasa por la misma admisión de confianza que lo que se
sube.

**La caché (HSP-107, HAR-13).** `hodei_catalog::proxy_cache` es el núcleo puro
que decide qué hacer con una lectura (servir, revalidar, traer, cuarentena,
offline) a partir de la política `config_settings.proxy.cache`, el libro de
recibos y la hora; la frescura sale del libro (esquema 8: `outcome` y
`last_modified` en el recibo), no de un estado nuevo. `src/proxy/cache.rs`
añade el single-flight por ruta y versión de configuración, la caché negativa (tope global de 4096) y los contadores por petición;
`fetch` hace peticiones condicionales y `resolution` distingue «confirmado»,
«bytes nuevos» y «otros bytes para contenido inmutable». Las reglas están en
[`uat/UAT-PROXY-VIRTUAL.md`](uat/UAT-PROXY-VIRTUAL.md).

**Lo que sigue sin estar.** No hay coordinación entre réplicas (single-flight y caché negativa son
por proceso), ni endpoint de métricas. No existe una ruta HTTP que cree
repositorios proxy: se siembran o se crean con el catálogo.

## 10-duovicies — M10: los repositorios virtuales o de grupo (HAR-14)

Un repositorio en modo `group` no guarda contenido: resuelve una ruta entre
sus miembros, en el orden en que se declaran. El contrato, las pruebas y los
límites están en [`uat/UAT-PROXY-VIRTUAL.md`](uat/UAT-PROXY-VIRTUAL.md).

**Qué hay.** `hodei_catalog::group` es el núcleo puro: la configuración
(`config_settings.group.members`) y `walk`, un recorrido en profundidad que
detecta ciclos, acota profundidad (3), miembros (16) y nodos (64), exige el
mismo formato y devuelve las hojas en orden. `hodei-catalog-storage` valida
el grupo al crear y al reconfigurar (miembros existentes, de la cuenta, sin
ciclos) y rechaza `group` fuera de un grupo y `proxy` fuera de un proxy.
`src/handlers/raw_group.rs` resuelve la lectura y `raw.rs` la conecta a
`GET`/`HEAD`; `PUT` responde `405`.

**Cómo decide.** El orden de los miembros es la precedencia. Un MISS (hosted
sin la ruta, proxy que no la tiene) continúa; un ERROR se propaga y detiene
la resolución sin saltar a otro miembro. Cedar se evalúa sobre el grupo y
sobre cada miembro, y un miembro ilegible se omite. La respuesta dice quién
ganó con `x-hodei-resolved-from` y `x-hodei-group-version`, y el servicio
audita la resolución con su recorrido.

**Lo que sigue sin estar.** No hay caché de resolución, enrutado por ruta ni
prioridades distintas del orden; los grupos de varios formatos tampoco. Los
límites aceptados están en la sección 4 del UAT.

## 10-tresvicies — M10: Maven 2 hosted (HAR-15)

Un repositorio de formato `maven2` y modo `hosted` publica y resuelve con el
protocolo de Maven. El contrato, las pruebas y los límites están en la
sección MAVEN de [`uat/UAT-FORMATS.md`](uat/UAT-FORMATS.md).

**Qué hay.** `hodei_catalog::maven` es el núcleo puro: análisis del layout,
versiones y su orden, POM, metadatos, checksums y la política
(`config_settings.maven`: `version_policy`, `release_redeploy`).
`CatalogReader::assets_under` lista los ficheros bajo un prefijo (con tope) y
`hodei-catalog-storage` valida la configuración Maven al crear o reconfigurar.
`src/handlers/maven.rs` aplica el protocolo y `raw.rs` lo conecta a
`PUT`/`GET`/`HEAD`.

**Cómo decide.** El modelo de contenido no cambia: cada fichero es un asset
direccionado por su digest. `maven-metadata.xml` y los checksums se
**derivan** del catálogo en cada lectura, y lo que un cliente sube de ellos
se valida y se acepta sin almacenar. Eso elimina la carrera de
lectura-modificación-escritura de los metadatos y deja los fallos parciales
recuperables por construcción. Las releases son inmutables salvo
`release_redeploy`; los snapshots flotantes se reponen.

**Lo que sigue sin estar.** Metadatos de plugins derivados en un hosted y
`Range` en lo derivado. El proxy y el grupo Maven llegaron en HSP-110 (abajo).

## 10-quatervicies — M10: Maven 2 proxy y group (HAR-16)

Un repositorio `maven2` en modo `proxy` o `group` resuelve dependencias para
el mismo cliente Maven que usa el hosted. El contrato, las pruebas y los
límites están en la sección MAVEN-REMOTE de
[`uat/UAT-FORMATS.md`](uat/UAT-FORMATS.md).

**Qué hay.** Nada de la infraestructura se duplica: el proxy es el de
HSP-106/107 (`the_proxy_asset`, caché, TTL, offline, cuarentena) y el grupo
el de HSP-108 (`raw_group`, `walk_group`). HSP-110 añade la adaptación Maven:
`proxy_cache::kind_of` trata el nombre flotante de un snapshot como
revalidable, `hodei_catalog::maven` aporta `Contribution`,
`declared_contribution` y `merged_metadata`, y `handlers/maven.rs`
(`the_group_derived`) resuelve metadatos y checksums de un grupo. `raw.rs`
despacha Maven 2 por modo.

**Cómo decide.** El proxy sirve lo que el upstream entrega sin reescribirlo;
solo aplica la `version_policy`. En el grupo, un fichero lo gana el primer
miembro que lo tiene y su checksum se calcula del fichero ganador; los
metadatos de artefacto son la unión de las versiones de las hojas y los de
versión los del primer miembro que los tiene. Un metadato ilegible de un
upstream es `502`, no una lista parcial.

**Lo que sigue sin estar.** Fusión de metadatos de plugins y `Range` en lo
derivado.
