#!/usr/bin/env bash
# Classifies why an API container failed closed (HSP-273), from its log on stdin.
#
# Prints exactly one word and exits 0:
#   unsupported-backend  the configuration was rejected before any I/O
#                        (e.g. "Unsupported database type 'dgraph'")
#   connectivity         a backend was configured but could not be reached
#                        (connection refused, DNS failure, timeout, bucket unusable)
#   unknown              neither signature is present
#
# Unsupported-backend wins when both appear: a rejected configuration never
# gets as far as opening a connection, so its signature is the root cause.
#
# Usage: podman logs <container> 2>&1 | scripts/compose-classify-failure.sh

set -euo pipefail

LOG="$(cat)"

if grep -qiE 'unsupported (database|storage|backend) type|only .(rocksdb|dgraph). is supported' <<<"$LOG"; then
  echo unsupported-backend
elif grep -qiE 'tcp connect error|connection refused|dns error|failed to lookup|timed out|cannot be used' <<<"$LOG"; then
  echo connectivity
else
  echo unknown
fi
