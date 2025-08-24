# **Documento de Requisitos de Producto (PRD): Hodei Artifacts**

## **1. Resumen Ejecutivo**

**Hodei Artifacts** es un sistema de repositorio de artefactos de software, de alto rendimiento y nativo para la nube, desarrollado íntegramente en Rust. Nace como una alternativa moderna a soluciones consolidadas como Nexus, Artifactory y Archiva, con un enfoque estratégico en la seguridad de la cadena de suministro de software (software supply chain), la escalabilidad y la extensibilidad.

El sistema se fundamenta en una arquitectura híbrida que combina **Vertical Slices (VSA)** para la organización funcional, **Arquitectura Hexagonal** para el desacoplamiento del núcleo de negocio y **Arquitectura Orientada a Eventos (EDA)** para la comunicación asíncrona. Este diseño permite un desarrollo ágil y modular, desplegado inicialmente como un "monolito modular" con una ruta de evolución clara hacia microservicios.

Con soporte nativo para almacenamiento compatible con S3 (usando MinIO como referencia) y un modelo de autorización avanzado basado en Atributos (ABAC) con Cedar, Hodei Artifacts está diseñado para integrarse de forma nativa en ecosistemas de CI/CD modernos, proporcionando a los desarrolladores, equipos de DevSecOps y administradores una plataforma robusta, segura y observable.

## **2. Visión y Objetivos del Sistema**

### **2.1. Visión General**

Crear un repositorio de artefactos unificado que no solo iguale, sino que supere las capacidades de las soluciones existentes en rendimiento, seguridad y flexibilidad, aprovechando las ventajas del ecosistema de Rust y los patrones de diseño nativos para la nube.

### **2.2. Objetivos Clave**

* **Alto Rendimiento:** Ofrecer latencias inferiores a 50ms en operaciones críticas de metadatos (p99), gracias a la eficiencia de memoria y la concurrencia de Rust y el runtime asíncrono Tokio.
* **Escalabilidad Horizontal:** Diseñado para operar sobre Kubernetes, con capacidad de autoescalado basado en métricas de rendimiento (CPU, memoria, RPS) para gestionar cargas de trabajo variables.
* **Soporte Multi-formato:** Brindar soporte de primera clase y unificado para los ecosistemas de artefactos más relevantes, incluyendo **Maven, npm, Docker, NuGet, PyPI, Go, RubyGems y Helm**.
* **Flexibilidad de Almacenamiento:** Abstraer el almacenamiento físico a través de una capa de `StorageDriver`, con una implementación de referencia para cualquier proveedor **compatible con S3**.
* **Seguridad Integrada (Security by Design):** Implementar un modelo de seguridad robusto desde el núcleo, con un motor de políticas de **Control de Acceso Basado en Atributos (ABAC)**, generación de SBOMs y firma de artefactos.
* **Desarrollo Guiado por Contratos (Contract-First):** Utilizar la especificación **OpenAPI** como única fuente de verdad para todas las APIs síncronas, permitiendo el desarrollo en paralelo y garantizando la consistencia.

## **3. Principios y Patrones Arquitectónicos**

La arquitectura de Hodei Artifacts es una síntesis deliberada de patrones complementarios para maximizar la agilidad, la mantenibilidad y la escalabilidad.

* **Arquitectura de Slice Vertical (VSA) como Estructura Macro:** El código base se organiza en torno a capacidades de negocio (`Slices`) en lugar de capas técnicas. Cada Slice es una unidad autocontenida de funcionalidad, lo que minimiza el acoplamiento entre funcionalidades y acelera el desarrollo en paralelo.
* **Arquitectura Hexagonal como Estructura Micro:** Dentro de cada Slice, la lógica de negocio se aísla de las dependencias de infraestructura (bases de datos, frameworks web) mediante **Puertos y Adaptadores**. Las dependencias siempre apuntan hacia el interior, del adaptador al puerto del dominio, garantizando la testabilidad y la flexibilidad tecnológica del núcleo.
* **Arquitectura Orientada a Eventos (EDA) como Tejido Conectivo:** La comunicación entre Slices se realiza principalmente de forma asíncrona a través de un bus de eventos (Kafka). Esto promueve un bajo acoplamiento, mejora la resiliencia (el fallo de un componente no se propaga en cascada) y permite el procesamiento escalable de tareas.

