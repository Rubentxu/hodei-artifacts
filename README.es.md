# Hodei Artifacts

> **Parte de Hodei Security Platform** · [English version](README.md)

**Hodei Artifacts** es un registro de artefactos nativo en Rust y base de seguridad para la cadena de suministro: cuentas aisladas, políticas Cedar ABAC, auditoría de seguridad, almacén direccionado por contenido y superficies conformes para los principales ecosistemas de paquetes.

> **Estado actual:** lo que está implementado, verificado y excluido se detalla en **[ARCHITECTURE_STATUS.md](ARCHITECTURE_STATUS.md)**.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust Edition: 2024](https://img.shields.io/badge/Rust-Edition%202024-orange)](https://www.rust-lang.org/)

---

## Quickstart y guías de usuario

- **Quickstart comunitario (sin compilar Rust):** [`docs/community/quickstart.md`](docs/community/quickstart.md) (`scripts/community-quickstart.sh run`)
- **Guías de usuario por ecosistema (rutas reales, límites, credenciales y ejemplos offline):** [`docs/community/ecosystems.md`](docs/community/ecosystems.md)
  - [Maven 2](docs/community/ecosystems.md#maven-2-maven2) · [npm](docs/community/ecosystems.md#npm-npm) · [PyPI](docs/community/ecosystems.md#pypi-pypi) · [OCI](docs/community/ecosystems.md#oci-oci) · [Cargo](docs/community/ecosystems.md#cargo-cargo) · [NuGet](docs/community/ecosystems.md#nuget-nuget) · [Go](docs/community/ecosystems.md#go-go) · [APT](docs/community/ecosystems.md#apt-apt) · [RPM / Yum](docs/community/ecosystems.md#rpm--yum-rpm) · [Helm-OCI](docs/community/ecosystems.md#helm-oci-oci--helm)

> **Regla de producción (TLS y firmas):** en producción expón siempre Hodei sobre **HTTPS/TLS** (`public_base_url = "https://..."`) y mantén activa la verificación de firmas e integridad (GPG en APT/RPM, digests/firmas en OCI y Helm-OCI, `integrity` SRI en npm, SHA-1/SHA-256 en Maven y PyPI). Los ejemplos con `http://127.0.0.1` están acotados exclusivamente a desarrollo y laboratorio offline.

---

## Matriz de capacidades (`GET /api/v1/capabilities`)

Derivada de [`RepositoryFormat::served()`](crates/hodei-catalog/src/model.rs), [`RepositoryFormat::serves_modes()`](crates/hodei-catalog/src/model.rs) y [`conformance.rs`](crates/hodei-catalog/src/conformance.rs):

| Formato | `hosted` | `proxy` | `group` | Prefijo de ruta HTTP |
| --- | --- | --- | --- | --- |
| `raw` | Sí | Sí | Sí | `/repository/{repo}/...` |
| `oci` | Sí | No | Sí | `/v2/{repo}/...` |
| `maven2` | Sí | Sí | Sí | `/repository/{repo}/...` |
| `npm` | Sí | Sí | Sí | `/npm/{repo}/...` |
| `pypi` | Sí | Sí | Sí | `/pypi/{repo}/...` |
| `cargo` | Sí | No | No | `/cargo/{repo}/...` |
| `go` | Sí | Sí | Sí | `/go/{repo}/...` |
| `nuget` | Sí | Sí | No | `/nuget/{repo}/...` |
| `apt` | Sí | No | No | `/apt/{repo}/...` |
| `rpm` | Sí | No | No | `/yum/{repo}/...` |
| `helm` | No | No | No | Solo declarado; los charts Helm usan `oci` (`/v2/{repo}/...`) |

---

## Arquitectura del workspace

El workspace activo en [`Cargo.toml`](Cargo.toml) contiene 26 miembros (25 crates en `crates/` más el crate raíz `.`). Nueve crates heredados en `crates/` están excluidos del workspace (ver [ARCHITECTURE_STATUS.md](ARCHITECTURE_STATUS.md)).

- `.` — `hodei-artifacts-api` (API HTTP Axum, router, handlers, bootstrap, CLI de recuperación)
- `crates/kernel` — Núcleo compartido: HRN v2, dominio, puertos
- `crates/hodei-iam` — Identity & Access Management
- `crates/hodei-policies` — Motor Cedar: schema, validación, evaluación
- `crates/hodei-evidence` — Dominio y verificación de evidencia
- `crates/hodei-evidence-storage` — Almacenamiento SurrealDB para evidencia
- `crates/hodei-trust-policy` — Políticas de confianza y puertas de promoción
- `crates/hodei-trust-policy-storage` — Almacenamiento SurrealDB para políticas de confianza
- `crates/hodei-risk` — Hechos y evaluación de riesgo
- `crates/hodei-risk-storage` — Almacenamiento SurrealDB para riesgo
- `crates/hodei-reachability` — Análisis de alcanzabilidad y linaje
- `crates/hodei-reachability-storage` — Almacenamiento SurrealDB para alcanzabilidad
- `crates/hodei-reactive` — Coordinación reactiva de eventos y ciclo de vida
- `crates/hodei-hub` — Hub de blobs direccionado por contenido e integridad
- `crates/hodei-hub-s3` — Adaptador de almacén compatible con S3
- `crates/hodei-catalog` — Catálogo de repositorios, matriz de conformidad y reglas proxy/group
- `crates/hodei-catalog-storage` — Almacenamiento SurrealDB para el catálogo
- `crates/hodei-audit` — Registro de eventos de auditoría de seguridad
- `crates/hodei-export-tool` — Herramienta de exportación portable
- `crates/hodei-investigator` — Motor de investigación de cadena de suministro
- `crates/hodei-investigator-storage` — Almacenamiento SurrealDB para investigaciones
- `crates/hodei-simulation` — Simulación de políticas y grafo
- `crates/hodei-simulation-storage` — Almacenamiento SurrealDB para simulaciones
- `crates/hodei-counterfactual` — Evaluación contrafactual de políticas
- `crates/hodei-spike-ladybug` — Spike de almacenamiento en grafo
- `crates/hodei-graph-client` — Cliente de consultas de grafo

---

## Seguridad por defecto y dirección pública

- **Superficie administrativa (`/api/v1` IAM, políticas, esquemas, playground):** cerrada por defecto (`admin_api_enabled = false`) y exige authenticator y autorizador Cedar. Ver [docs/security/admin-surface.md](docs/security/admin-surface.md) y [docs/security/production-auth.md](docs/security/production-auth.md).
- **Rutas de negocio (`/api/v1/repositories`, `/api/v1/catalog/repositories`, `/api/v1/search`):** exigen Bearer verificado y permiso Cedar por repositorio. Ver [docs/security/repository-read.md](docs/security/repository-read.md) y [docs/security/audit-events.md](docs/security/audit-events.md).
- **Dirección pública (`[public_url]`):** `public_base_url` y `trusted_proxies` se gobiernan exclusivamente por configuración, nunca por cabeceras `Host` o `X-Forwarded-*` no confiables.

---

## Desarrollo, pruebas y operaciones

```bash
# Compilar todo el workspace
cargo build --workspace

# Ejecutar pruebas afectadas (puerta del día a día)
./scripts/test-affected.sh

# Arrancar el servidor API
cargo run --bin hodei-artifacts-api
```

- **CI local:** [`pipeline.kts`](pipeline.kts) (PipelineK) y [`docs/ci-strategy.md`](docs/ci-strategy.md)
- **Organización de tests:** [`docs/testing-organization.md`](docs/testing-organization.md)
- **Contratos UAT:** [`uat/README.md`](uat/README.md), [`uat/UAT-FORMATS.md`](uat/UAT-FORMATS.md)
- **Copia de seguridad, restauración, scrub y reparación (`hodei-recovery`):** [`uat/UAT-OPERATIONS.md`](uat/UAT-OPERATIONS.md#recovery--copia-restauración-scrub-y-reparación)
- **Actualización y empaquetado de release:** [`scripts/package-release.sh`](scripts/package-release.sh) y [`docs/community/ecosystems.md`](docs/community/ecosystems.md#3-operaciones-copia-de-seguridad-restauración-y-actualización)
- **Documentación de arquitectura y seguridad:** [`docs/architecture.md`](docs/architecture.md), [`docs/security.md`](docs/security.md)

## Licencia

MIT — ver [LICENSE](LICENSE).
