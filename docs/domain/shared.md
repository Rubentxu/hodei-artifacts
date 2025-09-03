# Especificación Completa del Modelo de Datos: Crate `shared` (Núcleo Compartido)

**Versión:** 6.0
**Crate:** `crates/shared`

### 1\. Propósito y Principios de Diseño

El crate `shared` es la base fundamental del modelo de dominio de Hodei. No contiene lógica de negocio, sino las definiciones inmutables y universales que son utilizadas por múltiples contextos. Sus principios son:

* **Universalidad**: Los tipos definidos aquí son relevantes para más de un Bounded Context.
* **Inmutabilidad**: La mayoría de los tipos son Value Objects inmutables.
* **Cero Dependencias**: No depende de ningún otro crate de dominio del proyecto (`artifact`, `repository`, etc.).

-----

### 2\. Definiciones Completas en `rust`

A continuación se presentan las definiciones completas del código que compondrían los diferentes módulos dentro de `crates/shared/src/`.

#### 2.1. Módulo de Identificación: `hrn.rs`

**Responsabilidad**: Definir el sistema de identificación canónico (HRN) y los tipos de ID fuertemente tipados.

```rust
// crates/shared/src/hrn.rs

use serde::{Serialize, Deserialize};

/// Un HRN (Hodei Resource Name) validado, modelado a partir de los ARN de AWS.
/// Es el identificador canónico, único y global para cualquier recurso en Hodei.
/// El campo interno es privado para forzar la creación a través de constructores que garantizan la validez.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Hrn(String);

impl Hrn {
    /// Construye un nuevo HRN, validando su formato de 6 partes.
    /// La validación asegura que la estructura `hrn:<partition>:<service>:<region>:<org_id>:<path>` es correcta.
    pub fn new(input: &str) -> Result<Self, HrnError> {
        // ... Lógica de validación estricta del formato ...
        Ok(Self(input.to_string()))
    }

    /// Devuelve el HRN como un string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
    
    // ... Métodos de acceso para los componentes del HRN (ej. `organization_id`, `service`, etc.) ...
}

// --- Tipos de ID fuertemente tipados para seguridad de tipos en todo el sistema ---

/// Identificador para una `Organization`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OrganizationId(pub Hrn);

/// Identificador para un `Repository`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RepositoryId(pub Hrn);

/// Identificador para una `PackageVersion`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackageVersionId(pub Hrn);

/// Identificador para un `PhysicalArtifact`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PhysicalArtifactId(pub Hrn);

/// Identificador para un `User`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(pub Hrn);

/// Identificador para una `ApiKey`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ApiKeyId(pub Hrn);

/// Identificador para una `Policy` o `RetentionPolicy`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PolicyId(pub Hrn);

/// Identificador para una `PublicKey` usada en firmas.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PublicKeyId(pub Hrn);

/// Identificador para una `Attestation`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AttestationId(pub Hrn);

/// Identificador para un `SecurityScanResult`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScanResultId(pub Hrn);

/// Identificador para una `VulnerabilityDefinition`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VulnerabilityDefinitionId(pub Hrn);

/// Identificador para una `VulnerabilityOccurrence`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VulnerabilityOccurrenceId(pub Hrn);
```

#### 2.2. Módulo de Dominio Común: `models.rs`

**Responsabilidad**: Definir Value Objects que representan conceptos de dominio transversales.

```rust
// crates/shared/src/models.rs

use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use crate::hrn::{Hrn, PhysicalArtifactId};
use crate::enums::{HashAlgorithm, ArtifactType, ArtifactRole};

/// El hash criptográfico del contenido de un fichero físico. Es inmutable.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentHash {
    /// El algoritmo utilizado para generar el hash (ej. Sha256).
    pub algorithm: HashAlgorithm,
    /// El valor del hash en formato hexadecimal.
    pub value: String,
}

/// Coordenadas universales que identifican un paquete en cualquier ecosistema.
/// No contiene el ecosistema, ya que este se infiere del `Repository` contenedor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageCoordinates {
    /// El espacio de nombres del paquete (ej. `@scope` en npm, `groupId` en Maven).
    pub namespace: Option<String>,
    /// El nombre del paquete (ej. `react`, `log4j-core`).
    pub name: String,
    /// La versión del paquete (ej. "18.2.0", "2.17.1").
    pub version: String,
    /// Pares clave-valor para metadatos específicos del ecosistema que son necesarios para la identificación
    /// (ej. `classifier="sources"` en Maven, `os="linux"` en OCI).
    pub qualifiers: HashMap<String, String>,
}

/// Una referencia tipada desde una `PackageVersion` a un `PhysicalArtifact`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactReference {
    /// El HRN del `PhysicalArtifact` al que se refiere.
    pub artifact_hrn: PhysicalArtifactId,
    /// El tipo de fichero (binario principal, firma, SBOM, etc.).
    pub artifact_type: ArtifactType,
    /// El rol semántico del fichero dentro del paquete (ej. "sources", "javadoc").
    pub role: Option<ArtifactRole>,
}
```

