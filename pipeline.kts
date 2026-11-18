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
//   e2e            la compatibilidad con clientes de verdad y con la suite
//                  oficial del `distribution-spec`
//   frontend       los componentes, que necesitan otro lenguaje y otro toolchain
//
// La conformidad de caja negra escribe su informe en
// `target/uat/raw-conformity.md`. Ese informe es la evidencia: una
// conformidad que solo existe en el log de una máquina se pierde en
// cuanto la máquina se limpia, y una suite que pasa sin escribir el
// informe no ha hecho su trabajo. Por eso el informe se archiva, y su
// ausencia hace fallar la etapa.
//
// El push de OCI Distribution tiene **su** informe, en
// `target/uat/oci-push-conformity.md`, y también se archiva. Son dos
// superficies y dos informes: un informe que certifica casos de OCI con
// el título de la conformidad cruda dice una cosa y su título otra.
//
// Y el índice, las etiquetas y los referrers tienen el suyo, en
// `target/uat/oci-index-conformity.md`. Son tres superficies y tres
// informes, y cada uno dice lo que **no** certifica: el del índice dice
// que el registro no valida firmas criptográficas, y el del push no
// puede decir lo mismo porque no las ha visto.
//
// La compatibilidad con clientes de verdad es una etapa aparte, porque
// necesita herramientas que el resto no necesita: la suite oficial del
// `distribution-spec` (que se compila con Go), el `podman`, el `docker`
// de la CLI y el `oras`. Su informe está en
// `target/uat/oci-compatibility.md`, y el detalle por prueba de la suite
// oficial, en `target/uat/oci-conformance/`. El contrato que dice qué se
// ha medido y con qué versiones está en `uat/UAT-OCI.md`.
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
                        "--test hsp92_result_contract",
                        "--test hsp92_conformance",
                        "--test m7_uat",
                        "--test investigator_contract",
                        "--test deterministic_contract",
                        "--test freshness_contract",
                        "--test m8_uat",
                        "--test hub_contract",
                        "--test m9_uat",
                        "--test har01_cas_contract",
                        "--test hsp104_reclaim_contract",
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
                        "--test hsp93_counterfactual",
                        "--test hsp116_parity_suite",
                        "--test hsp118_bundle_contract",
                        "--test hsp123_migration_pure",
                        "--test hsp124_quota_pure",
                        "--test hsp126_capsule_oci",
                        "--test hsp134_risk_facts",
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
                        "--test hsp104_gc_ledger_contract",
                        "--test hsp104_gc_service",
                        "--test hsp105_trust_ledger_contract",
                        "--test hsp106_provenance_ledger_contract",
                        "--test hsp108_group_contract",
                        "--test hsp109_maven_contract",
                        "--test hsp113_search_contract",
                        "--test hsp113_search_e2e",
                        "--test hsp114_console_contract",
                        "--test hsp132_registry_list",
                        "--test hsp88_investigation_query",
                        "--test hsp89_investigation_freshness",
                        "--test freshness_storage",
                        "--test hsp92_surreal_conformance",
                        "--test hsp134_risk_facts_e2e",
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
                        "--test oci_push_contract",
                        "--test oci_index_contract",
                        "--test hsp104_gc_e2e",
                        "--test hsp115_recovery_e2e",
                        "--test hsp105_trust_e2e",
                        "--test hsp106_proxy_e2e",
                        "--test hsp107_proxy_cache_e2e",
                        "--test hsp108_group_e2e",
                        "--test hsp109_maven_e2e",
                        "--test hsp109_maven_cli",
                        "--test hsp110_maven_remote_e2e",
                        "--test hsp110_maven_cli",
                        "--test hsp111_npm_e2e",
                        "--test hsp111_npm_cli",
                        "--test hsp112_pypi_e2e",
                        "--test hsp112_pypi_cli",
                        "--test hsp116_localstack_fidelity",
                        "--test hsp116_s3_contract",
                        "--test hsp116_s3_bench",
                        "--test hsp119_cargo_e2e",
                        "--test hsp119_cargo_cli",
                        "--test hsp120_dotnet_client",
                        "--test hsp120_nuget_e2e",
                        "--test hsp121_go_client",
                        "--test hsp121_go_e2e",
                        "--test hsp121_helm_oci",
                        "--test hsp122_apt_client",
                        "--test hsp122_apt_e2e",
                        "--test hsp122_yum_client",
                        "--test hsp122_yum_e2e",
                        "--test hsp123_migration_e2e",
                        "--test hsp124_quota_e2e",
                        "--test hsp125_replication_spike",
                        "--test hsp135_vigencia_e2e",
                    ).joinToString(" ")
                )
                sh(
                    "if [ ! -f target/uat/raw-conformity.md ]; then " +
                        "echo 'the conformity report is missing: the suite did not write it' >&2; " +
                        "exit 1; fi"
                )
                // El informe del push OCI es la evidencia de la otra
                // mitad de la distribucion, y es **otro** informe: el de
                // la superficie cruda lo leen los gates de la
                // conformidad RAW, y un caso de OCI en el no dice nada
                // de lo que el gate cree.
                sh(
                    "if [ ! -f target/uat/oci-push-conformity.md ]; then " +
                        "echo 'the OCI push report is missing: the suite did not write it' >&2; " +
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
                // Y el del push OCI, que dice que el protocolo se habla
                // con un cliente de verdad.
                sh("cat target/uat/oci-push-conformity.md")
                archiveArtifacts(
                    artifacts = "target/uat/oci-push-conformity.md",
                    allowEmptyArchive = false,
                )
                // Y el del indice, las etiquetas y los referrers, que es
                // **otro** informe otra vez: el del push certify el
                // push, y un caso de referrers en el no dice nada de lo
                // que el gate cree.
                sh(
                    "if [ ! -f target/uat/oci-index-conformity.md ]; then " +
                        "echo 'the OCI index report is missing: the suite did not write it' >&2; " +
                        "exit 1; fi"
                )
                sh("cat target/uat/oci-index-conformity.md")
                archiveArtifacts(
                    artifacts = "target/uat/oci-index-conformity.md",
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

        stage("e2e (real clients and conformance)") {
            timestamps {
                // Las herramientas primero: una que falta se ve como un
                // fallo del registro, y el `assert` que falla no dice
                // que lo que falta es el `podman`.
                sh("bash scripts/oci-e2e-preflight.sh")
                // La suite oficial de conformidad no está en el
                // repositorio: es código de OpenContainers, con su
                // licencia y su ritmo. Lo que se fija aquí es el commit
                // exacto del que se usa y el digest de sus fuentes, y lo
                // que se ejecuta se compila antes de correr.
                sh("bash scripts/oci-conformance.sh")
                // El informe de una ejecución anterior no es la evidencia
                // de esta: se borra antes de correr.
                sh("rm -rf target/uat/oci-compatibility.md target/uat/oci-conformance")
                sh("mkdir -p target/uat")
                // Un solo hilo: los casos eligen el puerto al azar y el
                // almacén de imágenes del `podman` es de la máquina, y
                // dos casos a la vez se pisan el puerto y el almacén. La
                // suite tarda más, y lo que se mide se mide igual.
                //
                // Y la suite toma, ademas, un cerrojo para las
                // construcciones de imagen: dos `podman build` a la vez se
                // pelean por el `fuse-overlayfs`, el desmontaje se queda
                // esperando y la máquina se queda colgada.
                //
                // El código de salida se guarda en un fichero porque cada
                // `sh` es una consola distinta: el veredicto se da al
                // final, con la evidencia ya archivada.
                sh(
                    "cargo test --workspace --test oci_e2e_contract -- --test-threads=1 " +
                        "> target/uat/oci-e2e.log 2>&1; echo $? > target/uat/oci-e2e.status"
                )
                // El log y el informe se leen **antes** de fallar: la
                // evidencia de un fallo es justo lo que hay que poder
                // leer, y un fallo que se lleva la evidencia consigo no
                // deja nada que mirar.
                sh("cat target/uat/oci-e2e.log")
                sh("cat target/uat/oci-compatibility.md || true")
                archiveArtifacts(
                    artifacts = "target/uat/oci-e2e.log",
                    allowEmptyArchive = true,
                )
                archiveArtifacts(
                    artifacts = "target/uat/oci-compatibility.md",
                    allowEmptyArchive = true,
                )
                // Y el detalle de la suite oficial: el informe declara
                // las categorías, y estos son los resultados de cada
                // prueba.
                archiveArtifacts(
                    artifacts = "target/uat/oci-conformance",
                    allowEmptyArchive = true,
                )
                // Y ahora el veredicto, con la evidencia ya guardada: la
                // suite en verde y el informe escrito.
                sh(
                    "if [ \"$(cat target/uat/oci-e2e.status)\" != \"0\" ]; then " +
                        "echo 'the OCI E2E suite did not pass: the log is target/uat/oci-e2e.log' >&2; " +
                        "exit 1; fi"
                )
                sh(
                    "if [ ! -f target/uat/oci-compatibility.md ]; then " +
                        "echo 'the OCI compatibility report is missing: the suite did not write it' >&2; " +
                        "exit 1; fi"
                )
                // Y el informe tiene que estar **completo**. Un informe
                // que existe pero a quien le faltan casos--porque un caso
                // se renombró, se marcó como ignorado o se borró-- es el
                // informe de una suite que no es la que esta etapa cree.
                // El fichero se archiva antes: aunque este paso falle,
                // el informe de lo que pasó se puede leer.
                sh("bash scripts/check-oci-compatibility.sh target/uat/oci-compatibility.md")
            }
        }

        stage("frontend (components)") {
            timestamps {
                dir("frontend") {
                    sh("npm ci --ignore-scripts")
                    sh("npm test")
                    sh("npm run type-check")
                    // La consola contra el servicio de verdad. El binario
                    // lo construye la etapa de validacion, y el navegador
                    // se instala aqui porque `npm ci --ignore-scripts` no
                    // lo descarga. La instalacion no hace nada cuando el
                    // navegador ya esta.
                    sh("npx playwright install chromium")
                    sh("npx playwright test")
                }
            }
        }
    }
}
