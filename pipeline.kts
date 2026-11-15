// La CI de este repositorio es local, y la lleva PipelineK.
//
// Antes la llevaba GitHub Actions, y eso significaba dos cosas malas: los
// gates no se podían ejecutar sin enviar el código a otro sitio, y el
// repositorio tenía dos definiciones de la misma política —una aquí y otra
// en un YAML de otro repositorio—. Ahora hay una sola, y está aquí.
//
// La segmentación de las suites no es una decoración: cada suite se
// ejecuta **exactamente una vez**, y `tests/ci_suites.rs` la comprueba
// contra los tests que existen en disco, en los dos sentidos. Si una suite
// nombra un target que no existe, el gate falla; si un target no lo nombra
// ninguna suite, el gate falla. Un target que se queda fuera de todas las
// suites deja de ejecutarse sin que nadie se entere, y por eso el
// guardián existe.
//
// Las etapas van por responsabilidad observable:
//
//   validate       ¿el código está formatted, sin avisos y compila?
//   fast           los contratos y las pruebas unitarias, sin base ni procesos
//   doctests       los ejemplos del código, que no son un target y nadie los ve
//   integration    las pruebas que usan la base de datos embebida
//   infrastructure las que arrancan el servicio, lo reinician o lo ejecutan
//                  como proceso, y la conformidad de caja negra
//   frontend       los componentes, que necesitan otro lenguaje y otro toolchain
//
// La conformidad de caja negra escribe su informe en
// `target/uat/raw-conformity.md`. Ese informe es la evidencia: una
// conformidad que solo existe en el log de una máquina se pierde en
// cuanto la máquina se limpia, y una suite que pasa sin escribir el
// informe no ha hecho su trabajo. Por eso el informe se archiva, y su
// ausencia hace fallar la etapa.
//
// La suite con `nextest` queda fuera, a propósito: la ejecuta
// `cargo nextest run --workspace --no-fail-fast --test-threads=1`, que es
// el aislamiento por prueba, y las pruebas de RocksDB y de binario
// necesitan un solo hilo. Duplicaría la cobertura en cada ejecución sin
// estar en el camino de la entrega.

