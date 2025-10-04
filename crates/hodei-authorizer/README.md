# hodei-authorizer

El crate `hodei-authorizer` es el cerebro orquestador del sistema de gobernanza y autorización de Hodei. Combina políticas de IAM con Service Control Policies (SCPs) para tomar decisiones de acceso seguras y jerárquicas.

## Arquitectura

Este crate sigue una arquitectura limpia con separación clara de concerns:

- **Puertos (Ports)**: Traits abstractos que definen las interfaces necesarias
- **Adaptadores (Adapters)**: Implementaciones concretas de los puertos
- **Servicio de Autorización**: Lógica central que orquesta la evaluación de políticas

## Puertos

### IamPolicyProvider
```rust
#[async_trait]
pub trait IamPolicyProvider: Send + Sync {
    async fn get_identity_policies_for(&self, principal_hrn: &Hrn) -> Result<PolicySet, AuthorizationError>;
}
```

### OrganizationBoundaryProvider
```rust
#[async_trait]
pub trait OrganizationBoundaryProvider: Send + Sync {
    async fn get_effective_scps_for(&self, entity_hrn: &Hrn) -> Result<Vec<ServiceControlPolicy>, AuthorizationError>;
}
```

## Servicio de Autorización

### AuthorizerService
```rust
pub struct AuthorizerService<IAM: IamPolicyProvider, ORG: OrganizationBoundaryProvider> {
    iam_provider: IAM,
    org_provider: ORG,
    policy_evaluator: PolicyEvaluator,
}

impl<IAM: IamPolicyProvider, ORG: OrganizationBoundaryProvider> AuthorizerService<IAM, ORG> {
    pub fn new(iam_provider: IAM, org_provider: ORG, policy_evaluator: PolicyEvaluator) -> Self { ... }
    pub async fn is_authorized(&self, request: AuthorizationRequest) -> Result<AuthorizationResponse, AuthorizationError> { ... }
}
```

## Flujo de Autorización

1. El `AuthorizerService` recibe una solicitud de autorización
2. Obtiene las políticas de IAM del principal a través de `IamPolicyProvider`
3. Obtiene las SCPs efectivas para la entidad del principal a través de `OrganizationBoundaryProvider`
4. Combina todas las políticas y las evalúa usando el `PolicyEvaluator` de `hodei-policies`
5. Devuelve una respuesta de autorización con la decisión final

## Reglas de Autorización

1. **Deny Explícito Anula Todo**: Si cualquier política (IAM o SCP) contiene un `forbid` que coincida con la solicitud, la decisión final es `Deny`
2. **Se Requiere un Allow de Identidad**: Si no hay un `Deny` explícito, se requiere que las políticas de IAM contengan un `permit` explícito
3. **Las Barreras de la Organización Deben Permitir**: Si hay un `Allow` de IAM, las SCPs efectivas también deben permitir la acción

## Tests

Los tests están ubicados en el directorio `tests/` y utilizan mocks para probar la lógica de autorización sin depender de implementaciones concretas de los puertos.
