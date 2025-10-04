# Plan de Integración de Arquitectura - Hodei Artifacts

## Estado Actual del Análisis

### Crates Existentes y sus Responsabilidades

#### 1. **policies** (Motor Base de Políticas Cedar)
**Ubicación**: `crates/policies/`

**Responsabilidades**:
- Motor de evaluación de políticas Cedar
- `AuthorizationEngine`: Motor principal que evalúa políticas Cedar
- `AuthorizationRequest`: Request con entidades HodeiEntity
- `PolicyStore`: Almacenamiento de políticas
- `EngineBuilder`: Constructor para registrar tipos de entidades y acciones
- Abstracciones de dominio: `Principal`, `Resource`, `Action`, `HodeiEntity`

**API Principal**:
```rust
// AuthorizationEngine en policies/src/shared/application/engine.rs
pub struct AuthorizationEngine {
    pub schema: Arc<Schema>,
    pub store: PolicyStore,
}

impl AuthorizationEngine {
    pub async fn is_authorized(&self, request: &AuthorizationRequest<'_>) -> Response {
        // Evalúa usando Cedar Authorizer directamente
    }
}

// Request usa trait objects de HodeiEntity
pub struct AuthorizationRequest<'a> {
    pub principal: &'a dyn HodeiEntity,
    pub action: cedar_policy::EntityUid,
    pub resource: &'a dyn HodeiEntity,
    pub context: Context,
    pub entities: Vec<&'a dyn HodeiEntity>,
}
```

**Tests existentes**: Features de playground, análisis, batch eval (todos pasan)

#### 2. **hodei-iam** (Entidades y Políticas IAM)
**Ubicación**: `crates/hodei-iam/`

**Responsabilidades**:
- Definir entidades IAM: `User`, `Group`, `ServiceAccount`, `Namespace`
- Acciones IAM: `CreateUserAction`, `CreateGroupAction`
- Repositorios para persistencia de entidades IAM
- Features CRUD: create_user, create_group, add_user_to_group

**Implementa traits de policies**:
- `User`, `Group` implementan `Principal` y `Resource`
- `ServiceAccount` implementa `Principal` y `Resource`

**Tests existentes**: 
- Tests de integración CRUD (create_user, create_group, add_user_to_group)
- Tests unitarios de dominio
- Estado: ✅ Todos pasan

#### 3. **hodei-organizations** (Estructura Organizacional y SCPs)
**Ubicación**: `crates/hodei-organizations/`

**Responsabilidades**:
- Definir estructura organizacional: `Account`, `OrganizationalUnit`
- Service Control Policies (SCPs): `ServiceControlPolicy`
- Gestión de jerarquía organizacional
- Features: create_account, create_ou, create_scp, attach_scp, move_account, get_effective_scps

**Dominio**:
- `OrganizationalUnit`: Tiene `attached_scps: Vec<Hrn>`
- `Account`: Pertenece a un OU (`parent_hrn`)
- `ServiceControlPolicy`: Contiene `document: String` (política Cedar)

**Tests existentes**:
- Tests unitarios de features (con mocks)
- Tests de integración
- Estado: ✅ Tests unitarios pasan

#### 4. **hodei-authorizer** (Orquestador de Autorización)
**Ubicación**: `crates/hodei-authorizer/`

**Estado Actual**: ⚠️ **PROBLEMA: Reimplementando funcionalidad**

**Lo que está haciendo**:
- Tiene su propio `EvaluatePermissionsUseCase` que parece duplicar `AuthorizationEngine`
- Define sus propios ports: `IamPolicyProvider`, `OrganizationBoundaryProvider`
- Intenta combinar políticas IAM + SCPs + Resource policies
- Tiene cache, logging, metrics integrados

**Problema identificado**:
```rust
// hodei-authorizer está creando su propio sistema de evaluación
// en lugar de USAR el AuthorizationEngine de policies
pub struct EvaluatePermissionsUseCase<IAM, ORG, CACHE, LOGGER, METRICS, RESOLVER> {
    iam_provider: IAM,
    org_provider: ORG,
    // ... más dependencias
    cedar_authorizer: Authorizer,  // ⚠️ Usando Cedar directamente
}
```

**Debería hacer**:
- Orquestar la recolección de políticas de IAM y Organizations
- Agregar todas las políticas al PolicyStore
- Delegar la evaluación al `AuthorizationEngine` de policies
- Agregar capas de cache, logging, metrics ALREDEDOR del engine