pipeline {
    stages {
        stage("validate (format, lint and build)") {
            timestamps {
                sh("cargo fmt --all -- --check")
                sh("cargo clippy --workspace --all-targets -- -D warnings")
                sh("cargo build --workspace")
            }
        }

        stage("contract (openapi)") {
            timestamps {
                // El contrato de la API es producto: quien lo edita y el
                // gate tienen que ver las mismas reglas (redocly.yaml), y
                // el bundle versionado tiene que estar en sincronía con
                // las fuentes modulares. La comprobación vive en un script
                // porque encadena lint, bundle y deriva con códigos de
                // salida propios.
                sh("bash scripts/check-openapi.sh")
            }
        }

        stage("fast (unit and contract)") {
            timestamps {
                sh(
                    "cargo test --workspace --lib --bins " + listOf(
                        "--test authorization_isolation",
                        "--test outbox_flow",
                        "--test planned_isolation",
                        "--test architecture_fitness",
                        "--test capsule_contract",
                        "--test trust_policy_contract",
                        "--test promotion_contract",
                        "--test m3_uat",
                        "--test risk_contract",
                        "--test m4_uat",
                        "--test reachability_contract",
                        "--test reactive_contract",
                        "--test simulation_contract",
                        "--test base_contract",
                        "--test m7_uat",
                        "--test investigator_contract",
                        "--test m8_uat",
                        "--test hub_contract",
                        "--test m9_uat",
                        "--test har01_cas_contract",
                        "--test m6_uat",
                        "--test m5_uat",
                        "--test capsule_read",
                        "--test ci_suites",
                        "--test evidence_contract",
                        "--test graph_contract",
                        "--test m1_uat",
                        "--test m2_uat",
                        "--test graph_projection",
                        "--test hrn_migration",
                        "--test hrn_parity",
                        "--test legacy_quarantine",
                        "--test module_tree",
                        "--test scale_model",
                        "--test standards_contract",
                    ).joinToString(" ")
                )
            }
        }

        stage("fast doctests") {
            timestamps {
                sh("cargo test --workspace --doc")
            }
        }

        stage("integration (database)") {
            timestamps {
                sh(
                    "cargo test --workspace " + listOf(
                        "--test admin_surface",
                        "--test atomic_projection",
                        "--test audit_events",
                        "--test http_contract",
                        "--test iam_account_index",
                        "--test iam_audit",
                        "--test ingestion_contract",
                        "--test ledger_contract",
                        "--test planned_migration",
                        "--test reactions_isolation",
                        "--test relation_deltas",
                        "--test iam_backfill",
                        "--test iam_policy_timestamps",
                        "--test iam_scope",
                        "--test impact_view",
                        "--test intent_store",
                        "--test integration_create_group_comprehensive_test",
                        "--test integration_create_policy_new_test",
                        "--test integration_create_user_comprehensive_test",
                        "--test playground_isolation",
                        "--test policy_circuits",
                        "--test repository_create",
                        "--test repository_durable",
                        "--test repository_read",
                        "--test schema_persistence",
                        "--test support_reconciliation",
                        "--test har03_outbox_contract",
                        "--test har04_raw_hosted_contract",
                    ).joinToString(" ")
                )
            }
        }

        stage("infrastructure (boot and processes)") {
            timestamps {
                // El informe de una ejecución anterior no es evidencia de
                // esta: se borra antes de correr, para que una suite que
                // pase sin escribirlo se note.
                sh("rm -rf target/uat")
                sh(
                    "cargo test --workspace " + listOf(
                        "--test binary_contract",
                        "--test bootstrap_integration",
                        "--test crash_recovery",
                        "--test distribution_contract",
                        "--test production_auth",
                        "--test repository_restart_contract",
                        "--test har02_catalog_contract",
                        "--test raw_hosted_restart_contract",
                        "--test raw_conformity_uat",
                        "--test blob_integrity_contract",
                        "--test doc9_uat",
                        "--test oci_pull_contract",
                    ).joinToString(" ")
                )
                sh(
                    "if [ ! -f target/uat/raw-conformity.md ]; then " +
                        "echo 'the conformity report is missing: the suite did not write it' >&2; " +
                        "exit 1; fi"
                )
                // La UAT de doc-9 trae sus dieciseis escenarios, cada uno
                // con su comando y su resultado. La cuenta se comprueba
                // aqui y no en el test, porque el binario de pruebas no
                // garantiza el orden: un escenario que no se ejecute se
                // nota cuando el binario ha terminado.
                sh(
                    "if [ ! -f target/uat/doc9.md ]; then " +
                        "echo 'the doc-9 report is missing: the suite did not write it' >&2; " +
                        "exit 1; fi"
                )
                // La cuenta de escenarios: el informe tiene que traer
                // los dieciseis, cada uno una vez. La comprobacion
                // vive en un script porque el DSL de la pipeline no
                // admite variables de shell en los literales, y una
                // comprobacion con variables es lo que hace falta.
                sh("bash scripts/check-doc9-uat.sh target/uat/doc9.md")
                // El informe se lee en la salida de la ejecución: una
                // conformidad que nadie puede leer no es una conformidad.
                sh("cat target/uat/raw-conformity.md")
                archiveArtifacts(
                    artifacts = "target/uat/raw-conformity.md",
                    allowEmptyArchive = false,
                )
                // Y el de la UAT de doc-9, que es la evidencia de la
                // puerta del hito: dieciseis escenarios, cada uno con su
                // comando y su resultado, y la lista de lo que no se ha
                // podido certificar.
                sh("cat target/uat/doc9.md")
                archiveArtifacts(
                    artifacts = "target/uat/doc9.md",
                    allowEmptyArchive = false,
                )
            }
        }

        stage("frontend (components)") {
            timestamps {
                dir("frontend") {
                    sh("npm ci --ignore-scripts")
                    sh("npm test")
                    sh("npm run type-check")
                }
            }
        }
    }
}
