# Dgraph air-gap deployment (HSP-177)

Local Zero+Alpha deployment of `docker.io/dgraph/dgraph` pinned by tag and
digest, running in a closed podman network (no egress), with dedicated volumes
and ACL enabled with a locally generated HMAC secret.

## Contents

- `compose.yaml` — Compose manifest (podman compose / docker compose v2).
  The UAT recorded in `target/uat/` ran the equivalent raw podman commands
  (documented in the file header); the mapping is 1:1.

The HMAC secret (`secrets/hmac-secret`) is generated locally for each
deployment and is never committed: `head -c 32 /dev/urandom > secrets/hmac-secret`.
Nothing under `secrets/` belongs in git (repository policy rejects
`**/secrets/**` paths on commit).

## Air-gap guarantees (verified in UAT)

- Image pulled once by digest; every subsequent run uses the local image only.
- Network `dgraph-airgap` is `internal: true`: DNS and TCP probes to the
  internet fail from inside the network.
- Telemetry disabled (`--telemetry "reports=false"`): v25 defaults to
  `reports=true` and would call home on startup.
- No commercial license anywhere: Dgraph v25 removed enterprise licensing
  (ACL and backup are part of the Apache-2.0 build and are exercised by the
  UAT without any license configuration).

## Startup (raw podman equivalent of compose.yaml)

```sh
head -c 32 /dev/urandom > deploy/dgraph/secrets/hmac-secret
podman network create --internal dgraph-airgap
podman volume create dgraph-zero-data dgraph-alpha-data dgraph-backups
# zero
podman run -d --name dgraph-zero --hostname dgraph-zero --network dgraph-airgap \
  -v dgraph-zero-data:/dgraph-data docker.io/dgraph/dgraph:v25.5.1@sha256:f7459f391921c94db78f8205de9b4624210e2396c6b4ef5736838a906a38becb \
  dgraph zero --my=dgraph-zero:5080 --wal=/dgraph-data/zero --telemetry "reports=false"
# alpha
podman run -d --name dgraph-alpha --hostname dgraph-alpha --network dgraph-airgap \
  -p 127.0.0.1:8080:8080 -p 127.0.0.1:9080:9080 \
  -v dgraph-alpha-data:/dgraph-data -v dgraph-backups:/dgraph-backups \
  -v "$(pwd)/deploy/dgraph/secrets/hmac-secret:/dgraph/secrets/hmac-secret:ro,z" \
  docker.io/dgraph/dgraph:v25.5.1@sha256:f7459f391921c94db78f8205de9b4624210e2396c6b4ef5736838a906a38becb \
  dgraph alpha --my=dgraph-alpha:7080 --zero=dgraph-zero:5080 \
  --postings=/dgraph-data/p --wal=/dgraph-data/w \
  --acl "secret-file=/dgraph/secrets/hmac-secret; access-ttl=24h; refresh-ttl=72h; jwt-alg=HS256" \
  --telemetry "reports=false"
```

On SELinux hosts (Fedora) the secret bind-mount needs the `z` relabel or the
alpha fails with "permission denied" while reading the ACL secret.

## Notes for the next integration slice (HSP-180+)

- The access JWT header in v25 is `X-Dgraph-AccessToken` (not
  `X-Dgraph-ACCESS-TOKEN` as in older guides).
- The REST login payload keys are `userid`/`password`; the response fields are
  `data.accessJWT` / `data.refreshJWT`.
- Guardian login and admin operations (alter, backup) are IP-restricted to
  loopback unless `--security "whitelist=..."` is configured.
- Backup/restore flow: admin GraphQL `backup(destination: dir)` then
  `dgraph restore -p <parent> -l <dir> -z <zero>` against a NEW cluster
  (documented in `dgraph restore --help`).
- Full UAT evidence and findings: `target/uat/` (gitignored, local).