---

## Problema Principal Identificado

### ❌ **hodei-authorizer está reimplementando el motor de políticas**

**Evidencia**:
1. Usa `cedar_policy::Authorizer` directamente en lugar de `AuthorizationEngine`
2. Tiene su propia lógica de evaluación
3. No reutiliza `AuthorizationRequest` de policies
4. Define DTOs propios en lugar de usar los del crate policies

**Consecuencias**:
- Duplicación de código
- Tests fallando porque la lógica no está completa
- Mantenimiento duplicado
- No aprovecha las features del crate policies

---

## Arquitectura Correcta Propuesta

### Principio: **Delegation Over Duplication**

```
┌─────────────────────────────────────────────────────────┐
│                   hodei-authorizer                      │
│  (Orquestador - NO implementa evaluación)               │
│                                                          │
│  1. Recolecta políticas IAM (via hodei-iam)            │
│  2. Recolecta SCPs (via hodei-organizations)           │
│  3. Agrega todo al PolicyStore                          │
│  4. DELEGA evaluación a AuthorizationEngine             │
│  5. Agrega cache/logging/metrics alrededor              │
└─────────────────────────────────────────────────────────┘
                           │
                           │ usa
                           ▼
┌─────────────────────────────────────────────────────────┐
│                   policies crate                         │
│  (Motor de evaluación Cedar - ÚNICA FUENTE DE VERDAD)   │
│                                                          │
│  • AuthorizationEngine                                   │
│  • PolicyStore                                           │
│  • AuthorizationRequest                                  │
│  • Cedar evaluation logic                                │
└─────────────────────────────────────────────────────────┘
                           │
                    ┌──────┴──────┐
                    │             │
                    ▼             ▼
          ┌─────────────┐  ┌──────────────────┐
          │  hodei-iam  │  │ hodei-organizations│
          │             │  │                    │
          │ • Entities  │  │ • OUs, Accounts   │
          │ • Actions   │  │ • SCPs            │
          └─────────────┘  └──────────────────┘
```

---

## Plan de Refactorización

### Fase 1: Análisis Completo ✅ (COMPLETADO)

- [x] Identificar qué hace cada crate
- [x] Encontrar duplicación
- [x] Revisar tests existentes
- [x] Documentar problema

### Fase 2: Refactorizar hodei-authorizer (SIGUIENTE)

#### 2.1. Definir la Responsabilidad Correcta

**hodei-authorizer debe SER**:
```rust
/// Orquestador que combina políticas de múltiples fuentes
/// y delega la evaluación al AuthorizationEngine de policies
pub struct AuthorizationOrchestrator {
    // Providers para recolectar políticas
    iam_policy_provider: Arc<dyn IamPolicyProvider>,
    org_policy_provider: Arc<dyn OrganizationPolicyProvider>,
    
    // El MOTOR REAL de evaluación (de policies crate)
    authorization_engine: Arc<AuthorizationEngine>,
    
    // Capas adicionales (no reemplazan el engine)
    cache: Option<Arc<dyn AuthorizationCache>>,
    logger: Arc<dyn AuthorizationLogger>,
    metrics: Arc<dyn AuthorizationMetrics>,
}

impl AuthorizationOrchestrator {
    pub async fn evaluate_permissions(
        &self,
        request: AuthorizationRequest  // Del crate policies
    ) -> Result<Response, Error> {
        // 1. Check cache
        if let Some(cached) = self.check_cache(&request).await? {
            return Ok(cached);
        }
        
        // 2. Recolectar políticas IAM
        let iam_policies = self.iam_policy_provider
            .get_policies_for(&request.principal).await?;
        
        // 3. Recolectar SCPs organizacionales
        let scps = self.org_policy_provider
            .get_effective_scps(&request.context).await?;
        
        // 4. Agregar todas las políticas al store
        self.authorization_engine.store
            .add_policies(iam_policies).await?;
        self.authorization_engine.store
            .add_policies(scps).await?;
        
        // 5. DELEGAR evaluación al engine (NO reimplementar)
        let response = self.authorization_engine
            .is_authorized(&request).await;
        
        // 6. Cache, log, metrics
        self.cache_response(&request, &response).await?;
        self.log_decision(&request, &response).await?;
        self.record_metrics(&response).await?;
        
        Ok(response)
    }
}
```

