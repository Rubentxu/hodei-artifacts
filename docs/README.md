# 📚 Documentación - Hodei Artifacts

Sistema de repositorio de artefactos construido en Rust con arquitectura moderna (VSA + Hexagonal + Event-Driven).

## 📁 Documentos

| Documento | Descripción | Audiencia |
|-----------|-------------|-----------|
| **[prd.md](./prd.md)** | Requisitos de producto y objetivos | Product Managers, Stakeholders |
| **[arquitectura-sistema.md](./arquitectura-sistema.md)** | Especificaciones técnicas completas | Arquitectos, Desarrolladores |
| **[domain.md](./domain.md)** | Modelo de dominio y entidades | Backend Developers, Data Architects |
| **[epicas.md](./epicas.md)** | Features y roadmap (200+ features) | Product Managers, Tech Leads |
| **[evento-catalog.md](./evento-catalog.md)** | Catálogo de eventos (120+ eventos) | Backend Developers, Integration Engineers |
| **[feature-style-guide.md](./feature-style-guide.md)** | Guía de estilo para implementar features (VSA + Hexagonal) | Backend Developers, Tech Leads |
| **[testing-organization.md](./testing-organization.md)** | Estrategia de testing y organización | QA Engineers, Backend Developers |

## 🚀 Inicio Rápido

### Para nuevos desarrolladores:
1. Lee **[prd.md](./prd.md)** para entender el proyecto
2. Revisa **[domain.md](./domain.md)** para el modelo de datos
3. Consulta **[arquitectura-sistema.md](./arquitectura-sistema.md)** para patrones técnicos

### Para implementar features:
1. Busca la feature en **[epicas.md](./epicas.md)**
2. Revisa eventos relacionados en **[evento-catalog.md](./evento-catalog.md)**
3. **Lee [feature-style-guide.md](./feature-style-guide.md)** para patrones de implementación
4. Sigue la arquitectura general en **[arquitectura-sistema.md](./arquitectura-sistema.md)**

## 🔧 Stack Tecnológico

- **Lenguaje**: Rust + Tokio + Axum
- **Base de Datos**: MongoDB + Redis
- **Almacenamiento**: S3-compatible (MinIO)
- **Eventos**: Apache Kafka
- **Autorización**: Cedar Policy Engine (ABAC)
- **Deployment**: Kubernetes + Helm

---

**Contacto**: Solution Architecture Team