Este enfoque híbrido da como resultado un **"monolito modular escalable"**: un sistema que se despliega como una única unidad, pero que está estructurado internamente como un conjunto de microservicios. Cada Slice es un candidato natural para ser extraído a un servicio independiente en el futuro con un mínimo esfuerzo, equilibrando la velocidad de desarrollo a corto plazo con la flexibilidad estratégica a largo plazo.

## **4. Usuarios y Roles (Personas)**

* **Desarrollador de Software:** Publica y recupera artefactos y dependencias. Requiere rapidez, fiabilidad y una documentación de API clara.
* **Sistema de CI/CD (Actor Automatizado):** El usuario principal. Empuja artefactos de compilación, consume dependencias y activa flujos de trabajo automatizados. Requiere APIs robustas, de alto rendimiento y mecanismos de autenticación seguros (Tokens OIDC).
* **Ingeniero de DevSecOps / Auditor de Seguridad:** Audita artefactos, revisa SBOMs, verifica firmas y gestiona políticas de acceso. Requiere pistas de auditoría completas, eventos auditables y controles de seguridad granulares.
* **Administrador de Sistemas / SRE:** Despliega, mantiene y monitoriza la plataforma. Requiere guías de despliegue claras (Helm charts), observabilidad completa (métricas, logs, trazas) y procedimientos operativos definidos.

## **5. Arquitectura Funcional: Vertical Slices**

Cada subsección representa un Slice vertical autocontenido. La implementación de cada uno se adhiere a los principios de Arquitectura Hexagonal y se comunica mediante eventos.

-----

### **Slice 1: Ingesta de Artefactos con Validación de Seguridad**

* **Descripción:** Gestiona la subida de artefactos, la validación de integridad, el almacenamiento seguro y el escaneo proactivo de vulnerabilidades.
* **Historia de Usuario:** "Como Sistema de CI/CD, quiero subir un artefacto con sus metadatos para que se almacene de forma segura y se inicie un escaneo de seguridad automáticamente."
* **Puntos de API:**
    * `POST /v1/artifacts/{repoType}/{groupId}/{artifactId}/{version}`
    * `POST /v1/artifacts/multipart` (para archivos grandes)
* **Componentes Core:** `ArtifactUploadService`, `SecurityScannerIntegration`, `ChecksumValidator`, `MetadataExtractor`.
* **Adaptadores:** `MinIOStorageAdapter`, `MongoDBArtifactRepository`, `CedarPolicyAuthorizer`, `VulnerabilityScannerAdapter`.
* **Eventos Publicados:** `ArtifactUploadedEvent`, `SecurityScanStartedEvent`.

-----

### **Slice 2: Recuperación Segura de Artefactos con Control de Acceso**

* **Descripción:** Proporciona acceso controlado, eficiente y auditable a los artefactos almacenados.
* **Historia de Usuario:** "Como Desarrollador, quiero descargar una versión específica de un artefacto, y el sistema debe verificar que tengo los permisos necesarios para hacerlo."
* **Puntos de API:**
    * `GET /v1/artifacts/{repoType}/{groupId}/{artifactId}/{version}`
    * `GET /v1/artifacts/{repoType}/{groupId}/{artifactId}/{version}/content`
* **Componentes Core:** `ArtifactDownloadService`, `AccessControlService`, `LicenseValidator`, `UsageTracker`.
* **Adaptadores:** `MinIODownloadAdapter`, `CDNDistributionAdapter`, `CedarPolicyEngine`.
* **Optimizaciones:** Soporte para `Range requests`, `Conditional GET` (ETag/If-Modified-Since), y cabeceras `Cache-Control`.

-----

### **Slice 3: Búsqueda y Análisis de Dependencias**

* **Descripción:** Permite la búsqueda avanzada de artefactos y el análisis de su composición, dependencias transitivas y licencias.
* **Historia de Usuario:** "Como Ingeniero de DevSecOps, quiero buscar artefactos por su hash y generar un grafo de dependencias para entender su impacto."
* **Puntos de API:**
    * `GET /v1/search/artifacts?q={query}`
    * `GET /v1/search/dependencies?package={packageName}`
    * `POST /v1/analysis/dependency-graph`
