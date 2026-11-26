# Scripts Directory

This directory contains utility scripts for the Hodei Artifacts API project.

## test-affected.sh

La puerta del día a día: corre **solo** las pruebas de lo que ha cambiado.

La pipeline de `pipeline.kts` es la puerta de la entrega --ocho etapas sobre
el workspace entero-- y por eso no se abre cada diez minutos. Esta corre lo
que toca: mira lo que ha cambiado contra la rama base, deduce los paquetes
afectados **y los que dependen de ellos**, y corre el formato, los avisos y
las pruebas de esos paquetes.

### Uso

```bash
# Contra la rama base por defecto (main)
./scripts/test-affected.sh

# Contra otra rama base
./scripts/test-affected.sh origin/main

# Solo las pruebas (sin formato ni avisos)
./scripts/test-affected.sh --no-clippy

# Sin pruebas (solo formato y avisos)
./scripts/test-affected.sh --no-tests

# El plan sin ejecutar nada: qué se correría y por qué
./scripts/test-affected.sh --plan

# Sin los límites de recursos de `bounded`
./scripts/test-affected.sh --no-bounded

# Dónde escribe el informe JSON
./scripts/test-affected.sh --report target/test-affected/report.json
```

### Qué deduce

El mapeo lo hace el manifiesto del workspace (`cargo metadata`), no un
recorrido de rutas: el manifiesto sabe qué archivos son de qué paquete y
quién depende de quién. Un cambio en un crate de dominio arrastra a quien
depende de él, y si no se arrastra el fallo aparece en el sitio equivocado.

El formato se pide **por paquete** y no con `cargo fmt --all`: el primero son
unos segundos y el segundo recorre todos los archivos del repositorio.

La selección de gates se decide por **causa**, y cada gate dice cuál es:

| Cambio | Gates que se eligen |
| --- | --- |
| `Cargo.lock`, `Cargo.toml` | `clippy` y `test` de todos los paquetes: el grafo de dependencias cambió |
| código de un paquete | `fmt`, `clippy` y `test` de ese paquete, y `test` de quien depende de él |
| `config/` | `test` de la API: la configuración se carga al arrancar |
| `*.cedar` | `test` de la API y de los paquetes que hablan Cedar |
| `docs/openapi/` | el gate `openapi` y `test` de la API |
| `frontend/` | el gate `frontend` |
| `pipeline.kts` | el gate `ci_suites`: el guardián de la segmentación |

### Fallar es honesto

- Si la rama base **no existe**, el script falla con el motivo: sin ella no
  hay diff, y sin diff no se puede decir que lo que ha cambiado está bien.
- Si los metadatos del workspace no se pueden leer, el script falla: sin el
  grafo de dependencias no se sabe a quién arrastra un cambio.
- Un gate que **no se ha ejecutado** es `NOT_RUN`, y un `NOT_RUN` nunca
  cuenta como `PASS`. Si falta `frontend/node_modules`, por ejemplo, el gate
  del frontend queda `NOT_RUN` y la puerta no se queda en verde.

### El informe JSON

Cada ejecución escribe `target/test-affected/report.json` (o donde diga
`--report`). Registra el commit y la base comparados, los crates afectados,
los gates elegidos **con la razón de cada elección**, la duración y el
resultado. Es la evidencia de qué se.corrió, y por eso los logs de cada gate
se quedan en `target/test-affected/logs/`.

### `bounded`

`bounded` es un tope de recursos **opcional**: donde está instalado, los
gates se corren con él; donde no, se corren igual y el script lo dice. Con
`--no-bounded` se fuerza el camino sin límites. En cualquier caso, los
límites no son la garantía: la garantía es que el gate se ha ejecutado.

### Cuando correr la pipeline

La pipeline sigue siendo la puerta de la entrega: se corre cuando el
repositorio entero tiene que estar bien --al cerrar un lote de trabajo, antes
de un tag-- y esta no la sustituye, la adelanta.

## test_api.sh

A comprehensive test script for the Hodei Artifacts API that validates all endpoints, OpenAPI specification, and Swagger UI functionality.

### Features

- ✅ Automated server startup and shutdown
- ✅ Health endpoint validation
- ✅ OpenAPI specification verification
- ✅ Swagger UI accessibility check
- ✅ Schema endpoints testing
- ✅ Policy endpoints testing
- ✅ IAM endpoints testing
- ✅ Playground endpoint testing
- ✅ Colored output for easy reading
- ✅ Multiple execution modes

### Prerequisites

- `curl` - For making HTTP requests
- `cargo` - For building the project
- `jq` (optional) - For enhanced JSON validation

### Usage

```bash
# Run all tests (builds, starts server, runs tests)
./scripts/test_api.sh

# Start server only (for manual testing)
./scripts/test_api.sh -s

# Run tests against an already running server
./scripts/test_api.sh -t

# Use a custom API URL
./scripts/test_api.sh -u http://localhost:8080

# Show help
./scripts/test_api.sh -h
```

