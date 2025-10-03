# Hodei Artifacts

> 🏗️ **Parte de [Hodei Platform](https://github.com/Rubentxu)** — Una alternativa opensource a Azure DevOps, construida pieza a pieza.

**Hodei Artifacts** es el componente de gestión de artefactos y autorización multi-capa del ecosistema Hodei. Proporciona IAM granular, gestión de organizaciones, análisis de políticas Cedar y APIs completas para integración con otras piezas de la plataforma.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-000000?style=flat&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Hodei-Platform-0F6CBD)](https://github.com/Rubentxu)

## 🎯 Propósito en Hodei Platform

Este monorepo implementa los **cimientos de seguridad y gestión de artifacts** para toda la plataforma:
- **Autorización granular** con políticas Cedar (IAM, Organizations, SCPs)
- **Registry de artifacts** multi-formato con control de acceso
- **API unificada** con docs OpenAPI para integración
- **Frontend** para gestión visual de políticas y artifacts

## 🏗️ Otros componentes del ecosistema
- [hodei_pipelines](https://github.com/Rubentxu/hodei_pipelines) — Plataforma ejecución CI/CD (Kotlin)
- [hodei_packages](https://github.com/Rubentxu/hodei_packages) — Registry de paquetes (Kotlin)  
- [hodei-draw](https://github.com/Rubentxu/hodei-draw) — Canvas diagramas (Rust/WASM)
- [hodei-dsl](https://github.com/Rubentxu/hodei-dsl) — DSL pipelines cloud-native (Kotlin)
- [hodei_devops](https://github.com/Rubentxu/hodei_devops) — Base plataforma DevOps (Go)

---

## Arquitectura

Este monorepo Rust contiene múltiples crates organizados por dominio:

```
crates/
├── artifact/          # Gestión de artifacts y registry
├── distribution/      # Distribución y entrega
├── hodei-authorizer/  # Motor de autorización Cedar
├── hodei-iam/         # Identity & Access Management
├── hodei-organizations/ # Gestión de organizaciones
├── policies/          # Análisis y validación de políticas
├── repository/        # Abstracción de repositorios
├── search/           # Búsqueda y indexación
├── security/         # Seguridad y compliance  
├── shared/           # Utilidades compartidas
└── supply-chain/     # Supply chain security
```

## Características principales

### 🔐 Autorización multi-capa
- **IAM**: Usuarios, roles, permisos granulares
- **Organizations**: Gestión jerárquica con SCPs (Service Control Policies)
- **Cedar**: Motor de políticas declarativas con análisis estático
- **Multi-tenancy**: Aislamiento completo entre organizaciones

### 📦 Registry de artifacts
- Soporte multi-formato (Docker, Maven, npm, PyPI, etc.)
- Control de acceso basado en políticas
- Metadatos ricos y búsqueda avanzada
- Supply chain security y firma de artifacts

### 🔍 Análisis de políticas
- Validación estática de políticas Cedar
- Simulación de autorizaciones
- Reportes de cobertura y gaps de seguridad
- Playground interactivo para testing

### 🌐 API y Frontend
- **API REST** completa con docs OpenAPI
- **Frontend Vue.js** para gestión visual
- **WebSockets** para actualizaciones en tiempo real
- **CLI** para automatización

## Stack tecnológico

- **Backend**: Rust (Axum), Tokio async runtime
- **Base de datos**: SurrealDB (multi-modelo)
- **Autorización**: Cedar Policy Language
- **Frontend**: Vue.js 3, TypeScript, Tailwind CSS
- **Testing**: Property-based testing, E2E con Playwright
- **Deployment**: Docker, Kubernetes, Terraform

## Quickstart

```bash
# Clonar el repositorio
git clone https://github.com/Rubentxu/hodei-artifacts.git
cd hodei-artifacts

# Compilar todas las crates
cargo build --workspace

# Ejecutar tests
cargo test --workspace

# Lanzar la API de desarrollo
cargo run --bin hodei-artifacts-api

# Frontend (en otra terminal)
cd frontend
npm install
npm run dev
```

## Documentación

- **📖 [Documentación completa](./docs/README.md)**
- **🔧 [Guía de desarrollo](./docs/development.md)**
- **🚀 [Deployment](./docs/deployment.md)**
- **📋 [Roadmap](./docs/roadmap.md)**
- **🔒 [Seguridad](./docs/security.md)**

## Contribuir

¿Te interesa contribuir a **Hodei Platform**? 

1. Lee la [guía de contribución](./CONTRIBUTING.md)
2. Revisa los [issues abiertos](https://github.com/Rubentxu/hodei-artifacts/issues)
3. Únete a las [discusiones](https://github.com/Rubentxu/hodei-artifacts/discussions)

## Autor

**Rubén Darío** ([@Rubentxu](https://github.com/Rubentxu))  
Cloud Solutions Architect & DevOps Lead | Construyendo Hodei Platform paso a paso

- Blog: https://blog.rubentxu.dev
- LinkedIn: https://linkedin.com/in/rubentxu
- Email: rubentxu74@gmail.com

## Licencia

Este proyecto está bajo la licencia [MIT](LICENSE).

---

> 💡 **Hodei Platform vision**: Crear una suite DevOps completa, opensource y cloud-native que rivalice con Azure DevOps — pero modular, extensible y construida con tecnologías modernas.