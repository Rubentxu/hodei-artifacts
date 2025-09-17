# 4. Bounded Contexts and Crates Structure

## 4.1 Crate Structure Overview

```
.
├── Cargo.toml
├── crates/
│   ├── artifact/                # Artifact Core Management
│   ├── distribution/            # Protocol Distribution
│   ├── iam/                     # Identity & Access Management
│   ├── policies/                # Policy Engine
│   ├── organization/            # Organization Management
│   ├── repository/              # Repository Management
│   ├── supply-chain/            # Supply Chain Security
│   ├── search/                  # Search Engine
│   └── shared/                  # Shared Kernel
├── services/
│   └── api-http/                # HTTP API Service
│   └── event-processor/         # Event Processing Service
└── ...
```

## 4.2 Artifact Crate (artifact)

**Responsibility:** Core artifact lifecycle management, independent of protocols and policies.

**Structure:**
```
crates/artifact/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── domain/
│   │   ├── hrn.rs               # HRN implementation
│   │   ├── merkle.rs            # Merkle Tree implementation
│   │   ├── version.rs           # Versioning logic
│   │   └── mod.rs               # Domain exports
│   └── features/
│       ├── upload_core/         # Basic artifact upload
│       │   ├── mod.rs
│       │   ├── use_case.rs
│       │   ├── ports.rs
│       │   ├── adapter.rs
│       │   ├── dto.rs
│       │   ├── error.rs
│       │   ├── event_handler.rs
│       │   ├── di.rs
│       │   ├── mocks.rs
│       │   ├── use_case_test.rs
│       │   └── event_handler_test.rs
│       ├── upload_multipart/    # Multipart upload
│       │   └── ... (same structure)
│       ├── duplicate_detection/ # Duplicate detection
│       │   └── ... (same structure)
│       ├── versioning_logic/    # Versioning logic
│       │   └── ... (same structure)
│       └── ...                  # Other features
├── tests/                       # Integration tests
│   ├── it_upload_core_test.rs
│   ├── it_upload_multipart_test.rs
│   └── ...
└── ...
```

**Key Features:**
- E1.F01: Artifact Upload Core
- E1.F02: Artifact Upload Multipart
- E1.F05: Duplicate Detection
- E1.F09: Artifact Versioning Logic
- E1.F18: Artifact Checksums Multiple

## 4.3 Distribution Crate (distribution)

**Responsibility:** Implementation of specific protocols that use the artifact core.

**Structure:**
```
crates/distribution/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── domain/
│   │   ├── maven/               # Maven specifications
│   │   │   ├── pom_parser.rs    # POM XML processing
│   │   │   ├── metadata.rs      # Maven metadata generation
│   │   │   └── mod.rs           # Maven domain exports
│   │   ├── npm/                 # npm specifications
│   │   │   ├── package_parser.rs # package.json processing
│   │   │   ├── metadata.rs      # npm metadata generation
│   │   │   └── mod.rs           # npm domain exports
│   │   ├── docker/              # Docker specifications
│   │   │   └── ...
│   │   └── protocol.rs          # Protocol types and common logic
│   └── features/
│       ├── maven_support/       # Maven protocol support
│       │   ├── mod.rs
│       │   ├── use_case.rs
│       │   ├── ports.rs
│       │   ├── adapter.rs
│       │   ├── dto.rs
│       │   ├── error.rs
│       │   ├── event_handler.rs
│       │   ├── di.rs
│       │   ├── mocks.rs
│       │   ├── use_case_test.rs
│       │   └── event_handler_test.rs
│       ├── npm_support/         # npm protocol support
│       │   └── ... (same structure)
│       ├── docker_support/      # Docker protocol support
│       │   └── ... (same structure)
│       └── ...                  # Other protocol supports
├── tests/                       # Integration tests
│   ├── it_maven_test.rs
│   ├── it_npm_test.rs
│   └── ...
└── ...
```

**Key Features:**
- E1.F03: Artifact Metadata Extraction (protocol-specific)
- Maven, npm, Docker, PyPI protocol implementations
- Protocol-specific validation

## 4.4 IAM Crate (iam)

**Responsibility:** Identity and authentication management.

