# Chart de Helm de hodei-artifacts

Despliega `hodei-artifacts-api` en Kubernetes con tres perfiles de
almacenamiento. Las variables y los perfiles siguen
[`config/standalone.toml`](../../config/standalone.toml),
[`config/external.toml`](../../config/external.toml),
[`src/config.rs`](../../src/config.rs) y
[`docs/community/configuration.md`](../../docs/community/configuration.md).
Si algo de aquí discrepa del código, manda el código.

## Estado honesto

| Perfil | Fichero de valores | ¿Arranca hoy? |
| --- | --- | --- |
| `standalone` | `values.yaml` (por defecto) | Sí: RocksDB y CAS en una PVC. |
| `rocksdb-garage` | `values-rocksdb-garage.yaml` | La configuración (RocksDB + `content.backend = "s3"`) es válida según `src/config.rs`. El Job de Garage **no se ha ejecutado contra un clúster real** (ver Verificación). |
| `external` | `values-external.yaml` | **No.** El servicio solo acepta `database.db_type = "rocksdb"`; con `RUN_MODE=external` termina con `Unsupported database type 'dgraph'`. El perfil es configuración declarativa para cuando exista un adaptador Dgraph. |

El servicio **no termina TLS** (`src/main.rs` sirve HTTP en claro). Termina TLS
en el Ingress o en el balanceador y define `config.publicBaseUrl` con
esquema `https`. El servicio no tiene clave de configuración para el endpoint
de Dgraph; el chart despliega Zero/Alpha, pero la API no los consume todavía.

## Qué hace cada perfil

- **standalone**: 1 réplica, `strategy.type: Recreate`, una PVC `ReadWriteOnce`
  en `/app/data` (RocksDB y CAS en el sistema de ficheros).
- **rocksdb-garage**: igual para la API (1 réplica, `Recreate`, PVC de RocksDB)
  más un StatefulSet de Garage con **dos `volumeClaimTemplates` RWO
  separados** por réplica (`meta`, pensado para SSD de baja latencia, y
  `data`, para capacidad) y un Job de arranque idempotente (layout, bucket e
  importación de clave desde un Secret mediante la API de administración v2
  de Garage).
- **external**: API sin estado, `replicaCount >= 2`, `RollingUpdate`, sin PVC,
  `PodDisruptionBudget`; Dgraph Zero/Alpha como StatefulSets con PVC `RWO`
  (opcional con `dgraph.enabled`) y S3 externo o Garage.

Todo perfil con RocksDB **fuerza 1 réplica y `Recreate`**, con `replicaCount`
ignorado. El pod corre como no-root (UID/GID y `fsGroup` 10001, el de la
imagen), con `readOnlyRootFilesystem`, sin capacidades y con `seccomp`
`RuntimeDefault`. Las sondas son `/health/live` (arranque y vida) y
`/health/ready` (disponibilidad).

## Secretos

El chart **nunca renderiza un `Secret`**: solo referencia Secrets existentes.

```bash
kubectl create secret generic hodei-s3 \
  --from-literal=access-key-id="<GK...>" \
  --from-literal=secret-access-key="<secret>" \
  --from-literal=rpc-secret="$(openssl rand -hex 32)" \
  --from-literal=admin-token="$(openssl rand -base64 32)"
kubectl create secret generic hodei-dgraph-acl \
  --from-literal=hmac-secret="$(openssl rand -hex 32)"   # solo perfil external
```

El identificador de clave de Garage debe cumplir su formato (`GK` más 24
caracteres hexadecimales). Las credenciales de S3 llegan a la API como
`AWS_ACCESS_KEY_ID` y `AWS_SECRET_ACCESS_KEY` (cadena estándar del SDK).
`secrets.existingSecret` aporta, opcionalmente, `HODEI_SECURITY__AUTH_JWT_SECRET`
y `HODEI_METADATA_SIGNER_ARMOR`.

## Uso

```bash
helm install hodei charts/hodei-artifacts                                   # standalone
helm install hodei charts/hodei-artifacts -f charts/hodei-artifacts/values-rocksdb-garage.yaml
helm package charts/hodei-artifacts                                         # reproducible
```

## Verificación

```bash
scripts/test-helm-chart.sh   # lint, contratos de manifiestos y kubeconform
```

El script solo renderiza: no necesita clúster. No hay prueba contra un clúster
real; el Job de Garage (rutas `/v2/...` de la API de administración, imagen
`curlimages/curl`) y los StatefulSets de Dgraph están sin probar en vivo, y
las versiones de imagen son etiquetas, no resúmenes verificados, salvo Dgraph
(el de `deploy/dgraph/compose.yaml`).
