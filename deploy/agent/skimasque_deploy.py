#!/usr/bin/env python3
"""skimasque-deploy: the pull-based release agent for SkiMasque VMs.

Run by a systemd timer. Reads a per-role *channel pointer* (a version string)
from a release source, and when it differs from the installed version:
downloads that version's binary, verifies its SHA-256, swaps it in atomically,
restarts the service, and health-checks it. An unhealthy new version is rolled
back and remembered as bad so it is not retried. Optionally it also syncs
secrets from GCP Secret Manager into a root-only env file first.

Standard library only: it runs on a stock Ubuntu image with no extra packages.

Configuration is environment (see ``Config.from_env``); state lives under
``$SKIMASQUE_ROOT`` (default ``/``) so the tests can run in a temporary tree.
"""

from __future__ import annotations

import base64
import hashlib
import http.client
import json
import os
import re
import shutil
import socket
import ssl
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Optional

# A version is a path component: never `..`, never a separator.
VERSION_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$")
# What a secret value may contain, so an EnvironmentFile line cannot be broken
# or injected into. Tokens and hex secrets all fit.
SECRET_VALUE_RE = re.compile(r"^[A-Za-z0-9._~+/=:@-]+$")
ENV_VAR_RE = re.compile(r"^[A-Z_][A-Z0-9_]*$")

KEEP_RELEASES = 2
HEALTH_TIMEOUT_S = 60.0
HEALTH_INTERVAL_S = 2.0

METADATA_TOKEN_URL = (
    "http://metadata.google.internal/computeMetadata/v1/"
    "instance/service-accounts/default/token"
)


class DeployError(Exception):
    """A problem worth exiting non-zero for."""


class NotFound(Exception):
    """An object or secret does not exist."""


def log(message: str) -> None:
    print(f"skimasque-deploy: {message}", file=sys.stderr, flush=True)


# --- credentials ------------------------------------------------------------


def metadata_token() -> str:
    """An OAuth access token for the VM's service account."""
    req = urllib.request.Request(
        METADATA_TOKEN_URL, headers={"Metadata-Flavor": "Google"}
    )
    with urllib.request.urlopen(req, timeout=10) as resp:
        return json.load(resp)["access_token"]


# --- release sources --------------------------------------------------------


class FileSource:
    """A directory standing in for the release bucket (tests, and air-gapped use)."""

    def __init__(self, root: str | Path):
        self.root = Path(root)

    def read(self, key: str) -> bytes:
        path = self.root / key
        if not path.is_file():
            raise NotFound(key)
        return path.read_bytes()


class GcsSource:
    """A private GCS bucket, read with the VM's own credentials."""

    def __init__(self, bucket: str, token: Callable[[], str] = metadata_token):
        self.bucket = bucket
        self._token = token

    def read(self, key: str) -> bytes:
        url = (
            f"https://storage.googleapis.com/storage/v1/b/{self.bucket}/o/"
            f"{urllib.parse.quote(key, safe='')}?alt=media"
        )
        req = urllib.request.Request(
            url, headers={"Authorization": f"Bearer {self._token()}"}
        )
        try:
            with urllib.request.urlopen(req, timeout=120) as resp:
                return resp.read()
        except urllib.error.HTTPError as err:
            if err.code == 404:
                raise NotFound(key) from err
            raise


def parse_source(spec: str):
    if spec.startswith("gs://"):
        return GcsSource(spec[len("gs://") :].strip("/"))
    if spec.startswith("file://"):
        return FileSource(spec[len("file://") :])
    raise DeployError(f"unsupported release source {spec!r} (want gs:// or file://)")


# --- secrets ----------------------------------------------------------------


@dataclass(frozen=True)
class SecretSpec:
    secret: str
    var: str
    optional: bool


def parse_secret_map(text: str) -> list[SecretSpec]:
    """``name=VAR,other=VAR2?`` -- a trailing ``?`` marks a secret as optional."""
    specs = []
    for item in filter(None, (part.strip() for part in text.split(","))):
        name, _, var = item.partition("=")
        optional = var.endswith("?")
        var = var.rstrip("?")
        if not name or not ENV_VAR_RE.match(var):
            raise DeployError(f"bad secret mapping {item!r} (want name=ENV_VAR[?])")
        specs.append(SecretSpec(name, var, optional))
    return specs