* **Componentes Core:** `DependencyAnalysisService`, `VulnerabilityAggregator`, `LicenseComplianceChecker`, `SBOMGenerator`.
* **Adaptadores:** `GraphDatabaseAdapter` (e.g., Neo4j), `ElasticSearchAdapter`, `MongoDBIndexAdapter`.
* **Características:** Búsqueda full-text, análisis de dependencias transitivas, detección de conflictos de licencias y generación de SBOM.

-----

### **Slice 4: Gestión de Usuarios y Políticas ABAC**

* **Descripción:** Sistema centralizado para la gestión de identidades y políticas de acceso granulares basadas en atributos.
* **Historia de Usuario:** "Como Administrador, quiero crear una política que permita a los miembros del grupo 'dev-team' publicar artefactos solo en repositorios de 'desarrollo'."
* **Puntos de API:**
    * `POST /v1/users`, `PUT /v1/users/{userId}/policies`
    * `GET /v1/groups/{groupId}/members`
    * `POST /v1/policies/validate`
* **Componentes Core:** `UserManagementService`, `PolicyManagementService`, `AccessDecisionPoint`, `AuditLogger`.
* **Adaptadores:** `MongoDBUserRepository`, `CedarPolicyEngineAdapter`, `LDAP/OIDC Provider Integration`.
* **Políticas ABAC:** Basadas en atributos de usuario, propiedades de artefactos, contexto de seguridad y riesgo operacional.

-----

### **Slice 5: Administración de Repositorios**

* **Descripción:** Creación y gestión de repositorios (locales, remotos, virtuales) con políticas de ciclo de vida.
* **Historia de Usuario:** "Como SRE, quiero configurar un repositorio virtual que agregue varios repositorios locales y aplique una política de retención para eliminar snapshots de más de 90 días."
* **Puntos de API:**
    * `POST /v1/repositories`, `PUT /v1/repositories/{repoId}/settings`
    * `POST /v1/repositories/{repoId}/cleanup`
    * `GET /v1/repositories/{repoId}/stats`
* **Componentes Core:** `RepositoryManager`, `RetentionPolicyEngine`, `StorageQuotaService`, `ReplicationCoordinator`.
* **Adaptadores:** `MongoDBRepoRepository`, `MinIOQuotaAdapter`, `CrossRegionReplicator`.
* **Características:** Políticas de retención, quotas de almacenamiento, réplica multi-región y limpieza inteligente.

-----

### **Slice 6: Monitorización y Analítica de Seguridad**

* **Descripción:** Dashboard centralizado para la visualización de la postura de seguridad, tendencias de vulnerabilidades y cumplimiento de políticas.
* **Historia de Usuario:** "Como Auditor de Seguridad, quiero ver un dashboard con la distribución de vulnerabilidades por severidad y los proyectos con mayor riesgo."
* **Puntos de API:**
    * `GET /metrics` (formato Prometheus)
    * `GET /v1/security/dashboard`
    * `GET /v1/audit/logs`
* **Componentes Core:** `SecurityMetricsCollector`, `VulnerabilityTrendAnalyzer`, `ComplianceAuditor`, `RiskAssessmentEngine`.
* **Adaptadores:** `PrometheusExporter`, `OpenTelemetryAdapter`, `SIEMIntegrationAdapter`, `GrafanaDashboardManager`.
* **Métricas Clave:** Distribución de vulnerabilidades, estado de compliance, evolución de la puntuación de riesgo.

-----

### **Slice 7: Autenticación Federada y SSO**

* **Descripción:** Sistema de autenticación unificada con soporte para proveedores de identidad externos (OIDC, SAML) y Single Sign-On.
* **Historia de Usuario:** "Como usuario corporativo, quiero iniciar sesión en Hodei Artifacts utilizando mi cuenta de Active Directory sin necesidad de una nueva contraseña."
* **Puntos de API:**
    * `POST /v1/auth/login`, `POST /v1/auth/token/refresh`
    * `GET /v1/auth/userinfo`
* **Componentes Core:** `AuthenticationService`, `TokenManagementService`, `FederationService`, `SessionManager`.
* **Adaptadores:** `LDAPAuthAdapter`, `OIDCProviderAdapter`, `SAMLServiceProvider`.
* **Características:** Multi-factor authentication, revocación de tokens, sesión distribuida.

-----

### **Slice 8: Despliegue y Configuración Cloud-Native**

