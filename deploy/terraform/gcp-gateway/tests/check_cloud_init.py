#!/usr/bin/env python3
"""Check the gateway module's rendered cloud-init.

    check_cloud_init.py RENDERED.yaml

Verifies that the files cloud-init will write are byte-identical to the ones in
this repository (so a VM cannot run a different agent or unit than the repo's),
and that the gateway's configuration is what the module promises.
"""

import base64
import re
import sys
from pathlib import Path

import yaml

REPO_DEPLOY = Path(__file__).resolve().parents[3]  # .../deploy


def main(rendered: str, mode: str = "single-tenant") -> int:
    doc = yaml.safe_load(Path(rendered).read_text())
    files = {f["path"]: f for f in doc["write_files"]}
    failures = []

    def decoded(path: str) -> bytes:
        return base64.b64decode(files[path]["content"])

    embedded = {
        "/usr/local/sbin/skimasque-deploy": REPO_DEPLOY / "agent/skimasque_deploy.py",
        "/usr/local/sbin/skimasque-prepare-disk": REPO_DEPLOY / "agent/skimasque-prepare-disk",
        "/etc/systemd/system/skimasque-deploy.service": REPO_DEPLOY / "agent/skimasque-deploy.service",
        "/etc/systemd/system/skimasque-deploy.timer": REPO_DEPLOY / "agent/skimasque-deploy.timer",
        "/etc/systemd/system/skimasque-gateway.service": REPO_DEPLOY / "systemd/skimasque-gateway.service",
    }
    for target, source in embedded.items():
        if target not in files:
            failures.append(f"missing write_files entry {target}")
        elif decoded(target) != source.read_bytes():
            failures.append(f"{target} differs from {source}")

    def content(path: str) -> str:
        return files[path]["content"]

    def expect(text: str, needle: str, what: str) -> None:
        if needle not in text:
            failures.append(f"{what}: expected {needle!r}")

    args = content("/etc/skimasque/gateway.env")
    for flag in ("--acme", "--control-plane https://", "--control-plane-state /var/lib/skimasque/control",
                 "--metrics-listen 127.0.0.1:9090", "--github-oidc"):
        expect(args, flag, "gateway.env")
    if "SKIMASQUE_CONTROL_TOKEN" in args or "registration" in args:
        failures.append("gateway.env must not carry the registration token")

    deploy = content("/etc/skimasque/deploy.env")
    for line in ("SKIMASQUE_ROLE=gateway", "SKIMASQUE_BINARY=skimasque-server",
                 "SKIMASQUE_HEALTH=http://127.0.0.1:9090/healthz", "SKIMASQUE_CONTROL_TOKEN?"):
        expect(deploy, line, "deploy.env")

    # Every flag must exist in the real server. This is what catches a flag that is
    # not (yet) implemented, before a VM crash-loops on it.
    server_source = (REPO_DEPLOY.parent / "crates/skimasque-cli/src/bin/skimasque-server.rs").read_text()
    for flag in re.findall(r"(?<![\w-])--([a-z][a-z0-9-]*)", args):
        field = flag.replace("-", "_")
        if not (re.search(rf"\b{field}\s*:", server_source) or f'"{flag}"' in server_source):
            failures.append(f"gateway.env uses --{flag}, which skimasque-server does not define")
    if (mode == "platform") != ("--platform" in args):
        failures.append(f"mode {mode} does not match the --platform flag in gateway.env")
    if "--platform" in args:
        # Mode 1: the server derives every audience itself and refuses nothing here
        # except the flags below, so the module must not emit them.
        if "--oidc-audience" in args:
            failures.append("--platform ignores --oidc-audience; the module must not emit it")
        for refused in ("--oidc-issuer", "--policy-dir", "--policy-file", "--policy-reload", "--auth-token", "--policy-observe"):
            if refused in args:
                failures.append(f"--platform refuses {refused}")
        if "--control-plane-state" not in args:
            failures.append("--platform needs --control-plane-state for its identity and tenant cache")
    elif "--github-oidc" in args and "--oidc-audience" not in args:
        failures.append("--github-oidc without --oidc-audience fails closed at startup")

    dropin = content("/etc/systemd/system/skimasque-gateway.service.d/deploy.conf")
    expect(dropin, "RequiresMountsFor=/var/lib/private/skimasque", "drop-in")
    expect(dropin, "ConditionPathExists=/opt/skimasque/current/skimasque-server", "drop-in")
    expect(dropin, "EnvironmentFile=-/etc/skimasque/gateway-secrets.env", "drop-in")

    runcmd = [c if isinstance(c, str) else " ".join(c) for c in doc["runcmd"]]
    chained = [c for c in runcmd if "skimasque-prepare-disk" in c and "google-skimasque-state" in c]
    if not chained:
        failures.append("runcmd must prepare the state disk")
    else:
        script = chained[0]
        prepare = script.index("skimasque-prepare-disk")
        if "set -e" not in script:
            failures.append("the disk-prepare and service-enable steps must stop at the first failure (set -e)")
        for later in ("systemctl enable skimasque-gateway.service", "systemctl enable --now skimasque-deploy.timer"):
            if later not in script or script.index(later) < prepare:
                failures.append(f"{later!r} must come after the disk is prepared, in the same script")

    for failure in failures:
        print("FAIL:", failure)
    if not failures:
        print(f"cloud-init checks passed ({len(embedded)} embedded files identical)")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(*sys.argv[1:3]))
