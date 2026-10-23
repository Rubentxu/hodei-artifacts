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
