# Historia 4: Algoritmo de Resolución de SCPs Efectivas

## 1. Resumen

Este documento especifica el algoritmo utilizado por `SurrealOrganizationBoundaryProvider` para resolver las Service Control Policies (SCPs) efectivas de un recurso (Account o OU) en la jerarquía organizacional.

## 2. Propósito

Desacoplar la infraestructura (`SurrealOrganizationBoundaryProvider`) de la capa de aplicación (`GetEffectiveScpsUseCase`) implementando directamente la lógica de negocio usando repositorios.

## 3. Entrada y Salida

### Entrada
- `resource_hrn: &Hrn` - HRN del recurso objetivo (puede ser Account o OrganizationalUnit)

### Salida
- `Result<PolicySet, EvaluatePermissionsError>` - PolicySet de Cedar con todas las políticas efectivas

### Errores Posibles
- `InvalidTargetType` - HRN no es Account ni OU
- `TargetNotFound` - Recurso no existe en repositorio
- `OrganizationBoundaryProvider` - Errores de repositorio o parsing
- `CycleDetected` - Ciclo en jerarquía de OUs (protección)

## 4. Algoritmo Detallado

### 4.1. Clasificación del Recurso

```
function classify_resource(hrn: &Hrn) -> ResourceType:
    match hrn.resource_type:
        "Account" | "account" -> ResourceType::Account
        "OrganizationalUnit" | "ou" -> ResourceType::OU
        _ -> Error(InvalidTargetType)
```

### 4.2. Resolución del Punto de Entrada

#### Para Account:
```
function resolve_from_account(hrn: &Hrn) -> Result<(HashSet<Hrn>, Option<Hrn>)>:
    account = account_repository.find_by_hrn(hrn)?
    if account is None:
        return Error(TargetNotFound)
    
    scps = account.attached_scps.clone()
    parent_ou = account.parent_hrn.clone()
    
    return Ok((scps, parent_ou))
```

#### Para OU:
```
function resolve_from_ou(hrn: &Hrn) -> Result<(HashSet<Hrn>, Hrn)>:
    ou = ou_repository.find_by_hrn(hrn)?
    if ou is None:
        return Error(TargetNotFound)
    
    scps = ou.attached_scps.clone()
    
    return Ok((scps, hrn))
```

### 4.3. Recorrido Ascendente de la Jerarquía

```
function collect_scps_from_hierarchy(start_ou_hrn: Option<Hrn>) -> Result<HashSet<Hrn>>:
    accumulated_scps = HashSet::new()
    visited = HashSet::new()
    current_ou_hrn = start_ou_hrn
    
    while current_ou_hrn is Some:
        if visited.contains(current_ou_hrn):
            return Error(CycleDetected)
        
        visited.insert(current_ou_hrn.clone())
        
        ou = ou_repository.find_by_hrn(current_ou_hrn)?
        if ou is None:
            // OU referenciada no existe, detener recorrido
            break
        
        // Acumular SCPs de este nivel
        accumulated_scps.extend(ou.attached_scps)
        
        // Detectar raíz (asumimos que raíz no tiene parent_hrn válido o apunta a sí mismo)
        if ou.parent_hrn == current_ou_hrn:
            // Convención: raíz apunta a sí misma
            break
        
        // Intentar cargar padre
        parent_ou = ou_repository.find_by_hrn(&ou.parent_hrn)?
        if parent_ou is None:
            // Padre no existe, asumimos raíz alcanzada
            break
        
        current_ou_hrn = Some(ou.parent_hrn)
    
    return Ok(accumulated_scps)
```

### 4.4. Carga y Parsing de Políticas

```
function load_policy_set(scp_hrns: HashSet<Hrn>) -> Result<PolicySet>:
    policy_set = PolicySet::new()
    
    // Ordenar HRNs para determinismo en tests
    sorted_hrns = scp_hrns.into_iter().sorted()
    
    for scp_hrn in sorted_hrns:
        scp = scp_repository.find_by_hrn(&scp_hrn)?
        
        if scp is None:
            warn!("SCP referenced but not found: {}", scp_hrn)
            continue
        
        match Policy::from_str(&scp.document):
            Ok(policy) -> 
                policy_set.add(policy)
            Err(e) ->
                warn!("Failed to parse SCP {}: {}", scp_hrn, e)
                // Continuar con siguientes políticas
    
    return Ok(policy_set)
```

### 4.5. Algoritmo Principal

