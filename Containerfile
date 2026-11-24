# Reproducible, non-root, read-only-rootfs-compatible OCI image for hodei-artifacts-api.
#
# Build context is expected to be a clean staging directory containing:
#   bin/hodei-artifacts-api
#   bin/ensure-iam-index
#   bin/ensure-graph-index
#   config/default.toml
#   config/release.toml
#   config/standalone.toml
#   config/external.toml
#
# Use `scripts/package-oci.sh` to build reproducibly with SOURCE_DATE_EPOCH.

FROM docker.io/library/debian:bookworm-slim

ARG VERSION=0.2.0
ARG VCS_REF=unknown
ARG BUILD_DATE=1970-01-01T00:00:00Z

LABEL org.opencontainers.image.title="hodei-artifacts-api" \
      org.opencontainers.image.description="Hodei Artifacts Graph-First API" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${VCS_REF}" \
      org.opencontainers.image.created="${BUILD_DATE}" \
      org.opencontainers.image.licenses="MIT"

RUN apt-get update -qq \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* /var/cache/apt/archives/* \
    && groupadd --gid 10001 --system hodei \
    && useradd --uid 10001 --gid 10001 --system --home-dir /app --shell /usr/sbin/nologin hodei \
    && mkdir -p /app/bin /app/config /app/data \
    && chown 0:0 /app /app/bin /app/config \
    && chmod 0755 /app /app/bin /app/config \
    && chown 10001:10001 /app/data \
    && chmod 0750 /app/data

COPY --chown=0:0 --chmod=0755 bin/ /app/bin/
COPY --chown=0:0 --chmod=0644 config/ /app/config/

WORKDIR /app

ENV RUN_MODE=standalone

EXPOSE 3000

USER 10001:10001

ENTRYPOINT ["/app/bin/hodei-artifacts-api"]
