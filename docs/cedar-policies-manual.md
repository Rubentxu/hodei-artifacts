# Manual Completo de Políticas Cedar para Hodei

**Versión:** 1.0
**Última actualización:** 2025-09-03

## 1. Introducción a Cedar DSL en Hodei

Cedar es el lenguaje de políticas de autorización utilizado por Hodei para control de acceso granular. 
Este manual cubre la implementación específica de Cedar en el ecosistema Hodei.

### 1.1. Conceptos Fundamentales

- **Políticas**: Reglas escritas en Cedar DSL que definen "quién puede hacer qué sobre qué"
- **Entidades**: Recursos de Hodei (Users, Organizations, Repositories, Artifacts) representados en Cedar
- **HRNs**: Hodei Resource Names como identificadores únicos para entidades
- **Atributos**: Propiedades de las entidades utilizadas en condiciones de políticas

## 2. Entidades Hodei en Cedar

### 2.1. Mapeo de Entidades

Cada tipo de entidad Hodei se representa como un tipo de entidad Cedar:

```cedar
// Formato: EntityType::"HRN"
User::"hrn:hodei:iam:global:org_123:user/user_456"
Organization::"hrn:hodei:organization:us-east-1:org_123"
Repository::"hrn:hodei:repository:us-east-1:org_123:repo/my-repo"
Artifact::"hrn:hodei:artifact:us-east-1:org_123:package_version/react/18.2.0"
```

### 2.2. Atributos de Entidades

Cada entidad Hodei expone atributos para uso en políticas:

**Usuario (`User`)**
```cedar
principal.department      // "engineering"
principal.role           // "developer"
principal.status         // "Active", "Suspended"
principal.email          // "user@example.com"
```

**Organización (`Organization`)**
```cedar
resource.type            // "organization"
resource.status          // "Active", "Archived"
resource.primary_region  // "us-east-1"
```

**Repositorio (`Repository`)**
```cedar
resource.ecosystem       // "npm", "maven", "docker"
resource.visibility      // "public", "private"
resource.status          // "Active", "ReadOnly"
```

**Artifacto (`Artifact`)**
```cedar
resource.package_name    // "react"
resource.version         // "18.2.0"
resource.status          // "Active", "Deprecated"
```

## 3. Sintaxis de Políticas Cedar

### 3.1. Estructura Básica

```cedar
permit | forbid (
    principal,
    action,
    resource
)
when | unless {
    // condiciones
};
```

### 3.2. Ejemplos con Entidades Hodei

**Acceso básico a repositorio:**
```cedar
permit (
    principal == User::"hrn:hodei:iam:global:org_123:user/user_456",
    action == Action::"read",
    resource == Repository::"hrn:hodei:repository:us-east-1:org_123:repo/my-repo"
);
```

**Acceso basado en atributos:**
```cedar
permit (
    principal,
    action == Action::"write",
    resource in Repository::"hrn:hodei:repository:us-east-1:org_123:repo/*"
)
when {
    principal.department == "engineering" &&
    resource.visibility == "private"
};
```

**Acceso a artefactos con condiciones:**
```cedar
permit (
    principal,
    action == Action::"download",
    resource in Artifact::"hrn:hodei:artifact:us-east-1:org_123:package_version/*"
)
when {
    resource.status == "Active" &&
    principal in Group::"hrn:hodei:iam:global:org_123:group/developers"
};
```

## 4. Acciones y Recursos Hodei

### 4.1. Acciones Comunes

```cedar
// Acciones de Repository
Action::"createRepository"
Action::"readRepository"  
Action::"updateRepository"
Action::"deleteRepository"
Action::"listArtifacts"

// Acciones de Artifact
Action::"uploadArtifact"
Action::"downloadArtifact"
Action::"deleteArtifact"
Action::"scanArtifact"

// Acciones de Organization
Action::"manageOrganization"
Action::"inviteUser"
Action::"viewBilling"
```

### 4.2. Patrones de Recursos

**Acceso a todos los recursos de una organización:**
```cedar
resource in Repository::"hrn:hodei:repository:us-east-1:org_123:*"
```

**Acceso a recursos específicos por tipo:**
```cedar
resource in Artifact::"hrn:hodei:artifact:us-east-1:org_123:package_version/react/*"
```

## 5. Condiciones Avanzadas

### 5.1. Condiciones con Contexto

```cedar
permit (
    principal,
    action == Action::"uploadArtifact",
    resource
)
when {
    // Solo desde IPs corporativas
    context.request.ip in ip("10.0.0.0/8") &&
    
    // Solo durante horario laboral
    context.time.hour >= 9 && context.time.hour < 17 &&
    
    // Requiere autenticación fuerte
    context.authentication.strength >= 2
};
```

### 5.2. Condiciones con Múltiples Entidades

```cedar
permit (
    principal,
    action == Action::"manageRepository",
    resource == Repository::"hrn:hodei:repository:us-east-1:org_123:repo/*"
)
when {
    // Usuario es owner del repositorio O es admin de la organización
    resource.owner == principal ||
    principal in Group::"hrn:hodei:iam:global:org_123:group/admins"
};
```

## 6. Testing y Validación

### 6.1. Herramientas CLI de Cedar

**Validar sintaxis de políticas:**
```bash
# Instalar Cedar CLI
cargo install cedar-policy-cli

# Validar política
cedar validate --policy policy.cedar --schema schema.json
```

**Evaluar políticas:**
```bash
cedar evaluate \
  --principal 'User::"hrn:hodei:iam:global:org_123:user/user_456"' \
  --action 'Action::"read"' \
  --resource 'Repository::"hrn:hodei:repository:us-east-1:org_123:repo/my-repo"' \
  --context '{"time": {"hour": 14}}' \
  --policies policy.cedar
```

