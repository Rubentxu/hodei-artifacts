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

## Quorum topology (HSP-180): 3 Zero + 3 Alpha, RF3, TLS, ACL

`compose-quorum.yaml` extends this deployment to the quorum topology required
by doc-20: three Zero + three Alpha with an explicit replica factor of 3
(`--replicas 3` on every Zero), TLS on the internal (inter-node) gRPC port
AND on the external client ports (`--tls internal-port=true`), mutual TLS
between nodes (each node also presents a client cert), ACL with the same
local HMAC secret, closed network, and telemetry off.

```sh
cd deploy/dgraph
./gen-certs.sh                                # dgraph cert via the pinned image; SANs cover the 6 node names
head -c 32 /dev/urandom > secrets/hmac-secret
podman compose -f compose-quorum.yaml up -d   # or the raw podman commands in target/uat/01-topology-boot.log
```

TLS notes verified in UAT (v25.5.1):

- `--tls` does NOT accept a `dir=` option (unlike older guides). Valid
  options: `ca-cert, server-cert, server-key, client-cert, client-key,
  server-name, client-auth-type, use-system-ca, internal-port`; use absolute
  paths inside the container.
- Zero joining nodes need distinct `--raft "idx=N"`; two nodes with the
  default `idx=1` fail with `REUSE_RAFTID`.
- `dgraph cert --nodes a,b,c` issues ONE `node.crt` with SANs for all listed
  names; separate client certs need an explicit `dgraph cert --client <name>`.
- HTTP `/mutate` commits only with `?commitNow=true`; without it the
  transaction stays pending and later admin ops fail with "Pending
  transactions found".
- Guardian login is `POST /login` (not `/admin`), payload `userid`/`password`,
  loopback-only unless `--security whitelist` is set. Access JWT header is
  `X-Dgraph-AccessToken`.
- Loopback admin calls use `curl -k` in the UAT because the issued node cert
  SANs cover the node DNS names, not 127.0.0.1. All cross-node and
  host-client calls verify the cert against the SANs (CA + client cert).

Quorum behaviour verified in UAT (functional, no benchmarks):

- Killing the Zero leader and the Alpha leader: re-election in seconds
  (zero-2 leader at term 4, alpha-3 leader), all confirmed commits readable,
  new writes commit under the remaining 2/3 quorum.
- With a single Zero alive (no Zero quorum): writes and reads block (hang,
  client timeout) — fail-closed, no silent loss, no stale reads observed.
- Recovery: restarting the stopped nodes restores full service; writes that
  timed out during the no-quorum window committed once quorum returned
  (their UIDs appear after recovery) — a timed-out write must be reconciled
  by operation ID, never assumed failed (doc-20: "ACK perdido se reconcilia
  por operation ID; jamas exito inferido").
- Backup/restore: `POST /admin` GraphQL `backup(destination: dir)` from the
  group leader; offline `dgraph restore -p <parent> -l <dir>` writes
  `p<groupId>` under the target dir — point a fresh Alpha at that dir
  directly (`--postings=.../p1`). If the fresh Zero is not reachable by the
  restore process (e.g. TLS-only Zero and a restore without `--tls`), the
  timestamp must be assigned manually afterwards:
  `GET /assign?num=<restoreVersion>&what=timestamps` on the Zero.
- Known limitation (NOT solved here): bootstrapping a NEW RF3 alpha group
  from restored postings with empty WALs deadlocks in `hasPeer` ("Unable to
  reach leader in group 1") even when started sequentially. The restore was
  therefore functionally proven against a new single-Alpha cluster (RF1,
  HSP-177 flow). Restoring into alphas that KEEP their WAL (production
  procedure) is the follow-up (see target/uat/08-summary.md).

Full UAT evidence and findings: `target/uat/` (gitignored, local).

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
