#!/usr/bin/env bash
#
# Launch a skimasque gateway as a Docker container that terminates TLS itself
# via ACME (Let's Encrypt). No reverse proxy, no static certificate.
#
#   1. Point an A/AAAA record for your hostname at this host.
#   2. Open inbound UDP 443 (MASQUE/QUIC) and TCP 443 (the ACME TLS-ALPN-01
#      challenge). Let's Encrypt validates on 443 only.
#   3. SKM_HOSTNAME=gateway.example.com SKM_ACME_EMAIL=ops@example.com \
#        deploy/docker/run-gateway.sh
#
# The first issuance can take a minute; watch it with
#   docker logs -f skimasque-gateway
# and look for "ACME certificate issued/renewed and now being served".
#
# Re-run this script any time to pick up a new image or changed flags — it
# stops and replaces the existing container. The ACME cache and the
# control-plane identity survive (they are host directories, not the container).
#
# Configuration — all optional except SKM_HOSTNAME:
#   SKM_HOSTNAME      cert domain / --hostname             (required)
#   SKM_ACME_EMAIL    --acme-email; expiry warnings from Let's Encrypt
#   SKM_STAGING=1     use the Let's Encrypt staging environment (untrusted
#                     certs, generous rate limits) — do this until you see a
#                     successful issuance, then re-run without it
#   SKM_IMAGE         default ghcr.io/skimasque-dev/skimasque:latest
#   SKM_ACME_DIR      host dir for the account key + cert  (default ~/skimasque/acme)
#   SKM_CONTAINER     container name                       (default skimasque-gateway)
#   SKM_PORT          host port mapped to the gateway's 443 (default 443)
#
# By default this sets up modes 1 & 2 (SkiMasque Cloud Gateway / Customer
# Gateway): GitHub Actions OIDC as the workload identity, and SkiMasque Cloud
# as the control plane and policy source.
#   SKM_OIDC=0         disable the default --oidc identity check — only do
#                      this if you pass your own identity source after `--`
#                      (e.g. `-- --auth-token ...`); without any identity
#                      source anyone who can reach the port can use the
#                      gateway, and the server prints that warning too
#   SKM_OIDC_AUDIENCE  --oidc-audience                      (default https://SKM_HOSTNAME)
#   SKM_CONTROL_PLANE  control plane URL (default https://control.skimasque.com)
#                      — set to an empty string to run standalone, without a
#                      control plane (mode 3 / local policy only — pass your
#                      own policy source after `--`, e.g. `-- --policy-dir /path`)
#   SKM_CONTROL_TOKEN  the skmreg_… one-time registration token — needed only
#                      the first time this gateway registers (dashboard →
#                      Gateways → "Mint a registration token", or
#                      `skimasque gateway register` on a workstation; do NOT
#                      run `skimasque login` inside the container). The
#                      identity is then stored under SKM_STATE_DIR and the
#                      token isn't needed again.
#   SKM_CONTROL_NAME   fleet display name                  (default SKM_HOSTNAME)
#   SKM_CONTROL_LABELS space-separated key=value pairs for policy scoping
#   SKM_STATE_DIR      host dir for the control-plane identity + cached policy
#                      (default ~/skimasque/state)
#
# Anything after `--` is passed straight through to skimasque-server, and
# takes precedence where it'd conflict with the defaults above — e.g. your own
# --policy-dir instead of --control-plane, or --auth-token instead of --oidc
# (skimasque-server refuses to be given both of either pair).
set -euo pipefail

die() { echo "run-gateway: $*" >&2; exit 1; }

case "${1:-}" in -h | --help | help) sed -n '2,59p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;; esac

# Flags for skimasque-server come after a `--` separator.
passthrough=()
for arg in "$@"; do
  if [ "${seen_ddash:-}" = 1 ]; then passthrough+=("$arg"); fi
  [ "$arg" = "--" ] && seen_ddash=1
done

IMAGE="${SKM_IMAGE:-ghcr.io/skimasque-dev/skimasque:latest}"
CONTAINER="${SKM_CONTAINER:-skimasque-gateway}"
ACME_DIR="${SKM_ACME_DIR:-$HOME/skimasque/acme}"
STATE_DIR="${SKM_STATE_DIR:-$HOME/skimasque/state}"
PORT="${SKM_PORT:-443}"
HOSTNAME_="${SKM_HOSTNAME:-}"
# `-` (not `:-`) so an explicitly empty SKM_CONTROL_PLANE opts out of the
# default — distinct from leaving it unset.
CONTROL_PLANE="${SKM_CONTROL_PLANE-https://control.skimasque.com}"

command -v docker >/dev/null 2>&1 || die "docker not found"
[ -n "$HOSTNAME_" ] || die "set SKM_HOSTNAME to the gateway's public DNS name"
if [ -n "$CONTROL_PLANE" ]; then
  case "$CONTROL_PLANE" in
    https://* | http://*) ;;
    *) die "SKM_CONTROL_PLANE must be a full URL, e.g. https://control.skimasque.com" ;;
  esac
fi