#### 2.3. Módulo de Ciclo de Vida: `lifecycle.rs`

**Responsabilidad**: Definir un modelo unificado para la auditoría y gestión del ciclo de vida de los Agregados Raíz.

```rust
// crates/shared/src/lifecycle.rs

use serde::{Serialize, Deserialize};
use time::OffsetDateTime;
use crate::hrn::Hrn;

/// Representa el estado del ciclo de vida de un Agregado, unificado y sin ambigüedad.
/// Es una máquina de estados simple: Active -> Archived -> Deleted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LifecycleState {
    /// El recurso está activo y operativo.
    Active,
    /// El recurso está archivado. Generalmente es de solo lectura y puede ser restaurado.
    Archived { at: OffsetDateTime, by: Hrn },
    /// El recurso ha sido marcado para borrado o borrado lógicamente. Es irrecuperable.
    Deleted { at: OffsetDateTime, by: Hrn },
}

/// Un Value Object que contiene información completa y consistente del ciclo de vida de un Agregado.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lifecycle {
    /// Fecha y hora de creación del recurso.
    pub created_at: OffsetDateTime,
    /// HRN del principal (User o ApiKey) que creó el recurso.
    pub created_by: Hrn,
    /// Fecha y hora de la última modificación del recurso.
    pub updated_at: OffsetDateTime,
    /// HRN del principal que realizó la última modificación.
    pub updated_by: Hrn,
    /// El estado actual del recurso (Activo, Archivado o Borrado).
    pub state: LifecycleState,
}
```

#### 2.4. Módulo de Autorización: `security/`

**Responsabilidad**: Sistema completo de autorización basado en Cedar DSL con integración nativa.

```
crates/shared/src/security/
├── mod.rs              # Exporta todos los componentes
├── cedar_integration.rs # Conversión de entidades Hodei a Cedar
└── resources.rs        # Traits genéricos para recursos
```

#### 2.4.1. Traits Genéricos (`security/resources.rs`)

```rust
// crates/shared/src/security/resources.rs

use std::collections::HashMap;

/// Un trait para cualquier entidad de dominio que pueda ser representada
/// como un recurso o principal en un sistema de autorización.
/// Esta interfaz genérica permite flexibilidad en la implementación de infraestructura.
pub trait HodeiResource<IdType, AttrType> {
    /// Devuelve el identificador único de la entidad en formato adecuado para el sistema de autorización.
    fn resource_id(&self) -> IdType;

    /// Devuelve un mapa de los atributos de la entidad para evaluación de políticas.
    /// Los valores están en formato que puede interpretar el motor de autorización específico.
    fn resource_attributes(&self) -> HashMap<String, AttrType>;

    /// Devuelve una lista de identificadores de recursos padres para herencia de políticas.
    /// Permite la jerarquía de autorización con tipos específicos del motor de autorización.
    fn resource_parents(&self) -> Vec<IdType>;
}
```

#### 2.4.2. Módulo Principal (`security/mod.rs`)

```rust
// crates/shared/src/security/mod.rs

pub mod cedar_integration;
pub mod resources;

// Re-export para fácil acceso
pub use cedar_integration::{to_cedar_entity, CedarEntityConverter};
pub use resources::HodeiResource;

/// Ejemplo de implementación para Organization usando tipos Cedar
impl HodeiResource<cedar_policy::EntityUid, cedar_policy::Expr> for Organization {
    fn resource_id(&self) -> cedar_policy::EntityUid {
        cedar_policy::EntityUid::from_str(self.hrn.as_str()).unwrap()
    }

    fn resource_attributes(&self) -> HashMap<String, cedar_policy::Expr> {
        let mut attrs = HashMap::new();
        attrs.insert("type".to_string(), cedar_policy::Expr::val("organization"));
        attrs.insert("status".to_string(), cedar_policy::Expr::val(self.status.to_string()));
        attrs.insert("primary_region".to_string(), cedar_policy::Expr::val(self.primary_region.clone()));
        attrs
    }

    fn resource_parents(&self) -> Vec<cedar_policy::EntityUid> {
        Vec::new() // Organizaciones no tienen padres
    }
}
```