```
function get_effective_scps_for(resource_hrn: &Hrn) -> Result<PolicySet>:
    // Paso 1: Clasificar recurso
    resource_type = classify_resource(resource_hrn)?
    
    // Paso 2: Resolver punto de entrada
    (initial_scps, start_ou_hrn) = match resource_type:
        Account -> resolve_from_account(resource_hrn)?
        OU -> resolve_from_ou(resource_hrn)?
    
    // Paso 3: Acumular SCPs directos
    accumulated_scps = initial_scps
    
    // Paso 4: Recorrer jerarquía si hay OU padre
    if start_ou_hrn is Some:
        hierarchy_scps = collect_scps_from_hierarchy(start_ou_hrn)?
        accumulated_scps.extend(hierarchy_scps)
    
    // Paso 5: Cargar y parsear políticas
    policy_set = load_policy_set(accumulated_scps)?
    
    // Paso 6: Logging y retorno
    info!("Resolved {} effective SCPs for {}", policy_set.len(), resource_hrn)
    return Ok(policy_set)
```

## 5. Propiedades del Algoritmo

### 5.1. Complejidad
- **Tiempo**: O(H + S) donde:
  - H = altura de la jerarquía de OUs (típicamente < 10)
  - S = número total de SCPs acumulados (típicamente < 100)
- **Espacio**: O(H + S) para sets de visitados y SCPs acumulados

### 5.2. Invariantes
1. No se visita una OU más de una vez (protección contra ciclos)
2. SCPs se acumulan sin duplicados (uso de HashSet)
3. El orden de parsing es determinista (sort de HRNs)
4. Políticas malformadas no abortan la operación completa

### 5.3. Robustez
- **Tolerancia a errores**: Políticas individuales malformadas se ignoran con warning
- **Protección contra ciclos**: Set de visitados previene loops infinitos
- **Manejo de jerarquías incompletas**: Si OU padre no existe, se asume raíz alcanzada

## 6. Casos de Uso y Ejemplos

### 6.1. Account con Jerarquía Simple
```
Account(acc-123) 
  parent: OU(ou-456) [SCP-1]
    parent: Root [SCP-2, SCP-3]

Resultado: PolicySet { SCP-1, SCP-2, SCP-3 }
```

### 6.2. Account sin Padre
```
Account(acc-orphan) [SCP-1]
  parent: None

Resultado: PolicySet { SCP-1 }
```

### 6.3. OU Profunda
```
OU(ou-123) [SCP-A]
  parent: OU(ou-456) [SCP-B]
    parent: OU(ou-789) [SCP-C]
      parent: Root [SCP-D]

Resultado: PolicySet { SCP-A, SCP-B, SCP-C, SCP-D }
```

### 6.4. OU sin SCPs
```
OU(ou-empty)
  parent: Root [SCP-1]

Resultado: PolicySet { SCP-1 }
```

## 7. Logging y Observabilidad

### Spans
- `organization_boundary.get_effective_scps` - Span principal
  - Attributes: `resource_hrn`, `resource_type`

### Eventos
- `info`: "Starting SCP resolution for {hrn}"
- `info`: "Resolved {count} effective SCPs"
- `warn`: "SCP referenced but not found: {hrn}"
- `warn`: "Failed to parse SCP policy: {error}"
- `error`: "Cycle detected in OU hierarchy at {hrn}"
- `error`: "Target resource not found: {hrn}"

## 8. Diferencias con GetEffectiveScpsUseCase

| Aspecto | Use Case (Feature) | Provider (Infrastructure) |
|---------|-------------------|---------------------------|
| **Propósito** | Lógica de negocio pública para API | Adaptador interno para autorización |
| **Retorno** | `EffectiveScpsResponse` (con metadata) | `PolicySet` directo |
| **Dependencias** | Ports de feature | Repositorios de aplicación |
| **Testing** | Mocks de ports | Mocks de repositorios |
| **Logging** | Span feature-level | Span infrastructure-level |

## 9. Testing

### 9.1. Tests Unitarios (con Mocks)
- Account con herencia de 1 nivel
- Account con herencia de 4+ niveles
- Account sin padre
- OU sin SCPs
- SCP malformado (ignora y continúa)
- Ciclo detectado (error)
- SCP faltante (warning y continúa)

### 9.2. Tests de Integración (con SurrealDB)
- Jerarquía completa real
- Performance con jerarquía profunda (10+ niveles)
- Concurrencia (múltiples resoluciones paralelas)

## 10. Criterios de Éxito

- [ ] Algoritmo implementado sin dependencias de features
- [ ] Tests unitarios con cobertura > 90%
- [ ] No warnings de compilador
- [ ] Performance: < 50ms para jerarquía de 10 niveles (local)
- [ ] Logging completo y estructurado
- [ ] Documentación actualizada

---

**Versión**: 1.0  
**Fecha**: 2024  
**Autor**: Hodei Artifacts Team