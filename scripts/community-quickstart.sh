#!/usr/bin/env bash
#
# The community quickstart: the first publish and the first pull, without
# compiling Rust.
#
# # What this is
#
# The executable path of the community tutorial. Somebody with a clean
# machine runs it against a released distribution and ends up with a service
# that has an account, a Raw repository, a Maven repository, a published
# blob, a published package, and the digests that prove both came back.
#
# It is not a mock of the authentication: the token is a real JWT, signed
# with an ephemeral secret that only exists inside the lab, and the service
# verifies it exactly as it verifies a production token.
#
# # The profile
#
# HSP-314 F2: the lab runs on the productive profile of the released binary,
# the graph one (RUN_MODE=dgraph, a [graph] section and an S3 content store).
# The released tarball has no embedded engine, so the lab needs a Dgraph Alpha
# and an S3. Two ways to give them:
#
#   - your own: --graph-endpoint HOST:PORT --s3-endpoint URL --s3-bucket NAME,
#     with the ACL password in HODEI_GRAPH__CREDENTIALS__PASSWORD and the S3
#     keys in AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY;
#   - or none: the script starts a throwaway Dgraph (Zero + Alpha with ACL)
#     and a LocalStack S3 with podman, named "<prefix>-zero|alpha|s3-<pid>"
#     (prefix: HODEI_QUICKSTART_PREFIX, default "hodei-community"), and
#     removes them when it ends.
#
# Without endpoints and without podman, or without the images, the script
# ends with exit 77 (NOT_RUN): it never pretends it ran.
#
# # What it is isolated from
#
# Everything happens under one temporary directory: the HOME, the
# configuration, the content staging, the logs and the token.
# The service is started with an environment that has nothing of the
# caller's in it. The cleanup command removes that directory and nothing
# else, and it refuses to remove a directory that does not declare itself a
# lab.
#
# # The commands
#
#   run       the whole path: lab, service, account, publish, pull, restart
#   guard     the production profile without a secret must stay closed
#   cleanup   remove the lab directory and nothing else
#   help      this text
#
# # Exit codes
#
#   0 the path ran; 1 a step failed; 77 NOT_RUN (no backends available)
#
# # Usage
#
#   scripts/community-quickstart.sh run --archive dist/hodei-artifacts-api-<version>-<target>.tar.gz
#   scripts/community-quickstart.sh guard
#   scripts/community-quickstart.sh cleanup --lab-dir /tmp/hodei-community-lab.XXXXXX

set -euo pipefail

SCRIPT_NAME="$(basename "${BASH_SOURCE[0]}")"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

BIN_NAME="hodei-artifacts-api"

# The lab replaces HOME; podman must keep the image store of the person who
# runs the script, so it is called with the environment the script started with.
ORIG_HOME="$HOME"
ORIG_XDG_CONFIG_HOME="${XDG_CONFIG_HOME-}"
ORIG_XDG_CACHE_HOME="${XDG_CACHE_HOME-}"
pm() {
  (
    export HOME="$ORIG_HOME"
    unset XDG_CONFIG_HOME XDG_CACHE_HOME
    [ -z "$ORIG_XDG_CONFIG_HOME" ] || export XDG_CONFIG_HOME="$ORIG_XDG_CONFIG_HOME"
    [ -z "$ORIG_XDG_CACHE_HOME" ] || export XDG_CACHE_HOME="$ORIG_XDG_CACHE_HOME"
    exec podman "$@"
  )
}

# The account of the lab and the principal that acts in it. The account is
# part of the principal, so it is also the isolation: a principal of another
# account does not match the policy.
ACCOUNT="lab"
USER_NAME="pioneer"
ISSUER="https://issuer.lab.hodei.local"
AUDIENCE="hodei-artifacts-lab"

# The repositories the lab declares.
RAW_REPOSITORY="lab-raw"
MAVEN_REPOSITORY="lab-maven"
# The coordinates of the Maven package the lab publishes.
MAVEN_GROUP="com.acme"
MAVEN_ARTIFACT="lib"
MAVEN_VERSION="1.0.0"

# The name of the file that says "this directory is a lab". The cleanup only
# removes a directory that carries it.
MARKER_NAME=".hodei-community-lab"
MARKER_VALUE="hodei-community-lab/v1"

ARCHIVE=""
DISTRIBUTION=""
LAB_DIR=""
LAB_GIVEN=""
PORT=""
TOKEN=""
SECRET=""
DISTRIBUTION_DIR=""
TEMPLATES_DIR=""
PROFILE_PATH=""
SERVICE_PID=""
SERVICE_PORT=""
BASE_URL=""
STATUS=""

# The backends of the graph profile.
GRAPH_ENDPOINT=""
S3_ENDPOINT=""
S3_BUCKET=""
GRAPH_USER="${HODEI_GRAPH__CREDENTIALS__USERID:-groot}"
GRAPH_PASSWORD="${HODEI_GRAPH__CREDENTIALS__PASSWORD:-}"
S3_ACCESS_KEY="${AWS_ACCESS_KEY_ID:-}"
S3_SECRET_KEY="${AWS_SECRET_ACCESS_KEY:-}"
# Throwaway backends started with podman when none were given.
OWN_BACKENDS=""
BACKEND_PREFIX="${HODEI_QUICKSTART_PREFIX:-hodei-community}"
BACKEND_SUFFIX="$$"
BACKEND_CONTAINERS=()
BACKEND_NETWORK=""
BACKEND_VOLUMES=()
DGRAPH_IMAGE="docker.io/dgraph/dgraph:v25.5.1@sha256:f7459f391921c94db78f8205de9b4624210e2396c6b4ef5736838a906a38becb"
S3_IMAGE="docker.io/localstack/localstack:4.14"

usage() {
  cat <<USAGE
$SCRIPT_NAME - the community quickstart of Hodei Artifacts

usage:
  $SCRIPT_NAME run       [options]   the whole path, end to end
  $SCRIPT_NAME guard     [options]   the production profile without a secret must stay closed
  $SCRIPT_NAME cleanup   --lab-dir DIR  remove the lab directory and nothing else
  $SCRIPT_NAME help

options:
  --archive PATH       the release tarball to start from
  --distribution PATH  an already extracted distribution
  --lab-dir PATH       where the lab lives (default: a temporary directory)
  --port N             the port of the service (default: a free one)
  --account NAME       the account of the lab (default: $ACCOUNT)
  --user NAME          the principal of the lab (default: $USER_NAME)
  --profile PATH       the production profile of the guard (default: config/release.toml)
  --graph-endpoint H:P the gRPC address of your Dgraph Alpha (default: a throwaway one with podman)
  --s3-endpoint URL    the endpoint of your S3 (with --s3-bucket)
  --s3-bucket NAME     the bucket of the content (it must exist)
  -h, --help           this text
USAGE
}

