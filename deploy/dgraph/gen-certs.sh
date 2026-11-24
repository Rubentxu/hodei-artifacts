#!/usr/bin/env bash
# Generate the TLS material for the Dgraph quorum topology (HSP-180) using
# the pinned Dgraph image itself — no host tooling required.
#
# Outputs (in deploy/dgraph/$TLS_DIR, default tls/):
#   ca.crt, ca.key                  - local root CA (self-signed)
#   node.crt, node.key              - server cert, SANs cover all six node
#                                     DNS names (3 zero + 3 alpha)
#   client.hodei-client.crt/.key    - client cert for external clients (mTLS)
#
# Nothing in tls/ or secrets/ is committed: private keys must never enter
# the repository.
set -euo pipefail
cd "$(dirname "$0")"

IMG=docker.io/dgraph/dgraph:v25.5.1@sha256:aa5f08f685dc0349aa6e40c75f3b114532fe45bad7ec281d2250413b55e02baa
# Defaults target the quorum topology. For the single Zero+Alpha lab
# (compose.yaml + compose-tls.yaml) run:
#   TLS_DIR=tls-single NODES=dgraph-zero,dgraph-alpha,localhost,127.0.0.1 ./gen-certs.sh
NODES=${NODES:-dgraph-zero-1,dgraph-zero-2,dgraph-zero-3,dgraph-alpha-1,dgraph-alpha-2,dgraph-alpha-3}
TLS_DIR=${TLS_DIR:-tls}

mkdir -p "$TLS_DIR"
podman run --rm -v "$(pwd)/$TLS_DIR:/dgraph/tls:z" "$IMG" \
  dgraph cert --dir /dgraph/tls --nodes "$NODES" --duration 3650
podman run --rm -v "$(pwd)/$TLS_DIR:/dgraph/tls:z" "$IMG" \
  dgraph cert --dir /dgraph/tls --client hodei-client

chmod 600 "$TLS_DIR"/*.key
echo "--- issued material ---"
podman run --rm -v "$(pwd)/$TLS_DIR:/dgraph/tls:z" "$IMG" dgraph cert ls --dir /dgraph/tls
