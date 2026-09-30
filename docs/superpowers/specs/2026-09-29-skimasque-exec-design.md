# `skimasque exec` — design

Status: approved in conversation 2026-09-29; this document is the written spec.

Implements the developer command the canonical spec (§§ "Developer-friendly",
"Developer identities", the CLI hero, §48, §88) describes and the product
surface amendment lists as Planned:

```
skimasque exec --policy production --app terraform -- terraform apply
```

Run the command, get the access, lose the access when the command is done. It
must feel like a command-execution tool, not a VPN client, and it must work the
same against SkiMasque Cloud and a self-hosted (Black Run) control plane: it
uses only the open tunnel protocol and control-plane API that both implement.

## 1. Command surface

```
skimasque exec [--policy NAME] [--app NAME] [--gateway HOST[:PORT]]
               [--forward [LOCAL_PORT:]HOST:PORT]... [--org ORG]
               [--quiet] [credential and TLS flags] -- COMMAND [ARGS...]
```

- **Credential.** The same sources `skimasque connect` / `skimasque-client`
  accept, in the same precedence: `--auth-token` / `$SKIMASQUE_TOKEN`,
  `--github-oidc`, `--oidc-token` / `$SKIMASQUE_OIDC_TOKEN` (with
  `--oidc-audience`), otherwise a credential minted from the `skimasque login`
  session for `--org` (inferred when the user belongs to exactly one). Short-lived
  credentials are refreshed for as long as the command runs, exactly as
  `skimasque-client` does today. TLS flags (`--ca`, `--insecure`, `--authority`,
  `--template`) are shared too.
- **`--app`.** Sent as `X-Masque-Application`. Defaults to the file stem of
  `COMMAND` (`terraform` for `-- terraform apply`, `psql` for
  `-- /usr/bin/psql.exe …`).
- **`--policy`.** Sent as the new `X-Masque-Policy` header (§3). Optional; with
  none, selection is unchanged.
- **`--gateway`.** Else `$SKIMASQUE_GATEWAY`. Else, only when the control plane
  is SkiMasque Cloud (`https://control.skimasque.com`, compared after trimming a
  trailing `/`), `gateway.skimasque.com`. The control plane is `--control-plane`
  / `$SKIMASQUE_CONTROL_PLANE`, else the one recorded in the login session, else
  Cloud. Against any other control plane with no gateway given, exec fails
  before starting anything:
  `No gateway is configured for <url>. Pass --gateway or set SKIMASQUE_GATEWAY.`
- **`--forward`.** Repeatable. `HOST:PORT` binds an ephemeral loopback port;
  `LOCAL_PORT:HOST:PORT` binds that port. IPv6 hosts are bracketed
  (`15432:[fd00::5]:5432`).
- **`--quiet`.** Suppresses the header (§4). Errors still print.
- Everything after `--` is the command, passed to the OS unchanged (no shell).

## 2. How the child gets network access

1. exec opens one gateway session (the same `open_session` path
   `skimasque-client` uses, moved into the library so both binaries share it)
   with the application and policy headers as session defaults.
2. On `127.0.0.1`, ephemeral ports, it starts:
   - the existing **SOCKS5 relay** (`skimasque_cli::socks5`), and
   - a new minimal **HTTP CONNECT front end** (`skimasque_cli::http_connect`):
     accepts `CONNECT host:port HTTP/1.1`, opens a TCP tunnel to that
     destination, answers `200 Connection Established` and splices; answers
     `403` with the gateway's reason on a policy deny, `502` on other tunnel
     failures, `405` for any other method, `400` for a malformed request line or
     headers over 8 KiB.
3. The child's environment is the parent's plus:
   - `HTTPS_PROXY`, `https_proxy`, `HTTP_PROXY`, `http_proxy` =
     `http://127.0.0.1:<connect-port>`
   - `ALL_PROXY`, `all_proxy` = `socks5h://127.0.0.1:<socks-port>`
   - `NO_PROXY`, `no_proxy` = `localhost,127.0.0.1,::1` — so the child's
     connections to forward listeners are not themselves proxied. Any existing
     value is replaced, not merged: under exec every non-loopback destination
     goes through the tunnel.
   - `SKIMASQUE_PROXY_HTTP`, `SKIMASQUE_PROXY_SOCKS5` — the two front-end
     addresses.
   - one `SKIMASQUE_FORWARD_<HOST>_<PORT>=127.0.0.1:<local>` per forward, with
     `<HOST>` upper-cased and every non-alphanumeric byte turned into `_`
     (`db.prod:5432` → `SKIMASQUE_FORWARD_DB_PROD_5432`).
   Host names reach the gateway unresolved, so private names resolve there.
