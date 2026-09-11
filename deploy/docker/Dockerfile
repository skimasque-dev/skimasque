# syntax=docker/dockerfile:1
#
# The skimasque gateway — `skimasque-server` plus the `skimasque` and
# `skimasque-client` binaries. This is what customers self-host.
#
#   docker build -f deploy/docker/Dockerfile -t skimasque .
#
# Build from the repository root: the build stage needs the whole workspace and
# `Cargo.lock`.

# --- build -----------------------------------------------------------------
# Keep this in step with `rust-version` in the workspace Cargo.toml; the `msrv`
# CI job builds on exactly this toolchain so the two cannot drift silently.
FROM rust:1.88-slim-bookworm AS build

# `git` for the pinned `h3` patch in Cargo.toml; `ca-certificates` lets cargo
# fetch over HTTPS.
RUN apt-get update \
    && apt-get install --yes --no-install-recommends git ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --release --locked -p skimasque-cli \
    && cp target/release/skimasque target/release/skimasque-server \
          target/release/skimasque-client /usr/local/bin/

# --- runtime -------------------------------------------------------------
FROM debian:bookworm-slim AS gateway
# `ca-certificates` for outbound HTTPS (OIDC/JWKS, ACME); `wget` backs the
# health check.
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates wget \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --user-group --home-dir /nonexistent --shell /usr/sbin/nologin skimasque
USER skimasque:skimasque

COPY --from=build /usr/local/bin/skimasque /usr/local/bin/skimasque-server \
     /usr/local/bin/skimasque-client /usr/local/bin/

# QUIC is UDP. Publish it with `-p 4433:4433/udp`.
EXPOSE 4433/udp
# The ops listener (health + metrics), if `--metrics-listen 0.0.0.0:9090` is set.
EXPOSE 9090/tcp

# Assumes `--metrics-listen :9090`; harmless (a failing check) if it is not set.
HEALTHCHECK --interval=15s --timeout=3s --start-period=5s --retries=3 \
    CMD wget --quiet --tries=1 --spider http://127.0.0.1:9090/readyz || exit 1

ENTRYPOINT ["skimasque-server"]
# Overridden in practice; this is a safe-by-default hint that shows the flags.
CMD ["--listen", "0.0.0.0:4433", "--metrics-listen", "0.0.0.0:9090"]
