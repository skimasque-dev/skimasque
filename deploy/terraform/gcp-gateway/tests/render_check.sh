#!/usr/bin/env bash
# Render the gateway module's cloud-init through Terraform, validate it with
# cloud-init's own schema, and check its contents.
#
#   deploy/terraform/gcp-gateway/tests/render_check.sh
#
# Needs: terraform, python3 with PyYAML. `cloud-init` is used if installed
# (apt: cloud-init); CI installs it, and the script says so when it is absent.
set -euo pipefail

here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
example="$here/../examples/basic"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

for mode in single-tenant platform; do
  (
    cd "$example"
    terraform init -backend=false -input=false >/dev/null
    echo 'module.gateway.rendered_cloud_init' | TF_VAR_mode=$mode terraform console >"$work/raw.txt"
  )

  # `terraform console` prints a multi-line string as a <<EOT ... EOT heredoc.
  python3 - "$work/raw.txt" "$work/rendered.yaml" <<'PY'
import sys
lines = open(sys.argv[1], encoding="utf-8-sig").read().splitlines()
assert lines[0].startswith("<<EOT"), lines[0]
end = max(i for i, line in enumerate(lines) if line.strip() == "EOT")
open(sys.argv[2], "w", encoding="utf-8").write(chr(10).join(lines[1:end]) + chr(10))
PY

  echo "== mode: $mode"
  if command -v cloud-init >/dev/null 2>&1; then
    cloud-init schema --config-file "$work/rendered.yaml"
  else
    echo "note: cloud-init not installed; skipping the schema check (CI runs it)" >&2
  fi
  python3 "$here/check_cloud_init.py" "$work/rendered.yaml" "$mode"
done