#### 2.4.3. Integración con Cedar DSL (`security/cedar_integration.rs`)

**Responsabilidad**: Convertir entidades Hodei a entidades Cedar para evaluación de políticas DSL.

```rust
// crates/shared/src/security/cedar_integration.rs

use cedar_policy::{Entity, EntityType, EntityUid, RestrictedExpression};
use crate::security::HodeiResource;

/// Convierte cualquier entidad Hodei a una entidad Cedar para evaluación de políticas
pub fn to_cedar_entity<R, IdType, AttrType>(
    resource: &R,
) -> Result<Entity, CedarConversionError>
where
    R: HodeiResource<IdType, AttrType>,
    IdType: Into<EntityUid>,
    AttrType: Into<RestrictedExpression>,
{
    let uid = resource.resource_id().into();
    let attrs = resource.resource_attributes()
        .into_iter()
        .map(|(k, v)| (k, v.into()))
        .collect();
    let parents = resource.resource_parents()
        .into_iter()
        .map(|p| p.into())
        .collect();
    
    Ok(Entity::new(uid, attrs, parents))
}

/// Conversor para transformar múltiples entidades Hodei al formato Cedar
pub struct CedarEntityConverter;

impl CedarEntityConverter {
    /// Convierte un conjunto de entidades para evaluación de políticas Cedar DSL
    pub fn convert_entities<R, IdType, AttrType>(
        entities: &[R],
    ) -> Result<Vec<Entity>, CedarConversionError>
    where
        R: HodeiResource<IdType, AttrType>,
        IdType: Into<EntityUid>,
        AttrType: Into<RestrictedExpression>,
    {
        entities.iter().map(to_cedar_entity).collect()
    }
}
```

#### 2.5. Módulo de Eventos: `events.rs`

**Responsabilidad**: Definir la estructura contenedora para todos los eventos de dominio que se publican en el bus de eventos.

```rust
// crates/shared/src/events.rs

// Nota: Los tipos concretos de eventos (OrganizationEvent, etc.) se definen en sus
// respectivos crates para mantener la cohesión del Bounded Context.
// Este enum actúa como un contenedor universal para el transporte en Kafka.
use crate::organization::OrganizationEvent;
use crate::repository::RepositoryEvent;
use crate::artifact::ArtifactEvent;
// ... etc.

/// Enumeración de todos los eventos de dominio que pueden fluir por el bus de eventos.
/// Actúa como un sobre que contiene el evento específico de su dominio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DomainEvent {
    Organization(OrganizationEvent),
    Repository(RepositoryEvent),
    Artifact(ArtifactEvent),
    Iam(IamEvent),
    Security(SecurityEvent),
    SupplyChain(SupplyChainEvent),
}
```

#### 2.6. Módulo de Enumeraciones: `enums.rs`

**Responsabilidad**: Centralizar todas las enumeraciones que son utilizadas por más de un Bounded Context.

```rust
// crates/shared/src/enums.rs

use serde::{Serialize, Deserialize};

/// Ecosistemas de paquetes soportados por el sistema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Ecosystem {
    Maven, Npm, Docker, Oci, Pypi, Nuget, Go, RubyGems, Helm, Generic,
}

/// Algoritmos de hash soportados para la verificación de integridad.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HashAlgorithm {
    Sha256, Sha512, Md5,
}

/// Niveles de severidad de vulnerabilidades, basados en estándares como CVSS.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Ord, PartialOrd, PartialEq, Eq)]
pub enum VulnerabilitySeverity {
    Critical, High, Medium, Low, Info, Unknown,
}

/// El tipo de un fichero físico dentro de un paquete.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ArtifactType {
    Primary, Signature, Sbom, Metadata, Documentation, Sources, Other,
}

/// El rol semántico que un fichero físico juega dentro de un `PackageVersion`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ArtifactRole {
    Main, Pom, PackageJson, Sources, Javadoc, TypeDefinitions, Signature, Sbom, Other,
}
```