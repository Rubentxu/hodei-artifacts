# SPEC S-001 — Cedar, IAM, SCP y HRN v2 (P0)

## HRN v2
Forma propuesta: `hrn:<partition>:<service>:<region>:<account>:<kind>/<opaque-id>`. Ejemplo: `hrn:hodei:artifacts:eu-west-1:acme:repository/releases`. `partition` no puede ser `aws` de forma implícita; no clonar ARN comercial tal cual. Serialización determinista, longitudes/carácteres permitidos documentados; percent encoding estructurado si se admite slash/colon dentro de IDs. IDs inmutables sin reutilización. Migración de formato v1 real: `hrn:hodei:iam::default:User/alice` → canonical nuevo conservando alias SOLO con scope; no inferir cuentas desconocidas; conflictos explícitos.

## Cedar EntityUid
UID canónico: `Hodei::Repository::"hrn:hodei:artifacts:eu-west-1:acme:repository/releases"`. Namespace derivado de tipo conocido, NUNCA vía PascalCase improvisado de string no confiable. Sin omisión de tenant/region/kind. Escapes mediante APIs Cedar tipadas/serde; no interpolar texto de HRN o claims externos. Acciones: `Hodei::Action::"ReadAsset"` etc, esquema declarado y versionado.

## Fórmula de decisión
```
allowed = tenant_boundary
          AND (identity_permit OR resource_permit)
          AND organization_boundary
          AND NOT explicit_forbid
          AND domain_invariant_ok
```
SCP nunca debe aportar grant; un `permit` de SCP define máximo (boundary), no habilita solo. `forbid` explícito prevalece, default deny. Las políticas Cedar operan sobre principal, acción, recurso y contexto de atributos confiables. Separar permiso de dominio (`BlockRedeploy`, digest mismatch).

## Consistencia de seguridad
- Evaluación con snapshot inmutable `(policy_rev,entity_rev,resource_rev)`, sin mezclas de solicitudes; compilación/validación estricto de schema y policy al cambiar; contexto PARC validado.
- Para revocaciones: invalidación coherente en todos los nodos o lectura obligatoria de versión activa; si no puede probar freshness, denegar; no usar TTL libre para permisos críticos.
- No cachear `ALLOW` sin versionado y revocación demostrados; caché de blob no implica permiso.
- Autenticación de workloads/usuarios OIDC/API token, secrets refs, expiración/rotación y propagation of identity desde protocolo cliente; nunca principal hard-coded `system`.
- Group-lookup de Cedar desde grafo con relaciones tipadas y cierre de ascendientes acotado. Herencia no se deduce del prefijo textual HRN.
- Auditoría de decisiones relevantes: request ID, policy revision, reason, sobre quién/recurso, sin tokens/secrets.

## Casos discriminantes permanentes
`alice@org-A ≠ alice@org-B`, recurso privado de B → deny; SCP-only permit no concede; identidad permit con SCP restrictive niega; forbid prevalece; cambio policy rev provoca deny tras activación; schema inválido no entra en producción; dos solicitudes concurrentes sin contaminación; error TypeDB/engine falla cerrado.

## Migración legado
Aprovechar `kernel/src/domain/hrn.rs`, `hodei-iam`, `hodei-policies`, `docs/cedar-policies-manual.md`; no reactivar `hodei-authorizer` actual que concatena policies antes de diseño correcto.

## Perfil de conformidad H1a (parcial, explícito)

Este perfil delimita qué parte de S-001 se cumple en H1a y qué queda abierto en H1b. Un incumplimiento del perfil es un defecto; lo que el perfil difiere **no** se declara cumplido.

### HRN v2 canónico
Forma: `hrn:<partition>:<service>:<region>:<account>:<kind>/<opaque-id>`. Los HRN v1 (`hrn:<partition>:<service>::<account>:<kind>/<opaque-id>`) se conservan como **alias con la región vacía**: se parsean, se vuelven a serializar byte a byte idénticos y se distinguen del v2 solo porque llevan región. Un HRN v1 no se reescribe a un HRN v2 con región inventada: una cuenta o región desconocida no se infiere, se deja vacía y se registra como conflicto si exige migración. `partition` no se infiere de `aws` ni de ningún valor implícito.

### EntityUid Cedar con scope
El identificador Cedar de una entidad es `(tipo)::"id opaco con scope"`, donde el id opaco con scope incluye **partición, servicio, región, cuenta y tipo**, no solo el nombre del recurso. Dos recursos homónimos en cuentas distintas producen **UIDs distintos**, y por tanto decisiones distintas. El id se construye con las APIs Cedar tipadas (`EntityTypeName`, `EntityId`, `EntityUid::from_type_name_and_id`), sin interpolar texto de HRN ni de claims externos en un UID.