4. Each `--forward` listener accepts TCP connections and splices each onto a
   new TCP tunnel to its destination.
5. exec spawns the child (stdin/stdout/stderr inherited) and waits. Ctrl-C /
   SIGINT does not end exec: the terminal already delivers it to the child,
   which shares exec's process group, so exec ignores it and keeps the tunnel
   up until the child exits. SIGTERM and SIGHUP sent to exec are forwarded to
   the child. On Windows the child shares the console and receives Ctrl-C
   itself; exec ignores it the same way. When the child exits, exec stops the
   listeners, closes the gateway session (ending every tunnel) and exits.
   exec does not kill the child when the credential or a policy session limit
   expires; the gateway ends the tunnels and the child sees ordinary network
   errors.
6. **Exit status.** The child's exit code. If the child died from a signal
   (Unix), `128 + signal`. `125` if exec fails before launching (bad flags, no
   gateway, authentication, session, preflight). `126` if the command exists but
   cannot be executed, `127` if it is not found.

## 3. The policy pin (open protocol change)

- **Header.** `X-Masque-Policy: <name>`, constant `POLICY_HEADER`
  (`"x-masque-policy"`) exported from `skimasque` beside `APPLICATION_HEADER`.
  Documented in the protocol docs as part of the open tunnel protocol.
- **Engine.** `skimasque_policy::RequestContext` gains
  `pub requested_policy: Option<String>`. `PolicySet::evaluate` selects exactly
  as today. Then, if `requested_policy` is `Some(name)` and the selected
  policy's name is not `name` — including when no policy was selected — the
  result is `Decision::Deny` with the new
  `DenyReason::PolicyMismatch { requested: String, selected: Option<String> }`,
  `policy` set to the selected policy's name (if any), `suggested_rule` as usual
  and `closest` empty. Otherwise evaluation proceeds as today.
  - The pin never *chooses* a policy. Letting the requested name pick among the
    policies whose match accepts the identity would let a workload escape a
    more specific, stricter policy by naming a broader one. The pin can only
    narrow access.
  - `summary()`:
    `Policy "<requested>" does not apply to this identity; "<selected>" does.`
    or, with none selected,
    `Policy "<requested>" does not apply to this identity; no policy does.`
  - `Policy::evaluate` and `PolicySet::evaluate_named` ignore the field: they
    already name their policy.
- **Gateway.** `Enforce::call` reads `POLICY_HEADER` (non-UTF-8 or empty →
  treated as absent) into the context. Observe mode passes it through to the
  engine but still allows.
- **Audit.** `AuditEvent` gains
  `#[serde(skip_serializing_if = "Option::is_none")] pub requested_policy: Option<String>`,
  set from the context on allow and deny. Existing consumers are unaffected
  when it is absent.
- **Every other constructor** of `RequestContext` in the workspace (CLI
  `why`/`policy check`/`explain`/`test`, policy tests, learn) sets
  `requested_policy: None`; `skimasque why` gains `--policy NAME` so the pin can
  be simulated locally.

## 4. Preflight and the header

- **Identity.** Platform credentials are JWTs carrying the
  `WorkloadIdentity` claims. `skimasque_identity::credential` gains
  `peek_identity(token) -> Result<WorkloadIdentity, Error>`: decodes the claims
  **without verifying the signature**, for display and preflight only; its doc
  comment says so. When the bearer is not a platform credential (a static
  `--auth-token`), identity is shown as `unknown`.
- **Preflight** runs only when a `skimasque login` session exists for the
  control plane (not in OIDC/token-only runs, where the gateway's enforcement is
  the only check). For each `--forward` destination — or, with no forwards, the
  placeholder `preflight.invalid:1`, which still reveals which policy is
  selected — exec calls the existing
  `POST /v1/orgs/{org}/policy/simulate` with the peeked identity, the
  application, the destination and transport `tcp`. Then:
  - if `--policy` was given and the simulation's `policy` differs, exec exits
    125: `Policy "production" does not apply to you (alice). SkiMasque selected "staging" for this identity.`
    (or `…selected no policy…`);
  - if every forward destination is denied, exec exits 125 listing each denial
    and its suggested rule;
  - a partially denied set of forwards is shown with ✗ and exec continues.
  A preflight transport failure (control plane unreachable) is a warning, not
  an error: the gateway still enforces.
