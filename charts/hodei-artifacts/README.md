# Chart de Helm de hodei-artifacts

Despliega `hodei-artifacts-api` en Kubernetes sobre el **perfil de grafo**
(`RUN_MODE=dgraph`, HSP-314 F2): un Alpha de Dgraph es la autoridad única de los
metadatos y los bytes viven en S3. Las variables siguen
[`config/dgraph.toml`](../../config/dgraph.toml),
[`src/config.rs`](../../src/config.rs) y
[`docs/community/configuration.md`](../../docs/community/configuration.md).
Si algo de aquí discrepa del código, manda el código.

## Estado honesto

- Los perfiles `standalone` y `rocksdb-garage` **se retiraron**: la imagen ya no
  lleva el motor embebido (RocksDB), así que `helm template` falla con un
  mensaje que remite al perfil `graph`.
- El perfil `graph` (`values.yaml`, por defecto) renderiza una API sin estado
  (`replicaCount >= 1`, `RollingUpdate`, sin PVC, `PodDisruptionBudget`) que
  llega al Alpha por `graph.endpoint` o, con `dgraph.enabled`, al Alpha del
  propio chart. La contraseña ACL de la cuenta de servicio se referencia desde
  un Secret (`graph.credentials.existingSecret`): el chart nunca la renderiza.
- **Sin probar en vivo:** el chart solo se renderiza (`scripts/test-helm-chart.sh`);
  no se ha desplegado en un clúster real. Lo que sí está probado con Dgraph y S3
  reales es el binario y la imagen en perfil de grafo (`hsp237`, `hsp308`).
- **Superficies cerradas hasta F3 (HSP-314):** la investigación, los hechos de
  riesgo y el reactor de CVE no tienen cableado sobre Dgraph y siguen cerrados
  en este perfil.

El servicio **no termina TLS** (`src/main.rs` sirve HTTP en claro). Termina TLS
en el Ingress o en el balanceador y define `config.publicBaseUrl` con
esquema `https`.

## Qué renderiza

- **API**: sin estado, `RollingUpdate`, sin PVC, `PodDisruptionBudget`. Corre
  como no-root (UID/GID y `fsGroup` 10001, el de la imagen), con
  `readOnlyRootFilesystem`, sin capacidades y con `seccomp` `RuntimeDefault`.
  Las sondas son `/health/live` (arranque y vida) y `/health/ready`
  (disponibilidad, que depende del Alpha y de S3).
- **Dgraph** (opcional, `dgraph.enabled`): Zero/Alpha como StatefulSets con PVC
  `RWO` y ACL con el secreto HMAC de un Secret existente. Overlay:
  `values-graph.yaml`.
- **Garage** (opcional, `garage.enabled`): un StatefulSet con **dos
  `volumeClaimTemplates` RWO separados** por réplica (`meta`, pensado para SSD
  de baja latencia, y `data`, para capacidad) y un Job de arranque idempotente
  (layout, bucket e importación de clave desde un Secret mediante la API de
  administración v2 de Garage).

## Secretos

El chart **nunca renderiza un `Secret`**: solo referencia Secrets existentes.

```bash
kubectl create secret generic hodei-s3 \
  --from-literal=access-key-id="<GK...>" \
  --from-literal=secret-access-key="<secret>" \
  --from-literal=rpc-secret="$(openssl rand -hex 32)" \
  --from-literal=admin-token="$(openssl rand -base64 32)"
kubectl create secret generic hodei-dgraph-acl \
  --from-literal=hmac-secret="$(openssl rand -hex 32)"   # solo con dgraph.enabled
kubectl create secret generic hodei-dgraph-credentials \
  --from-literal=password="<contraseña ACL de la cuenta de servicio>"
```

El identificador de clave de Garage debe cumplir su formato (`GK` más 24
caracteres hexadecimales). Las credenciales de S3 llegan a la API como
`AWS_ACCESS_KEY_ID` y `AWS_SECRET_ACCESS_KEY` (cadena estándar del SDK).
`secrets.existingSecret` aporta, opcionalmente, `HODEI_SECURITY__AUTH_JWT_SECRET`
y `HODEI_METADATA_SIGNER_ARMOR`.

## Uso

```bash
# Alpha propio (TLS declarado) y S3 propio
helm install hodei charts/hodei-artifacts \
  --set graph.endpoint=alpha.example.com:9080 \
  --set graph.credentials.existingSecret=hodei-dgraph-credentials \
  --set s3.existingSecret=hodei-s3 --set s3.endpoint=s3.example.com
# Dgraph dentro del clúster
helm install hodei charts/hodei-artifacts -f charts/hodei-artifacts/values-graph.yaml
SOURCE_DATE_EPOCH=1700000000 helm package charts/hodei-artifacts            # reproducible
```

## Verificación

```bash
scripts/test-helm-chart.sh   # lint, contratos de manifiestos, empaquetado reproducible,
                             # perfil graph con Dgraph y Garage, y bootstrap contra un doble de la API
```

`helm package` solo es reproducible con `SOURCE_DATE_EPOCH`: sin ella el
archivo incrusta la fecha de modificación de cada fichero y dos copias del
mismo chart dan hashes distintos (comprobado con Helm v4.3.2). El script lo
verifica comparando el sha256 de dos copias con fechas distintas. Lo ejecuta
`scripts/test-affected.sh` cuando cambia `charts/**` (gate `helm-chart`; sale
con 77, es decir `NOT_RUN`, si falta `helm`, `yq`, `python3` o `curl`).

El script solo renderiza: no necesita clúster. El `bootstrap.sh` del Job de
Garage se ejecuta contra un doble local de la API de administración v2
(`scripts/garage-admin-double.py`): comprueba la idempotencia y los 404 y 5xx,
pero no contra un Garage real. No hay prueba contra un clúster real; el Job
(imagen `curlimages/curl`) y los StatefulSets de Dgraph están sin probar en vivo, y
las versiones de imagen son etiquetas, no resúmenes verificados, salvo Dgraph
(el de `deploy/dgraph/compose.yaml`).
