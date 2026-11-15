# OpenAPI (modular)

Estructura modular del contrato OpenAPI con separación de responsabilidades.

- openapi.yaml (raíz, OpenAPI 3.0.3)
- paths/
  - repositories.yaml (v1)
  - artifacts.yaml (v1)
  - search.yaml (v1)
  - maven.yaml (v2/maven2)
  - npm.yaml (v2 npm)
  - pypi.yaml (v2 PyPI)
  - auth.yaml (v2 tokens)
- components/
  - securitySchemes.yaml
  - responses.yaml
  - schemas/
    - index.yaml
    - error.yaml
    - uploadResponse.yaml
    - npmMetadata.yaml
    - npmPublish.yaml
    - pythonUpload.yaml
    - tokens.yaml
    - search.yaml
    - repository.yaml
    - artifact.yaml

Validación y empaquetado: los lleva el gate, `scripts/check-openapi.sh`
(lint con las reglas de `redocly.yaml`, bundle y comparación de deriva
contra `bundled.yaml`). La etapa `contract (openapi)` de `pipeline.kts`
lo ejecuta y falla si el contrato no valida limpio o si el bundle
versionado diverge de las fuentes modulares.

Para regenerar el bundle a mano:

```bash
npx -y @redocly/cli bundle docs/openapi/openapi.yaml -o docs/openapi/bundled.yaml
```

Notas:
- Los endpoints v1 (core) se mantienen para compatibilidad.
- Los endpoints v2 añaden compatibilidad completa con Maven/npm/PyPI/Poetry.
- El spec es OpenAPI **3.0.3**: no tiene `webhooks` (llegaron en 3.1). El
  fichero `webhooks.yaml` que hubo se retiró con su referencia.
- La **publicación** del contrato (explorador de API, Pages, portal de
  desarrolladores) no vive en la CI: la CI es local y no publica nada.
  El entregable es `bundled.yaml`, versionado y en sincronía por el gate;
  publicarlo fuera es una decisión de la persona, separada del gate.
