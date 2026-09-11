# skimasque-policy

The identity-aware network-access policy engine: **WHO** may reach **WHAT**,
**WHERE**, and under which **LIMITS**. Independent of MASQUE.

This crate answers one question — *may this workload reach this destination, and
under what limits?* — and answers it as a pure function. Nothing here opens a
socket, resolves a name, or speaks HTTP; a policy is a value, an evaluation is a
[`Decision`], and the transport layer acts on the result. That separation is
what lets a policy be parsed, tested, and diffed in CI with no network at all.

## The model

Every authorization decision has four parts:

| Part | Type | Example |
|---|---|---|
| **WHO** | `WorkloadIdentity` | `acme/widget`, workflow `deploy.yml`, ref `refs/heads/main` |
| **WHAT** | the application name | `terraform` |
| **WHERE** | `Destination` | `db.production.example.com:5432` |
| **LIMITS** | `Limits` and `SessionSpec` | 20m, 100 Mbps, 50 connections |

A `Policy` binds an identity match to an ordered set of `Rule`s over
applications and destinations. A `PolicySet` is the collection loaded from a
policy directory.

## Two principles the code enforces

**Deny by default.** `Policy::evaluate` returns `Decision::Deny` unless a rule
explicitly allows the destination — there is no implicit trailing `deny *`, the
absence of an allow *is* the denial. Every denial carries a reason and, where it
can, a suggested rule and the closest rules that already exist, so a `DENY` is
something a developer can act on rather than a dead end.

**The application name is session context, not proof.** The engine matches on
whatever application string it is handed, but that string is only as trustworthy
as its source. Treat it as a hint the developer supplied, not an authenticated
fact.

## Example

```rust
use skimasque_policy::{Destination, Policy, RequestContext, Transport, WorkloadIdentity};

let policy = Policy::from_toml(r#"
    name = "production"

    [match]
    repository = "acme/widget"
    branch = "main"

    [[rules]]
    application = "terraform"
    action = "allow"
    destinations = ["api.production.example.com:443"]
"#).unwrap();

let ctx = RequestContext {
    workload: WorkloadIdentity {
        repository: Some("acme/widget".into()),
        git_ref: Some("refs/heads/main".into()),
        ..Default::default()
    },
    application: "terraform".into(),
    transport: Transport::Tcp,
    destination: Destination::parse("api.production.example.com:443").unwrap(),
};

assert!(policy.evaluate(&ctx).is_allow());
```

Policies parse from a rule-oriented TOML layout or an
`identity` / `application` / `network` YAML layout — both produce the same
`Policy` and share all validation. `[[tests]]` assertions travel with a policy
and run in CI; a lint pass warns when a `[match]` block is broad enough to be a
mistake; and learning mode (`suggest_policy`) turns observed
`(application, transport, destination)` tuples into a reviewable draft.

## Part of skimasque

skimasque is identity-aware, least-privilege network access for CI/CD jobs and
developers, built on a from-scratch implementation of IETF MASQUE in Rust. This
engine is usable on its own — it has no MASQUE dependency. See the
[project repository](https://github.com/skimasque-dev/skimasque).

## License

Licensed under either of [Apache License, Version 2.0](../../LICENSE-APACHE) or
[MIT license](../../LICENSE-MIT) at your option. Unless you explicitly state
otherwise, any contribution intentionally submitted for inclusion in this crate
by you, as defined in the Apache-2.0 license, shall be dual licensed as above,
without any additional terms or conditions.