class SecretManager:
    """GCP Secret Manager, read over REST with the VM's own credentials."""

    def __init__(self, project: str, token: Callable[[], str] = metadata_token):
        self.project = project
        self._token = token

    def access(self, name: str) -> Optional[str]:
        """The latest version's value, or ``None`` if there is none."""
        url = (
            f"https://secretmanager.googleapis.com/v1/projects/{self.project}/"
            f"secrets/{urllib.parse.quote(name, safe='')}/versions/latest:access"
        )
        req = urllib.request.Request(
            url, headers={"Authorization": f"Bearer {self._token()}"}
        )
        try:
            with urllib.request.urlopen(req, timeout=30) as resp:
                payload = json.load(resp)
        except urllib.error.HTTPError as err:
            if err.code == 404:
                return None
            raise
        return base64.b64decode(payload["payload"]["data"]).decode("utf-8")


def write_if_changed(path: Path, content: str, mode: int = 0o600) -> bool:
    """Write ``content`` atomically with ``mode``. ``False`` if nothing changed."""
    if path.exists() and path.read_text() == content:
        os.chmod(path, mode)
        return False
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=path.parent, prefix=f".{path.name}.")
    try:
        with os.fdopen(fd, "w") as handle:
            handle.write(content)
            handle.flush()
            os.fsync(handle.fileno())
        os.chmod(tmp, mode)
        os.replace(tmp, path)
    except BaseException:
        if os.path.exists(tmp):
            os.unlink(tmp)
        raise
    return True


def sync_secrets(specs: list[SecretSpec], manager, path: Path) -> bool:
    """Rewrite the secrets env file. ``True`` if its content changed.

    Any failure other than "the secret does not exist" raises before the
    existing file is touched, so a network blip never blanks a live config.
    """
    lines, missing = [], []
    for spec in specs:
        value = manager.access(spec.secret)
        if value is None:
            if not spec.optional:
                missing.append(spec.secret)
            continue
        value = value.strip()
        if not value or not SECRET_VALUE_RE.match(value):
            raise DeployError(
                f"secret {spec.secret!r} is empty or has characters that are not "
                "allowed in an env file"
            )
        lines.append(f"{spec.var}={value}")
    if missing:
        raise DeployError("missing required secret(s): " + ", ".join(missing))
    return write_if_changed(path, "\n".join(lines) + ("\n" if lines else ""))


# --- systemd and health -----------------------------------------------------


class Systemd:
    def restart(self, unit: str) -> None:
        subprocess.run(["systemctl", "restart", unit], check=True)

    def is_active(self, unit: str) -> bool:
        return (
            subprocess.run(["systemctl", "is-active", "--quiet", unit]).returncode == 0
        )


class _LocalHTTPS(http.client.HTTPSConnection):
    """Connect to 127.0.0.1 but present ``host`` for SNI and certificate checks."""

    def connect(self) -> None:
        sock = socket.create_connection(("127.0.0.1", self.port), self.timeout)
        self.sock = self._context.wrap_socket(sock, server_hostname=self.host)


def make_probe(spec: str, unit: str, systemd) -> Callable[[], bool]:
    """A health probe from ``active``, ``http://...`` or ``https-local://host/path``."""
    if spec == "active":
        return lambda: systemd.is_active(unit)
    if spec.startswith("http://"):

        def probe_http() -> bool:
            try:
                with urllib.request.urlopen(spec, timeout=5) as resp:
                    return 200 <= resp.status < 300
            except (OSError, urllib.error.URLError):
                return False

        return probe_http
    if spec.startswith("https-local://"):
        parsed = urllib.parse.urlparse("https://" + spec[len("https-local://") :])
        host, path = parsed.hostname or "", parsed.path or "/"

        def probe_https() -> bool:
            conn = _LocalHTTPS(
                host, parsed.port or 443, timeout=5, context=ssl.create_default_context()
            )
            try:
                conn.request("GET", path, headers={"Host": host})
                return 200 <= conn.getresponse().status < 300
            except (OSError, ssl.SSLError, http.client.HTTPException):
                return False
            finally:
                conn.close()

        return probe_https
    raise DeployError(f"unsupported health check {spec!r}")