El **tipo** se deriva de la regla de tipo declarado de la plataforma, no de una improvisación por petición: namespace = servicio del tipo (`iam` → `Iam`) y nombre = tipo de recurso declarado (`User` → `User`). Es exactamente la regla que usa `HodeiEntityType::entity_type_name()` al registrar el schema Cedar, de modo que el tipo que evalúa el motor es el tipo que el schema registra. La forma es única: no hay dos rutas que produzcan UID de entidad distintos para la misma identidad.

**Delta pendiente (H1b).** S-001 exige que el namespace provenga de un **tipo conocido** y no del texto del HRN, con el namespace de plataforma `Hodei::*` (`Hodei::Repository::"hrn:hodei:artifacts:eu-west-1:acme:repository/releases"`). En H1a el namespace sigue al servicio declarado en el HRN y el motor evalúa sin schema (defecto O8), así que no existe registro contra el que resolver el tipo ni un namespace de plataforma único. Lo que H1a garantiza es el **scope** (aislamiento inter-tenant) y la **forma única** del tipo, no la procedencia del namespace desde un registro. Eso, junto con el namespace de acción `Hodei::Action::"X"`, se resuelve en H1b con el schema como autoridad (ver `ADR/011-h1a-identity-and-decision-core.md`).

### UID de acción (forma única en H1a)
Las tres rutas que construyen el UID de acción —el motor de evaluación, el playground y `ActionTrait::action_name()`— producen **la misma forma**: `Action::"<Nombre>"`, plano, tal y como el schema IAM registrado declara sus tipos de acción. La divergencia previa (`Iam::Action::"X"` en una ruta, `Action::"X"` en otra) queda cerrada. El objetivo de S-001 `Hodei::Action::"ReadAsset"` **no** se cumple en H1a: exige que el schema sea la autoridad del namespace de acción, y que el schema se aplique a la petición; ambas cosas son H1b.

### Snapshot inmutable por petición
La evaluación usa un snapshot inmutable de policies y entities construido **por petición**. Ninguna evaluación ve ni modifica el estado de otra. No existe un registro mutable compartido de policies/entities en el motor entre peticiones. Un fallo al construir el snapshot (policy inválida, entidad inválida, error del motor) produce **deny**, nunca un permiso.

### Decisión
`forbid` explícito prevalece sobre `permit`; el default es deny. En H1a el límite de tenant es **estructural** (el scope completo del UID hace que un recurso de otra cuenta sea un recurso distinto, no el mismo), no la intersección SCP de H1b. La intersección de límites (IAM ∧ recurso ∧ organización ∧ SCP-deny) y las revocaciones activas son H1b.

## Perfil de conformidad H1b-1 (parcial, explícito)

H1b-1 es el sub-bloque de H1b que arregla la **lectura** del schema en las dos
rutas que lo consumen (defectos O7 y O2). Este perfil delimita qué se cumple y
qué sigue diferido; lo que difiere **no** se declara cumplido.

### El schema se lee con un único formato

Un schema es el sobre `cedar-schema-fragments-v1`: `{"format": …,
"fragments": [...]}`, la misma representación que la ruta de persistencia
escribe y que la ruta de lectura de la persistencia ya reconstruía. Las tres
rutas —persistir, leer del almacén, leer inline— coinciden ahora en **qué es un
schema**. Un contenido que no es un sobre de ese formato **se rechaza** en vez
de leerse como un schema vacío: un schema vacío no declara nada, así que
aceptarlo en silencio haría que toda política que nombrase un tipo declarado
pareciese válida y toda política que no nombrase nada pareciese válida
también — el falso verde que S-001 prohíbe en la línea de «schema inválido no
entra en producción».

La excepción declarada es el schema **explícitamente vacío** (`{}` o `null`):
es una petición legítima, y se distingue de «esto no es un schema» porque lo
dice.

### Validación contra el schema leído

El playground **aplica** el schema que leyó: una política sobre un tipo que el
schema declara se acepta, y una política sobre un tipo que **no** declara se
reporta nombrando el tipo. La comprobación se hace con el validador de Cedar
contra el schema compuesto, no con una comparación textual.

**La regla del schema que no declara nada.** Un schema sin tipos de entidad ni
acciones no puede reprobar nada: aplicarlo tal cual haría que *toda* política
que nombrase un tipo pareciese inválida. Eso es un **falso rechazo**, no una
validación, y por tanto está prohibido. Cuando el schema no declara nada, lo
único que se comprueba es la **sintaxis** de la política — la misma regla que
ya aplica el caso «no hay esquema almacenado».

### El esquema activo es observable por HTTP

`GET /api/v1/schemas/load` no es un marcador: delega en el puerto de lectura ya
cableado y responde con un **estado declarado** (`active` / `no_active_schema`),
la **versión** bajo la que el esquema está almacenado, su **identificador** y su
**contenido**. «No hay esquema» es una respuesta (`200` con
`no_active_schema`), no un error ni un stub: el consumidor puede distinguirlo
de «hay un esquema y este es».

### Lo que H1b-1 **no** cumple (O8, y sigue siendo H1b)