**Structure:**
```
crates/iam/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── domain/
│   │   ├── user.rs             # User entity
│   │   ├── token.rs            # Token management
│   │   ├── mfa.rs              # MFA support
│   │   └── mod.rs              # Domain exports
│   └── features/
│       ├── user_management/    # User management
│       │   ├── mod.rs
│       │   ├── use_case.rs
│       │   ├── ports.rs
│       │   ├── adapter.rs
│       │   ├── dto.rs
│       │   ├── error.rs
│       │   ├── event_handler.rs
│       │   ├── di.rs
│       │   ├── mocks.rs
│       │   ├── use_case_test.rs
│       │   └── event_handler_test.rs
│       ├── oidc_integration/   # OIDC integration
│       │   └── ... (same structure)
│       ├── saml_integration/   # SAML integration
│       │   └── ... (same structure)
│       └── ...
├── tests/                      # Integration tests
│   ├── it_user_management_test.rs
│   ├── it_oidc_test.rs
│   └── ...
└── ...
```

**Key Features:**
- User and group management
- Authentication via local credentials
- Integration with external identity providers
- MFA support

## 4.5 Policies Crate (policies)

**Responsibility:** Policy evaluation and management.

**Structure:**
```
crates/policies/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── domain/
│   │   ├── policy.rs           # Cedar policy model
│   │   ├── hrn_validator.rs    # HRN validation
│   │   ├── evaluation.rs       # Policy evaluation logic
│   │   └── mod.rs              # Domain exports
│   └── features/
│       ├── policy_evaluation/  # Real-time evaluation
│       │   ├── mod.rs
│       │   ├── use_case.rs
│       │   ├── ports.rs
│       │   ├── adapter.rs
│       │   ├── dto.rs
│       │   ├── error.rs
│       │   ├── event_handler.rs
│       │   ├── di.rs
│       │   ├── mocks.rs
│       │   ├── use_case_test.rs
│       │   └── event_handler_test.rs
│       ├── policy_playground/  # Interactive editor
│       │   └── ... (same structure)
│       ├── policy_validation/  # Static validation
│       │   └── ... (same structure)
│       └── ...
├── tests/                      # Integration tests
│   ├── it_policy_evaluation_test.rs
│   ├── it_policy_playground_test.rs
│   └── ...
└── ...
```

**Key Features:**
- Policy evaluation in real-time
- Interactive policy playground
- Static policy validation
- Policy coverage reports

## 4.6 Organization Crate (organization)

**Responsibility:** Hierarchical organization structure management.

**Structure:**
```
crates/organization/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── domain/
│   │   ├── organization.rs     # Organization model
│   │   ├── scp.rs              # Service Control Policies
│   │   ├── hierarchy.rs        # Hierarchical structure
│   │   └── mod.rs              # Domain exports
│   └── features/
│       ├── organization_create/ # Organization creation
│       │   ├── mod.rs
│       │   ├── use_case.rs
│       │   ├── ports.rs
│       │   ├── adapter.rs
│       │   ├── dto.rs
│       │   ├── error.rs
│       │   ├── event_handler.rs
│       │   ├── di.rs
│       │   ├── mocks.rs
│       │   ├── use_case_test.rs
│       │   └── event_handler_test.rs
│       ├── scp_management/     # SCP management
│       │   └── ... (same structure)
│       ├── quota_management/   # Quota management
│       │   └── ... (same structure)
│       └── ...
├── tests/                      # Integration tests
│   ├── it_organization_test.rs
│   ├── it_scp_test.rs
│   └── ...
└── ...
```

**Key Features:**
- Organization and organizational unit management
- Service Control Policies (SCPs)
- Quotas and limits per organization
- Hierarchical policy inheritance

## 4.7 Repository Crate (repository)

**Responsibility:** Repository management and configuration.

**Structure:**
```
crates/repository/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── domain/
│   │   ├── repository.rs       # Repository model
│   │   ├── repo_type.rs        # Types: local, proxy, virtual
│   │   ├── repo_hrn.rs         # Repository HRN
│   │   └── mod.rs              # Domain exports
│   └── features/
│       ├── repository_crud/    # Repository CRUD
│       │   ├── mod.rs
│       │   ├── use_case.rs
│       │   ├── ports.rs
│       │   ├── adapter.rs
│       │   ├── dto.rs
│       │   ├── error.rs
│       │   ├── event_handler.rs
│       │   ├── di.rs
│       │   ├── mocks.rs
│       │   ├── use_case_test.rs
│       │   └── event_handler_test.rs
│       ├── virtual_repository/ # Virtual repositories
│       │   └── ... (same structure)
│       ├── proxy_repository/   # Proxy repositories
│       │   └── ... (same structure)
│       └── ...
├── tests/                      # Integration tests
│   ├── it_repository_test.rs
│   ├── it_virtual_repo_test.rs
│   └── ...
└── ...
```