def wait_healthy(
    probe: Callable[[], bool],
    timeout: float = HEALTH_TIMEOUT_S,
    interval: float = HEALTH_INTERVAL_S,
    sleep: Callable[[float], None] = time.sleep,
    clock: Callable[[], float] = time.monotonic,
) -> bool:
    deadline = clock() + timeout
    while True:
        if probe():
            return True
        if clock() >= deadline:
            return False
        sleep(interval)


# --- configuration and state ------------------------------------------------


@dataclass(frozen=True)
class Config:
    role: str  # "control" or "gateway": the channel and bucket prefix
    source: str  # gs://bucket or file:///dir
    service: str  # systemd unit to restart
    binary: str  # file name inside a release
    health: str  # see make_probe
    health_timeout: float = HEALTH_TIMEOUT_S
    root: Path = Path("/")
    secret_map: str = ""
    secrets_file: str = ""
    gcp_project: str = ""

    @classmethod
    def from_env(cls, env) -> "Config":
        def need(name: str) -> str:
            value = env.get(name, "").strip()
            if not value:
                raise DeployError(f"{name} is required")
            return value

        secret_map = env.get("SKIMASQUE_SECRET_MAP", "").strip()
        secrets_file = env.get("SKIMASQUE_SECRETS_FILE", "").strip()
        project = env.get("SKIMASQUE_GCP_PROJECT", "").strip()
        if secret_map and not (secrets_file and project):
            raise DeployError(
                "SKIMASQUE_SECRET_MAP needs SKIMASQUE_SECRETS_FILE and SKIMASQUE_GCP_PROJECT"
            )
        try:
            health_timeout = float(env.get("SKIMASQUE_HEALTH_TIMEOUT", HEALTH_TIMEOUT_S))
        except ValueError as err:
            raise DeployError("SKIMASQUE_HEALTH_TIMEOUT must be a number of seconds") from err
        if health_timeout <= 0:
            raise DeployError("SKIMASQUE_HEALTH_TIMEOUT must be positive")
        return cls(
            role=need("SKIMASQUE_ROLE"),
            source=need("SKIMASQUE_RELEASE_SOURCE"),
            service=need("SKIMASQUE_SERVICE"),
            binary=need("SKIMASQUE_BINARY"),
            health=need("SKIMASQUE_HEALTH"),
            health_timeout=health_timeout,
            root=Path(env.get("SKIMASQUE_ROOT", "/")),
            secret_map=secret_map,
            secrets_file=secrets_file,
            gcp_project=project,
        )

    def path(self, absolute: str) -> Path:
        return self.root / absolute.lstrip("/")

    @property
    def releases(self) -> Path:
        return self.path("/opt/skimasque/releases")

    @property
    def current(self) -> Path:
        return self.path("/opt/skimasque/current")

    @property
    def state(self) -> Path:
        return self.path("/var/lib/skimasque-deploy")


def current_version(cfg: Config) -> Optional[str]:
    try:
        return Path(os.readlink(cfg.current)).name
    except OSError:
        return None


def read_bad(cfg: Config) -> set[str]:
    try:
        return set((cfg.state / f"bad-{cfg.role}").read_text().split())
    except OSError:
        return set()


def add_bad(cfg: Config, version: str) -> None:
    bad = read_bad(cfg) | {version}
    write_if_changed(cfg.state / f"bad-{cfg.role}", "\n".join(sorted(bad)) + "\n", 0o644)


def point_current_at(cfg: Config, version: str) -> None:
    """Atomically repoint ``current`` at ``releases/<version>``."""
    cfg.current.parent.mkdir(parents=True, exist_ok=True)
    tmp = cfg.current.with_name(".current.new")
    if tmp.is_symlink() or tmp.exists():
        tmp.unlink()
    tmp.symlink_to(Path("releases") / version)
    os.replace(tmp, cfg.current)