- **Header** (stderr, suppressed by `--quiet`), aligned like §48 of the
  canonical spec:

  ```
  SkiMasque

  Identity     alice
  Policy       production
  Application  terraform
  Gateway      gateway.skimasque.com

  Access
    db.prod:5432     ✓  → 127.0.0.1:15432

  Session
    20 minutes

  Connected.
  ```

  `Policy` shows the pinned name, else the preflight's selected policy, else
  `(selected by the gateway)`. `Access` lists forwards only (omitted with none;
  ✓/✗ only when preflight ran). `Session` is the `value` of the first allowed
  simulation's limit labelled `duration` (the control plane's label for
  `[session] max_duration`), else omitted. The header is plain text; the CLI
  palette (plan 6) colours it later.

## 5. Code layout

- `crates/skimasque-cli/src/session.rs` — `ConnectionArgs`, `open_session`,
  bearer resolution and credential refresh, moved out of
  `bin/skimasque-client.rs` so `skimasque-client` and `exec` share them.
  Behaviour of `skimasque-client` is unchanged.
- `crates/skimasque-cli/src/http_connect.rs` — the HTTP CONNECT front end.
- `crates/skimasque-cli/src/exec.rs` — argument model, forward-spec parsing,
  env construction, preflight, header rendering, child supervision, exit codes.
- `crates/skimasque-cli/src/bin/skimasque.rs` — the `Exec` subcommand.
- `skimasque-policy` (`eval.rs`), `skimasque` (`service.rs`, `audit.rs`),
  `skimasque-identity` (`credential.rs`) — §3 and §4 changes.

## 6. Planned labels

exec stops being Planned in this project:

- skimasque repo: the product surface amendment's Planned list, the public
  site and CLI docs that mark `skimasque exec` Planned, and the visual library
  gallery copy that does.
- control repo: the console's Planned mentions of exec, together with the
  dependency bump that picks up the new `RequestContext` field, `PolicyMismatch`
  and `requested_policy` (shown in audit detail and Why). This is a separate
  control PR after the skimasque PR merges.

## 7. Errors

In the §89 style, naming the user's mental model:

- no gateway: see §1.
- pin mismatch: see §4.
- not signed in, no token: `Not signed in. Run skimasque login, or pass --github-oidc / --auth-token.`
- command not found: `skimasque exec: terraform: command not found` (127).
- gateway denied a tunnel mid-run: the front ends log one line per denial to
  stderr — `denied db.prod:5432: <reason>. To allow it: <suggested rule>` — and
  the child sees a refused connection (SOCKS5 `not allowed`, HTTP `403`).

## 8. Testing

- **Engine** (`skimasque-policy`): pin matches the selected policy → normal
  evaluation; pin names another existing policy → `PolicyMismatch`; pin names a
  broader policy whose match also accepts the identity → still `PolicyMismatch`
  (the escape case); pin with no matching policy → `PolicyMismatch` with
  `selected: None`; no pin → unchanged.
- **Gateway** (`skimasque`): header parsed into the context; mismatch denied
  with `403` and audited with `requested_policy`; absent header serialises no
  field.
- **Identity:** `peek_identity` round-trips a signer-issued credential and
  rejects a non-JWT.
- **HTTP CONNECT:** request parsing, `405`/`400`/`403`/`502` answers, splice.
- **exec units:** forward-spec parsing (ports, IPv6, bad input), env-var naming,
  `--app` default, Cloud detection and the no-gateway error, exit-code mapping.
- **End to end** against the in-process test gateway: `exec` runs a small test
  helper binary that (a) fetches through `HTTPS_PROXY` and (b) connects to a
  `--forward` address, both reaching a local echo server; the child's exit code
  passes through; a pin mismatch is denied and audited; listeners are closed
  after the child exits.
- CI runs the suite on Linux and Windows (spawn, signal and exit-code paths).

## 9. Out of scope

Header colours (plan 6's CLI palette), transparent interception (TUN / network
namespaces), choosing a gateway through
the control plane, killing the child when access expires, per-user CLI
credentials (still Planned), and UDP `--forward`.