### Test Modes

#### Full Test Mode (Default)

Builds the project, starts the server, runs all tests, and provides a summary:

```bash
./scripts/test_api.sh
```

This mode:
1. Checks for required dependencies
2. Builds the release binary (if needed)
3. Starts the server in the background
4. Waits for the server to be ready (up to 60 seconds)
5. Runs all endpoint tests
6. Displays a comprehensive summary
7. Keeps the server running for manual testing
8. Press Ctrl+C to stop

#### Server Only Mode

Starts the server without running tests:

```bash
./scripts/test_api.sh -s
```

Useful when you want to:
- Manually test endpoints
- Use Swagger UI for interactive testing
- Keep the server running for development

#### Tests Only Mode

Runs tests against an already running server:

```bash
./scripts/test_api.sh -t
```

Useful when:
- The server is already running
- You want to quickly re-run tests
- Testing against a remote server

#### Custom URL Mode

Test against a different server:

```bash
./scripts/test_api.sh -u http://production.example.com
```

### Test Coverage

The script validates the following:

1. **Health Endpoint** (`/health`)
   - Returns 200 OK
   - Valid JSON response with status

2. **OpenAPI Specification** (`/api-docs/openapi.json`)
   - Valid OpenAPI 3.0 format
   - Counts endpoints and schemas
   - Validates JSON structure

3. **Swagger UI** (`/swagger-ui/`)
   - UI is accessible
   - Contains swagger-ui content

4. **Schema Endpoints** (`/api/v1/schemas/*`)
   - Build schema endpoint
   - Load schema endpoint

5. **Policy Endpoints** (`/api/v1/policies/*`)
   - Policy validation
   - Policy evaluation

6. **IAM Endpoints** (`/api/v1/iam/*`)
   - List policies
   - Create policy

7. **Playground Endpoint** (`/api/v1/playground/evaluate`)
   - Policy evaluation in playground mode

### Output

The script provides colored output:

- 🔵 **[INFO]** - Informational messages
- 🟢 **[SUCCESS]** - Successful operations
- 🟡 **[WARNING]** - Warnings (non-critical issues)
- 🔴 **[ERROR]** - Errors (critical failures)

### Exit Codes

- `0` - All tests passed successfully
- `1` - One or more tests failed or error occurred

### Configuration

The script uses the following defaults:

- **API URL**: `http://localhost:3000`
- **Startup Timeout**: 60 seconds (30 attempts × 2 seconds)
- **Binary Path**: `./target/release/hodei-artifacts-api`
- **Run Mode**: `release`

These can be customized by modifying the script or using environment variables.

### Troubleshooting

#### Server fails to start

```
[ERROR] Server failed to start within timeout period
```

**Solutions:**
- Check if port 3000 is already in use: `lsof -i :3000`
- Verify the Dgraph Alpha (`HODEI_GRAPH__ENDPOINT`) and the S3 store are reachable; the service fails closed while they are not
- Check server logs for configuration errors
- Ensure `./data` directory is writable

#### Binary not found

```
[WARNING] Release binary not found, building project...
```

This is normal on first run. The script will build the project automatically.

#### Permission errors

Ensure the script is executable:

```bash
chmod +x scripts/test_api.sh
```

#### Tests fail with 422 responses

Some endpoints return 422 (Unprocessable Entity) when test payloads are intentionally incomplete or invalid. This is expected behavior and logged as warnings, not errors.

### CI/CD Integration

This script can be integrated into CI/CD pipelines:

```yaml
# GitHub Actions example
- name: Run API tests
  run: |
    timeout 120 ./scripts/test_api.sh || exit 1
```

```yaml
# GitLab CI example
test:api:
  script:
    - chmod +x scripts/test_api.sh
    - timeout 120 ./scripts/test_api.sh
```

### Development Workflow

Recommended workflow for API development:

1. **Start server for development:**
   ```bash
   ./scripts/test_api.sh -s
   ```

2. **In another terminal, make changes to code**

3. **Rebuild and restart:**
   ```bash
   # Stop server (Ctrl+C)
   cargo build --release
   ./scripts/test_api.sh -s
   ```

4. **Run quick tests:**
   ```bash
   # In another terminal
   ./scripts/test_api.sh -t
   ```

5. **Or use Swagger UI for interactive testing:**
   Open http://localhost:3000/swagger-ui/ in your browser

### Related Documentation

- [API Documentation](../README.md)
- [Configuration Guide](../config/README.md)
- [OpenAPI Specification](http://localhost:3000/api-docs/openapi.json) (when server is running)
- [Swagger UI](http://localhost:3000/swagger-ui/) (when server is running)

### Contributing

When modifying this script:

1. Maintain backward compatibility
2. Update this README with new features
3. Test all execution modes
4. Ensure colored output works correctly
5. Handle errors gracefully with proper cleanup

### License

See [LICENSE](../LICENSE) file in the project root.