* **Descripción:** Gestión de la configuración, salud del clúster y despliegues en entornos orquestados como Kubernetes.
* **Historia de Usuario:** "Como SRE, quiero actualizar una configuración en producción sin reiniciar el servicio y verificar el estado de salud de todos los nodos."
* **Puntos de API:**
    * `GET /v1/config/current`, `POST /v1/config/update`
    * `GET /v1/health`, `GET /v1/cluster/status`
* **Componentes Core:** `ConfigurationManager`, `DeploymentOrchestrator`, `HealthCheckService`, `ClusterCoordinator`.
* **Adaptadores:** `KubernetesOperatorAdapter`, `ConsulConfigAdapter`, `VaultSecretManager`.
* **Características:** Hot-reload de configuración, health checks personalizables, auto-escalado y despliegues zero-downtime.

-----

### **Slice 9: Soporte Multi-Formato con Escaneo Integrado**

* **Descripción:** Soporte para múltiples formatos de paquetes, con extracción de metadatos y análisis de dependencias específico para cada ecosistema.
* **Historia de Usuario:** "Como desarrollador de .NET, quiero publicar y consumir paquetes NuGet, y que el sistema entienda sus dependencias nativas."
* **Formatos Soportados:** Maven, npm, Docker, NuGet, PyPI, Go, RubyGems, Helm.
* **Componentes Core:** `PackageFormatDetector`, `MetadataExtractor`, `VulnerabilityMatcher`, `LicenseDetector`.
* **Adaptadores Específicos:** `MavenMetadataAdapter`, `NpmPackageAnalyzer`, `DockerManifestScanner`, `NuGetDependencyResolver`.
* **Características:** Detección automática de formato, extracción de metadatos enriquecidos y escaneo recursivo de dependencias.

-----

### **Slice 10: Pipeline de Seguridad Orientado a Eventos**

* **Descripción:** Orquestación asíncrona de flujos de trabajo de seguridad en respuesta a eventos del sistema.
* **Historia de Usuario:** "Como administrador, quiero definir un workflow que, cuando se detecta una vulnerabilidad 'CRITICAL', ponga el artefacto en cuarentena y envíe una notificación a Slack."
* **Eventos Consumidos:** `ArtifactUploadedEvent`, `SecurityScanCompletedEvent`, `PolicyViolationEvent`.
* **Componentes Core:** `EventDispatcher`, `SecurityPipelineManager`, `IncidentResponseCoordinator`, `WorkflowOrchestrator`.
* **Adaptadores:** `KafkaEventAdapter`, `WebhookDispatcher`, `NotificationService` (Slack, Email).
* **Características:** Pipelines de procesamiento configurables, gestión de Dead-Letter Queues (DLQ) y mecanismos de reintento con backoff.

## **7. Especificación de APIs y Catálogo de Eventos**

### **7.1. API REST (Contract-First con OpenAPI)**

Todas las APIs síncronas se definirán en un archivo `openapi.yaml` que servirá como contrato. Este enfoque permite la generación automática de clientes, stubs de servidor y documentación, además de la validación en el pipeline de CI/CD.

### **7.2. Catálogo de Eventos de Dominio**

Se utilizará el patrón **Event-Carried State Transfer**, donde la carga útil del evento contiene toda la información necesaria para los consumidores.

| Nombre del Evento | Slice Emisor | Descripción | Esquema de Carga Útil (Ejemplo) | Consumidores Potenciales |
| :--- | :--- | :--- | :--- | :--- |
| `ArtifactUploaded` | Ingesta | Un artefacto se ha almacenado con éxito. | `{ "artifactId": "...", "repository": "...", "sha256": "..." }` | Búsqueda, Escaneo, Notificación |
| `ScanCompleted` | Seguridad | Se completa un escaneo de vulnerabilidades. | `{ "artifactId": "...", "status": "succeeded", "vulnerabilities": [...] }` | Notificación, Aplicación de Políticas |
| `PolicyViolation` | Autorización | Una acción viola una política de seguridad. | `{ "subject": "...", "resource": "...", "action": "...", "timestamp": "..." }` | Auditoría, Alertas |

### **7.3. Gestión de Errores Asíncronos con Dead Letter Queues (DLQs)**

Para cada tema de Kafka, se configurará una **DLQ**. Si un consumidor falla al procesar un mensaje tras un número configurable de reintentos, el mensaje se moverá a la DLQ con metadatos del error. Esto evita bloqueos y permite una intervención manual. Los consumidores serán **idempotentes**.