### 6.2. Extensiones VS Code

**Cedar Policy Language Support:**
- Syntax highlighting para archivos `.cedar`
- Autocompletado de entidades y acciones
- Validación en tiempo real
- Formateo automático

**Instalación:**
1. Abrir VS Code
2. Ir a Extensiones (Ctrl+Shift+X)
3. Buscar "Cedar Policy"
4. Instalar la extensión oficial de AWS

### 6.3. Linters e Integraciones IDE

**ESLint para Cedar (si se usa en frontend):**
```javascript
// .eslintrc.js
module.exports = {
  plugins: ['cedar'],
  rules: {
    'cedar/valid-syntax': 'error',
  }
};
```

**Pre-commit hooks para validación:**
```bash
# .pre-commit-config.yaml
repos:
  - repo: local
    hooks:
      - id: cedar-validation
        name: Validate Cedar Policies
        entry: cedar validate --policy
        language: system
        files: \.cedar$
```

### 6.4. Testing en Hodei

**Validación integrada:**
```rust
use iam::infrastructure::CedarPolicyValidator;

let validator = CedarPolicyValidator;
let policy_content = r#"permit(principal, action, resource);"#;

// Validar sintaxis
validator.validate_policy_syntax(policy_content)?;

// Validar semántica (requiere schema)
validator.validate_policy_semantics(policy_content, entities)?;
```

**Tests unitarios:**
```rust
#[test]
fn test_policy_validation() {
    let policy = r#"
        permit(
            principal == User::"hrn:hodei:iam:global:org_123:user/test",
            action == Action::"read",
            resource == Repository::"hrn:hodei:repository:us-east-1:org_123:repo/test"
        );
    "#;
    
    assert!(validator.validate_policy_syntax(policy).is_ok());
}
```

## 7. Mejores Prácticas para Hodei

### 7.1. Organización de Políticas

**Políticas por equipo:**
```
policies/
├── engineering-team.cedar
├── qa-team.cedar
├── admin-team.cedar
└── security-policies.cedar
```

**Políticas por recurso:**
```
policies/
├── repository-access.cedar
├── artifact-management.cedar
├── user-permissions.cedar
└── organization-settings.cedar
```

### 7.2. Nomenclatura de Políticas

```cedar
// Buenas prácticas de nombres
permit(
    principal,
    action == Action::"read",
    resource
)
when {
    // Política: team-engineers-read-all-repos
    principal.department == "engineering"
};

// Comentarios descriptivos
/*
 * Política: engineers-can-write-to-private-repos
 * Descripción: Permite a ingenieros escribir en repositorios privados
 * Scope: Todos los repositorios privados de la organización
 */
```

### 7.3. Seguridad y Auditoría

**Políticas de denegación explícitas:**
```cedar
// Denegar acceso a recursos archivados
forbid (
    principal,
    action,
    resource
)
when {
    resource.status == "Archived"
};

// Denegar acceso fuera de horario laboral
forbid (
    principal,
    action,
    resource
)
unless {
    context.time.hour >= 9 && context.time.hour < 17
};
```

## 8. Integración con Sistema Hodei

### 8.1. Flujo de Autorización

1. **Conversión de entidades**: Entidades Hodei → Entidades Cedar
2. **Evaluación**: Motor Cedar evalúa políticas contra el contexto
3. **Decisión**: Permitir o denegar basado en políticas coincidentes
4. **Cache**: Resultados cacheados para mejor performance

### 8.2. Ejemplo de Implementación

```rust
// Convertir entidad Hodei a Cedar
let cedar_entity = to_cedar_entity(&user_entity)?;

// Crear request de autorización
let request = Request::new(
    Some(cedar_entity.uid()),
    Some(action_entity_uid),
    Some(resource_entity_uid),
    context
);

// Evaluar políticas
let response = authorizer.is_authorized(&request, &policies, &entities);

// Tomar decisión basada en response
if response.decision() == Decision::Allow {
    // Acceso permitido
} else {
    // Acceso denegado
}
```

## 9. Troubleshooting y Debugging

### 9.1. Errores Comunes

**Error de sintaxis:**
```
Error: unexpected token `resource` expected `)`
  --> policy.cedar:5:1
```

**Solución:** Verificar paréntesis balanceados y sintaxis correcta

**Error de entidad desconocida:**
```
Error: entity `User::"invalid-hrn"` not found
```

**Solución:** Verificar que el HRN sea válido y la entidad exista

### 9.2. Debugging de Políticas

**Habilitar logging detallado:**
```rust
// En desarrollo, habilitar debug logging
std::env::set_var("RUST_LOG", "debug");
env_logger::init();
```

**Ver razones de decisión:**
```rust
let response = authorizer.is_authorized(&request, &policies, &entities);
for reason in response.diagnostics().reasons() {
    println!("Reason: {}", reason);
}
```

## 10. Recursos Adicionales

### 10.1. Documentación Oficial

- [Cedar Policy Language Guide](https://docs.cedarpolicy.com/)
- [Cedar GitHub Repository](https://github.com/cedar-policy/cedar)
- [AWS Cedar Documentation](https://docs.aws.amazon.com/cedar/)

### 10.2. Herramientas de la Comunidad

- **Cedar Playground**: Editor online para probar políticas
- **Cedar VS Code Extension**: Soporte para IDE
- **Cedar CLI**: Herramientas de línea de comandos

### 10.3. Soporte Hodei

- `#cedar-policies` en Slack interno
- Documentación de entidades Hodei en `/docs/domain`
- Ejemplos de políticas en tests de integración

---

*Este manual se actualiza regularmente. Consulte la versión más reciente en la documentación oficial de Hodei.*