die() {
  echo "error: $*" >&2
  exit 1
}

# The backends are not there: the path cannot run, and it says so.
not_run() {
  echo "NOT_RUN: $*" >&2
  # A lab this run created and that holds nothing is not left behind.
  if [ -n "$LAB_DIR" ] && [ -f "$LAB_DIR/$MARKER_NAME" ] && [ -z "$LAB_GIVEN" ]; then
    stop_service 2>/dev/null || true
    remove_backends "$LAB_DIR"
    rm -rf -- "$LAB_DIR"
  fi
  exit 77
}

step() {
  echo
  echo "==> $*"
}

note() {
  echo "    $*"
}

need() {
  command -v "$1" >/dev/null 2>&1 || die "the command '$1' is needed and it is not installed"
}

# A port that nobody is listening on.
#
# The port is opened and closed: the service occupies it afterwards. It is a
# race that is almost never lost, and the service is started again with
# another port if it loses it.
free_port() {
  local port attempt
  for attempt in $(seq 1 50); do
    port=$(( (RANDOM % 20000) + 30000 ))
    if ! (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then
      echo "$port"
      return 0
    fi
  done
  die "no free port was found"
}

# base64url without padding, which is what a JWT carries.
b64url() {
  openssl base64 -A | tr '+/' '-_' | tr -d '='
}

# The token of the lab.
#
# A real HS256 JWT with the claims the service verifies: the subject is the
# canonical HRN of the principal, the account is the account of the subject,
# and the issuer, the audience and the expiration are the ones the
# deployment declares. The secret is ephemeral: it is minted for this run
# and it never leaves the lab directory.
mint_token() {
  local secret="$1" account="$2" user="$3" issuer="$4" audience="$5" ttl="$6"
  local now expires header payload signature
  now="$(date +%s)"
  expires=$(( now + ttl ))
  header="$(printf '{"alg":"HS256","typ":"JWT"}' | b64url)"
  payload="$(printf '{"sub":"hrn:hodei:iam::%s:User/%s","account":"%s","iss":"%s","aud":"%s","exp":%s}' \
    "$account" "$user" "$account" "$issuer" "$audience" "$expires" | b64url)"
  signature="$(printf '%s' "$header.$payload" \
    | openssl dgst -sha256 -mac HMAC -macopt "key:$secret" -binary \
    | b64url)"
  printf '%s.%s.%s' "$header" "$payload" "$signature"
}

# One request against the service. The status lands in $STATUS and the body
# in the file that was named.
request() {
  local method="$1" path="$2" token="$3" body_file="$4" out="$5"
  shift 5
  local args=(-sS -o "$out" -w '%{http_code}' -X "$method" "$BASE_URL$path")
  [ -n "$token" ] && args+=(-H "Authorization: Bearer $token")
  [ -n "$body_file" ] && args+=(--data-binary "@$body_file")
  while [ "$#" -gt 0 ]; do
    args+=(-H "$1")
    shift
  done
  if ! STATUS="$(curl "${args[@]}")"; then
    STATUS="000"
  fi
}

# A field of a JSON answer. `jq` when it is there, and a plain reading when
# it is not: the tutorial does not depend on a tool it does not check for.
json_field() {
  if command -v jq >/dev/null 2>&1; then
    jq -r ".$2 // empty" "$1" 2>/dev/null || true
  else
    sed -n "s/.*\"$2\":\"\([^\"]*\)\".*/\1/p" "$1"
  fi
}

# Where the templates of the lab live: the schema and the policy.
resolve_templates() {
  local candidate
  for candidate in \
    "$SCRIPT_DIR/config/community" \
    "$REPO_DIR/config/community" \
    "${DISTRIBUTION_DIR:-}/config/community"; do
    if [ -n "$candidate" ] && [ -f "$candidate/policies.cedar" ] && [ -f "$candidate/schema.json" ]; then
      TEMPLATES_DIR="$candidate"
      return 0
    fi
  done
  die "the templates of the lab were not found (config/community/policies.cedar and schema.json)"
}

# The distribution the service comes from: a tarball, an extracted
# directory, the directory the script itself lives in, or the tarball the
# release script left in dist/.
resolve_distribution() {
  if [ -n "$ARCHIVE" ]; then
    [ -f "$ARCHIVE" ] || die "the archive '$ARCHIVE' does not exist"
  elif [ -n "$DISTRIBUTION" ]; then
    [ -d "$DISTRIBUTION" ] || die "the distribution '$DISTRIBUTION' does not exist"
    DISTRIBUTION_DIR="$(cd "$DISTRIBUTION" && pwd)"
  elif [ -x "$SCRIPT_DIR/bin/$BIN_NAME" ]; then
    # The script is inside the distribution: this is the community path.
    DISTRIBUTION_DIR="$SCRIPT_DIR"
  else
    ARCHIVE="$(ls -1t "$REPO_DIR"/dist/"$BIN_NAME"-*.tar.gz 2>/dev/null | head -n 1 || true)"
    [ -n "$ARCHIVE" ] || die "no distribution was found: build one with scripts/package-release.sh or pass --archive"
  fi
  if [ -n "$ARCHIVE" ]; then
    DISTRIBUTION_DIR="$LAB_DIR/distribution"
    mkdir -p "$DISTRIBUTION_DIR"
    echo "    unpacking $(basename "$ARCHIVE")"
    tar -xzf "$ARCHIVE" -C "$DISTRIBUTION_DIR" --strip-components=1
  fi
  [ -x "$DISTRIBUTION_DIR/bin/$BIN_NAME" ] \
    || die "the distribution has no binary at $DISTRIBUTION_DIR/bin/$BIN_NAME"
  [ -f "$DISTRIBUTION_DIR/config/default.toml" ] \
    || die "the distribution has no config/default.toml; the service would not start"
}

# The lab: a temporary directory that carries everything, including the HOME
# of the tutorial.
prepare_lab() {
  if [ -n "$LAB_DIR" ]; then
    LAB_GIVEN=1
    mkdir -p "$LAB_DIR"
  else
    LAB_DIR="$(mktemp -d "${TMPDIR:-/tmp}/hodei-community-lab.XXXXXX")"
  fi
  LAB_DIR="$(cd "$LAB_DIR" && pwd)"
  printf '%s\n' "$MARKER_VALUE" > "$LAB_DIR/$MARKER_NAME"
  chmod 700 "$LAB_DIR"
  mkdir -p "$LAB_DIR/home" "$LAB_DIR/config" "$LAB_DIR/data" "$LAB_DIR/logs" \
    "$LAB_DIR/policy" "$LAB_DIR/work"
  # The HOME of the tutorial is the HOME of the lab: nothing is written in
  # the home of the person who runs it.
  export HOME="$LAB_DIR/home"
  export XDG_CONFIG_HOME="$LAB_DIR/home/.config"
  export XDG_CACHE_HOME="$LAB_DIR/home/.cache"
  # The secret of this run: ephemeral, and only inside the lab.
  openssl rand -base64 32 | tr -d '\n' > "$LAB_DIR/secret"
  chmod 600 "$LAB_DIR/secret"
  SECRET="$(cat "$LAB_DIR/secret")"
}

# The configuration of the lab.
#
# It lives in the lab directory and not in the repository: the lab is a
# temporary deployment and its configuration goes with it. The secret is not
# written here on purpose: it travels in the environment of the service, it
# is minted for this run and it dies with the lab.
write_lab_config() {
  # "closed" writes the profile with no authenticator and no policy: the guard
  # starts it to show that it stays closed, or refuses, and never serves.
  local security
  if [ "${1:-}" = "closed" ]; then
    security='[security]
admin_api_enabled = false
raw_api_enabled = false'
  else
    security="[security]
admin_api_enabled = false
raw_api_enabled = true
# The secret is not here: it is in the environment of the service, it is
# ephemeral and it is minted for this run only.
auth_issuer = \"$ISSUER\"
auth_audience = \"$AUDIENCE\"
authorization_policy_path = \"$LAB_DIR/policy/policies.cedar\"
authorization_schema_path = \"$LAB_DIR/policy/schema.json\""
  fi
  # The baseline of the service travels with the lab: the configuration is
  # deserialized strictly, and a deployment without it does not start.
  cp "$DISTRIBUTION_DIR/config/default.toml" "$LAB_DIR/config/default.toml"
  cat > "$LAB_DIR/config/dgraph.toml" <<TOML
# The profile of the community lab: the productive graph profile (HSP-209).
#
# The authority of the metadata is the Dgraph Alpha declared in [graph] and the
# bytes live in the S3 declared in [content.s3]. The credentials of both travel
# in the environment of the service, never here.

[server]
host = "127.0.0.1"

[graph]
endpoint = "$GRAPH_ENDPOINT"
namespace = 0

[graph.tls]
enabled = false

[graph.credentials]
userid = "$GRAPH_USER"

[database]
db_type = "dgraph"

[content]
backend = "s3"

[content.s3]
bucket = "$S3_BUCKET"
region = "us-east-1"
endpoint = "$S3_ENDPOINT"
tls = false

[schema]
register_iam_on_startup = false
validate = true
storage_type = "dgraph"

[repository]
durable = false

$security

[logging]
level = "info"
format = "pretty"
TOML
}

# The policy of the lab: the template with the account and the principal.
write_lab_policy() {
  sed -e "s/__ACCOUNT__/$ACCOUNT/g" -e "s/__USER__/$USER_NAME/g" \
    "$TEMPLATES_DIR/policies.cedar" > "$LAB_DIR/policy/policies.cedar"
  cp "$TEMPLATES_DIR/schema.json" "$LAB_DIR/policy/schema.json"
}

# The throwaway backends of the lab (podman): a Dgraph Zero + Alpha with ACL
# on an internal network, and a LocalStack S3. Everything is named after the
# prefix and removed when the script ends (see stop_backends).
start_backends() {
  command -v podman >/dev/null 2>&1 \
    || not_run "no backends were given (--graph-endpoint, --s3-endpoint, --s3-bucket) and podman is not installed"
  local image
  for image in "$DGRAPH_IMAGE" "$S3_IMAGE"; do
    if ! pm image exists "$image" 2>/dev/null; then
      pm pull -q "$image" >/dev/null 2>&1 \
        || not_run "the image $image is not available and could not be pulled"
    fi
  done
  OWN_BACKENDS=1
  local p="$BACKEND_PREFIX" n="$BACKEND_SUFFIX"
  BACKEND_NETWORK="$p-net-$n"
  local zero="$p-zero-$n" alpha="$p-alpha-$n" s3="$p-s3-$n"
  local http_port grpc_port s3_port
  http_port="$(free_port)"; grpc_port="$(free_port)"; s3_port="$(free_port)"
  mkdir -p "$LAB_DIR/backend"
  openssl rand -hex 32 | tr -d '\n' > "$LAB_DIR/backend/hmac-secret"
  chmod 644 "$LAB_DIR/backend/hmac-secret"
  # The names go to the lab first: a lab that dies half way is still cleanable.
  printf '%s\n' "$zero" "$alpha" "$s3" > "$LAB_DIR/backend/containers"
  printf '%s\n' "$BACKEND_NETWORK" > "$LAB_DIR/backend/network"
  BACKEND_CONTAINERS=("$zero" "$alpha" "$s3")
  BACKEND_VOLUMES=("$p-zero-data-$n" "$p-alpha-data-$n")
  printf '%s\n' "${BACKEND_VOLUMES[@]}" > "$LAB_DIR/backend/volumes"
  echo "    starting a throwaway Dgraph and S3 with podman ($p-*-$n)"
  pm network create --internal "$BACKEND_NETWORK" >/dev/null || die "the backend network was not created"
  local v
  for v in "${BACKEND_VOLUMES[@]}"; do pm volume create "$v" >/dev/null || die "the volume $v was not created"; done
  pm run -d --name "$zero" --hostname dgraph-zero --network "$BACKEND_NETWORK" \
    -v "$p-zero-data-$n:/dgraph-data" "$DGRAPH_IMAGE" dgraph zero --my=dgraph-zero:5080 \
    --wal=/dgraph-data/zero --telemetry reports=false >/dev/null || die "Dgraph Zero did not start"
  pm run -d --name "$alpha" --hostname dgraph-alpha --network "$BACKEND_NETWORK" \
    -p "127.0.0.1:$http_port:8080" -p "127.0.0.1:$grpc_port:9080" \
    -v "$p-alpha-data-$n:/dgraph-data" \
    -v "$LAB_DIR/backend/hmac-secret:/dgraph/secrets/hmac-secret:ro,z" \
    "$DGRAPH_IMAGE" dgraph alpha --my=dgraph-alpha:7080 --zero=dgraph-zero:5080 \
    --postings=/dgraph-data/p --wal=/dgraph-data/w \
    --acl "secret-file=/dgraph/secrets/hmac-secret; access-ttl=24h; refresh-ttl=72h; jwt-alg=HS256" \
    --security whitelist=0.0.0.0/0 --telemetry reports=false >/dev/null || die "Dgraph Alpha did not start"
  pm run -d --name "$s3" -p "127.0.0.1:$s3_port:4566" "$S3_IMAGE" >/dev/null || die "the S3 did not start"
  local deadline=$(( SECONDS + 120 ))
  until curl -s -m 3 "http://127.0.0.1:$http_port/health" 2>/dev/null | grep -q healthy; do
    [ "$SECONDS" -lt "$deadline" ] || die "Dgraph Alpha was not healthy in time"
    sleep 1
  done
  # /health can answer before the gRPC listener serves: the service connects over gRPC.
  sleep 5
  until curl -s -m 3 "http://127.0.0.1:$s3_port/_localstack/health" 2>/dev/null | grep -Eq '"s3": "(available|running)"'; do
    [ "$SECONDS" -lt "$deadline" ] || die "the S3 was not available in time"
    sleep 1
  done
  GRAPH_ENDPOINT="127.0.0.1:$grpc_port"
  GRAPH_USER="groot"
  GRAPH_PASSWORD="password"
  S3_ENDPOINT="http://127.0.0.1:$s3_port"
  S3_BUCKET="community-lab-$n"
  S3_ACCESS_KEY="test"
  S3_SECRET_KEY="test"
  curl -fsS -X PUT "$S3_ENDPOINT/$S3_BUCKET" >/dev/null || die "the bucket $S3_BUCKET was not created"
}

# Remove what the lab started: containers, volumes and network. It reads the
# names from the lab, so it also cleans a run that died half way.
remove_backends() {
  local dir="$1" name
  [ -d "$dir/backend" ] || return 0
  command -v podman >/dev/null 2>&1 || return 0
  if [ -f "$dir/backend/containers" ]; then
    while read -r name; do [ -n "$name" ] && pm rm -f -t 0 "$name" >/dev/null 2>&1; done < "$dir/backend/containers"
  fi
  if [ -f "$dir/backend/volumes" ]; then
    while read -r name; do [ -n "$name" ] && pm volume rm -f "$name" >/dev/null 2>&1; done < "$dir/backend/volumes"
  fi
  if [ -f "$dir/backend/network" ]; then
    while read -r name; do [ -n "$name" ] && pm network rm -f "$name" >/dev/null 2>&1; done < "$dir/backend/network"
  fi
  return 0
}

stop_backends() {
  [ -n "$OWN_BACKENDS" ] || return 0
  remove_backends "$LAB_DIR"
  OWN_BACKENDS=""
}

# The backends the lab runs on: the ones that were given, or throwaway ones.
ensure_backends() {
  if [ -n "$GRAPH_ENDPOINT" ] || [ -n "$S3_ENDPOINT" ] || [ -n "$S3_BUCKET" ]; then
    [ -n "$GRAPH_ENDPOINT" ] && [ -n "$S3_ENDPOINT" ] && [ -n "$S3_BUCKET" ] \
      || die "give --graph-endpoint, --s3-endpoint and --s3-bucket together, or none of them"
    [ -n "$GRAPH_PASSWORD" ] || die "HODEI_GRAPH__CREDENTIALS__PASSWORD is needed with --graph-endpoint"
    [ -n "$S3_ACCESS_KEY" ] && [ -n "$S3_SECRET_KEY" ] \
      || die "AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY are needed with --s3-endpoint"
    return 0
  fi
  start_backends
}

# The environment of the graph profile: where the Alpha and the S3 are, and the
# credentials of both. They travel in the environment of the service, never in a file.
graph_environment() {
  GRAPH_ENV=(
    "HODEI_GRAPH__ENDPOINT=$GRAPH_ENDPOINT"
    "HODEI_GRAPH__TLS__ENABLED=false"
    "HODEI_GRAPH__CREDENTIALS__USERID=$GRAPH_USER"
    "HODEI_GRAPH__CREDENTIALS__PASSWORD=$GRAPH_PASSWORD"
    "HODEI_CONTENT__BACKEND=s3"
    "HODEI_CONTENT__S3__BUCKET=$S3_BUCKET"
    "HODEI_CONTENT__S3__ENDPOINT=$S3_ENDPOINT"
    "HODEI_CONTENT__S3__REGION=us-east-1"
    "HODEI_CONTENT__S3__TLS=false"
    "AWS_ACCESS_KEY_ID=$S3_ACCESS_KEY"
    "AWS_SECRET_ACCESS_KEY=$S3_SECRET_KEY"
  )
}

# Start the service of the lab and wait until it answers.
start_service() {
  local port="$1" run_mode="${2:-dgraph}"
  SERVICE_PORT="$port"
  BASE_URL="http://127.0.0.1:$port"
  graph_environment
  (
    cd "$LAB_DIR" || exit 1
    exec env -i \
      PATH="$PATH" \
      HOME="$LAB_DIR/home" \
      RUN_MODE="$run_mode" \
      RUST_LOG=info \
      HODEI_SERVER__HOST=127.0.0.1 \
      HODEI_SERVER__PORT="$port" \
      "${GRAPH_ENV[@]}" \
      HODEI_SECURITY__RAW_API_ENABLED=true \
      HODEI_SECURITY__AUTH_JWT_SECRET="$SECRET" \
      HODEI_SECURITY__AUTH_ISSUER="$ISSUER" \
      HODEI_SECURITY__AUTH_AUDIENCE="$AUDIENCE" \
      HODEI_SECURITY__AUTHORIZATION_POLICY_PATH="$LAB_DIR/policy/policies.cedar" \
      HODEI_SECURITY__AUTHORIZATION_SCHEMA_PATH="$LAB_DIR/policy/schema.json" \
      "$DISTRIBUTION_DIR/bin/$BIN_NAME"
  ) >> "$LAB_DIR/logs/service.log" 2>&1 &
  SERVICE_PID=$!
}

# Start the service of a profile that has no authenticator at all.
start_service_without_credentials() {
  local port="$1" run_mode="$2"
  SERVICE_PORT="$port"
  BASE_URL="http://127.0.0.1:$port"
  graph_environment
  (
    cd "$LAB_DIR" || exit 1
    exec env -i \
      PATH="$PATH" \
      HOME="$LAB_DIR/home" \
      RUN_MODE="$run_mode" \
      RUST_LOG=info \
      HODEI_SERVER__HOST=127.0.0.1 \
      HODEI_SERVER__PORT="$port" \
      "${GRAPH_ENV[@]}" \
      "$DISTRIBUTION_DIR/bin/$BIN_NAME"
  ) >> "$LAB_DIR/logs/service.log" 2>&1 &
  SERVICE_PID=$!
}

stop_service() {
  [ -n "$SERVICE_PID" ] || return 0
  kill -TERM "$SERVICE_PID" 2>/dev/null || true
  local attempt
  for attempt in $(seq 1 60); do
    kill -0 "$SERVICE_PID" 2>/dev/null || break
    sleep 0.2
  done
  kill -KILL "$SERVICE_PID" 2>/dev/null || true
  wait "$SERVICE_PID" 2>/dev/null || true
  SERVICE_PID=""
}

wait_for_health() {
  local deadline=$(( SECONDS + 60 ))
  local code
  while [ "$SECONDS" -lt "$deadline" ]; do
    if ! kill -0 "$SERVICE_PID" 2>/dev/null; then
      echo "the service died while it was starting; its log says:" >&2
      tail -n 30 "$LAB_DIR/logs/service.log" >&2 || true
      return 1
    fi
    code="$(curl -s -o /dev/null -w '%{http_code}' "$BASE_URL/health" 2>/dev/null || true)"
    if [ "$code" = "200" ]; then
      return 0
    fi
    sleep 0.3
  done
  echo "the service did not answer on /health in time; its log says:" >&2
  tail -n 30 "$LAB_DIR/logs/service.log" >&2 || true
  return 1
}

# Wait until the process is gone, and say what it said.
wait_for_the_exit() {
  local deadline=$(( SECONDS + 30 ))
  while [ "$SECONDS" -lt "$deadline" ]; do
    if ! kill -0 "$SERVICE_PID" 2>/dev/null; then
      return 0
    fi
    sleep 0.2
  done
  return 1
}

# The raw blob of the lab.
write_the_blob() {
  printf 'Hodei community quickstart: the first blob of the lab.\n' > "$LAB_DIR/work/hello.txt"
}

# The Maven package of the lab: a POM whose coordinates match the path, and
# a jar. The jar is a real zip when the machine has `zip`, and a file with
# the same bytes when it does not: what the service stores is the content
# and its digest, and the digest is what the tutorial checks.
write_the_package() {
  local base="com/acme/lib/$MAVEN_VERSION"
  mkdir -p "$LAB_DIR/work/$base"
  cat > "$LAB_DIR/work/$base/$MAVEN_ARTIFACT-$MAVEN_VERSION.pom" <<POM
<?xml version="1.0" encoding="UTF-8"?>
<project>
  <modelVersion>4.0.0</modelVersion>
  <groupId>$MAVEN_GROUP</groupId>
  <artifactId>$MAVEN_ARTIFACT</artifactId>
  <version>$MAVEN_VERSION</version>
</project>
POM
  if command -v zip >/dev/null 2>&1; then
    mkdir -p "$LAB_DIR/work/jar/META-INF"
    printf 'Manifest-Version: 1.0\n\n' > "$LAB_DIR/work/jar/META-INF/MANIFEST.MF"
    # The jar carries a fixed timestamp: the digest of the artifact of the
    # tutorial is the same in every run, and a digest that changes with the
    # clock is a digest nobody can compare.
    find "$LAB_DIR/work/jar" -exec touch -t 200001010000 {} +
    ( cd "$LAB_DIR/work/jar" && zip -q -X -r "../$base/$MAVEN_ARTIFACT-$MAVEN_VERSION.jar" . )
  else
    printf 'Hodei community quickstart: the jar of the lab.\n' \
      > "$LAB_DIR/work/$base/$MAVEN_ARTIFACT-$MAVEN_VERSION.jar"
  fi
}

sha_of() {
  sha256sum "$1" | cut -d' ' -f1
}

sha1_of() {
  sha1sum "$1" | cut -d' ' -f1
}

# The whole path of the tutorial.
run_scenario() {
  local raw_path="quickstart/hello.txt"
  local maven_base="com/acme/lib/$MAVEN_VERSION"
  local jar_name="$MAVEN_ARTIFACT-$MAVEN_VERSION.jar"
  local pom_name="$MAVEN_ARTIFACT-$MAVEN_VERSION.pom"

  step "The lab"
  prepare_lab
  resolve_distribution
  resolve_templates
  write_lab_policy
  ensure_backends
  write_lab_config
  note "profile:    graph (RUN_MODE=dgraph)"
  note "dgraph:     $GRAPH_ENDPOINT"
  note "s3:         $S3_ENDPOINT (bucket $S3_BUCKET)"
  note "lab:        $LAB_DIR"
  note "home:       $HOME"
  note "distribution: $DISTRIBUTION_DIR"
  note "binary:     $DISTRIBUTION_DIR/bin/$BIN_NAME"

  step "The service"
  PORT="${PORT:-$(free_port)}"
  start_service "$PORT"
  wait_for_health
  note "it answers on $BASE_URL (pid $SERVICE_PID)"

  step "The temporary token"
  TOKEN="$(mint_token "$SECRET" "$ACCOUNT" "$USER_NAME" "$ISSUER" "$AUDIENCE" 3600)"
  printf '%s' "$TOKEN" > "$LAB_DIR/token"
  chmod 600 "$LAB_DIR/token"
  note "principal: hrn:hodei:iam::$ACCOUNT:User/$USER_NAME"
  note "the secret is ephemeral and lives in $LAB_DIR/secret"

  step "The account and its repositories"
  local body
  body="$(printf '{"name":"%s","format":"raw","mode":"hosted"}' "$RAW_REPOSITORY")"
  printf '%s' "$body" > "$LAB_DIR/work/raw-repository.json"
  request POST "/api/v1/catalog/repositories" "$TOKEN" "$LAB_DIR/work/raw-repository.json" \
    "$LAB_DIR/work/raw-repository-answer.json" "Content-Type: application/json"
  [ "$STATUS" = "201" ] || die "the Raw repository was not created (status $STATUS): $(cat "$LAB_DIR/work/raw-repository-answer.json")"
  note "created $RAW_REPOSITORY (raw, hosted) -> $STATUS"

  body="$(printf '{"name":"%s","format":"maven2","mode":"hosted","settings":{"maven":{"version_policy":"release"}}}' "$MAVEN_REPOSITORY")"
  printf '%s' "$body" > "$LAB_DIR/work/maven-repository.json"
  request POST "/api/v1/catalog/repositories" "$TOKEN" "$LAB_DIR/work/maven-repository.json" \
    "$LAB_DIR/work/maven-repository-answer.json" "Content-Type: application/json"
  [ "$STATUS" = "201" ] || die "the Maven repository was not created (status $STATUS): $(cat "$LAB_DIR/work/maven-repository-answer.json")"
  note "created $MAVEN_REPOSITORY (maven2, hosted) -> $STATUS"

  step "The first publish: a Raw blob"
  write_the_blob
  local blob_sha
  blob_sha="$(sha_of "$LAB_DIR/work/hello.txt")"
  request PUT "/repository/$RAW_REPOSITORY/$raw_path" "$TOKEN" "$LAB_DIR/work/hello.txt" \
    "$LAB_DIR/work/blob-answer.json" "x-checksum-sha256: $blob_sha"
  [ "$STATUS" = "201" ] || die "the blob was not published (status $STATUS): $(cat "$LAB_DIR/work/blob-answer.json")"
  local reported
  reported="$(json_field "$LAB_DIR/work/blob-answer.json" digest)"
  note "published $RAW_REPOSITORY/$raw_path -> $STATUS"
  note "digest: $reported"
  [ "$reported" = "sha256:$blob_sha" ] \
    || die "the service reported another digest: $reported != sha256:$blob_sha"

  step "The first publish: a Maven package"
  write_the_package
  local jar="$LAB_DIR/work/$maven_base/$jar_name"
  local pom="$LAB_DIR/work/$maven_base/$pom_name"
  local jar_sha1 pom_sha1
  jar_sha1="$(sha1_of "$jar")"
  pom_sha1="$(sha1_of "$pom")"
  request PUT "/repository/$MAVEN_REPOSITORY/$maven_base/$jar_name" "$TOKEN" "$jar" \
    "$LAB_DIR/work/jar-answer.json" "x-checksum-sha256: $(sha_of "$jar")"
  [ "$STATUS" = "201" ] || die "the jar was not published (status $STATUS): $(cat "$LAB_DIR/work/jar-answer.json")"
  note "published $MAVEN_REPOSITORY/$maven_base/$jar_name -> $STATUS"
  printf '%s' "$jar_sha1" > "$LAB_DIR/work/jar-sha1.txt"
  request PUT "/repository/$MAVEN_REPOSITORY/$maven_base/$jar_name.sha1" "$TOKEN" \
    "$LAB_DIR/work/jar-sha1.txt" "$LAB_DIR/work/jar-sha1-answer.json"
  [ "$STATUS" = "200" ] || die "the checksum of the jar was not accepted (status $STATUS)"
  note "published the sha1 of the jar -> $STATUS"
  request PUT "/repository/$MAVEN_REPOSITORY/$maven_base/$pom_name" "$TOKEN" "$pom" \
    "$LAB_DIR/work/pom-answer.json" "x-checksum-sha256: $(sha_of "$pom")"
  [ "$STATUS" = "201" ] || die "the pom was not published (status $STATUS): $(cat "$LAB_DIR/work/pom-answer.json")"
  note "published $MAVEN_REPOSITORY/$maven_base/$pom_name -> $STATUS"
  printf '%s' "$pom_sha1" > "$LAB_DIR/work/pom-sha1.txt"
  request PUT "/repository/$MAVEN_REPOSITORY/$maven_base/$pom_name.sha1" "$TOKEN" \
    "$LAB_DIR/work/pom-sha1.txt" "$LAB_DIR/work/pom-sha1-answer.json"
  [ "$STATUS" = "200" ] || die "the checksum of the pom was not accepted (status $STATUS)"
  note "published the sha1 of the pom -> $STATUS"

  step "The first pull"
  request GET "/repository/$RAW_REPOSITORY/$raw_path" "$TOKEN" "" "$LAB_DIR/work/blob.downloaded"
  [ "$STATUS" = "200" ] || die "the blob did not come back (status $STATUS)"
  local downloaded_sha
  downloaded_sha="$(sha_of "$LAB_DIR/work/blob.downloaded")"
  [ "$downloaded_sha" = "$blob_sha" ] || die "the blob came back with another digest: $downloaded_sha != $blob_sha"
  note "the Raw blob came back with the same digest: $downloaded_sha"

  request GET "/repository/$MAVEN_REPOSITORY/$maven_base/$jar_name" "$TOKEN" "" "$LAB_DIR/work/jar.downloaded"
  [ "$STATUS" = "200" ] || die "the jar did not come back (status $STATUS)"
  [ "$(sha_of "$LAB_DIR/work/jar.downloaded")" = "$(sha_of "$jar")" ] \
    || die "the jar came back with another content"
  note "the Maven jar came back with the same content"
  request GET "/repository/$MAVEN_REPOSITORY/$maven_base/$jar_name.sha1" "$TOKEN" "" "$LAB_DIR/work/jar-sha1.downloaded"
  [ "$STATUS" = "200" ] || die "the checksum of the jar did not come back (status $STATUS)"
  [ "$(tr -d '[:space:]' < "$LAB_DIR/work/jar-sha1.downloaded")" = "$jar_sha1" ] \
    || die "the checksum of the jar did not come back"
  note "the checksum of the jar came back: $jar_sha1"

  step "Another account is not this account"
  # A real token, signed with the secret of the lab and valid in every
  # claim, but whose principal belongs to another account. The credential
  # verifies (so it is not a 401) and the policy does not grant it anything
  # (so it is a 403): the two answers must not be confused.
  local foreign_token
  foreign_token="$(mint_token "$SECRET" "other" "intruder" "$ISSUER" "$AUDIENCE" 3600)"
  request POST "/api/v1/catalog/repositories" "$foreign_token" "$LAB_DIR/work/raw-repository.json" \
    "$LAB_DIR/work/foreign-answer.json" "Content-Type: application/json"
  [ "$STATUS" = "403" ] \
    || die "a valid token of another account got $STATUS instead of 403 on POST /api/v1/catalog/repositories"
  note "a valid token of another account (other/intruder) cannot create a repository -> $STATUS"
  request GET "/repository/$RAW_REPOSITORY/$raw_path" "$foreign_token" "" "$LAB_DIR/work/foreign-blob.downloaded"
  case "$STATUS" in
    403|404) ;;
    *) die "a valid token of another account got $STATUS on the Raw blob of the lab" ;;
  esac
  note "the same token cannot read the Raw blob of the lab -> $STATUS"
  cmp -s "$LAB_DIR/work/foreign-blob.downloaded" "$LAB_DIR/work/hello.txt" \
    && die "the blob of the lab was served to another account"

  step "The restart"
  stop_service
  start_service "$PORT"
  wait_for_health
  note "the service is up again on the same port and the same data"

  request GET "/api/v1/catalog/repositories" "$TOKEN" "" "$LAB_DIR/work/catalog.json"
  [ "$STATUS" = "200" ] || die "the catalog did not answer after the restart (status $STATUS)"
  grep -q "$RAW_REPOSITORY" "$LAB_DIR/work/catalog.json" || die "the Raw repository is not in the catalog after the restart"
  grep -q "$MAVEN_REPOSITORY" "$LAB_DIR/work/catalog.json" || die "the Maven repository is not in the catalog after the restart"
  note "the catalog kept both repositories"

  request GET "/repository/$RAW_REPOSITORY/$raw_path" "$TOKEN" "" "$LAB_DIR/work/blob.after-restart"
  [ "$STATUS" = "200" ] || die "the blob did not survive the restart (status $STATUS)"
  [ "$(sha_of "$LAB_DIR/work/blob.after-restart")" = "$blob_sha" ] \
    || die "the blob came back with another digest after the restart"
  note "the Raw blob kept its digest: $blob_sha"

  request GET "/repository/$MAVEN_REPOSITORY/$maven_base/$jar_name" "$TOKEN" "" "$LAB_DIR/work/jar.after-restart"
  [ "$STATUS" = "200" ] || die "the jar did not survive the restart (status $STATUS)"
  [ "$(sha_of "$LAB_DIR/work/jar.after-restart")" = "$(sha_of "$jar")" ] \
    || die "the jar came back with another content after the restart"
  note "the Maven package kept its content"

  request GET "/api/v1/catalog/repositories" "" "" "$LAB_DIR/work/catalog.anonymous"
  [ "$STATUS" = "401" ] || die "the catalog answered an anonymous call with $STATUS instead of 401"
  note "the same token is still the credential, and without it the answer is 401"

  stop_service

  step "The summary"
  echo "    raw blob:    sha256:$blob_sha"
  echo "    maven jar:   sha256:$(sha_of "$jar")"
  echo "    maven sha1:  $jar_sha1"
  echo "    lab:         $LAB_DIR"
  echo
  echo "    the service is stopped. To keep it running:"
  echo "      $SCRIPT_NAME serve --lab-dir $LAB_DIR"
  echo
  echo "    to remove everything the lab created:"
  echo "      $SCRIPT_NAME cleanup --lab-dir $LAB_DIR"
}

