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
        // The history needs a real start: this stamp is where the duration
        // of the whole run is read from. A stamp still here at the start of
        // the next run means the previous run never reached the report
        // stage, and it is recorded as FAILED (the installed PipelineK has
        // no `post {}` to run a finalizer on failure).
        stage("start (test cost history)") {
            timestamps {
                sh("mkdir -p target/reports/runs")
                sh("python3 scripts/test-cost-history.py pipeline-start --out-dir target/reports")
            }
        }

        stage("validate (format, lint and build)") {
            timestamps {
                sh("cargo fmt --all -- --check")
                sh("cargo clippy --workspace --all-targets -- -D warnings")
                // HSP-314 F2: `legacy` ya no es el defecto; el motor embebido
                // se valida aparte, con la misma regla, hasta F4.
                sh("cargo clippy --workspace --all-targets --features legacy -- -D warnings")
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
                        "--test ci_suites",
                        "--test compose_smoke_contract",
                        "--test test_affected_selection",
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
                        "--test hsp123_migration_pure",
                        "--test hsp124_quota_pure",
                        "--test hsp134_risk_facts",
                    ).joinToString(" ")
                )
            }
        }

        stage("scripts (python contracts)") {
            timestamps {
                // The test cost history and the planner of `test-affected`
                // are scripts, not crates: nothing else runs their tests.
                sh("python3 -m unittest discover -s scripts -p 'test_*.py'")
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
                        "--test atomic_projection",
                        "--test dgraph_profile_failclosed",
                        "--test backend_selection_failclosed",
                        "--test ingestion_contract",
                        "--test ledger_contract",
                        "--test planned_migration",
                        "--test reactions_isolation",
                        "--test relation_deltas",
                        "--test impact_view",
                        "--test intent_store",
                        "--test integration_create_group_comprehensive_test",
                        "--test integration_create_policy_new_test",
                        "--test integration_create_user_comprehensive_test",
                        "--test support_reconciliation",
                        "--test har03_outbox_contract",
                        "--test hsp104_gc_ledger_contract",
                        "--test hsp104_gc_service",
                        "--test hsp174_gc_census_cache",
                        "--test hsp175_precondition_guard",
                        "--test hsp176_capsule_subject_bounded",
                        "--test hsp178_pypi_simple_index_bounded",
                        "--test hsp179_export_portable",
                        "--test hsp146_put_transaction",
                        "--test hsp149_lifecycle",
                        "--test hsp179_keyset_plan",
                        "--test hsp192_gc_coordinated_pass",
                        "--test hsp210_idempotent_import",
                        "--test hsp211_projection_reconcile",
                        "--test hsp212_cutover_rehearsal",
                        "--test hsp154_trust_root_rotation",
                        "--test hsp155_intelligence_sla",
                        "--test hsp166_dataset",
                        "--test intelligence_oracle",
                        "--test slsa_contract",
                        "--test trust_chain_contract",
                        "--test hsp105_trust_ledger_contract",
                        "--test hsp106_provenance_ledger_contract",
                        "--test hsp108_group_contract",
                        "--test hsp109_maven_contract",
                        "--test hsp113_search_contract",
                        "--test freshness_storage",
                        "--test hsp92_surreal_conformance",
                    ).joinToString(" ")
                )
            }
        }

        // Los `*_it` de Dgraph de las slices HSP-187 a HSP-220 (HSP-209 y
        // HSP-192 siguen en su sitio de "integration (database)"). Cada
        // target se salta a si mismo (exit 0) sin HODEI_GRAPH_IT_ENDPOINT,
        // asi que el primer paso declara NOT_RUN de forma visible y deja
        // target/uat/dgraph-it-status.txt: un salto no se hace pasar por
        // verde. Con el endpoint exportado corren contra el Zero+Alpha de
        // deploy/dgraph/compose.yaml. Un solo hilo: las claves propias se
        // leen por membresia sobre el alpha compartido.
        stage("integration (dgraph)") {
            timestamps {
                sh("bash scripts/dgraph-it-gate.sh")
                sh(
                    "cargo test --workspace " + listOf(
                        "--test hsp189_dgraph_graph_it",
                        "--test hsp189_upsert_probe_it",
                        "--test hsp198_quota_dgraph_it",
                        "--test hsp199_dgraph_frontier_it",
                        "--test hsp200_dgraph_bounded_it",
                        "--test hsp201_sessions_dgraph_it",
                        "--test hsp202_dgraph_failclosed_it",
                        "--test hsp202_dgraph_impact_it",
                        "--test hsp203_trust_ledger_dgraph_it",
                        "--test hsp204_dgraph_reachability_it",
                        "--test hsp205_intents_dgraph_it",
                        "--test hsp206_dgraph_investigation_it",
                        "--test hsp208_simulation_dgraph_it",
                        "--test hsp314_dgraph_risk_facts_it",
                        "--test hsp314_f3b0_dgraph_ports_it",
                        "--test hsp314_graph_risk_facts_it",
                        "--test hsp314_graph_evidence_reader_it",
                        "--test hsp314_f3b2_reactor_dgraph_it",
                        "--test hsp314_f3b3_reactor_wiring_it",
                        "--test hsp314_f3c_investigation_wiring_it",
                        "--test hsp220_drain_outbox_it",
                        "--test hsp314_surface_probe",
                    ).joinToString(" ") + " -- --test-threads=1"
                )
                // HSP-314 F2: los IT que montan tiendas a mano con el motor
                // embebido solo se compilan con `--features legacy`; corren en
                // su propio comando (mismo gate de endpoint, un solo hilo).
                sh(
                    "cargo test --workspace --features legacy " + listOf(
                        "--test hsp187_catalog_dgraph_it",
                        "--test hsp190_oci_catalog_dgraph_it",
                        "--test hsp191_maven_dgraph_it",
                        "--test hsp193_npm_dgraph_it",
                        "--test hsp194_pypi_dgraph_it",
                        "--test hsp195_apt_dgraph_it",
                        "--test hsp196_proxy_dgraph_it",
                        "--test hsp197_yum_dgraph_it",
                        "--test hsp207_capsule_dgraph_it",
                        "--test hsp304_formats_dgraph_it",
                    ).joinToString(" ") + " -- --test-threads=1"
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
                        "--test distribution_contract",
                        "--test startup_errors",
                        "--test crash_recovery",
                        "--test har02_catalog_contract",
                        "--test hsp116_localstack_fidelity",
                        "--test hsp116_s3_contract",
                        "--test hsp116_s3_bench",
                        "--test hsp139_localstack_conformance",
                        "--test hsp139_rustfs_conformance",
                        // El recorrido del perfil de grafo (HSP-237, HSP-292):
                        // con HSP237_PROFILE_E2E=1 levanta con podman un
                        // Dgraph real y un S3, arranca el binario en perfil
                        // de grafo y mide la readiness al pararlos; sin la
                        // variable se salta (exit 0; el script lo reporta
                        // como NOT_RUN, exit 77).
                        "--test hsp237_profile_external_e2e",
                        // Imagen OCI reproducible, perfiles declarativos y
                        // smoke de despliegue real con Podman (HSP-242).
                        "--test hsp242_oci_image",
                        // El camino comunitario (HSP-241) sobre el perfil de
                        // grafo: el tarball real sin legacy contra un Dgraph y
                        // un S3 propios levantados con podman (sin podman el
                        // script sale 77 y la prueba lo dice: NOT_RUN visible).
                        // Con la documentacion que lee HSP-243, que no
                        // necesita ni Dgraph ni legacy.
                        "--test hsp241_community_quickstart",
                        "--test hsp243_user_guides",
                        // Release verificable: SBOM SPDX/CycloneDX, provenance,
                        // matriz de licencias y firma OpenPGP (HSP-244).
                        "--test hsp244_verifiable_release",
                        // Instalador y bundle de release verificable
                        // (HSP-259, HSP-263): scripts contra tarballs reales.
                        "--test hsp259_installer",
                        "--test hsp263_release_bundle",
                        // Politica de licencias, estado real de surrealdb,
                        // arbol limpio y coherencia ARCH/target (HSP-289).
                        "--test hsp289_release_policy",
                        // Prueba contra Dgraph real (HSP-181): sin
                        // HODEI_GRAPH_IT_ENDPOINT la suite se salta a si
                        // misma (exit 0); con la variable corre los tres
                        // escenarios contra el Zero+Alpha con ACL y TLS.
                        "--test dgraph_it",
                        // Prueba de identidad HRN y esquema versionado
                        // (HSP-183): sin HODEI_GRAPH_IT_ENDPOINT se salta
                        // a si misma (exit 0); con la variable corre la
                        // convergencia de upserts concurrentes, el arranque
                        // idempotente y los rechazos de esquema
                        // incompatible contra el mismo Zero+Alpha.
                        "--test hsp183_identity_it",
                        // Prueba de guardas transaccionales (HSP-184):
                        // sin HODEI_GRAPH_IT_ENDPOINT se salta a si
                        // misma (exit 0); con la variable corre la
                        // carrera de dos clientes por la misma revision
                        // de tag, la serializacion de diez escritores
                        // por la guarda comun del ambito, la
                        // convergencia de identidad y la repeticion sin
                        // duplicar efecto tras un ACK perdido.
                        "--test hsp184_guards_it",
                        // Prueba del ledger de evidencia secuenciado
                        // (HSP-185): sin HODEI_GRAPH_IT_ENDPOINT se salta
                        // a si misma (exit 0); con la variable reproduce
                        // el contrato del ledger sobre el grafo
                        // (durabilidad tras cerrar el escritor, chequeo
                        // optimista de version, reentrega una sola vez,
                        // cursor sin saltos ni repeticiones, conflicto
                        // entre dos clientes) y la carrera de dos
                        // clientes con ACK perdido contra el mismo
                        // Zero+Alpha.
                        "--test hsp185_ledger_it",
                        // Prueba de IAM sobre Dgraph (HSP-186): sin
                        // HODEI_GRAPH_IT_ENDPOINT se salta; con el perfil
                        // real valida persistencia y aislamiento tras reconexion.
                        "--test hsp186_dgraph_iam_it",
                        // Prueba del EvidenceStore Dgraph (HSP-188): sin
                        // HODEI_GRAPH_IT_ENDPOINT se salta; con el perfil
                        // real valida recibos, identidad, scope e integridad CAS.
                        "--test hsp188_dgraph_evidence_it",
                        // Sesiones OCI reanudables de punta a punta (HSP-201),
                        // sin Dgraph.
                        "--test hsp201_oci_sessions_e2e",
                    ).joinToString(" ")
                )
                // Prueba del libro coordinado de GC (HSP-192): sin
                // HODEI_GRAPH_IT_ENDPOINT se salta; con el perfil real
                // ejercita el claim CAS durable, el zombie con lease
                // vencido, las guardas de referencia en la misma
                // transaccion y el censo paginado por membresia contra
                // el mismo Zero+Alpha. Va en comando aparte porque las
                // claves de GC comparten el alpha con otras slices y
                // necesita un solo hilo.
                sh(
                    "cargo test --workspace --test hsp192_gc_ledger_dgraph_it -- --test-threads=1"
                )
                // El humo del perfil productivo sobre la autoridad unica
                // (HSP-209): sin HODEI_GRAPH_IT_ENDPOINT se salta; con el
                // alpha real arranca el binario con RUN_MODE=dgraph
                // contra el mismo Zero+Alpha y el object store de la
                // serie HSP-116, y fija que Raw, OCI y confianza sirven
                // juntos sobre la autoridad unica, que el arranque no
                // toca RocksDB y que el fallback rechazado muere
                // nombrandolo. La evidencia queda en target/uat/hsp209/.
                // Un solo hilo: las claves propias se leen por membresia
                // sobre el alpha compartido.
                sh(
                    "cargo test --workspace --test hsp209_profile_smoke_it -- --test-threads=1"
                )
                // El cierre productivo (HSP-213): la promesa de que el
                // arbol del binario no enlaza ni surrealdb ni rocksdb se
                // comprueba contra el propio arbol de dependencias, con
                // la contraprueba de que `legacy` sigue trayendolo. El
                // script solo resuelve el tree: el corte lo sostiene el
                // build de la feature.
                sh("bash scripts/check-production-closure.sh")
                // La mitad observable del retiro, con el proceso real:
                // el binario construido SIN `legacy` --el arbol que
                // empaqueta scripts/package-release.sh-- rechaza el
                // perfil embebido con el mensaje de migracion: nombra la
                // retirada, la herramienta y la ventana, y no crea el
                // almacen local. Con las features del arbol el target
                // queda vacio a proposito: el binario default no puede
                // rechazar nada, y una prueba que no afirma nada es peor
                // que no tenerla. En el mismo arbol corre el ensayo de
                // rollback (HSP-213/AC3, maquina de HSP-212): demostrar
                // que volver atras no levanta dos autoridades justo aqui
                // --sin el motor embebido retirado-- es parte de la
                // prueba.
                sh(
                    "cargo test -p hodei-artifacts-api --no-default-features --test hsp213_profile_rejection --test hsp213_rollback_single_authority"
                )
            }
        }

        // HSP-314 F2: `legacy` ya no es el defecto, asi que el motor embebido
        // (SurrealDB sobre RocksDB) tiene su propia etapa. Aqui corren las
        // suites que lo siembran o lo montan y que solo se compilan con
        // `--features legacy` (`required-features` en Cargo.toml). La etapa
        // cubre el motor embebido hasta F4, cuando se retira surrealdb y
        // estas suites migran a [graph] o se borran con el. Cada target
        // vive en exactamente una suite (tests/ci_suites.rs).
        stage("legacy (embedded engine)") {
            timestamps {
                // Los informes de una ejecucion anterior no son evidencia de esta:
                // se borran antes de correr, para que una suite que pase sin
                // escribirlos se note.
                sh("rm -f target/uat/raw-conformity.md target/uat/oci-push-conformity.md target/uat/oci-index-conformity.md target/uat/doc9.md")
                sh(
                    "cargo test --workspace --features legacy " + listOf(
                        "--test binary_contract_embedded",
                        "--test distribution_contract_embedded",
                        "--test startup_errors_embedded",
                        "--test capsule_read",
                        "--test hsp118_bundle_contract",
                        "--test hsp126_capsule_oci",
                        "--test admin_surface",
                        "--test audit_events",
                        "--test http_contract",
                        "--test iam_account_index",
                        "--test iam_audit",
                        "--test iam_backfill",
                        // La readiness y la liveness son dos preguntas
                        // distintas (HSP-237): con el almacen desconectado
                        // en caliente, la readiness deja de ser 200 y la
                        // liveness no. Arranca el motor embebido.
                        "--test hsp237_readiness",
                        "--test iam_policy_timestamps",
                        "--test iam_scope",
                        "--test playground_isolation",
                        "--test policy_circuits",
                        "--test repository_create",
                        "--test repository_durable",
                        "--test repository_read",
                        "--test schema_persistence",
                        "--test har04_raw_hosted_contract",
                        "--test hsp113_search_e2e",
                        // La superficie HTTP del cursor keyset (HSP-222):
                        // el contrato vive en el almacen (HSP-191) y los
                        // IT de Dgraph lo ejercitan (HSP-147); lo que se
                        // fija aqui es que /api/v1/search lo acepta, lo
                        // propaga, lo echo, rechaza por su nombre el que
                        // no se lee o el de otra cuenta y no degrada a
                        // offset. Almacen falso in-process: lane rapida.
                        "--test hsp222_search_cursor_surface",
                        "--test hsp114_console_contract",
                        "--test hsp132_registry_list",
                        "--test hsp88_investigation_query",
                        "--test hsp89_investigation_freshness",
                        "--test hsp134_risk_facts_e2e",
                        "--test hsp142_capability_matrix",
                        "--test hsp243_user_guides_recipes",
                        "--test hsp288_native_guides",
                        "--test bootstrap_integration",
                        "--test production_auth",
                        "--test repository_restart_contract",
                        "--test raw_hosted_restart_contract",
                        "--test raw_conformity_uat",
                        "--test blob_integrity_contract",
                        "--test doc9_uat",
                        "--test oci_pull_contract",
                        "--test oci_push_contract",
                        "--test oci_index_contract",
                        "--test hsp104_gc_e2e",
                        "--test hsp115_recovery_e2e",
                        "--test hsp223_registry_lab",
                        "--test hsp224_maven_central_real",
                        "--test hsp225_npm_registry_real",
                        "--test hsp226_pypi_registry_real",
                        "--test hsp227_raw_external",
                        "--test hsp228_oci_proxy",
                        "--test hsp229_oci_group",
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
                        // La direccion publica del despliegue (HSP-238): lo
                        // que el registro anuncia lo decide la
                        // configuracion y no la peticion.
                        "--test hsp238_public_url",
                        "--test hsp143_namespaces_e2e",
                        "--test hsp143_npm_namespaces_cli",
                        "--test hsp119_cargo_e2e",
                        "--test hsp119_cargo_cli",
                        "--test hsp120_dotnet_client",
                        "--test hsp120_nuget_e2e",
                        "--test hsp121_go_client",
                        "--test hsp121_go_e2e",
                        "--test hsp232_go_group",
                        // Modulo publico real a traves del group Go, verificado contra
                        // sum.golang.org y un h1 fijado; muta el zip (HSP-300).
                        "--test hsp300_go_sumdb",
                        // La readiness del perfil de grafo (HSP-237): con un
                        // Dgraph de mentira que cae y se recupera, la
                        // readiness depende del alpha y la liveness no.
                        "--test hsp237_graph_readiness",
                        // El timeout HTTP de la readiness (HSP-292): un probe
                        // que no contesta da 503 con timeout en menos de
                        // 2,5 s, sin Dgraph ni S3.
                        "--test hsp292_readiness_timeout",
                        "--test hsp121_helm_oci",
                        "--test hsp122_apt_client",
                        "--test hsp122_apt_e2e",
                        "--test hsp122_yum_client",
                        "--test hsp122_yum_e2e",
                        // El proxy rpm (HSP-235): el repodata del upstream se
                        // sirve tal cual, con su checksum comprobado.
                        "--test hsp235_rpm_proxy",
                        // Los formatos adicionales: mirror y registry de
                        // Terraform, Conan, Ansible Galaxy y Hugging Face.
                        // Los clientes reales (terraform, conan,
                        // ansible-galaxy, hf) NO se saltan: la suite hace
                        // assert de que la herramienta existe y falla si
                        // falta. El mirror de Terraform se prueba por
                        // protocolo (curl y un upstream falso), no con el
                        // cliente terraform.
                        "--test hsp157_terraform_mirror",
                        "--test hsp296_formats_audit",
                        "--test hsp297_format_prefixes",
                        "--test hsp297_trusted_listings",
                        "--test hsp158_terraform_modules_client",
                        "--test hsp159_conan_client",
                        "--test hsp217_galaxy_client",
                        "--test hsp218_hf_client",
                        // Residuales de la pila de formatos (HSP-304): terraform
                        // init real contra el mirror sobre https verificado
                        // (NOT_RUN visible si faltan terraform/socat/openssl) y
                        // la matriz 401/403/cuenta ajena/413 de las escrituras.
                        "--test hsp304_terraform_init",
                        "--test hsp304_format_access",
                        // URLs de zip firmadas del mirror de providers: ligadas a
                        // ruta, repositorio, cuenta y expiracion; rotacion; falla
                        // cerrado sin clave; sin secretos en el log.
                        "--test hsp313_signed_archive_url",
                        "--test hsp123_migration_e2e",
                        "--test hsp124_quota_e2e",
                        "--test hsp125_replication_spike",
                        "--test hsp135_vigencia_e2e",
                        "--test hsp137_ciclo_cve_e2e",
                        "--test hsp138_s3_arranque_e2e",
                    ).joinToString(" ")
                )
                // Y la otra cara (HSP-213): en el arbol con `legacy` --el
                // de la herramienta aislada que empaqueta
                // scripts/package-legacy-tools.sh y el de las suites
                // historicas-- el mismo perfil sigue arrancando y
                // sirviendo /health. Un solo hilo: levanta el almacem
                // embebido y sondea su puerto.
                sh(
                    "cargo test -p hodei-artifacts-api --features legacy --test hsp213_embedded_serves -- --test-threads=1"
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
                    "cargo test --workspace --features legacy --test oci_e2e_contract -- --test-threads=1 " +
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

        stage("report (test cost history)") {
            timestamps {
                // Duration is the wall time since the start stamp and status
                // is `auto`: this stage is only reached when every earlier
                // stage passed. Nothing here is typed in by hand.
                sh(
                    "python3 scripts/test-cost-history.py record " +
                        "--source-type pipeline " +
                        "--stage pipeline " +
                        "--phase execution " +
                        "--status auto " +
                        "--started-at-file target/reports/.pipeline-start.json " +
                        "--suite 'pipeline:full' " +
                        "--out-dir target/reports " +
                        "--render"
                )
                archiveArtifacts(
                    artifacts = "target/reports/index.html",
                    allowEmptyArchive = false,
                )
            }
        }
    }
}