#### 2.2. Eliminar Código Duplicado

**Eliminar**:
- ❌ `cedar_authorizer: Authorizer` en use_case.rs
- ❌ Lógica de evaluación Cedar directa
- ❌ DTOs duplicados (usar los de policies)

**Mantener**:
- ✅ Ports para providers (IamPolicyProvider, OrganizationBoundaryProvider)
- ✅ Cache, logging, metrics (son capas adicionales legítimas)
- ✅ Mocks para testing

#### 2.3. Implementar Providers Reales

**IamPolicyProvider**:
```rust
pub trait IamPolicyProvider: Send + Sync {
    /// Obtiene las políticas IAM para un principal
    async fn get_identity_policies_for(
        &self, 
        principal_hrn: &Hrn
    ) -> Result<Vec<String>, Error>; // Retorna políticas Cedar como strings
}

// Implementación usando hodei-iam
pub struct HodeiIamPolicyProvider {
    user_repo: Arc<dyn UserRepository>,
    group_repo: Arc<dyn GroupRepository>,
    policy_repo: Arc<dyn PolicyRepository>,
}

impl IamPolicyProvider for HodeiIamPolicyProvider {
    async fn get_identity_policies_for(&self, principal_hrn: &Hrn) -> Result<Vec<String>, Error> {
        // 1. Obtener usuario de hodei-iam
        let user = self.user_repo.find_by_hrn(principal_hrn).await?;
        
        // 2. Obtener grupos del usuario
        let groups = self.group_repo.find_by_user(&user).await?;
        
        // 3. Obtener políticas adjuntas a usuario y grupos
        let mut policies = Vec::new();
        for group in groups {
            policies.extend(
                self.policy_repo.get_attached_policies(&group.hrn).await?
            );
        }
        
        Ok(policies)
    }
}
```

**OrganizationBoundaryProvider**:
```rust
pub trait OrganizationBoundaryProvider: Send + Sync {
    /// Obtiene SCPs efectivos para una cuenta/OU
    async fn get_effective_scps_for(
        &self,
        entity_hrn: &Hrn
    ) -> Result<Vec<String>, Error>; // Retorna documentos Cedar de SCPs
}

// Implementación usando hodei-organizations
pub struct HodeiOrganizationBoundaryProvider {
    account_repo: Arc<dyn AccountRepository>,
    ou_repo: Arc<dyn OuRepository>,
    scp_repo: Arc<dyn ScpRepository>,
}

impl OrganizationBoundaryProvider for HodeiOrganizationBoundaryProvider {
    async fn get_effective_scps_for(&self, entity_hrn: &Hrn) -> Result<Vec<String>, Error> {
        // Usar el use case get_effective_scps de hodei-organizations
        let scps = self.scp_repo.get_effective_scps_for(entity_hrn).await?;
        
        // Extraer los documentos Cedar
        Ok(scps.into_iter().map(|scp| scp.document).collect())
    }
}
```

### Fase 3: Actualizar Tests

#### 3.1. Tests Unitarios de hodei-authorizer

**Objetivo**: Verificar la orquestación, NO la evaluación Cedar

```rust
#[tokio::test]
async fn test_orchestrator_combines_iam_and_scp_policies() {
    // Arrange
    let iam_provider = MockIamPolicyProvider::new()
        .with_policy("user1", "permit(principal, action, resource);");
    
    let org_provider = MockOrgProvider::new()
        .with_scp("account1", "forbid(principal, action::DeleteUser, resource);");
    
    // El engine REAL de policies (no mock)
    let engine = AuthorizationEngine::builder()
        .register_principal::<User>()
        .register_resource::<User>()
        .register_action::<DeleteUserAction>()
        .build(storage)
        .unwrap();
    
    let orchestrator = AuthorizationOrchestrator::new(
        iam_provider,
        org_provider,
        engine,
        cache,
        logger,
        metrics
    );
    
    // Act: El orchestrator delega al engine
    let result = orchestrator.evaluate_permissions(request).await;
    
    // Assert: Verificamos que el SCP DENY prevalece
    assert!(result.is_ok());
    assert_eq!(result.unwrap().decision(), Decision::Deny);
}
```

#### 3.2. Tests de Integración E2E

