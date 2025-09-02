# Project Brief: Hodei Artifacts

## Executive Summary

Hodei Artifacts es un sistema de repositorio de artefactos de software de nueva generación, diseñado para ofrecer máximo rendimiento, seguridad y escalabilidad. Este proyecto busca ser una alternativa moderna a soluciones existentes, con un enfoque estratégico en la seguridad de la cadena de suministro de software y la multi-tenancy a través de conceptos como HRN (Hodei Resource Name) y Organizaciones, inspirados en modelos como AWS IAM.

## Problem Statement

El problema principal que Hodei Artifacts busca resolver es la necesidad de un repositorio de artefactos moderno que supere las limitaciones de rendimiento, seguridad y escalabilidad de las soluciones actuales. Las soluciones existentes a menudo carecen de un enfoque nativo en la seguridad de la cadena de suministro, control de acceso granular basado en atributos (ABAC) y una arquitectura diseñada para la eficiencia en entornos cloud-native.

## Proposed Solution

Hodei Artifacts será un sistema de repositorio de artefactos nativo en la nube y de alto rendimiento, desarrollado íntegramente en Rust. Adoptará una arquitectura híbrida que fusiona Vertical Slice Architecture (VSA), Arquitectura Hexagonal y un modelo Dirigido por Eventos (EDA). Se enfocará en la ingesta y recuperación optimizada de artefactos, búsqueda avanzada, gestión granular de políticas ABAC con Cedar, administración de repositorios, monitorización de seguridad, autenticación federada, despliegue cloud-native, soporte multi-formato con escaneo integrado, y pipelines de seguridad orientadas a eventos.

## Target Users

Los usuarios principales de Hodei Artifacts incluyen:
*   **Desarrolladores:** Para la subida, descarga y búsqueda de artefactos.
*   **Sistemas CI/CD:** Para la integración automatizada de artefactos en pipelines de desarrollo.
*   **Equipos de Seguridad:** Para la monitorización de vulnerabilidades, cumplimiento de políticas y gestión de la cadena de suministro.
*   **Administradores de Plataforma:** Para la gestión de repositorios, usuarios, políticas y la infraestructura del sistema.

## Goals & Success Metrics

**Objetivos:**
*   Ofrecer un rendimiento superior en la ingesta y recuperación de artefactos.
*   Garantizar una seguridad robusta de la cadena de suministro de software.
*   Proporcionar escalabilidad y resiliencia en entornos cloud-native.
*   Simplificar la gestión de artefactos y políticas de acceso.

**Métricas Clave (KPIs):**
*   **Rendimiento:** Latencia de subida/descarga (p99 < X ms), Throughput (Y artefactos/segundo).
*   **Seguridad:** Cobertura de escaneo de vulnerabilidades (100%), Tiempo de detección de vulnerabilidades (Z horas).
*   **Escalabilidad:** Capacidad de manejar N usuarios concurrentes y M repositorios.
*   **Adopción:** Número de organizaciones y usuarios activos.

## MVP Scope

El MVP de Hodei Artifacts se centrará en las funcionalidades core para la gestión del ciclo de vida de artefactos, incluyendo:

*   **Ingesta de Artefactos:** Subida básica y multipart con validación de integridad (checksums), extracción de metadatos básicos y almacenamiento en S3/MongoDB.
*   **Recuperación de Artefactos:** Descarga básica con autorización y generación de URLs pre-firmadas.
*   **Gestión de Repositorios:** Creación y gestión de repositorios básicos.
*   **Control de Acceso (ABAC):** Integración inicial con Cedar para políticas de autorización básicas.
*   **Búsqueda Básica:** Búsqueda por nombre y versión.

**Fuera del Alcance para el MVP:**
*   Funcionalidades avanzadas de seguridad (ej. SBOM, firma de artefactos, ML para seguridad).
*   Integraciones extensas con ecosistemas (ej. todos los formatos de paquetes, plugins CI/CD).
*   Dashboards de analítica avanzada y BI.
*   UI completa (inicialmente, enfoque en API).

**Criterios de Éxito del MVP:**
*   Capacidad de subir y descargar artefactos de forma segura y eficiente.
*   Funcionalidad básica de ABAC para controlar el acceso.
*   Estabilidad y rendimiento aceptables para un conjunto limitado de usuarios.

## Post-MVP Vision

La visión a largo plazo de Hodei Artifacts incluye:

*   **Expansión de Funcionalidades Core:** Soporte multi-formato completo, búsqueda avanzada con ML, gestión de políticas ABAC sofisticada, y administración de repositorios con políticas de retención y limpieza.
*   **Seguridad Integral de la Cadena de Suministro:** Generación de SBOM, firma y verificación de artefactos, detección de malware, análisis de la cadena de suministro, y automatización de flujos de trabajo de seguridad.
*   **Observabilidad y Analítica Avanzada:** Dashboards en tiempo real, analítica predictiva, análisis de costes y comportamiento de usuario.
*   **Integración con Ecosistemas:** Amplia gama de integraciones con herramientas de desarrollo, CI/CD, y plataformas de orquestación.
*   **Experiencia del Desarrollador (DX):** Herramientas CLI, plugins IDE, scaffolding automatizado.
*   **UI Completa:** Una interfaz de usuario web rica e intuitiva.
*   **Consideraciones Arquitectónicas:** Abordar desafíos como el "Shared Monolith", manejo robusto de eventos, infraestructura de analítica y reporting de datos, y gestión del contrato de API.

## Technical Considerations

*   **Plataformas Objetivo:** Entornos cloud-native, principalmente Kubernetes.
*   **Requisitos de Rendimiento:** Baja latencia y alto throughput para operaciones de I/O.
*   **Tecnologías Preferidas:**
    *   **Backend:** Rust con Tokio y Axum.
    *   **Base de Datos (Metadatos):** MongoDB.
    *   **Almacenamiento de Objetos:** Compatible con S3 (ej. MinIO).
    *   **Bus de Eventos:** Apache Kafka o RabbitMQ.
    *   **Motor de Autorización:** Cedar.
    *   **Caché:** Redis.
    *   **Métricas:** Prometheus.
    *   **Trazas y Logs:** OpenTelemetry.
*   **Consideraciones de Arquitectura:**
    *   **Estructura de Repositorio:** Monorepo de crates de Rust, con organización por Vertical Slices.
    *   **Arquitectura de Servicio:** Híbrido VSA + Hexagonal + EDA.
    *   **Requisitos de Integración:** API REST Contract-First, comunicación asíncrona por eventos.
    *   **Seguridad/Cumplimiento:** ABAC con HRN, Security by Design, auditoría completa.

## Constraints & Assumptions

**Restricciones:**
*   **Tecnológicas:** Adherencia a Rust como lenguaje principal.
*   **Arquitectónicas:** Mantenimiento de los principios VSA, Hexagonal y EDA.

**Supuestos Clave:**
*   La consistencia eventual es aceptable para la mayoría de las operaciones entre slices.
*   La flexibilidad de esquema de MongoDB es adecuada para la evolución de metadatos.
*   La comunidad de Rust y el ecosistema de herramientas son lo suficientemente maduros para el desarrollo a largo plazo.

## Risks & Open Questions

**Riesgos Clave:**
*   **Crecimiento descontrolado del crate `shared`:** Riesgo de convertirse en un "shared monolith" si no se aplican directrices estrictas.
*   **Manejo de fallos en eventos:** Asegurar la fiabilidad y resiliencia de la comunicación asíncrona.
*   **Complejidad de analítica transversal:** Combinar datos de múltiples slices para reporting sin acoplamiento directo.
*   **"Drift" en el contrato de API:** Desalineación entre la especificación OpenAPI y la implementación real.

**Preguntas Abiertas:**
*   ¿Cómo se gestionarán las migraciones de índices en colecciones grandes de MongoDB?
*   ¿Cómo se asegurará el backpressure si el consumo de eventos de Kafka es lento?
*   ¿Cuál será la estrategia para la gestión de políticas ABAC complejas y su versionado?

## Appendices

### A. Research Summary

Este Project Brief se basa en el "Resumen de Ideas y Profundización para Hodei Artifacts" (`docs/brainstorming_summary.md`), que consolida requisitos detallados, ideas de mejora y consideraciones arquitectónicas de una sesión de brainstorming estructurada.

## Next Steps

### Immediate Actions

1.  Revisar este Project Brief para asegurar que captura la visión y el alcance inicial del proyecto.
2.  Iniciar el proceso de refinamiento de requisitos detallados para las funcionalidades del MVP.
3.  Definir el plan de implementación detallado para la Fase 1 del proyecto.

### PM Handoff

Este Project Brief proporciona el contexto completo para Hodei Artifacts. Por favor, inicie en 'PRD Generation Mode', revise el brief a fondo para trabajar con el usuario en la creación de la sección PRD por sección, pidiendo cualquier aclaración necesaria o sugiriendo mejoras.