def install_release(cfg: Config, version: str, binary: bytes) -> None:
    dest = cfg.releases / version
    cfg.releases.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(dir=cfg.releases, prefix=f".{version}."))
    try:
        target = staging / cfg.binary
        target.write_bytes(binary)
        os.chmod(target, 0o755)
        if dest.exists():
            shutil.rmtree(dest)
        os.replace(staging, dest)
    except BaseException:
        shutil.rmtree(staging, ignore_errors=True)
        raise


def prune(cfg: Config, keep: int = KEEP_RELEASES) -> None:
    if not cfg.releases.is_dir():
        return
    dirs = [d for d in cfg.releases.iterdir() if d.is_dir() and not d.name.startswith(".")]
    newest = sorted(dirs, key=lambda d: d.stat().st_mtime, reverse=True)[:keep]
    keepers = {d.name for d in newest}
    current = current_version(cfg)
    if current:
        keepers.add(current)
    for d in dirs:
        if d.name not in keepers:
            shutil.rmtree(d, ignore_errors=True)


# --- the deploy -------------------------------------------------------------


def deploy(
    cfg: Config,
    source,
    systemd,
    sync: Callable[[], bool],
    probe: Callable[[], bool],
    sleep: Callable[[float], None] = time.sleep,
    clock: Callable[[], float] = time.monotonic,
) -> int:
    """One pass. Returns the process exit code."""
    try:
        pointer = source.read(f"channels/{cfg.role}")
    except NotFound:
        log(f"no release promoted for {cfg.role!r} yet; nothing to do")
        return 0
    version = pointer.decode("utf-8", "replace").strip()
    if not VERSION_RE.match(version):
        log(f"the {cfg.role} channel names an invalid version {version!r}; refusing")
        return 1

    current = current_version(cfg)
    if version == current:
        if sync():
            log("secrets changed; restarting")
            systemd.restart(cfg.service)
        return 0
    if version in read_bad(cfg):
        log(f"{version} was rolled back earlier; skipping until the channel moves")
        return 0

    try:
        binary = source.read(f"{cfg.role}/{version}/{cfg.binary}")
        checksum = source.read(f"{cfg.role}/{version}/{cfg.binary}.sha256")
    except NotFound as err:
        log(f"{version}: artifact missing from the release source ({err})")
        return 1
    expected = (checksum.decode("utf-8", "replace").split() or [""])[0].lower()
    if hashlib.sha256(binary).hexdigest() != expected:
        log(f"{version}: SHA-256 mismatch; refusing to install")
        return 1

    # Secrets first: a missing required secret aborts before anything changes.
    sync()

    previous = current
    install_release(cfg, version, binary)
    point_current_at(cfg, version)
    systemd.restart(cfg.service)
    log(f"installed {version} (was {previous}); checking health")

    if wait_healthy(probe, timeout=cfg.health_timeout, sleep=sleep, clock=clock):
        prune(cfg)
        log(f"{version} is healthy")
        return 0

    add_bad(cfg, version)
    if previous and (cfg.releases / previous).is_dir():
        point_current_at(cfg, previous)
        systemd.restart(cfg.service)
        log(f"{version} is UNHEALTHY; rolled back to {previous}")
    else:
        log(f"{version} is UNHEALTHY and there is no earlier release to roll back to")
    return 1


def main(env=None) -> int:
    env = os.environ if env is None else env
    try:
        cfg = Config.from_env(env)
        import fcntl  # imported late so the module also imports on non-Linux hosts

        cfg.state.mkdir(parents=True, exist_ok=True)
        with open(cfg.state / "lock", "w") as lock:
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                log("another run is in progress")
                return 0

            systemd = Systemd()
            specs = parse_secret_map(cfg.secret_map)
            if specs:
                manager = SecretManager(cfg.gcp_project)
                secrets_path = Path(cfg.secrets_file)

                def sync() -> bool:
                    return sync_secrets(specs, manager, secrets_path)

            else:

                def sync() -> bool:
                    return False

            return deploy(
                cfg,
                parse_source(cfg.source),
                systemd,
                sync,
                make_probe(cfg.health, cfg.service, systemd),
            )
    except DeployError as err:
        log(str(err))
        return 1
    except (OSError, urllib.error.URLError, subprocess.CalledProcessError) as err:
        log(f"failed: {err}")
        return 1


if __name__ == "__main__":
    sys.exit(main())