**Objetivo**: Probar el flujo completo con todos los crates reales

```rust
// tests/integration/e2e_authorization_test.rs

#[tokio::test]
async fn test_complete_authorization_flow() {
    // 1. Setup: Usar implementaciones REALES de todos los crates
    let db = setup_surrealdb().await;
    
    // 2. Crear estructura organizacional (hodei-organizations)
    let ou_use_case = create_ou::use_case(&db);
    let account_use_case = create_account::use_case(&db);
    let scp_use_case = create_scp::use_case(&db);
    
    // Crear OU y Account
    let ou = ou_use_case.execute(CreateOuCommand { ... }).await.unwrap();
    let account = account_use_case.execute(CreateAccountCommand { ... }).await.unwrap();
    
    // Crear y adjuntar SCP
    let scp = scp_use_case.execute(CreateScpCommand {
        document: "forbid(principal, action::DeleteUser, resource);"
    }).await.unwrap();
    
    attach_scp_use_case.execute(AttachScpCommand { 
        ou_hrn: ou.hrn, 
        scp_hrn: scp.hrn 
    }).await.unwrap();
    
    // 3. Crear usuario IAM (hodei-iam)
    let user_use_case = create_user::use_case(&db);
    let user = user_use_case.execute(CreateUserCommand { ... }).await.unwrap();
    
    // 4. Setup AuthorizationEngine (policies)
    let engine = build_engine_with_iam_entities(&db).await;
    
    // 5. Setup Orchestrator (hodei-authorizer)
    let iam_provider = SurrealIamPolicyProvider::new(db.clone());
    let org_provider = SurrealOrgBoundaryProvider::new(db.clone());
    
    let orchestrator = AuthorizationOrchestrator::new(
        iam_provider,
        org_provider,
        engine,
        None,  // No cache for test
        logger,
        metrics
    );
    
    // 6. Ejecutar evaluación
    let request = AuthorizationRequest {
        principal: &user,
        action: DeleteUserAction::euid(),
        resource: &target_user,
        context: account_context,
        entities: vec![&user, &target_user]
    };
    
    let response = orchestrator.evaluate_permissions(request).await.unwrap();
    
    // 7. Verificar: SCP debe negar incluso si IAM permite
    assert_eq!(response.decision(), Decision::Deny);
}
```

### Fase 4: Documentación y Validación

- [ ] Actualizar README de hodei-authorizer explicando su rol
- [ ] Documentar flujo de integración entre crates
- [ ] Crear diagramas de secuencia
- [ ] Validar que no hay duplicación
- [ ] Ejecutar todos los tests: `cargo test --workspace`

---

## Criterios de Éxito

### ✅ Arquitectura Limpia
- [ ] hodei-authorizer NO tiene lógica de evaluación Cedar
- [ ] hodei-authorizer DELEGA a AuthorizationEngine
- [ ] No hay código duplicado entre crates
- [ ] Cada crate tiene responsabilidad clara

### ✅ Tests Pasando
- [ ] Todos los tests unitarios de policies: ✅
- [ ] Todos los tests unitarios de hodei-iam: ✅
- [ ] Todos los tests unitarios de hodei-organizations: ✅
- [ ] Todos los tests unitarios de hodei-authorizer: ⚠️ (16/26 pasando)
- [ ] Tests de integración E2E: ⚠️ (por implementar)

### ✅ Cobertura Completa
- [ ] Tests unitarios de orquestación (sin tocar Cedar directamente)
- [ ] Tests de integración con todas las piezas reales
- [ ] Tests de casos edge y límites
- [ ] Documentación de integración

---

## Próximos Pasos Inmediatos

1. **STOP**: No agregar más código hasta refactorizar hodei-authorizer
2. **Refactorizar**: Eliminar duplicación, usar AuthorizationEngine
3. **Implementar Providers**: Conectar realmente con hodei-iam y hodei-organizations
4. **Actualizar Tests**: Que prueben orquestación, no evaluación
5. **Tests E2E**: Probar flujo completo con implementaciones reales

---

## Notas Importantes

- ⚠️ **NO reimplementar**: Si policies ya lo hace, úsalo
- ✅ **Delegation**: hodei-authorizer orquesta, no evalúa
- 🎯 **Single Source of Truth**: AuthorizationEngine es el único evaluador
- 🧩 **Composición**: Los crates se integran, no se duplican