# Architecture

> **Applies to:** ✓ Mode 1 · ✓ Mode 2 · ✓ Mode 3

How SkiMasque is put together, and the invariants every part holds regardless of
deployment mode. See [`threat-model.md`](threat-model.md) for what it defends
against, [`protocol.md`](protocol.md) for the Gateway↔control-plane wire
contract.

## Two products, one protocol

```
┌──────────────────────────────┐
│        Control plane         │   SkiMasque Cloud (Modes 1–2), or your own (Mode 3)
│  identity · policy workflow   │   PROPRIETARY (SkiMasque Cloud's implementation)
│  dashboard · audit · fleet    │
└──────────────┬───────────────┘
               │  skimasque-protocol  (open — the /v1 endpoints and JSON shapes)
┌──────────────▼───────────────┐
│           Gateway            │   OPEN SOURCE (MIT OR Apache-2.0)
│  MASQUE · QUIC · HTTP/3       │   run by SkiMasque (Mode 1) or you (Modes 2–3)
│  policy enforcement · egress  │
└──────────────┬───────────────┘
               ▼
        your infrastructure
```

**The control plane knows how to manage gateways. Gateways do not need to know
how the control plane is implemented.** That decoupling is what makes Modes 2
and 3 possible.

## What is open source vs SkiMasque Cloud vs customer-operated

| Component | Open source | SkiMasque Cloud | Who runs it |
|---|---|---|---|
| `skimasque-core` — wire formats | ✓ | | — (library) |
| `skimasque-policy` — the policy **engine** | ✓ | | in the gateway + CLI |
| `skimasque-identity` — OIDC verification | ✓ | | in the gateway |
| `skimasque-protocol` — the control protocol | ✓ | | — (contract) |
| `skimasque-server` — the gateway | ✓ | | SkiMasque (Mode 1) / you (Modes 2–3) |
| `skimasque-client` + `skimasque` CLI | ✓ | | you |
| GitHub Action (`skimasque-dev/connect`) | ✓ | | you |
| Control-plane server implementation | | ✓ | SkiMasque (Modes 1–2) / you supply (Mode 3) |
| The managed policy **workflow** — revisions, diffs, simulation, access requests | | ✓ | SkiMasque |
| Dashboard, audit history, org/roles/billing, support | | ✓ | SkiMasque |

The policy **engine** is open source (the gateway and `skimasque policy` need
it); the managed policy **workflow** — reviewed revisions with history, the
"test access" form, access requests — is SkiMasque Cloud.

## The invariant

> **The control plane is authoritative for _desired_ state. Gateways are
> authoritative for _enforcement_.**

- **No control-plane operation changes gateway enforcement directly.** Every
  change flows one way: `draft → validate + test → revision → publish →
  distribution (ETag + long-poll) → gateway`.
- **A control-plane outage degrades management, never enforcement.** A gateway
  that loses its control plane keeps enforcing its cached policy, keeps opening
  tunnels, and keeps writing audit locally, for as long as its cache lease
  allows (soft lease / hard TTL, default 15m / 30m — [`control-plane.md`](control-plane.md)).
- **One decision engine.** `skimasque-policy`'s evaluator is the only thing that
  decides allow/deny — reused by the gateway hot path, `POST
  /v1/orgs/{org}/policy/simulate`, the dashboard "Test access" and "Why", and
  `skimasque why`. Never re-implemented.
- **The gateway enforces; it does not trust.** A valid tunnel credential is not
  an allow. Identity + application + destination + policy + expiry must all
  resolve to an allow, checked *in the gateway*.

## The mental model: an Access Graph

```
IDENTITY  (repo · ref · workflow · actor · org · developer)
   │  is allowed by
   ▼
ACCESS RULE  ── what?  when?  from where?  with what limits?
   │
   ▼
DESTINATION  (host:port, today; a first-class object later)
```

The **gateway** is the path that carries an allowed tunnel — an implementation
detail of *where* enforcement happens, not a top-level concept the customer
reasons about.

## Request lifecycle

