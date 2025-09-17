# 1. Executive Summary

**Hodei Artifacts** es una plataforma de gestión de la cadena de suministro de software de última generación, nativa de la nube y desarrollada en Rust. Se posiciona como una **alternativa moderna y superior a productos consolidados como Artifactory y Nexus**, enfocándose en tres pilares fundamentales:

1. **Seguridad basada en Policy-as-Code**: Implementación robusta de políticas de seguridad mediante Cedar, con el sistema **Hodei Resource Name (HRN)** como núcleo de identificación universal.

2. **Rendimiento extremo**: Latencias sub-50ms para operaciones críticas y capacidad para manejar altos volúmenes de tráfico sin degradación del servicio.

3. **Compatibilidad transparente con flujos de trabajo existentes**: Soporte nativo para protocolos estándar (Maven, npm, Docker, etc.) sin requerir modificaciones en pipelines, scripts o herramientas de línea de comandos.

La plataforma está construida con una arquitectura **multi-crate** basada en **Vertical Slice Architecture (VSA)** y **Clean Architecture**, permitiendo un desarrollo modular, escalable y mantenible. Cada bounded context es un crate independiente que encapsula toda su funcionalidad, incluyendo sus adaptadores de infraestructura.