**Key Features:**
- Repository CRUD operations
- Repository types (local, proxy, virtual)
- Repository-specific policies
- Repository replication

## 4.8 Supply Chain Security Crate (supply-chain)

**Responsibility:** Comprehensive software supply chain security.

**Structure:**
```
crates/supply-chain/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── domain/
│   │   ├── graph.rs            # Dependency graph model
│   │   ├── sbom.rs             # SBOM generation
│   │   ├── vulnerability.rs    # Vulnerability analysis
│   │   └── mod.rs              # Domain exports
│   └── features/
│       ├── dependency_graph/   # Dependency graph
│       │   ├── mod.rs
│       │   ├── use_case.rs
│       │   ├── ports.rs
│       │   ├── adapter.rs
│       │   ├── dto.rs
│       │   ├── error.rs
│       │   ├── event_handler.rs
│       │   ├── di.rs
│       │   ├── mocks.rs
│       │   ├── use_case_test.rs
│       │   └── event_handler_test.rs
│       ├── vulnerability_scan/ # Vulnerability scanning
│       │   └── ... (same structure)
│       ├── sbom_generation/    # SBOM generation
│       │   └── ... (same structure)
│       ├── artifact_signing/   # Artifact signing
│       │   └── ... (same structure)
│       └── ...
├── tests/                      # Integration tests
│   ├── it_dependency_graph_test.rs
│   ├── it_vulnerability_scan_test.rs
│   └── ...
└── ...
```

**Key Features:**
- Dependency graph modeling
- Vulnerability impact analysis
- SBOM generation in multiple formats
- Artifact signing and verification

## 4.9 Search Crate (search)

**Responsibility:** Unified search engine for artifacts and metadata.

**Structure:**
```
crates/search/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── domain/
│   │   ├── query.rs            # Query model
│   │   ├── index.rs            # Index management
│   │   ├── result.rs           # Search results
│   │   └── mod.rs              # Domain exports
│   └── features/
│       ├── basic_search/       # Basic search
│       │   ├── mod.rs
│       │   ├── use_case.rs
│       │   ├── ports.rs
│       │   ├── adapter.rs
│       │   ├── dto.rs
│       │   ├── error.rs
│       │   ├── event_handler.rs
│       │   ├── di.rs
│       │   ├── mocks.rs
│       │   ├── use_case_test.rs
│       │   └── event_handler_test.rs
│       ├── advanced_search/    # Advanced search
│       │   └── ... (same structure)
│       ├── dependency_search/  # Dependency search
│       │   └── ... (same structure)
│       └── ...
├── tests/                      # Integration tests
│   ├── it_basic_search_test.rs
│   ├── it_advanced_search_test.rs
│   └── ...
└── ...
```

**Key Features:**
- Basic and advanced search capabilities
- Dependency-based search
- Search analytics and reporting
- Integration with business intelligence

## 4.10 Shared Crate (shared)

**Responsibility:** Truly shared and stable elements across bounded contexts.

**Structure:**
```
crates/shared/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── domain/
│   │   ├── hrn.rs              # HRN base definition
│   │   ├── events.rs           # Shared domain events
│   │   ├── id.rs               # Shared ID types
│   │   └── mod.rs              # Domain exports
│   ├── auth.rs                 # Authentication traits
│   ├── logging.rs              # Logging configuration
│   ├── error.rs                # Common error types
│   ├── security/
│   │   ├── resources.rs        # HodeiResource trait
│   │   └── mod.rs              # Security exports
│   └── mod.rs                  # Public exports
└── ...
```

**Key Elements:**
- HRN base definition and parser
- Authentication context provider trait
- Common error types and utilities
- Shared domain events
- Logging and tracing configuration
- HodeiResource trait for policy integration