```
GitHub Actions runner
  │ 1. OIDC token (aud = the gateway URL)
  ▼
gateway: token-exchange endpoint
  │ 2. verify RS256 · iss · aud · exp → WorkloadIdentity
  │ 3. mint/sign a short-lived platform credential (control plane, or local HS256)
  ▼
client holds the credential
  │ 4. open a tunnel: Proxy-Authorization: Bearer <credential>
  ▼
gateway tower stack
  │ IdentityLayer  — verify the credential locally, recover WorkloadIdentity
  │ PolicyLayer    — evaluate WHO/WHAT/WHERE → AuthorizedDestination or 403 (reason in Proxy-Status)
  │ QuotaLayer     — the decision's [limits]
  │ AddressPolicy  — the SSRF floor, on the RESOLVED address
  ▼
relay to the AuthorizedDestination   (never a bare target)
```

The credential is re-exchanged a quarter of its TTL before expiry, so a job that
outlasts the TTL keeps working. Every allow and deny is recorded through an
`AuditSink`.

## MASQUE transport

The transport is a from-scratch implementation of IETF MASQUE — QUIC, HTTP/3,
and extended `CONNECT` — so UDP (and, in progress, IP) keeps its end-to-end
semantics rather than being wrapped in a reliable stream.

| Mechanism | How SkiMasque uses it |
|---|---|
| **CONNECT-UDP** (RFC 9298) | complete, over HTTP/3 extended `CONNECT`; HTTP Datagrams framed by `skimasque-core` (see the design note below) |
| **CONNECT-TCP** (`draft-ietf-httpbis-connect-tcp`) | classic `CONNECT host:port`, on by default (`--no-connect-tcp` for UDP-only) — the target arrives in `:authority`, the stream is a plain reliable byte stream with no Capsule framing. The template-driven `:protocol = connect-tcp` variant is blocked on `h3` (a closed `Protocol` enum rejects the value before SkiMasque code runs). |
| **CONNECT-IP** (RFC 9484) | wire formats complete; behind the `connect-ip` feature; TUN forwarding not wired |
| **SOCKS5** (RFC 1928) | `CONNECT` (TCP) and `UDP ASSOCIATE`, as the front end `ALL_PROXY` points at |

Internally: `skimasque-core` holds the wire types (no I/O). `skimasque` holds
the transport (`quinn` + `h3`), a `Dispatch` service routing to `UdpProxy` /
`TcpProxy` on the request's protocol, and the `identity → auth → policy → quota`
tower stack above it. `relay_tcp` / the UDP relay run as dedicated tasks, not
Tower requests — the packet hot path is out of the request lifecycle.

## Design notes

- **We frame HTTP Datagrams ourselves.** `h3-datagram` 0.0.2's `Datagram::encode`
  writes the Quarter Stream ID into a scratch buffer and then emits zeroes in
  its place — invisible on stream 0, silently crossing every tunnel after the
  first. `skimasque` does RFC 9297 framing in `skimasque-core` and sends raw
  QUIC datagrams.
- **The address policy runs after DNS.** Filtering on the hostname a client sent
  would be defeated by any name that resolves into private space.
  IPv4-mapped IPv6 addresses are unwrapped first.
- **The policy decision, not the request, chooses the destination.** The proxy
  forwards to an `AuthorizedDestination`; the one escape hatch,
  `AuthorizedDestination::trusting`, is named to be greppable.
- **Tunnels are unreliable, on purpose.** Adding reliability would put a second
  retransmission layer underneath whatever the tunnel carries.

## Where this is heading

1. **Destination objects** — a named `Destination` carrying address, environment,
   owner, sensitivity, so a rule reads `acme/backend → production-db`.
2. **Control-plane event stream** — admin actions as events; Activity becomes a
   projection over gateway decisions *and* those events.
3. **Granular RBAC** — permissions grouped into roles, past owner/member.
4. **Signed per-request authorization artifacts** — a possible future where the
   control plane issues a narrowly-scoped signed capability per request and the
   gateway validates it, instead of pulling the whole policy. The current
   fail-static full-policy model is what ships today.
