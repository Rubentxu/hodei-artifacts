# hodei-organizations

Este crate implementa la funcionalidad de gestión de organizaciones, cuentas y políticas de control de servicio (SCPs) siguiendo una arquitectura hexagonal y VSA.

## Estructura

```
src/
├── shared/
│   ├── domain/
│   │   ├── account.rs
│   │   ├── ou.rs
│   │   └── scp.rs
│   ├── application/
│   │   └── ports/
│   │       ├── account_repository.rs
│   │       ├── ou_repository.rs
│   │       └── scp_repository.rs
│   └── infrastructure/
│       └── surreal/
│           ├── account_repository.rs
│           ├── ou_repository.rs
│           └── scp_repository.rs
└── features/
    ├── create_account/
    │   ├── mod.rs
    │   ├── use_case.rs
    │   ├── ports.rs
    │   ├── error.rs
    │   ├── dto.rs
    │   ├── adapter.rs
    │   ├── use_case_test.rs
    │   └── mocks.rs
    ├── create_ou/
    ├── move_account/
    ├── create_scp/
    └── attach_scp/
```

## Features Implementadas

### create_account
Permite crear una nueva cuenta en la organización.

## Próximas Features

- create_ou
- move_account
- create_scp
- attach_scp
