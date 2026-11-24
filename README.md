# Hodei Artifacts

> **Part of Hodei Security Platform** · [Versión en español](README.es.md)

**Hodei Artifacts** is a Rust-native artifact registry and supply-chain security foundation: isolated accounts, Cedar ABAC policies, security audit logging, content-addressed storage, and protocol-conformant surfaces across major package ecosystems.

> **Current status:** what is implemented, verified, and excluded is tracked in **[ARCHITECTURE_STATUS.md](ARCHITECTURE_STATUS.md)**.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust Edition: 2024](https://img.shields.io/badge/Rust-Edition%202024-orange)](https://www.rust-lang.org/)

---

## Quickstart & User Guides

- **Community Quickstart (no Rust compilation required):** [`docs/community/quickstart.md`](docs/community/quickstart.md) (`scripts/community-quickstart.sh run`)
- **Ecosystem User Guides (routes, limits, credentials, offline examples):** [`docs/community/ecosystems.md`](docs/community/ecosystems.md)
  - [Maven 2](docs/community/ecosystems.md#maven-2-maven2) · [npm](docs/community/ecosystems.md#npm-npm) · [PyPI](docs/community/ecosystems.md#pypi-pypi) · [OCI](docs/community/ecosystems.md#oci-oci) · [Cargo](docs/community/ecosystems.md#cargo-cargo) · [NuGet](docs/community/ecosystems.md#nuget-nuget) · [Go](docs/community/ecosystems.md#go-go) · [APT](docs/community/ecosystems.md#apt-apt) · [RPM / Yum](docs/community/ecosystems.md#rpm--yum-rpm) · [Helm-OCI](docs/community/ecosystems.md#helm-oci-oci--helm)

> **Production Security Rule:** Always serve Hodei over **HTTPS/TLS** (`public_base_url = "https://..."`) and keep signature/checksum verification enabled (GPG for APT/RPM, digests/signatures for OCI and Helm-OCI, SRI `integrity` for npm, SHA-1/SHA-256 for Maven and PyPI). Local `http://127.0.0.1` examples are strictly scoped to offline development/lab environments.

---

## Capabilities Matrix (`GET /api/v1/capabilities`)

Derived from [`RepositoryFormat::served()`](crates/hodei-catalog/src/model.rs), [`RepositoryFormat::serves_modes()`](crates/hodei-catalog/src/model.rs), and [`conformance.rs`](crates/hodei-catalog/src/conformance.rs):

| Format | `hosted` | `proxy` | `group` | HTTP Route Prefix |
| --- | --- | --- | --- | --- |
| `raw` | Yes | Yes | Yes | `/repository/{repo}/...` |
| `oci` | Yes | No | Yes | `/v2/{repo}/...` |
| `maven2` | Yes | Yes | Yes | `/repository/{repo}/...` |
| `npm` | Yes | Yes | Yes | `/npm/{repo}/...` |
| `pypi` | Yes | Yes | Yes | `/pypi/{repo}/...` |
| `cargo` | Yes | No | No | `/cargo/{repo}/...` |
| `go` | Yes | Yes | Yes | `/go/{repo}/...` |
| `nuget` | Yes | Yes | No | `/nuget/{repo}/...` |
| `apt` | Yes | No | No | `/apt/{repo}/...` |
| `rpm` | Yes | No | No | `/yum/{repo}/...` |
| `helm` | No | No | No | Declared only; Helm charts use `oci` (`/v2/{repo}/...`) |

---

## Workspace Architecture

The active Rust workspace in [`Cargo.toml`](Cargo.toml) contains 26 members (25 crates under `crates/` plus the root API crate `.`). Nine legacy crates in `crates/` are excluded from the workspace (see [ARCHITECTURE_STATUS.md](ARCHITECTURE_STATUS.md)).

- `.` — `hodei-artifacts-api` (Axum HTTP API, router, handlers, bootstrap, recovery CLI)
- `crates/kernel` — Shared kernel: HRN v2, domain primitives, ports
- `crates/hodei-iam` — Identity & Access Management
- `crates/hodei-policies` — Cedar policy engine: schema, validation, evaluation
- `crates/hodei-evidence` — Evidence domain & verification
- `crates/hodei-evidence-storage` — SurrealDB storage for evidence
- `crates/hodei-trust-policy` — Trust policy & promotion gates
- `crates/hodei-trust-policy-storage` — SurrealDB storage for trust policies
- `crates/hodei-risk` — Risk facts & evaluation
- `crates/hodei-risk-storage` — SurrealDB storage for risk facts
- `crates/hodei-reachability` — Reachability & lineage analysis
- `crates/hodei-reachability-storage` — SurrealDB storage for reachability
- `crates/hodei-reactive` — Reactive event & lifecycle coordination
- `crates/hodei-hub` — Content-addressed blob hub & integrity
- `crates/hodei-hub-s3` — S3-compatible blob store adapter
- `crates/hodei-catalog` — Repository catalog, conformance matrix, proxy/group rules
- `crates/hodei-catalog-storage` — SurrealDB storage for the catalog
- `crates/hodei-audit` — Security audit event log
- `crates/hodei-export-tool` — Portable export tooling
- `crates/hodei-investigator` — Supply-chain investigation engine
- `crates/hodei-investigator-storage` — SurrealDB storage for investigations
- `crates/hodei-simulation` — Policy/graph simulation
- `crates/hodei-simulation-storage` — SurrealDB storage for simulations
- `crates/hodei-counterfactual` — Counterfactual policy evaluation
- `crates/hodei-spike-ladybug` — Graph storage spike
- `crates/hodei-graph-client` — Graph query client

---

## Security by Default & Public URL

- **Admin surface (`/api/v1` IAM, policies, schemas, playground)** is closed by default (`admin_api_enabled = false`) and requires both authenticator and Cedar authorizer. See [docs/security/admin-surface.md](docs/security/admin-surface.md) and [docs/security/production-auth.md](docs/security/production-auth.md).
- **Business routes (`/api/v1/repositories`, `/api/v1/catalog/repositories`, `/api/v1/search`)** require a verified Bearer token and per-repository Cedar authorization. See [docs/security/repository-read.md](docs/security/repository-read.md) and [docs/security/audit-events.md](docs/security/audit-events.md).
- **Public URL (`[public_url]`)**: `public_base_url` and `trusted_proxies` are controlled strictly by configuration, never by untrusted `Host` or `X-Forwarded-*` headers.

---

## Development, Testing & Operations

```bash
# Build workspace
cargo build --workspace

# Run affected tests (day-to-day gate)
./scripts/test-affected.sh

# Run API server
cargo run --bin hodei-artifacts-api
```

- **Local CI:** [`pipeline.kts`](pipeline.kts) (PipelineK) and [`docs/ci-strategy.md`](docs/ci-strategy.md)
- **Testing organization:** [`docs/testing-organization.md`](docs/testing-organization.md)
- **UAT contracts:** [`uat/README.md`](uat/README.md), [`uat/UAT-FORMATS.md`](uat/UAT-FORMATS.md)
- **Backup, restore, scrub & repair (`hodei-recovery`):** [`uat/UAT-OPERATIONS.md`](uat/UAT-OPERATIONS.md#recovery--copia-restauración-scrub-y-reparación)
- **Upgrade & release packaging:** [`scripts/package-release.sh`](scripts/package-release.sh) and [`docs/community/ecosystems.md`](docs/community/ecosystems.md#3-operaciones-copia-de-seguridad-restauración-y-actualización)
- **Architecture & Security docs:** [`docs/architecture.md`](docs/architecture.md), [`docs/security.md`](docs/security.md)

## License

MIT — see [LICENSE](LICENSE).
