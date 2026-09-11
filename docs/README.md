# SkiMasque documentation

Start with the [project README](../README.md) for what SkiMasque is, then:

## Getting going

| Doc | Read it when… |
|---|---|
| [`getting-started.md`](getting-started.md) | you want a GitHub Actions job reaching one protected destination, in ~5 minutes |
| [`deployment-modes.md`](deployment-modes.md) | you're choosing between fully managed, your own gateway, or fully self-hosted |
| [`github-actions.md`](github-actions.md) | you want the OIDC details, other CI systems, or the composite action internals |
| [`policies.md`](policies.md) | you're writing a policy — WHO → WHAT → WHERE → LIMITS, the DSL in full |
| [`cli.md`](cli.md) | you need a specific command or flag of `skimasque` / `skimasque-server` / `skimasque-client` |
| [`configuration.md`](configuration.md) | you need every flag, environment variable, and action input with its default |
| [`troubleshooting.md`](troubleshooting.md) | something failed and you want the fix |

## Running infrastructure

| Doc | Covers |
|---|---|
| [`gateways.md`](gateways.md) | running `skimasque-server` yourself — Docker, systemd, Helm, Terraform, firewall, egress IP (Modes 2–3) |
| [`self-hosting.md`](self-hosting.md) | a fully self-hosted deployment: your own control plane (Mode 3) |
| [`control-plane.md`](control-plane.md) | what a control plane is responsible for, and the SkiMasque Cloud ↔ gateway contract |
| [`protocol.md`](protocol.md) | the control protocol reference — every gateway-facing `/v1` endpoint |

## Understanding it

| Doc | Covers |
|---|---|
| [`architecture.md`](architecture.md) | the invariant, the domain map, the MASQUE transport, the OSS/Cloud/customer split |
| [`security.md`](security.md) | identity, authorization, default-deny, trust boundaries; SkiMasque's guarantees vs your responsibilities |
| [`threat-model.md`](threat-model.md) | assets, attackers, mitigations and their limits, non-goals |
| [`development.md`](development.md) | building, testing, and contributing to this repo |

## Deployment-mode key

Every page below the fold is marked with the modes it applies to:

> **Applies to:** ✓ Mode 1 (fully managed) · ✓ Mode 2 (your gateway) · ✓ Mode 3 (fully self-hosted)
