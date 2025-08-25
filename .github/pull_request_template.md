## Descripción

Por favor, asegúrese de que este pull request cumple con los siguientes requisitos:

*   El título del pull request sigue la convención de commits. Consulte [docs/commits.md](docs/commits.md) para más información.
*   Los commits individuales siguen la convención de commits.
*   No se incluyen líneas `Signed-off-by:` ni `Co-authored-by:` en los mensajes de commit.

## Tipo de cambio

Por favor, seleccione el tipo de cambio que introduce este pull request:

*   \[ ] Feature (nueva característica)
*   \[ ] Fix (corrección de error)
*   \[ ] Refactor (cambio interno sin alterar el comportamiento)
*   \[ ] Performance (mejora de rendimiento)
*   \[ ] Test (añadir o mejorar tests)
*   \[ ] Docs (cambios en la documentación)
*   \[ ] Build (cambios en el sistema de construcción)
*   \[ ] Chore (tareas de mantenimiento)
*   \[ ] CI (cambios en la integración continua)
*   \[ ] Revert (reversión de un commit)

## Checklist

### General
*   \[ ] He revisado mi propio código.
*   \[ ] He añadido tests unitarios para cubrir mis cambios.
*   \[ ] He actualizado la documentación (si es necesario).

### Features (si aplica)
*   \[ ] Command/Query definido y documentado.
*   \[ ] Validaciones puras con tests unitarios.
*   \[ ] UseCase ejecuta orden canónico (validate → dedupe → side-effects).
*   \[ ] Handler sin lógica de negocio (≤ 40 LOC).
*   \[ ] Idempotencia implementada y testada (si aplica).
*   \[ ] Evento con correlation id y payload completo.
*   \[ ] Métricas básicas (counter + histogram).
*   \[ ] Spans de tracing configurados.
*   \[ ] Errores mapeados exhaustivamente.
*   \[ ] Sin `#[cfg(test)]` en `src/**`.
*   \[ ] OpenAPI actualizado con nuevos endpoints.
*   \[ ] Tests: unitarios (validaciones) + integración (flujo completo).
*   \[ ] Coverage ≥ 85% en módulos logic/.

Consultar [docs/feature-style-guide.md](../docs/feature-style-guide.md) para detalles.
