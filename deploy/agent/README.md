# The SkiMasque release agent

`skimasque-deploy` keeps a VM on the release an operator has *promoted*. It is
pull-based: the VM reads a pointer, so nothing needs inbound access to it and
CI never holds a login to production.

Every two minutes (a systemd timer) it:

1. reads the **channel pointer** `channels/<role>` from the release source. The
   pointer is a version string such as `v0.4.1`; if it is missing, nothing
   happens;
2. if that version is already installed, re-syncs secrets and restarts the
   service only if they changed;
3. otherwise downloads `<role>/<version>/<binary>` and `<binary>.sha256`,
   **verifies the SHA-256**, installs it under
   `/opt/skimasque/releases/<version>/`, atomically repoints
   `/opt/skimasque/current`, and restarts the service;
4. health-checks the service for up to `SKIMASQUE_HEALTH_TIMEOUT` seconds
   (default 60). If it does not come up, the agent **rolls back** to the previous
   release, remembers the bad version in `/var/lib/skimasque-deploy/bad-<role>`
   so it is not retried, and exits non-zero (visible in `journalctl`).

A release is only *trusted* once it has been seen healthy (a `verified-<role>` state file). If a run is
interrupted after the swap, or the first install fails because something outside the VM is not ready yet
(DNS for ACME), the next run health-checks the live release again, without restarting it: it recovers by
itself when the cause clears, or rolls back if there is an earlier release.

**Rollback** is moving the pointer back to an older version. **Integrity** is
the SHA-256 from the same bucket; signature verification is a follow-up.

## Configuration (`/etc/skimasque/deploy.env`)

| Variable | Meaning |
|---|---|
| `SKIMASQUE_ROLE` | `control` or `gateway`: the channel and bucket prefix |
| `SKIMASQUE_RELEASE_SOURCE` | `gs://BUCKET` (or `file:///dir`, for tests) |
| `SKIMASQUE_SERVICE` | the systemd unit to restart |
| `SKIMASQUE_BINARY` | the file name inside a release |
| `SKIMASQUE_HEALTH` | `active`, `http://127.0.0.1:PORT/path`, or `https-local://HOST/path` (connects to 127.0.0.1 but checks HOST's certificate) |
| `SKIMASQUE_HEALTH_TIMEOUT` | seconds to wait for health (default 60) |
| `SKIMASQUE_SECRET_MAP` | optional: `secret-id=ENV_VAR,other=VAR2?`, where `?` marks a secret optional |
| `SKIMASQUE_SECRETS_FILE` | where the secrets env file is written (mode 0600) |
| `SKIMASQUE_GCP_PROJECT` | the project whose Secret Manager holds them |

Secrets are read with the VM's own service account over the metadata server, and
written to an env file the service loads. A missing *required* secret stops the
deploy before anything changes; a failed fetch never truncates the existing file;
values that could break an env file are refused.

`skimasque-prepare-disk DEVICE MOUNTPOINT LABEL` formats a persistent data disk
**only if it is positively blank** and mounts it. A disk that already has a
filesystem is never touched.

## Tests

```bash
python3 -W error::ResourceWarning -m unittest discover -s deploy/agent/tests -v
bash deploy/agent/tests/test_prepare_disk.sh
```

Linux only (symlinks, `fcntl`). On Windows, run them in a container, e.g.
`docker run --rm -v "$PWD:/w" -w /w python:3.12-slim sh -c '...'`.
