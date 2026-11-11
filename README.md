# Hodei Artifacts

> 🏗️ **Parte de [Hodei Platform](https://github.com/Rubentxu)** — Una alternativa opensource a Azure DevOps, construida pieza a pieza.

**Hodei Artifacts** es la base de seguridad y de confianza sobre la que se construye el catálogo de artefactos del ecosistema Hodei: IAM granular, políticas Cedar, auditoría de seguridad y una API HTTP verificada.

> **Estado actual: hito M0 — Convergencia.** Lo que el producto hace hoy, lo que es experimental y lo que todavía no existe está en **[ARCHITECTURE_STATUS.md](ARCHITECTURE_STATUS.md)**. Ese documento manda sobre cualquier otro material de arquitectura: este README describe la base, no el producto completo.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-000000?style=flat&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Hodei-Platform-0F6CBD)](https://github.com/Rubentxu)

## 🎯 Propósito en Hodei Platform

Este monorepo construye la **base de seguridad** sobre la que se levanta el
catálogo de artefactos de la plataforma:
- **Autorización granular** con políticas Cedar, aislada por cuenta
- **Auditoría de seguridad** de las mutaciones que cambian la configuración
- **API HTTP** con OpenAPI, con su contrato verificado por pruebas
- **Frontend** experimental, con sus pruebas de componente

## 🏗️ Otros componentes del ecosistema
- [hodei_pipelines](https://github.com/Rubentxu/hodei_pipelines) — Plataforma ejecución CI/CD (Kotlin)
- [hodei_packages](https://github.com/Rubentxu/hodei_packages) — Registry de paquetes (Kotlin)  
- [hodei-draw](https://github.com/Rubentxu/hodei-draw) — Canvas diagramas (Rust/WASM)
- [hodei-dsl](https://github.com/Rubentxu/hodei-dsl) — DSL pipelines cloud-native (Kotlin)
- [hodei_devops](https://github.com/Rubentxu/hodei_devops) — Base plataforma DevOps (Go)

---

## Arquitectura

Este monorepo Rust tiene cuatro crates en el workspace y nueve crates
heredados que **no** se compilan ni se despliegan. La clasificación de cada
uno, con el motivo, está en [ARCHITECTURE_STATUS.md](ARCHITECTURE_STATUS.md).

```
crates/
├── kernel/            # Núcleo compartido: HRN v2, dominio, puertos
├── hodei-iam/         # Identity & Access Management
├── hodei-policies/    # Motor Cedar: schema, validación, evaluación
└── (9 crates heredados, excluidos del workspace; ver ARCHITECTURE_STATUS.md)
```

El kernel no depende de ningún mecanismo concreto: ni base de datos, ni HTTP.
Una comprobación de forma arquitectónica lo verifica en la CI.

## Características

Lo que existe hoy, con su etiqueta. `implementada` significa que hay ruta
real y prueba que se ejecuta en la CI.

### 🔐 Seguridad e IAM — `implementada`
- **Cuentas aisladas**: una cuenta no ve ni muta lo de otra, con un `404`
  uniforme que no revela la existencia del recurso ajeno.
- **Políticas Cedar**: schema declarativo, validación contra el schema que
  gobierna, y evaluación que resuelve el verbo contra ese schema.
- **Autenticación**: secreto compartido o JWKS/RS256, con verificación de la
  clave por `kid`.
- **Auditoría**: las mutaciones de la configuración de autorización dejan
  rastro con actor, operación, recurso y resultado, nunca con el token.
- **Bundle declarado**: el arranque comprueba que el bundle declara
  exactamente las acciones que el servicio autoriza y se niega a arrancar si
  discrepa.

### 📦 Catálogo de artefactos por digest — `planificada` (M1)
Todavía no existe. El roadmap v3 lo situa en M1: registrar un artefacto por
su digest y consultar su linaje, sin almacenar sus bytes.

### 🧬 Grafo de evidencia, cápsulas de confianza y puerta de promoción — `planificada` (M1–M3)
Todavía no existen. Son el corazón del producto y llegan después de la base
segura que este repositorio construye hoy.

### 🔍 Análisis de políticas — `implementada` en su parte viva
- Validación y evaluación de políticas Cedar contra el schema efectivo.
- Playground para probar una política a mano.
- Lo que no existe: simulación de autorizaciones, reportes de cobertura y
  análisis de brechas.

### 🌐 API y frontend
- **API REST** con OpenAPI, segmentada en suites de CI por lo que cada una
  necesita.
- **Frontend** (React, TypeScript, Tailwind): `experimental`. Tiene sus
  pruebas de componente y ahora se ejecutan, pero su navegación describe
  superficies que el backend actual no expone.

## Stack tecnológico

- **Backend**: Rust (Axum), Tokio async runtime
- **Base de datos**: SurrealDB (motor local RocksDB)
- **Autorización**: Cedar Policy Language
- **Identidad**: HRN v2 (`hodei-hrn`), con un único parser canónico
- **Frontend**: React 19, TypeScript, Tailwind CSS, Vitest
- **Testing**: suites de Rust segmentadas por CI, y pruebas de componente en
  el frontend
- **Despliegue**: empaquetado por script de release. No hay imagen de
  contenedor ni orquestación: no se afirma nada que no se pueda demostrar.

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

## 🔒 Seguridad por defecto

La superficie administrativa (`/api/v1`: IAM, políticas, esquemas y playground) está **cerrada por defecto**. `admin_api_enabled=true` solo la abre si el `AppState` tiene authenticator y authorizer configurados; sin ellos sigue cerrada con `403`. Con integración configurada, una petición anónima recibe `401`, una identidad sin permiso `403` y solo un `permit` Cedar alcanza el handler. Ver [admin-surface.md](docs/security/admin-surface.md).

`GET` y `POST /api/v1/repositories` son rutas de negocio separadas: no las abre `admin_api_enabled`; exigen bearer verificado y responden `401` sin credencial. `POST` emite eventos de auditoría sin credenciales. Ver [repository-read.md](docs/security/repository-read.md) y [audit-events.md](docs/security/audit-events.md).

```toml
[security]
# Solo abre la administración con authenticator + authorizer configurados.
admin_api_enabled = false
# Lista explícita de orígenes. Vacío = sin acceso cross-origin. '*' se rechaza.
cors_allowed_origins = []
```

- `HODEI_SECURITY__ADMIN_API_ENABLED=true` no sustituye autenticación: exige authenticator y authorizer efectivos. Si faltan, la superficie permanece cerrada y se registra un `error!`.
- Con integración configurada, las rutas administrativas responden `401` + `WWW-Authenticate: Bearer` al anónimo y `403` a una identidad sin permiso. La ruta de repositorios usa el mismo bundle JWT+Cedar; ver [production-auth.md](docs/security/production-auth.md).
- Los healthchecks (`/health`, `/health/ready`, `/health/live`) y Swagger/OpenAPI siguen públicos. Swagger expone el contrato (rutas y esquemas), no datos ni efectos; no es una frontera de seguridad.
- El preflight CORS (`OPTIONS`) lo responde la capa CORS exterior y no alcanza ningún handler.
- `tests/admin_surface.rs` verifica las 12 rutas administrativas (anónimo, sin permiso, permitido, flag false/true, método/recurso), la allowlist CORS (incluido `*` en configuración no validada) y que los healthchecks siguen disponibles.

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