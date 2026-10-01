# Giving a coding agent network access

> **Applies to:** ✓ Mode 1 · ✓ Mode 2 · ✓ Mode 3 — wherever there is a control plane, because that is what issues and ends sessions. A standalone gateway with policy in files has none.

A coding agent is a program that runs commands for you. You want it to reach a
staging API, not production, for the length of one task, and you want to be able
to stop it. SkiMasque does that in two parts:

- **An agent session** — a short-lived, revocable grant of network access, issued
  to the agent on your behalf and enforced by the gateway.
- **A sandbox you provide** — something that makes the agent use the gateway
  instead of connecting directly. SkiMasque decides *who may reach what*; it does
  not confine a process. See [what SkiMasque does not do](#what-skimasque-does-not-do).

## What a session is

`skimasque agent-session start` asks the control plane for a credential that
asserts **your own identity** (your GitHub login as `actor`) plus
`kind = agent` and a session id. Three consequences follow from that design:

1. **The agent can reach nothing you could not.** There is no separate agent
   authorization system. The credential is your identity with a kind added, so it
   matches the same policies you do. A policy that names `kind = agent` can
   narrow that, or give agents something you do not have.
2. **`kind` is a verified fact, not a claim.** The control plane sets it, only on
   this path. A gateway cannot mint an `agent` or `developer` credential, and a
   token cannot name its own kind.
3. **Labels are not identity.** `--runtime claude-code` and `--run-id` are
   recorded for audit and shown in the console. They are never matched by
   policy. An agent that calls itself `deployment-agent` gains nothing.

A session lasts 30 minutes unless you ask for longer, and never more than 4
hours. It ends when it expires or when you end it, whichever is first.

## Start, hand over, end

```console
$ skimasque login
$ SID=$(skimasque agent-session start --runtime claude-code --run-id fix-1432 \
      --ttl 45m --token-file /run/agent/skimasque.token)
Started agent session sess_3f9c… (ends in 45m).
Credential written to /run/agent/skimasque.token (owner-only).
End it any time: skimasque agent-session end sess_3f9c…

$ skimasque agent-session list
ID                       STARTED BY       RUNTIME        STATE    ENDS
sess_3f9c…               octocat          claude-code    active   in 44m

$ skimasque agent-session end $SID
```

The credential is a secret, so `start` never shows it unless you say where it
goes: `--token-file` (a new file only you can read; it will not overwrite an
existing one without `--force`) or `--print-token` (the token alone on stdout).
If the file cannot be written, the session is ended again rather than left
running with a lost credential.

You can also start, list and end sessions in the console (**Sessions → Agent
sessions**), which also shows what each session was allowed and refused.

### Delegating

`--parent <session-id>` starts a session delegated from one of yours (a sub-agent,
say). A child never outlives its parent, and ending the parent ends the children.

## A policy for agents

Because an agent is you plus `kind = agent`, a policy that does not mention
`kind` applies to both. To give agents less than people, write one policy for
each and name the kind:

```toml
# What an agent may reach: staging only.
name = "agent-staging"
[match]
organization = "acme"
actor = "octocat"
kind = "agent"

[session]
max_duration = "1h"

[[rules]]
application = "*"
action = "allow"
destinations = ["api.staging.acme.dev:443", "github.com:443"]
```

A policy that names a `kind` never matches an identity with none (a credential
from before kinds existed), and a policy that names none matches every kind.
Check what a given identity would get before you rely on it:

```console
$ skimasque why db.prod:5432 --kind agent --actor octocat
```

When an agent is refused, the message names the policy that decided it, and the
console's explanation says if a policy was missed only because of its `kind`.

## Making the agent use it

SkiMasque grants and audits access; **a sandbox makes the agent unable to go
around it.** The shape that works:

```
sandboxed agent ──▶ local proxy (outside the sandbox) ──▶ gateway ──▶ destination
```

1. Start the session and write its credential **outside** the sandbox.
2. Run the network client outside the sandbox too, pointed at that file:
   `skimasque-client --auth-token-file /run/agent/skimasque.token … socks5`.
   `--auth-token-file` keeps the credential out of the process list and the
   environment.
3. Give the sandboxed process only the proxy's address, and block its direct
   outbound networking.

The agent must not be able to read the credential file, the session
`skimasque login` stores (`credentials.json` under `$XDG_CONFIG_HOME/skimasque`,
`~/.config/skimasque`, or `%APPDATA%\skimasque`), the client's process, or any
control socket. Note that a client started with no token of its own falls back
to that login session and mints a *developer* credential from it, so always give
the agent's client a token file. A stolen credential works from anywhere until it expires or is ended.

Proxy environment variables (`ALL_PROXY`, `HTTPS_PROXY`) help programs find the
proxy; they do not enforce anything. What enforces it is the sandbox refusing
everything else. When you set one up, test that each of these **fails** from
inside it:

- a direct TCP or UDP connection to an outside address,
- the same with the proxy variables cleared,
- the same over IPv6,
- DNS to an outside resolver,
- the same from a child process the agent spawns.

An existing sandbox that supports an external HTTP or SOCKS proxy (Anthropic's
sandbox runtime does) is a good starting point, so you do not have to build one.

## Ending a session

`agent-session end` (or the console button) does three things:

- the session's credential is refused for any new tunnel,
- tunnels already open for it are closed,
- every session delegated from it ends too.

Gateways long-poll the control plane for the list of ended sessions, and the
control plane answers the moment one is ended, so this takes about one network
round trip. The list is kept on disk, so a gateway that restarts during an
outage still refuses sessions it knew were ended. If a gateway **cannot reach the
control plane**, it keeps enforcing the last list it has, and a session ended
during the outage is bounded by the credential's own expiry (at most 4 hours).
Tunnels for a session also close when its credential expires, so a tunnel opened
a minute before expiry does not outlive the session.

## What SkiMasque does not do

- **It does not sandbox the agent.** Nothing here stops a process from
  connecting directly if the host lets it.
- **It does not authorize inside a destination.** Reaching a database does not
  make an agent read-only. Keep database permissions and API scopes at the
  destination.
- **It does not prove which program is running.** The runtime label is
  self-reported; the application name is session context, not proof.
- **It does not bind a credential to a machine.** It is a bearer token.
- **It cuts a session's tunnels at expiry, but not other credentials'.** CI and
  developer credentials are checked when a tunnel opens and are not cut mid-way
  (a job longer than the credential's lifetime would break). Agent sessions are.
- **`[session] max_duration` is not enforced by the gateway.** Use `--ttl` to
  bound an agent session.