# Don't fight a policy source or identity flag already passed through after
# `--` — skimasque-server refuses --control-plane alongside
# --policy-dir/--policy-file, and --oidc alongside --auth-token.
policy_source_passed=0
for arg in "${passthrough[@]}"; do
  case "$arg" in
    --control-plane | --control-plane=* | --policy-dir | --policy-dir=* | --policy-file | --policy-file=*)
      policy_source_passed=1 ;;
  esac
done
[ "$policy_source_passed" = 1 ] && CONTROL_PLANE=""

identity_passed=0
for arg in "${passthrough[@]}"; do
  case "$arg" in
    --oidc | --github-oidc | --auth-token | --auth-token=*) identity_passed=1 ;;
  esac
done
USE_OIDC=1
[ "${SKM_OIDC:-1}" = 0 ] && USE_OIDC=0
[ "$identity_passed" = 1 ] && USE_OIDC=0

if [ -n "$CONTROL_PLANE" ] && [ -z "${SKM_CONTROL_TOKEN:-}" ] && [ ! -f "$STATE_DIR/gateway.json" ]; then
  die "first-time control-plane enrollment needs SKM_CONTROL_TOKEN (dashboard → Gateways → \"Mint a registration token\"), or set SKM_CONTROL_PLANE= to run standalone"
fi

# Two host dirs the container writes, bind-mounted: the ACME cache, and the
# control-plane identity + cached policy (harmless and empty when unused). The
# image's non-root user is uid/gid 10001 (see Dockerfile) and a bind mount keeps
# its host ownership, so both have to be chowned to that uid.
mkdir -p "$ACME_DIR" "$STATE_DIR"
mounts=(-v "$ACME_DIR:/acme" -v "$STATE_DIR:/state")

# Prep in one throwaway root container: chown the mounts (once a dir is owned by
# 10001 the invoking user can no longer write it, so anything the script wants
# in there has to be written from inside too), and maintain the staging/
# production marker. If the ACME environment changed since last run the cache is
# cleared, so a fresh order happens against the right Let's Encrypt directory.
env_tag=$([ -n "${SKM_STAGING:-}" ] && echo staging || echo production)
docker run --rm --user 0:0 -e "SKM_ENV_TAG=$env_tag" \
  "${mounts[@]}" --entrypoint sh "$IMAGE" -c '
    set -e
    chown -R 10001:10001 /acme /state
    marker=/acme/.skm-acme-env
    if [ -f "$marker" ] && [ "$(cat "$marker")" != "$SKM_ENV_TAG" ]; then
      echo "run-gateway: ACME environment changed ($(cat "$marker") -> $SKM_ENV_TAG); clearing the cache"
      find /acme -mindepth 1 ! -name .skm-acme-env -delete
    fi
    printf %s "$SKM_ENV_TAG" > "$marker"
  '

echo "run-gateway: pulling $IMAGE"
docker pull --quiet "$IMAGE" >/dev/null || die "could not pull $IMAGE (login to ghcr.io if the package is private)"

if docker inspect "$CONTAINER" >/dev/null 2>&1; then
  echo "run-gateway: replacing the existing '$CONTAINER' container"
  docker rm -f "$CONTAINER" >/dev/null
fi

server_flags=(
  --listen 0.0.0.0:443
  --hostname "$HOSTNAME_"
  --acme --acme-cache /acme
  --metrics-listen 0.0.0.0:9090   # in-container only; backs the image healthcheck
)
[ -n "${SKM_ACME_EMAIL:-}" ] && server_flags+=(--acme-email "$SKM_ACME_EMAIL")
[ -n "${SKM_STAGING:-}" ] && server_flags+=(--acme-staging)

[ "$USE_OIDC" = 1 ] && server_flags+=(--oidc --oidc-audience "${SKM_OIDC_AUDIENCE:-https://$HOSTNAME_}")

# The registration token goes in as an env var, not a flag, so it stays out of
# `docker inspect` / `ps`. It is one-time: ignored once /state holds an identity.
env_args=()
if [ -n "$CONTROL_PLANE" ]; then
  server_flags+=(--control-plane "$CONTROL_PLANE" --control-plane-state /state)
  server_flags+=(--control-plane-name "${SKM_CONTROL_NAME:-$HOSTNAME_}")
  for label in ${SKM_CONTROL_LABELS:-}; do
    server_flags+=(--control-plane-label "$label")
  done
  [ -n "${SKM_CONTROL_TOKEN:-}" ] && env_args+=(-e "SKIMASQUE_CONTROL_TOKEN=$SKM_CONTROL_TOKEN")
fi

server_flags+=("${passthrough[@]}")

echo "run-gateway: starting '$CONTAINER' for $HOSTNAME_ ($env_tag)${CONTROL_PLANE:+, control plane $CONTROL_PLANE}"
docker run -d \
  --name "$CONTAINER" \
  --restart unless-stopped \
  -p "${PORT}:443/udp" \
  -p "${PORT}:443/tcp" \
  "${mounts[@]}" \
  "${env_args[@]}" \
  "$IMAGE" \
  "${server_flags[@]}"

echo
echo "run-gateway: up. Follow it with:"
echo "    docker logs -f $CONTAINER"