- El motor de evaluación (`evaluate_policies`) sigue **cargando** el schema sin
  **aplicarlo** a la petición, y el evaluador del playground sigue decidiendo
  con `Authorizer::new()`, sin schema. H1b-1 hace que el schema **se lea** y que
  **valide**; **no** hace que la **decisión** dependa de él.
- Por tanto S-001 **sigue sin cumplirse por completo**: el namespace de
  entidad `Hodei::*` y el namespace de acción `Hodei::Action::"ReadAsset"`
  requieren que el schema sea la **autoridad** —que se aplique a la petición y
  que resuelva el namespace—, y eso es el bloque de gobernanza de H1b (O8).
- La intersección de límites (IAM ∧ recurso ∧ organización ∧ SCP-deny), las
  revocaciones activas y auditadas, y el CORS por origen (O9) siguen abiertos.

## Perfil de conformidad H1b-2 (parcial, explícito)

H1b-2 es el sub-bloque de H1b que hace que el schema **gobierne la decisión**
del playground (la mitad del playground de O8). Este perfil delimita qué se
cumple y qué sigue diferido; lo que difiere **no** se declara cumplido.

### El schema es la autoridad sobre el verbo

El verbo de la petición se resuelve **por id** contra las acciones que el schema
declara (`schema.actions()`), no contra el texto de la petición. La resolución
es **exacta**: una acción cuyo id difiere es otra acción, no una coincidencia
aproximada.

Tres desenlaces, y solo tres:

| Situación | Decisión | Motivo |
|---|---|---|
| el schema declara **una** acción con ese id | se evalúa contra ella | el schema ha resuelto el verbo |
| el schema **no declara** ninguna acción con ese id | **deniega** | el verbo no existe en el mundo del schema |
| el schema declara **más de una** acción con ese id | **deniega** | Cedar permite que dos acciones *namespaced* compartan id; elegir una sería evaluar contra una acción que el consumidor no pidió, así que se nombran **ambos** candidatos |

El tercer caso no es teórico: se comprobó que Cedar acepta un schema con dos
acciones namespaced de id `read` y que `schema.actions()` devuelve **las dos**.

### El schema decide el namespace de la acción

El namespace de la acción no es una elección de formato: es **lo que el schema
declara**. Una política escrita contra `Hodei::Action::"read"` concede cuando
el schema declara ese namespace, y **no concede** cuando el schema declara la
acción plana — porque entonces el verbo denota `Action::"read"`, que es otra
acción. La forma plana `Action::"<Nombre>"` se conserva **solo** para el caso
en que no hay schema que resuelva el verbo, y es la que ya usan el motor y el
esquema IAM registrado (H1a).

### La petición se aplica al schema

La petición Cedar se construye **con** el schema, de modo que Cedar compruebe
que encaja. Una petición cuyo tipo de principal o de recurso el schema no
declara **no conforms**: se **deniega con el motivo**, en vez de evaluarse
contra un schema que la ignora.

El orden en que Cedar hace las comprobaciones importa y es **medido**: Cedar
valida **los tipos de recurso y principal antes que la acción**, así que una
petición cuyo recurso no está declarado se rechaza por el **tipo**, no por la
acción. El motivo que se devuelve nombra lo que falló de verdad.

### El contexto es parte de la petición

El contexto de la petición se convierte y **llega** a la evaluación. Dos
consecuencias, ambas obligatorias:

- una política condicionada sobre un atributo de contexto concede o deniega
  según su valor;
- un schema que declara un tipo de contexto con atributos **puede** comprobarse
  contra la petición.

### El schema que no declara nada no constriñe nada

Un schema sin tipos de entidad ni acciones **no se aplica**. Aplicarlo tal cual
haría que *toda* petición pareciese no conformar — un **falso rechazo**, no
una validación. Es la misma regla que H1b-1 fijó para la validación, y ahora
también para la evaluación: el schema se aplica **cuando declara algo**.

### Lo que H1b-2 **no** cumple (la mitad del motor de O8)

- El motor de evaluación (`evaluate_policies`) sigue **cargando** el schema
  registrado sin **aplicarlo** a la petición. Su causa está **medida**: el
  esquema registrado es de alcance **IAM** y `POST /api/v1/policies/evaluate`
  es **genérico**, así que aplicarlo en crudo **denegaría toda petición fuera
  del mundo IAM** (los casos `G9`–`G12` sobreviven porque pasan
  `evaluation_mode: "NoSchema"`). Cerrar esa mitad exige una decisión de
  **alcance de plataforma** —qué esquema gobierna una petición genérica— que
  no es un cambio de código.
- Por tanto S-001 **sigue sin cumplirse por completo**: el namespace de
  entidad `Hodei::*` y el namespace de acción `Hodei::Action::"ReadAsset"`
  requieren que el schema sea la **autoridad** en el motor también.
- La intersección de límites (IAM ∧ recurso ∧ organización ∧ SCP-deny), las
  revocaciones activas y auditadas, y el CORS por origen (O9) siguen abiertos.
