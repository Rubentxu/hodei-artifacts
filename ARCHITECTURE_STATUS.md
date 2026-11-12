# Estado de la arquitectura

**Hito vigente:** M0 — Convergencia (del roadmap `hodei-artifacts-trustgraph-v3`).
**Generación de documento que manda:** PRD v3 y la UAT de convergencia que lo acompaña.
**Fecha:** 2026-11-11.

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
| `crates/hodei-evidence` | Contexto de evidencia y registro de eventos (M1): los contratos, sin adapter. | `implementada` |
| `crates/hodei-evidence-storage` | Los adaptadores de la evidencia sobre el almacen que ya existe. | `implementada` |
| `.` (`hodei-artifacts-api`) | API HTTP: composición, arranque, rutas, auditoría. | `implementada` |

El kernel no depende de ningún mecanismo concreto: ni SurrealDB, ni Axum, ni
HTTP. Eso no es una aspiración, lo comprueba una comprobación de forma
arquitectónica.

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
| Comando operador del índice de políticas | `ensure-iam-index` | `tests/iam_account_index.rs` |

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
de organizaciones". **Ninguna de las dos cosas es cierta hoy.**

- No hay almacenamiento de blobs. El registro de artefactos por digest es M1.
- No hay OCI, ni Maven, ni npm. Eso es M9, y su material está en
  `crates/distribution`, sin integrar.
- No hay organizaciones ni SCP. El modelo vivo es la cuenta por defecto.
- No hay grafo, ni evidencia, ni decisión de promoción. Son M1, M2 y M3.

## 4. Documentación

| Documento | Clasificación |
| --- | --- |
| Este documento | vigente |
| `docs/security/*.md` | vigente: describe el comportamiento comprobado |
| `docs/ci-strategy.md` | vigente |
| `docs/cedar-policies-manual.md` | vigente |
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
