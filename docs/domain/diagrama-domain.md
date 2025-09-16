# Diagrama de modelo de dominio

```mermaid
classDiagram
    class HodeiResource {
        <<trait>>
        +resource_id() IdType
        +resource_attributes() Map~String, AttributeValue~
        +resource_parents() List~IdType~
    }

    class AttributeValue {
        <<value-object>>
        String
        Long
        Boolean
        Set~AttributeValue~
        Record~Map~String, AttributeValue~~
    }
    
    class Hrn {
        +value: String
        +new(s: &str) Result~Hrn, HrnError~
        +as_str() &str
        +organization_name() Option~&str~
        +resource_id() Option~&str~
    }
    
    class HrnError {
        <<enumeration>>
        InvalidFormat
        InvalidResourceName(String)
        InvalidOrganizationName(String)
    }
    
    class OrganizationId {
        +hrn: Hrn
        +new(name: &str) Result~OrganizationId, HrnError~
        +as_str() &str
    }
    
    class RepositoryId {
        +hrn: Hrn
        +new(org_id: &str, name: &str) Result~RepositoryId, HrnError~
        +as_str() &str
    }
    
    class PhysicalArtifactId {
        +hrn: Hrn
        +new(hash: &str) Result~PhysicalArtifactId, HrnError~
        +as_str() &str
    }
    
    class UserId {
        +hrn: Hrn
        +new(org_id: &OrganizationId, username: &str) Result~UserId, HrnError~
        +new_system_user() UserId
        +as_str() &str
        +is_system_user() bool
    }
    
    class ArtifactId {
        +hrn: Hrn
        +new(repo_id: &RepositoryId, coordinates: &ArtifactCoordinates) Result~ArtifactId, HrnError~
        +as_str() &str
    }
    
    class PolicyId {
        +hrn: Hrn
        +new(org_id: &OrganizationId, name: &str) Result~PolicyId, HrnError~
        +as_str() &str
    }
    
    class TeamId {
        +hrn: Hrn
        +new(org_id: &OrganizationId, name: &str) Result~TeamId, HrnError~
        +as_str() &str
    }
    
    class AttestationId {
        +hrn: Hrn
        +new(artifact_id: &PhysicalArtifactId, type: &str) Result~AttestationId, HrnError~
        +as_str() &str
    }
    
    class ScanResultId {
        +hrn: Hrn
        +new(artifact_id: &PhysicalArtifactId, scanner: &str) Result~ScanResultId, HrnError~
        +as_str() &str
    }
    
    class EventStreamId {
        +hrn: Hrn
        +new(org_id: &OrganizationId, name: &str) Result~EventStreamId, HrnError~
        +as_str() &str
    }
    
    class DashboardId {
        +hrn: Hrn
        +new(org_id: &OrganizationId, name: &str) Result~DashboardId, HrnError~
        +as_str() &str
    }
    
    class StorageBucketId {
        +hrn: Hrn
        +new(org_id: &OrganizationId, name: &str) Result~StorageBucketId, HrnError~
        +as_str() &str
    }
    
    class MonitorId {
        +hrn: Hrn
        +new(org_id: &OrganizationId, name: &str) Result~MonitorId, HrnError~
        +as_str() &str
    }
    
    class Organization {
        +id: OrganizationId
        +name: String
        +domain: String
        +active: Boolean
        +settings: Map~String, String~
        +createdAt: Instant
        +updatedAt: Instant
    }
    
    class Repository {
        +id: RepositoryId
        +name: String
        +organization: OrganizationId
        +visibility: String
        +createdAt: Instant
        +updatedAt: Instant
        +repositoryType: RepositoryType
        +artifactType: ArtifactType
        +deploymentPolicy: DeploymentPolicy
        +cleanupPolicy: CleanupPolicy
    }
    
    class PhysicalArtifact {
        +id: PhysicalArtifactId
        +contentHash: ContentHash
        +size: Long
        +contentType: String
        +createdAt: Instant
        +sbom: Sbom?
        +provenance: Provenance?
        +signature: Signature?
        +merkleRoot: String
    }
    
    class User {
        +id: UserId
        +name: String
        +email: String
        +role: String
        +department: String?
        +active: Boolean
        +createdAt: Instant
        +lastLogin: Instant?
    }
    
    class ServiceAccount {
        +id: UserId
        +name: String
        +description: String?
        +permissions: Set~String~
        +active: Boolean
        +createdAt: Instant
    }
    
    class ApiKey {
        +id: Hrn
        +name: String
        +user: UserId
        +permissions: Set~String~
        +expiresAt: Instant?
        +active: Boolean
        +createdAt: Instant
        +lastUsed: Instant?
    }
    
    class Group {
        +id: Hrn
        +name: String
        +organization: OrganizationId
        +members: Set~UserId~
        +createdAt: Instant
        +updatedAt: Instant
    }
    
    class Policy {
        +id: PolicyId
        +name: String
        +description: String?
        +status: String
        +version: Long
        +createdAt: Instant
        +updatedAt: Instant
        +currentVersion: PolicyVersion
    }
    
    class PolicyVersion {
        +id: Hrn
        +policyId: PolicyId
        +version: Long
        +content: String
        +createdAt: Instant
        +createdBy: UserId
    }
    
    class PolicyDecision {
        +allowed: Boolean
        +reasons: List~String~
        +obligations: List~Obligation~
        +advice: List~Advice~
    }
    
    class Obligation {
        <<interface>>
        +enforce(context: PolicyEvaluationContext)
    }
    
    class AuditObligation {
        +logEvent: String
    }
    
    class Team {
        +id: TeamId
        +name: String
        +organization: OrganizationId
        +description: String?
        +members: Set~UserId~
        +createdAt: Instant
        +updatedAt: Instant
    }
    
    class Membership {
        +id: Hrn
        +user: UserId
        +organization: OrganizationId
        +team: TeamId?
        +role: String
        +active: Boolean
        +createdAt: Instant
        +updatedAt: Instant
    }
    
    class OrganizationSettings {
        +id: Hrn
        +organization: OrganizationId
        +settings: Map~String, String~
        +updatedAt: Instant
        +updatedBy: UserId
    }
    
    class CleanupPolicy {
        +maxVersionsToKeep: Int
        +retainSnapshotsForDays: Int
        +lastDownloadedThresholdDays: Int
        +cleanupCronExpression: String
    }
    
    class DeploymentPolicy {
        <<enumeration>>
        ALLOW_REDEPLOY_STABLE
        ALLOW_REDEPLOY_SNAPSHOT
        DISABLE_REDEPLOY
        READ_ONLY
    }
    
    class RepositoryType {
        <<enumeration>>
        HOSTED
        PROXY
        GROUP
    }
    
    class ArtifactType {
        <<enumeration>>
        MAVEN
        NPM
        DOCKER
        PYPI
        NUGET
        GENERIC
        HELM
        CONAN
        RUBYGEMS
        COMPOSER
        CRATES
        GO
    }
    
    class Artifact {
        +id: ArtifactId
        +repositoryId: RepositoryId
        +coordinates: ArtifactCoordinates
        +metadata: ArtifactMetadata
        +uploaderUserId: UserId
        +artifactType: ArtifactType
        +physicalArtifactId: PhysicalArtifactId
        +createdAt: Instant
    }
    
    class ArtifactCoordinates {
        +name: String
        +version: String
        +qualifier: String
    }
    
    class ArtifactMetadata {
        <<abstract>>
        +description: String
        +tags: List~String~
        +licenses: List~String~
        +dependencies: List~ArtifactDependency~
        +customProperties: Map~String, String~
    }
    
    class MavenArtifactMetadata {
        +groupId: String
        +artifactId: String
        +packaging: String
        +parent: MavenParent?
        +developers: List~MavenDeveloper~
    }
    
    class NpmArtifactMetadata {
        +name: String
        +scope: String?
        +scripts: Map~String, String~
        +engines: Map~String, String~
        +peerDependencies: List~ArtifactDependency~
    }
    
    class DockerArtifactMetadata {
        +imageId: String
        +layers: List~String~
        +manifest: String
        +architecture: String
    }
    
    class ArtifactDependency {
        +name: String
        +versionConstraint: String
        +scope: String
        +optional: Boolean
        +transitive: Boolean
    }
    
    class ContentHash {
        +value: String
        +algorithm: String
    }
    
    class Sbom {
        +format: SbomFormat
        +content: String
        +generatedAt: Instant
        +generator: String
    }
    
    class SbomFormat {
        <<enumeration>>
        SPDX_JSON
        SPDX_YAML
        CYCLONEDX_JSON
        CYCLONEDX_XML
    }
    
    class Provenance {
        +builderId: String
        +buildInvocationId: String
        +materials: List~Material~
        +recipe: Recipe
    }
    
    class Material {
        +uri: String
        +digest: Map~String, String~
    }
    
    class Recipe {
        +type: String
        +definedInMaterial: Int
        +entryPoint: String
        +arguments: List~String~
        +environment: Map~String, String~
    }
    
    class Signature {
        +signature: String
        +keyId: String
        +algorithm: String
        +signedAt: Instant
        +certificate: String?
    }
    
    class PublicKey {
        +id: Hrn
        +keyId: String
        +algorithm: String
        +publicKey: String
        +active: Boolean
        +createdAt: Instant
    }
    
    class ScanResult {
        +id: ScanResultId
        +artifact: PhysicalArtifactId
        +scanner: String
        +results: String
        +scannedAt: Instant
    }
    
    class VulnerabilityDefinition {
        +id: Hrn
        +cveId: String
        +severity: Severity
        +description: String
        +publishedAt: Instant
    }
    
    class VulnerabilityOccurrence {
        +id: Hrn
        +artifact: PhysicalArtifactId
        +vulnerability: Hrn
        +detectedAt: Instant
    }
    
    class Attestation {
        +id: AttestationId
        +artifact: PhysicalArtifactId
        +type: String
        +signature: String
        +createdAt: Instant
    }
    
    class DependencyNode {
        +artifactId: ArtifactId
        +version: String
        +dependencies: List~DependencyNode~
        +vulnerabilities: List~VulnerabilityOccurrence~
    }
    
    class Event {
        +id: Hrn
        +type: String
        +source: String
        +timestamp: Instant
        +data: Map~String, String~
    }
    
    class EventStream {
        +id: EventStreamId
        +name: String
        +organization: OrganizationId
        +filters: Set~String~
        +createdAt: Instant
        +updatedAt: Instant
    }
    
    class EventSubscription {
        +id: Hrn
        +stream: EventStreamId
        +subscriber: String
        +active: Boolean
        +createdAt: Instant
        +updatedAt: Instant
    }
    
    class Metric {
        +id: Hrn
        +name: String
        +type: String
        +value: String
        +timestamp: Instant
    }
    
    class Dashboard {
        +id: DashboardId
        +name: String
        +organization: OrganizationId
        +widgets: Set~String~
        +public: Boolean
        +createdAt: Instant
        +updatedAt: Instant
    }
    
    class Report {
        +id: Hrn
        +name: String
        +type: String
        +data: String
        +generatedAt: Instant
        +generatedBy: UserId
    }
    
    class Alert {
        +id: Hrn
        +name: String
        +condition: String
        +severity: Severity
        +active: Boolean
        +createdAt: Instant
        +updatedAt: Instant
    }
    
    class Severity {
        <<enumeration>>
        CRITICAL
        HIGH
        MEDIUM
        LOW
        UNKNOWN
    }
    
    class StorageBackend {
        +id: Hrn
        +name: String
        +type: String
        +configuration: String
        +active: Boolean
        +createdAt: Instant
        +updatedAt: Instant
    }
    
    class StorageBucket {
        +id: StorageBucketId
        +name: String
        +backend: Hrn
        +organization: OrganizationId
        +public: Boolean
        +createdAt: Instant
    }
    
    class StoragePolicy {
        +id: Hrn
        +name: String
        +rules: String
        +active: Boolean
        +createdAt: Instant
        +updatedAt: Instant
    }
    
    class Monitor {
        +id: MonitorId
        +name: String
        +organization: OrganizationId
        +target: String
        +checks: Set~String~
        +active: Boolean
        +createdAt: Instant
        +updatedAt: Instant
    }
    
    class HealthCheck {
        +id: Hrn
        +name: String
        +monitor: MonitorId
        +endpoint: String
        +interval: Long
        +active: Boolean
        +createdAt: Instant
        +updatedAt: Instant
    }
    
    class LogStream {
        +id: Hrn
        +name: String
        +source: String
        +filters: Set~String~
        +createdAt: Instant
    }
    
    class ArtifactService {
        +uploadArtifact(command: UploadArtifactCommand) Result~Artifact~
        +downloadArtifact(artifactId: ArtifactId, downloadedBy: UserId, clientIp: String, userAgent: String) Result~Artifact~
        +retrieveArtifactContent(repositoryId: RepositoryId, contentHash: ContentHash) Result~ByteArray~
    }
    
    class UploadArtifactCommand {
        +repositoryId: RepositoryId
        +filename: String
        +artifactType: ArtifactType
        +uploaderUserId: UserId
        +providedUserMeta Map~String, String~
        +content: ByteArray
    }
    
    class PolicyEvaluationContext {
        +principal: UserId
        +action: Action
        +resource: HodeiResource
        +time: Instant
        +additionalAttributes: Map~String, serde_json::Value~
    }
    
    class Action {
        <<enumeration>>
        CREATE_USER
        READ_USER
        UPDATE_USER
        DELETE_USER
        MANAGE_POLICIES
        MANAGE_API_KEYS
        CREATE_ORGANIZATION
        MANAGE_ORGANIZATION
        MANAGE_TEAMS
        MANAGE_MEMBERSHIPS
        CREATE_REPOSITORY
        READ_REPOSITORY
        WRITE_REPOSITORY
        DELETE_REPOSITORY
        READ_ARTIFACT
        WRITE_ARTIFACT
        DELETE_ARTIFACT
        MANAGE_METADATA
        READ_CONFIGURATION
        WRITE_CONFIGURATION
        MANAGE_CONFIG_VERSIONS
        PUBLISH_EVENT
        READ_EVENT
        MANAGE_EVENT_STREAMS
        MANAGE_SUBSCRIPTIONS
        READ_METRICS
        CREATE_DASHBOARD
        MANAGE_DASHBOARDS
        GENERATE_REPORTS
        MANAGE_ALERTS
        CREATE_ATTESTATION
        READ_ATTESTATION
        MANAGE_KEYS
        RUN_SCANS
        READ_SCAN_RESULTS
        MANAGE_PROVENANCE
        MANAGE_STORAGE
        READ_STORAGE
        WRITE_STORAGE
        CREATE_MONITOR
        READ_MONITOR
        MANAGE_MONITORS
        READ_LOGS
        ADMIN_ACCESS
        SYSTEM_ACCESS
    }
    
    class PolicyEvaluator {
        +evaluate(context: PolicyEvaluationContext) PolicyDecision
    }
    
    HodeiResource <|.. Organization
    HodeiResource <|.. Repository
    HodeiResource <|.. PhysicalArtifact
    HodeiResource <|.. User
    HodeiResource <|.. ServiceAccount
    HodeiResource <|.. ApiKey
    HodeiResource <|.. Group
    HodeiResource <|.. Policy
    HodeiResource <|.. Team
    HodeiResource <|.. Membership
    HodeiResource <|.. OrganizationSettings
    HodeiResource <|.. Artifact
    HodeiResource <|.. Sbom
    HodeiResource <|.. Provenance
    HodeiResource <|.. Signature
    HodeiResource <|.. PublicKey
    HodeiResource <|.. ScanResult
    HodeiResource <|.. VulnerabilityDefinition
    HodeiResource <|.. VulnerabilityOccurrence
    HodeiResource <|.. Attestation
    HodeiResource <|.. DependencyNode
    HodeiResource <|.. Event
    HodeiResource <|.. EventStream
    HodeiResource <|.. EventSubscription
    HodeiResource <|.. Metric
    HodeiResource <|.. Dashboard
    HodeiResource <|.. Report
    HodeiResource <|.. Alert
    HodeiResource <|.. StorageBackend
    HodeiResource <|.. StorageBucket
    HodeiResource <|.. StoragePolicy
    HodeiResource <|.. Monitor
    HodeiResource <|.. HealthCheck
    HodeiResource <|.. LogStream
    
    Organization "1" *-- "1" OrganizationId
    Repository "1" *-- "1" RepositoryId
    Repository "1" -- "1" OrganizationId : organization
    PhysicalArtifact "1" *-- "1" PhysicalArtifactId
    PhysicalArtifact "1" -- "1" ContentHash
    PhysicalArtifact "1" *-- "*" Attestation
    PhysicalArtifact "1" *-- "*" ScanResult
    PhysicalArtifact "1" *-- "*" VulnerabilityOccurrence
    User "1" *-- "1" UserId
    ServiceAccount "1" *-- "1" UserId
    ApiKey "1" *-- "1" Hrn
    Group "1" *-- "1" Hrn
    Policy "1" *-- "1" PolicyId
    Policy "1" -- "1" PolicyVersion : currentVersion
    Policy "1" *-- "*" PolicyVersion
    Team "1" *-- "1" TeamId
    Team "1" -- "1" OrganizationId : organization
    Membership "1" *-- "1" Hrn
    Membership "1" -- "1" UserId : user
    Membership "1" -- "1" OrganizationId : organization
    Membership "1" -- "1" TeamId : team (optional)
    OrganizationSettings "1" *-- "1" Hrn
    OrganizationSettings "1" -- "1" OrganizationId : organization
    Artifact "1" *-- "1" ArtifactId
    Artifact "1" -- "1" RepositoryId : hosted in
    Artifact "1" -- "1" UserId : uploader
    Artifact "1" -- "1" ArtifactCoordinates
    Artifact "1" *-- "1" ArtifactMetadata
    Artifact "1" -- "1" PhysicalArtifactId : links to
    ArtifactMetadata <|-- MavenArtifactMetadata
    ArtifactMetadata <|-- NpmArtifactMetadata
    ArtifactMetadata <|-- DockerArtifactMetadata
    Attestation "1" -- "1" PhysicalArtifactId : artifact
    ScanResult "1" -- "1" PhysicalArtifactId : artifact
    VulnerabilityOccurrence "1" -- "1" PhysicalArtifactId : artifact
    VulnerabilityOccurrence "1" -- "1" Hrn : vulnerability
    Event "1" *-- "1" Hrn
    EventStream "1" *-- "1" EventStreamId
    EventStream "1" -- "1" OrganizationId : organization
    EventSubscription "1" *-- "1" Hrn
    EventSubscription "1" -- "1" EventStreamId : stream
    Dashboard "1" *-- "1" DashboardId
    Dashboard "1" -- "1" OrganizationId : organization
    StorageBucket "1" *-- "1" StorageBucketId
    StorageBucket "1" -- "1" OrganizationId : organization
    Monitor "1" *-- "1" MonitorId
    Monitor "1" -- "1" OrganizationId : organization
    HealthCheck "1" *-- "1" Hrn
    HealthCheck "1" -- "1" MonitorId : monitor
    
    OrganizationId "1" *-- "1" Hrn
    RepositoryId "1" *-- "1" Hrn
    PhysicalArtifactId "1" *-- "1" Hrn
    UserId "1" *-- "1" Hrn
    ArtifactId "1" *-- "1" Hrn
    PolicyId "1" *-- "1" Hrn
    TeamId "1" *-- "1" Hrn
    AttestationId "1" *-- "1" Hrn
    ScanResultId "1" *-- "1" Hrn
    EventStreamId "1" *-- "1" Hrn
    DashboardId "1" *-- "1" Hrn
    StorageBucketId "1" *-- "1" Hrn
    MonitorId "1" *-- "1" Hrn
    
    Hrn ..> HrnError : returns on error
    
    ArtifactService ..> Artifact : creates/manages
    ArtifactService ..> UploadArtifactCommand : uses
    ArtifactService ..> PolicyEvaluator : uses
    
    PolicyEvaluator ..> PolicyEvaluationContext
    PolicyEvaluationContext o-- UserId : principal
    PolicyEvaluationContext o-- HodeiResource : resource
    PolicyEvaluationContext o-- Action
    PolicyEvaluator ..> PolicyDecision
```