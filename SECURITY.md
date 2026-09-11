# Security policy

SkiMasque is pre-1.0 and has not had an independent security review. It should
not yet be the only control in front of production infrastructure. See
[`docs/security.md`](docs/security.md) for the model (SkiMasque's guarantees vs
your responsibilities) and [`docs/threat-model.md`](docs/threat-model.md) for
the falsifiable threat analysis.

## Reporting a vulnerability

Please report suspected vulnerabilities privately, not in a public issue:

- **GitHub:** open a [private security advisory](https://github.com/skimasque-dev/skimasque/security/advisories/new).
- **Email:** mcaveniathor@gmail.com — PGP on request.

Include the version or commit, affected component, and a proof of concept if you
have one. Expect an acknowledgement within a few days.

## Incident response

When a report lands or a compromise is suspected:

1. **Triage** — acknowledge within a few days, confirm the affected versions and
   component, and assign a rough severity (impact on the enforcement path,
   whether it needs operator action, whether it is remotely reachable).
2. **Fix on a private branch.** Security fixes are developed in a
   [private advisory](https://github.com/skimasque-dev/skimasque/security/advisories)
   fork, not in public issues or PRs, until a release is ready.
3. **Coordinated disclosure.** The reporter is credited (unless they ask not to
   be) and told the target release. The default embargo is until a fixed
   release is out, or 90 days, whichever comes first.
4. **Release + advisory.** A patched release, a GitHub Security Advisory (which
   requests a CVE), release notes, and — for anything reachable from the
   network — a note in the release announcement telling operators to upgrade.
5. **If active exploitation is suspected**, operators should: rotate
   `--credential-secret` (invalidates every issued platform credential within
   the credential TTL), rotate the TLS key, review the audit log for the
   affected window, tighten or narrow the policy set, and — if the gateway
   itself may be compromised — take it offline; the customer firewall is still
   the outer boundary.

There is no bug-bounty programme yet.

## Secret management

The secrets a deployment holds, and how to treat them:

| Secret | What it is | Handling |
|---|---|---|
| `--credential-secret` / `SKIMASQUE_CREDENTIAL_SECRET` | HS256 key the token-exchange endpoint signs platform credentials with; shared across a gateway fleet so any gateway accepts another's credentials | Inject from a secrets manager — a Kubernetes `Secret` env, or the systemd unit's `EnvironmentFile` at mode `0640` — never in an image or a committed file. Unset means a random key per start, fine for a single gateway. Rotating it invalidates outstanding credentials within their TTL (default 1h); clients re-exchange automatically. Separate values isolate fleets. |
| TLS private key (`--key`, or the `--acme` cache) | The gateway's server identity | Same storage rules; the Helm chart mounts it from a cert-manager `Secret`, or `--acme` obtains and renews a Let's Encrypt certificate itself (its account key and cert live in `--acme-cache` on a persistent volume). `--tls-reload` / ACME both rotate it in place with no dropped tunnel. |
| `--auth-token` / `SKIMASQUE_TOKEN` | Static bearer for the pre-OIDC auth mode | Prefer `--oidc`, which needs no stored secret (verification is against the issuer's public JWKS). If used, treat as a password and rotate on staff changes. |
| CI OIDC token | The runner's short-lived identity token | Never stored — fetched per job, exchanged once, discarded. This is the point of the design: no long-lived CI secret. |

Operational notes:

- Logs do not contain secrets: `--auth-token` / `SKIMASQUE_TOKEN` are marked
  `hide_env_values` in `--help`, and the audit log records policy *decisions*
  (identity, destination, rule) — never a token or credential.
- The only CI secret is the automatic `GITHUB_TOKEN`, used to upload release
  assets and push the container image (and, on a public repo, its provenance
  attestation). No PATs, no deploy keys.
- `deny.toml` and `cargo audit` fail the build on a known-vulnerable or
  unmaintained dependency, so a supply-chain issue in a secret-handling crate
  (`jsonwebtoken`, `rustls`, `ring`) surfaces in CI.

## Supply chain

- `cargo deny check` (advisories, licenses, bans, sources — see [`deny.toml`](deny.toml))
  and `cargo audit` run on every push.
- An SPDX SBOM is attached to each release. Once the repository is public,
  release tarballs and the container image at `ghcr.io/skimasque-dev/skimasque`
  also carry a Sigstore build-provenance attestation
  (`gh attestation verify <tarball> --repo skimasque-dev/skimasque`,
  `gh attestation verify oci://ghcr.io/skimasque-dev/skimasque:<tag> --repo skimasque-dev/skimasque`);
  GitHub's attestation store is not available to user-owned private repos, so
  that step is skipped until then.
- The crypto stack is rustls + `ring`; no OpenSSL or system TLS is linked, and
  `deny.toml` fails the build if one is pulled in.

## Scope

In scope: the gateway (`skimasque-server`), the client, the policy engine, the
identity/credential path, and the deployment artifacts in [`deploy/`](deploy).
Out of scope: findings that require an already-compromised gateway host, and the
deliberately-insecure `--insecure` client flag.
