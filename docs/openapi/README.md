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

Validación y empaquetado (ejemplos):

- Swagger CLI
```bash
npx -y swagger-cli validate docs/openapi/openapi.yaml
npx -y swagger-cli bundle docs/openapi/openapi.yaml -o openapi.bundled.yaml -t yaml
```

- Redocly
```bash
npx -y @redocly/cli lint docs/openapi/openapi.yaml
npx -y @redocly/cli bundle docs/openapi/openapi.yaml -o openapi.bundled.yaml
```

Notas:
- Los endpoints v1 (core) se mantienen para compatibilidad.
- Los endpoints v2 añaden compatibilidad completa con Maven/npm/PyPI/Poetry.