# The guard of the production profile.
#
# A production configuration without an authenticator, or without a policy,
# must not start as if it were valid: it either starts closed or it refuses
# to start with an explicit reason. Both are checked here, against the real
# binary.
guard_scenario() {
  prepare_lab
  resolve_distribution
  resolve_templates
  write_lab_policy
  ensure_backends
  write_lab_config closed

  local profile=""
  local candidate
  if [ -n "$PROFILE_PATH" ]; then
    [ -f "$PROFILE_PATH" ] || die "the production profile '$PROFILE_PATH' does not exist"
    profile="$PROFILE_PATH"
  else
    for candidate in "$SCRIPT_DIR/config/release.toml" "$REPO_DIR/config/release.toml" "$DISTRIBUTION_DIR/config/release.toml"; do
      if [ -f "$candidate" ]; then
        profile="$candidate"
        break
      fi
    done
  fi
  [ -n "$profile" ] || die "the production profile was not found (config/release.toml)"
  note "production profile: $profile"

  step "The production profile declares no secret"
  if grep -Eq '^[[:space:]]*auth_jwt_secret[[:space:]]*=[[:space:]]*"[^"]+"' "$profile"; then
    die "the production profile declares a secret: $profile"
  fi
  note "no auth_jwt_secret in the production profile"

  step "Without a secret, the production profile starts closed"
  printf 'the content the closed surface is asked to publish\n' > "$LAB_DIR/work/hello.txt"
  printf '{"name":"whatever","format":"raw","mode":"hosted"}' > "$LAB_DIR/work/raw-repository.json"
  PORT="${PORT:-$(free_port)}"
  start_service_without_credentials "$PORT" dgraph
  wait_for_health
  note "the service is up on $BASE_URL without an authenticator"
  local health_status
  request GET "/health" "" "" "$LAB_DIR/work/health.json"
  health_status="$STATUS"
  [ "$health_status" = "200" ] || die "the health check answered $health_status"
  note "GET /health -> $health_status (the health check is public)"
  request PUT "/repository/whatever/whatever" "" "$LAB_DIR/work/hello.txt" "$LAB_DIR/work/closed.txt"
  note "PUT /repository/whatever/whatever -> $STATUS (the content surface is not mounted)"
  [ "$STATUS" != "201" ] || die "the content surface answered a publish without a credential"
  request POST "/api/v1/catalog/repositories" "" "$LAB_DIR/work/raw-repository.json" \
    "$LAB_DIR/work/closed.json" "Content-Type: application/json"
  note "POST /api/v1/catalog/repositories -> $STATUS (there is no authenticator)"
  [ "$STATUS" = "401" ] || die "the catalog answered $STATUS instead of 401"
  request GET "/api/v1/search" "" "" "$LAB_DIR/work/closed.json"
  note "GET /api/v1/search -> $STATUS (there is no authenticator)"
  [ "$STATUS" = "401" ] || die "the search answered $STATUS instead of 401"
  stop_service

  step "With a secret and without a policy, the boot refuses explicitly"
  : > "$LAB_DIR/logs/guard-secret.log"
  (
    cd "$LAB_DIR" || exit 1
    exec env -i \
      PATH="$PATH" \
      HOME="$LAB_DIR/home" \
      RUN_MODE=dgraph \
      RUST_LOG=info \
      HODEI_SERVER__HOST=127.0.0.1 \
      HODEI_SERVER__PORT="${PORT}" \
      "${GRAPH_ENV[@]}" \
      HODEI_SECURITY__AUTH_JWT_SECRET="$SECRET" \
      HODEI_SECURITY__AUTH_ISSUER="$ISSUER" \
      HODEI_SECURITY__AUTH_AUDIENCE="$AUDIENCE" \
      "$DISTRIBUTION_DIR/bin/$BIN_NAME"
  ) > "$LAB_DIR/logs/guard-secret.log" 2>&1 &
  SERVICE_PID=$!
  if wait_for_the_exit; then
    local status=0
    wait "$SERVICE_PID" 2>/dev/null || status=$?
    SERVICE_PID=""
    if [ "$status" = "0" ]; then
      die "the service started with a secret and without a policy; it must refuse"
    fi
    note "the service refused to start (exit $status):"
    grep -m 2 -E "Configuration error|Production security|Bootstrap failed" "$LAB_DIR/logs/guard-secret.log" | sed 's/^/      /' || true
    grep -q "Production security requires" "$LAB_DIR/logs/guard-secret.log" \
      || die "the refusal was not explicit: the log does not say what is missing"
  else
    stop_service
    die "the service did not stop: it accepted a secret without a policy"
  fi

  step "With a policy that cannot be read, the boot refuses explicitly"
  : > "$LAB_DIR/logs/guard-policy.log"
  (
    cd "$LAB_DIR" || exit 1
    exec env -i \
      PATH="$PATH" \
      HOME="$LAB_DIR/home" \
      RUN_MODE=dgraph \
      RUST_LOG=info \
      HODEI_SERVER__HOST=127.0.0.1 \
      HODEI_SERVER__PORT="${PORT}" \
      "${GRAPH_ENV[@]}" \
      HODEI_SECURITY__AUTH_JWT_SECRET="$SECRET" \
      HODEI_SECURITY__AUTH_ISSUER="$ISSUER" \
      HODEI_SECURITY__AUTH_AUDIENCE="$AUDIENCE" \
      HODEI_SECURITY__AUTHORIZATION_POLICY_PATH="$LAB_DIR/policy/does-not-exist.cedar" \
      HODEI_SECURITY__AUTHORIZATION_SCHEMA_PATH="$LAB_DIR/policy/schema.json" \
      "$DISTRIBUTION_DIR/bin/$BIN_NAME"
  ) > "$LAB_DIR/logs/guard-policy.log" 2>&1 &
  SERVICE_PID=$!
  if wait_for_the_exit; then
    local status=0
    wait "$SERVICE_PID" 2>/dev/null || status=$?
    SERVICE_PID=""
    if [ "$status" = "0" ]; then
      die "the service started with a policy it cannot read; it must refuse"
    fi
    note "the service refused to start (exit $status):"
    grep -m 2 -E "Configuration error|Production security|Bootstrap failed|cannot read" "$LAB_DIR/logs/guard-policy.log" | sed 's/^/      /' || true
    grep -q "cannot read authorization policy bundle" "$LAB_DIR/logs/guard-policy.log" \
      || die "the refusal was not explicit: the log does not say what it could not read"
  else
    stop_service
    die "the service did not stop: it accepted a policy it cannot read"
  fi

  step "The summary of the guard"
  echo "    the production profile without a secret starts closed: the content surface is not"
  echo "    mounted, the business surfaces answer 401, and the health check stays public."
  echo "    the production profile with a secret and without a policy refuses to start, and it"
  echo "    says what is missing."
  echo "    lab: $LAB_DIR"
}

