# 2. Goals & Context

## 2.1 Strategic Goals

* **Garantizar la integridad y seguridad de la cadena de suministro** mediante verificación automática de firmas digitales, árboles de Merkle y análisis de dependencias transitivas.

* **Ofrecer rendimiento líder en la industria** con latencias sub-50ms para operaciones críticas y capacidad para manejar picos de carga sin degradación del servicio.

* **Proporcionar una experiencia de migración sin fricción** que permita a los equipos adoptar Hodei Artifacts en horas en lugar de semanas, manteniendo sus herramientas y flujos de trabajo existentes.

* **Implementar gobernanza basada en código** mediante Cedar, permitiendo políticas de acceso y seguridad que se validan automáticamente durante el ciclo de vida del artefacto.

* **Ofrecer visibilidad completa de la cadena de suministro** mediante un grafo de dependencias en tiempo real y análisis de impacto de vulnerabilidades.

* **Minimizar la complejidad operativa** mediante despliegue cloud-native en Kubernetes con observabilidad integral (métricas, trazas y logs estructurados).

## 2.2 Context and Key Concepts

Hodei Artifacts se fundamenta en una arquitectura híbrida que combina **Vertical Slice Architecture** para la organización funcional, **Arquitectura Hexagonal** para el desacoplamiento del núcleo de negocio y **Arquitectura Orientada a Eventos (EDA)** para la comunicación asíncrona. Este diseño permite un desarrollo ágil y modular, desplegado inicialmente como un "monolito modular" con una ruta de evolución clara hacia microservicios.

**Conceptos Fundamentales:**

* **Hodei Resource Name (HRN):** Sistema de identificación jerárquica universal para todos los recursos que sigue el formato `hrn:hodei:<service>:<region>:<account-id>:<resource-type>:<resource-id>`.

* **Policy-as-Code:** Implementación de políticas de seguridad y acceso mediante código declarativo evaluado por Cedar, con capacidad de validación previa mediante un "playground" interactivo.

* **Árbol de Merkle:** Estructura criptográfica integrada en el proceso de ingesta para garantizar la integridad de los artefactos y habilitar verificación eficiente.

* **Multi-Protocolo Nativo:** Implementación fiel de los protocolos estándar (Maven, npm, Docker, etc.) que permite a los clientes existentes interactuar sin modificaciones.
