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