# Start the service of a lab that is already there and keep it running.
serve_scenario() {
  [ -n "$LAB_DIR" ] || die "serve needs --lab-dir"
  [ -f "$LAB_DIR/$MARKER_NAME" ] || die "'$LAB_DIR' is not a lab directory"
  [ -f "$LAB_DIR/secret" ] || die "'$LAB_DIR' has no secret; it was not created by this script"
  SECRET="$(cat "$LAB_DIR/secret")"
  PORT="${PORT:-$(free_port)}"
  resolve_distribution
  ensure_backends
  write_lab_config
  start_service "$PORT"
  wait_for_health
  TOKEN="$(cat "$LAB_DIR/token" 2>/dev/null || true)"
  echo
  echo "the service is up on $BASE_URL"
  echo "the token of the lab is in $LAB_DIR/token"
  echo "the log is in $LAB_DIR/logs/service.log"
  echo
  echo "press Ctrl+C to stop it"
  wait "$SERVICE_PID"
}

# The cleanup: it removes the lab and nothing else.
cleanup_lab() {
  [ -n "$LAB_DIR" ] || die "cleanup needs --lab-dir"
  [ -d "$LAB_DIR" ] || die "'$LAB_DIR' does not exist"
  local resolved
  resolved="$(cd "$LAB_DIR" && pwd)"
  case "$resolved" in
    /|/home|"$HOME"|"$HOME"/*)
      die "refusing to remove '$resolved': it is not a lab directory"
      ;;
  esac
  [ -f "$resolved/$MARKER_NAME" ] || die "refusing to remove '$resolved': it does not declare itself a lab"
  [ "$(cat "$resolved/$MARKER_NAME")" = "$MARKER_VALUE" ] \
    || die "refusing to remove '$resolved': it does not declare itself a lab"
  echo "removing $resolved"
  remove_backends "$resolved"
  rm -rf -- "$resolved"
  echo "the lab is gone"
}

main() {
  local command="${1:-run}"
  shift || true
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --archive) ARCHIVE="$2"; shift 2 ;;
      --distribution) DISTRIBUTION="$2"; shift 2 ;;
      --lab-dir) LAB_DIR="$2"; shift 2 ;;
      --port) PORT="$2"; shift 2 ;;
      --account) ACCOUNT="$2"; shift 2 ;;
      --user) USER_NAME="$2"; shift 2 ;;
      --profile) PROFILE_PATH="$2"; shift 2 ;;
      --graph-endpoint) GRAPH_ENDPOINT="$2"; shift 2 ;;
      --s3-endpoint) S3_ENDPOINT="$2"; shift 2 ;;
      --s3-bucket) S3_BUCKET="$2"; shift 2 ;;
      -h|--help) usage; exit 0 ;;
      *) die "unknown option '$1'" ;;
    esac
  done

  case "$ACCOUNT" in
    ''|[!A-Za-z0-9]*|*[!A-Za-z0-9_-]*) die "the account '$ACCOUNT' is not a valid name" ;;
  esac
  case "$USER_NAME" in
    ''|[!A-Za-z0-9]*|*[!A-Za-z0-9_-]*) die "the user '$USER_NAME' is not a valid name" ;;
  esac

  need curl
  need openssl
  need sha256sum
  need sha1sum
  need tar

  # The service never outlives the script: a lab that is left behind with a
  # process holding its database is a lab that cannot be cleaned.
  trap 'stop_service; stop_backends' EXIT
  trap 'stop_service; stop_backends; exit 130' INT TERM

  case "$command" in
    run) run_scenario ;;
    guard) guard_scenario ;;
    serve) serve_scenario ;;
    cleanup) cleanup_lab ;;
    help|-h|--help) usage ;;
    *) usage; die "unknown command '$command'" ;;
  esac
}

main "$@"
