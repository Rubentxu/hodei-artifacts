# Legacy test suites — quarantined (H0)

These files are **not compiled and not executed** by Cargo. They live under
`tests/legacy/`, which has no `main.rs`, so Cargo never turns it into a test
target. This is deliberate: the suites below drifted away from the current
API long before this directory existed, and nothing in the repository ever
noticed, because the root package shipped as a **binary only** — the suites
referenced `hodei_artifacts_api::…` paths that did not exist, so no test
target was ever produced for them.

They are **not deleted**. They are the record of intent for a set of
behaviours, and they are the specification that the permanent contract in
`tests/http_contract.rs` was written against.

## How the drift was measured

The suites were compiled once, on purpose, with a throwaway probe target
(`tests/_legacy_probe.rs`, deleted afterwards) that pulled the modules into a
test target. The compiler output is the evidence quoted below:

| Suite | Compile errors |
|---|---|
| `legacy/e2e/` | 9 |
| `legacy/integration/` + `legacy/common/` | 127 |

## `legacy/e2e/` — 46 test functions

| File | Tests | Blocked by |
|---|---|---|
| `e2e/mod.rs` | 1 | `config::Config` is private (renamed to `AppConfig`); `bootstrap()` takes 2 arguments now, the suite calls it with 1; `hyper` and `tokio_test` are not dependencies of the root package |
| `e2e/health.rs` | 10 | `futures` is not a dependency of the root package; `StatusCode` is not imported in the module that uses it |
| `e2e/policies.rs` | 14 | `futures` is not a dependency of the root package |
| `e2e/playground.rs` | 11 | depends on the `test_utils` module of `e2e/mod.rs` |
| `e2e/schemas.rs` | 10 | `futures` is not a dependency of the root package; the request body (`entity_types`, `action_types`) does not match `BuildSchemaRequest` (`version`, `validate`) |

**The health expectations are also self-contradictory** and must not be
restored verbatim:

- `test_health_ready` expects `status == "ready"`
- `test_health_live` expects `status == "live"`
- `test_health_consistency` expects all three probes to answer the *same*
  status

The single handler behind `/health`, `/health/ready` and `/health/live`
answers `"healthy"` for all three. Two of those three expectations cannot
hold at the same time. The permanent contract therefore asserts what the
service can actually guarantee — the three probes are served and report a
status — instead of inventing a per-probe vocabulary that the implementation
does not have.

## `legacy/integration/` — 47 test functions

| File | Tests | Blocked by |
|---|---|---|
| `integration/mod.rs` | 0 | module root |
| `integration/test_policy_lifecycle.rs` | 8 | see the shared blockers below |
| `integration/test_create_policy.rs` | 10 | see the shared blockers below |
| `integration/test_get_policy.rs` | 9 | see the shared blockers below |
| `integration/test_concurrency.rs` | 8 | see the shared blockers below |
| `integration/test_list_policies.rs` | 6 | see the shared blockers below |
| `integration/test_delete_policy.rs` | 3 | see the shared blockers below |
| `integration/test_update_policy.rs` | 3 | see the shared blockers below |

The suite declares **47** test functions.

Shared blockers, all measured:

- `kernel::HodeiPolicy` does not exist any more — the type moved to
  `kernel::domain::policy::HodeiPolicy`.
- `testcontainers` and `testcontainers-modules` are **not** declared as
  dependencies of the root package (they only exist in `[workspace.dependencies]`).
- `tracing_test::traced_test` is **not** a dependency of the root package.
- `surrealdb::engine::remote::ws::Client` requires the `protocol-ws` feature;
  the root package enables only `kv-mem` and `kv-rocksdb`.
- `use crate::common::…` cannot resolve: as an integration test target, the
  crate root is the test file itself, and `tests/common/mod.rs` is not a
  target because it has no `main.rs`.
- The suite needs a SurrealDB server in Docker (`testcontainers`), and
  **Docker is not available in this environment**. Even after the source
  drift is fixed, these tests would be `NOT_RUN` here.

## `legacy/common/` — 16 test functions

| File | Tests | Blocked by |
|---|---|---|
| `common/mod.rs` | 0 | module root |
| `common/fixtures.rs` | 7 | shared fixtures; only meaningful together with the two suites above |
| `common/helpers.rs` | 5 | shared helpers; only meaningful together with the two suites above |
| `common/test_db.rs` | 4 | testcontainers database setup; needs Docker, which is not available here |

## What is covered permanently today

`tests/http_contract.rs` (8 tests) covers the guarantees that survived the
drift, in-process and without any external service:

| Guarantee | Where it used to live |
|---|---|
| `/health` reports `healthy` and identifies the service | `legacy/e2e/health.rs` |
| `/health/ready` and `/health/live` are served | `legacy/e2e/health.rs` |
| an unknown route is a `404` | — (new) |
| a valid Cedar policy validates | `legacy/e2e/policies.rs` |
| an invalid Cedar policy is rejected with errors | `legacy/e2e/policies.rs` |
| the IAM policy lifecycle works end to end | `legacy/integration/test_policy_lifecycle.rs` |
| a duplicate policy is a conflict | `legacy/integration/test_create_policy.rs` |
| a policy without content is a bad request | `legacy/integration/test_create_policy.rs` |

`tests/legacy_quarantine.rs` fails if a file is added to this directory
without listing it in this report, so the quarantine cannot silently lose a
guarantee.

## Restoring a suite

1. Fix the source drift listed above (imports, renamed types, new
   signatures).
2. Declare the missing dependencies **for the root package** — including the
   `protocol-ws` feature on `surrealdb` — and move the file out of
   `tests/legacy/`.
3. Provide Docker (or replace the testcontainers setup with the embedded
   RocksDB engine the application already uses in production) and record the
   run as `NOT_RUN` while Docker is missing.
4. Update this report and `tests/legacy_quarantine.rs` in the same commit.

Do **not** change production behaviour to satisfy these suites: the delivery
rule of this project is that a red test is classified (real defect, changed
contract, coupled test, infrastructure, fixture, flaky, stale) and only a
real defect is fixed in the product.
