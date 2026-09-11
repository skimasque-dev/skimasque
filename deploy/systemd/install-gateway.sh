#!/usr/bin/env bash
# Install the skimasque gateway as a systemd service on a Linux host.
#
# Usage:
#   sudo ./deploy/systemd/install-gateway.sh
#   sudo ./deploy/systemd/install-gateway.sh --no-start
#   sudo ./deploy/systemd/install-gateway.sh --bin ./target/release/skimasque-server
#
# The script installs:
#   - /etc/systemd/system/skimasque-gateway.service
#   - /etc/skimasque/gateway.env (from the example template)
#   - /usr/local/bin/skimasque-server
# and then reloads systemd and enables the service.

set -euo pipefail

usage() {
  cat <<'EOF'
Usage: sudo ./deploy/systemd/install-gateway.sh [options]

Options:
  -h, --help        Show this help text.
  --no-start        Install files but do not enable/start the service.
  --bin PATH        Path to the skimasque-server binary to install.
  --env PATH        Path to a custom gateway.env template to install.
EOF
}

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/../.." && pwd)
SERVICE_SRC="$SCRIPT_DIR/skimasque-gateway.service"
ENV_TEMPLATE_SRC="$SCRIPT_DIR/gateway.env.example"

NO_START=0
BIN_OVERRIDE=""
ENV_OVERRIDE=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    -h|--help)
      usage
      exit 0
      ;;
    --no-start)
      NO_START=1
      ;;
    --bin)
      [[ $# -ge 2 ]] || { echo "missing path after --bin" >&2; exit 1; }
      BIN_OVERRIDE="$2"
      shift
      ;;
    --env)
      [[ $# -ge 2 ]] || { echo "missing path after --env" >&2; exit 1; }
      ENV_OVERRIDE="$2"
      shift
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
  shift
done

if [[ ${EUID:-$(id -u)} -ne 0 ]]; then
  echo "This script must be run as root (use sudo)." >&2
  exit 1
fi

if [[ ! -f "$SERVICE_SRC" ]]; then
  echo "service unit not found: $SERVICE_SRC" >&2
  exit 1
fi

if [[ -n "$ENV_OVERRIDE" ]]; then
  if [[ ! -f "$ENV_OVERRIDE" ]]; then
    echo "env template not found: $ENV_OVERRIDE" >&2
    exit 1
  fi
  ENV_TEMPLATE_SRC="$ENV_OVERRIDE"
fi

if [[ -n "$BIN_OVERRIDE" ]]; then
  BIN_SRC="$BIN_OVERRIDE"
else
  BIN_SRC=""
  for candidate in \
    "$REPO_ROOT/target/release/skimasque-server" \
    "$REPO_ROOT/target/debug/skimasque-server"; do
    if [[ -x "$candidate" ]]; then
      BIN_SRC="$candidate"
      break
    fi
  done

  if [[ -z "$BIN_SRC" ]]; then
    echo "No skimasque-server binary found in target/release or target/debug." >&2
    echo "Build it first with: cargo build --release --bin skimasque-server" >&2
    exit 1
  fi
fi

if [[ ! -x "$BIN_SRC" ]]; then
  echo "Binary is not executable: $BIN_SRC" >&2
  exit 1
fi

if ! command -v install >/dev/null 2>&1; then
  echo "install command not found." >&2
  exit 1
fi

install -d -m 0755 /etc/skimasque
install -d -m 0755 /var/lib/skimasque

install -o root -g root -m 0644 "$SERVICE_SRC" /etc/systemd/system/skimasque-gateway.service
install -o root -g root -m 0640 "$ENV_TEMPLATE_SRC" /etc/skimasque/gateway.env
install -o root -g root -m 0755 "$BIN_SRC" /usr/local/bin/skimasque-server

if ! command -v systemctl >/dev/null 2>&1; then
  echo "systemctl not found; files were installed but systemd was not reloaded." >&2
  exit 0
fi

systemctl daemon-reload
if [[ "$NO_START" -eq 0 ]]; then
  systemctl enable --now skimasque-gateway
else
  systemctl enable skimasque-gateway
fi

cat <<EOF
skimasque gateway installed.

Next steps:
  sudoedit /etc/skimasque/gateway.env
  sudo systemctl restart skimasque-gateway
  sudo systemctl status skimasque-gateway --no-pager
EOF