## **8. Requisitos No Funcionales (NFRs)**

### **8.1. Rendimiento y Escalabilidad**

| Métrica | Operación | Objetivo (al 80% de capacidad) |
| :--- | :--- | :--- |
| Latencia API (p99) | Subida de Artefacto (metadatos) | \< 100ms |
| Latencia API (p99) | Descarga de Artefacto (redirección) | \< 50ms |
| Latencia API (p95) | Búsqueda de Metadatos | \< 200ms |
| Rendimiento | Ingestas de Artefactos | \> 500 artefactos/minuto |
| Rendimiento | Descargas de Artefactos | \> 5,000 artefactos/minuto |

### **8.2. Seguridad de la Cadena de Suministro (Supply Chain)**

* **Cifrado Total:** TLS 1.3 para datos en tránsito y cifrado nativo del proveedor de almacenamiento para datos en reposo.
* **Escaneo de Vulnerabilidades:** Uso obligatorio de `cargo-audit` en el pipeline de CI para las dependencias del propio proyecto.
* **Generación de SBOM:** Generación automática de SBOM en formato CycloneDX para cada artefacto gestionado.
* **Firma de Artefactos:** Las imágenes de contenedor y binarios críticos serán firmados criptográficamente con `cosign` y un flujo sin clave (keyless) OIDC.

### **8.3. Observabilidad**

* **Logs Estructurados (JSON):** Todos los servicios emitirán logs en JSON con un ID de correlación.
* **Métricas (Prometheus):** Exposición de un endpoint `/metrics` en formato Prometheus.
* **Trazado Distribuido (OpenTelemetry):** Trazado obligatorio para todas las peticiones de API y eventos.

## **9. Pila Tecnológica y Estrategia de Despliegue**

* **Lenguaje/Runtime:** Rust (última versión estable) con Tokio.
* **Framework Web:** Axum.
* **Almacenamiento de Metadatos:** MongoDB.
* **Almacenamiento de Objetos:** Compatible con S3 (MinIO para desarrollo/pruebas).
* **Bus de Eventos:** Apache Kafka.
* **Contenerización y Orquestación:** Imagen Docker mínima desplegada en **Kubernetes** a través de un **Helm chart**.
* **Pipeline de CI/CD (GitLab CI):**
    1.  **Lint & Format (`cargo clippy`, `cargo fmt`)**
    2.  **Build (`cargo build --release`)**
    3.  **Tests (`cargo test`):** Pruebas de integración con **Testcontainers**.
    4.  **Security Scan (`cargo-audit`)**
    5.  **Build & Push Image (al registro de GitLab)**
    6.  **Sign Artifacts (`cosign`) & Generate SBOM**
    7.  **Deploy to Staging (Automático)**
    8.  **Promote to Production (Aprobación Manual)**

## **10. Roadmap de Implementación e Innovación**

### **10.1. Roadmap por Fases**

* **Fase 1: Core Artifactory (2-3 meses)**
    * **Slices:** Ingesta (1), Recuperación (2), Soporte Maven/npm (9), Autenticación básica (4).
* **Fase 2: Integración de Seguridad (2-3 meses)**
    * **Slices:** Análisis de dependencias (3), Dashboard de seguridad (6), Pipeline de eventos (10), Gestión de repositorios (5).
* **Fase 3: Capacidades Empresariales (3-4 meses)**
    * **Slices:** Federación y SSO (7), Despliegue Cloud-Native avanzado (8), Soporte para más formatos (9), Analítica avanzada (6).

### **10.2. Innovaciones Clave**

1.  **Rendimiento Nativo de Rust:** Menor consumo de memoria y mayor throughput en comparación con soluciones basadas en JVM.
2.  **Almacenamiento S3 como Ciudadano de Primera Clase:** Diseño optimizado desde el inicio para almacenamiento de objetos.
3.  **Políticas de Autorización Expresivas con Cedar:** Modelo de seguridad más granular y flexible que RBAC tradicional.
4.  **Sistema de Plugins con WebAssembly (Wasm):** A futuro, permitir la extensibilidad de forma segura y portable mediante plugins compilados a Wasm.
5.  **Observabilidad Nativa con OpenTelemetry:** Trazabilidad y métricas consistentes en todos los componentes del sistema.