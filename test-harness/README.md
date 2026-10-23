# Harness Base mínimo — escenarios temporales fuera de Git

Esta base facilita sesiones reproducibles de aceptación; **no contiene suites UAT/E2E/AAT permanentes**. Fuente normativa: https://github.com/Rubentxu/agent-skill/tree/main/skills/delivery-first-testing y `SKILL.md`/`references/lenses/ephemeral-uat.md`/`regression-promotion.md`.

Antes de usar, comparar y preferir la plantilla actual upstream `skills/delivery-first-testing/assets/test-harness-base/` si aporta mejor control de PID, recursos y recibos. Esta copia mínima sólo ofrece utilidades y se debe revisar sobre Linux/Podman de ejecución. No crea un runner CI distinto.

Ejemplo de script **creado efímeramente** en `.sessions/<run>/accept.sh` por el agente:

```bash
#!/usr/bin/env bash
set -Eeuo pipefail
export TH_TARGET_ID="<sha-del-binario-validado>"
source test-harness/lib/harness.sh
th_init
th_capture health curl --fail-with-body --silent http://127.0.0.1:"$TEST_PORT"/health
th_assert_contains "$TH_SESSION/health.log" 'ok'
# ... ejecutar binario actual/clientes reales, asserts negativos, evidencias
```

Las rutas `<...>` y `TEST_PORT` son placeholders a resolver por sesión. `th_capture` es para binarios existentes y no levanta TypeDB. No ejecutar este ejemplo sin adecuar puerto, identidad y servicio. Los casos se generan/destruyen en cada hito. Un resultado verde exige cleanup completo; si herramientas/cliente/target faltan el resultado es `BLOCKED` y no debe publicarse release. Consultar `docs/roadmaps/hodei-artifacts-graph-first/TESTING.md` y `ACCEPTANCE.md`.
