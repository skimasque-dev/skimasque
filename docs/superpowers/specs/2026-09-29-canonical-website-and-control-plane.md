<!-- Canonical product/content specification supplied by the product owner on 2026-09-29. Verbatim; the product-surface amendment (2026-09-30-product-surface-amendment.md) records where the build deviates (Planned markers, status wording, /app routes). Section numbers (§N) in plans refer to this file. -->

SkiMasque — Complete Public Website & Control Plane Specification

Status: Canonical product/content specification
Product: SkiMasque
Visual direction: Alpine infrastructure
Primary typeface: IBM Plex Mono
Rendering: Rust + Askama + HTML/CSS + inline SVG
JavaScript: Minimal; progressive enhancement only

---

1. Product Foundation

1.1 Core description

SkiMasque provides identity-aware, least-privilege network access for developers and CI/CD workloads.

The central idea:

«Give every workload exactly the network access it needs, only for as long as it needs it.»

Instead of giving a workload a VPN connection, broad network membership, or permanent credentials, SkiMasque evaluates an authenticated identity against policy and creates a constrained, short-lived network session.

The policy model is:

WHO
 ↓
WHAT
 ↓
WHERE
 ↓
LIMITS
 ↓
NETWORK ACCESS

Example:

WHO
acme/widget
deploy-production
main

WHAT
terraform

WHERE
db.prod:5432

LIMITS
20 minutes
100 Mbps
us-west

RESULT
ACCESS GRANTED

Default behavior:

«No matching allow rule means DENY.»

---

2. Brand Voice

SkiMasque should sound:

- technical without being cryptic
- confident without making exaggerated security claims
- concise
- infrastructure-oriented
- developer-friendly
- practical
- calm
- precise

Avoid:

- generic cybersecurity buzzwords
- fear-based marketing
- excessive “zero trust” terminology
- claims that every existing technology is obsolete
- presenting MASQUE as the product itself
- calling SkiMasque a VPN

Preferred language:

- workload
- identity
- policy
- access
- session
- destination
- gateway
- capability
- temporary
- least privilege
- private infrastructure
- network access

Use:

«“short-lived network access”»

instead of:

«“temporary VPN”»

Use:

«“identity-aware network access”»

instead of:

«“next-generation zero-trust networking”»

Use:

«“gateway”»

instead of:

«“VPN endpoint”»

---

3. Global Navigation

Public navigation

SkiMasque

Product
  How It Works
  Identities
  Policies
  CI/CD
  Developers
  Security
  Deployment

Compare
  How SkiMasque Compares

Technology
  Architecture
  MASQUE

Resources
  Documentation
  Use Cases
  Open Source
  FAQ

Pricing

[Sign In]
[Get Started]

On mobile:

SkiMasque                         [Menu]

---

4. Global Public Footer

SkiMasque

Identity-aware network access
for developers and workloads.

Product
  How It Works
  Identities
  Policies
  CI/CD
  Developers
  Security
  Deployment

Resources
  Documentation
  Architecture
  MASQUE
  Open Source
  FAQ

Company
  About
  Contact
  Status
  Security

Legal
  Privacy
  Terms

© SkiMasque

Footer Alpine treatment:

- dark pine/forest background
- subtle contour lines
- small mountain silhouette
- IBM Plex Mono
- muted slate text
- mint active links

---

5. Shared Public Website Visual System

Alpine visual language

The website should feel like:

«infrastructure mapped like an alpine route.»

Not:

«ski resort marketing.»

Use:

- contour lines
- mountain silhouettes
- elevation markers
- route lines
- trail markers
- topology diagrams
- network boundaries
- restrained Alpine colors

Avoid:

- ski photographs
- cartoon skiing imagery
- oversized goggles/skis
- resort aesthetics
- excessive snow graphics

---

6. Shared Color Semantics

Snow

Backgrounds and surfaces.

Snow
#F4F3ED

Ice

Secondary surface.

Ice
#E5F2EE

Alpine Mint

Primary active/accent color.

Alpine Mint
#72C7A5

Use for:

- active traffic
- access granted
- primary actions
- selected states

Pine

Primary dark infrastructure color.

Pine
#183C35

Use for:

- headers
- major nodes
- infrastructure
- footer
- strong text

Forest

Secondary infrastructure color.

Forest
#28584C

Earth

Gateway/network-edge accent.

Earth
#795C43

Slate

Neutral.

Slate
#66736F

Denied

Use restrained red/orange only for actual denial/warning states.

---

7. Shared Diagram Grammar

Nodes

Reusable SVG nodes:

- Workload
- Developer
- GitHub
- CI Job
- Application
- Identity
- Policy
- Session
- Gateway
- Network
- Database
- API
- Kubernetes
- Cloud
- Firewall
- Internet
- Allow
- Deny

Every node supports:

icon
label
secondary label
status

---

8. Connection Grammar

Solid

Data/network relationship.

Dashed

Control/authorization relationship.

Animated

Active network session.

Faded

Potential/inactive path.

Broken/X

Denied path.

---

9. Status Grammar

Use:

ACCESS GRANTED
ACCESS DENIED
ACTIVE
EXPIRED
PENDING
BLOCKED

Customer-facing UI should generally prefer:

«Access granted»

over:

«ALLOW»

while policy editors can use:

«ALLOW / DENY»

---

10. Shared Core Diagram

The most important SkiMasque diagram:

┌──────────┐
│   WHO    │
└────┬─────┘
     ↓
┌──────────┐
│   WHAT   │
└────┬─────┘
     ↓
┌──────────┐
│  WHERE   │
└────┬─────┘
     ↓
┌──────────┐
│  LIMITS  │
└────┬─────┘
     ↓
┌────────────────┐
│ NETWORK ACCESS │
└────────────────┘

This should appear throughout the website and control plane.

---

11. PUBLIC PAGE: HOMEPAGE

Route:

/

Hero

Give every workload exactly the network access it needs.

SkiMasque provides identity-aware, least-privilege network access for developers and CI/CD workloads.

No broad VPN membership.
No permanent network credentials.
No standing access.

Just the network access required for the job.

[Get Started]
[See How It Works]

Hero diagram:

        IDENTITY
           │
           ▼
        POLICY
           │
           ▼
       TEMPORARY
        SESSION
           │
           ▼
        GATEWAY
           │
           ▼
    PRIVATE SERVICE

Animate the session path subtly.

---

Problem section

Your deployment shouldn't need the whole network.

A deployment may need to reach one database.

That doesn't mean it should automatically gain access to:

- every internal API
- every database
- administrative services
- monitoring systems
- unrelated production infrastructure

Traditional solutions often solve this by putting the workload somewhere inside the network.

SkiMasque solves the problem at the access layer.

Diagram:

TRADITIONAL

CI JOB
  │
  ▼
VPN
  │
  ├── DB
  ├── API
  ├── ADMIN
  ├── MONITORING
  └── INTERNAL SERVICES


SKIMASQUE

CI JOB
  │
  ▼
IDENTITY
  │
  ▼
POLICY
  │
  ▼
DB:5432

---

Product model

Network access as a capability.

Every request answers four questions.

WHO
acme/widget

WHAT
terraform

WHERE
db.prod:5432

LIMITS
20m · 100Mbps

Then:

✓ ACCESS GRANTED

---

Feature cards

Identity-aware

Know what workload or developer is requesting access.

Least privilege

Grant access to specific destinations instead of entire networks.

Short-lived

Access expires automatically.

Policy-driven

Define access declaratively.

Developer-friendly

Use normal commands with "skimasque exec".

Real network access

Built around MASQUE, HTTP/3, and QUIC.

---

Closing CTA

Network access should be temporary.

Define the access your workloads need.

Give it to them.

Let it disappear when the work is done.

[Create Your First Policy]

---

12. PUBLIC PAGE: HOW IT WORKS

Route:

/how-it-works

Hero

From identity to network access.

SkiMasque turns an authenticated identity into a short-lived, policy-controlled network session.

Diagram:

WORKLOAD
   │
   ▼
IDENTITY
   │
   ▼
AUTHENTICATE
   │
   ▼
AUTHORIZE
   │
   ▼
SESSION
   │
   ▼
GATEWAY
   │
   ▼
PRIVATE NETWORK

---

Step 1 — Identify

SkiMasque establishes who or what is making the request.

Example:

repository: acme/widget
workflow: deploy-production
ref: main
application: terraform

---

Step 2 — Authenticate

The identity is verified using the configured identity source.

For GitHub Actions, this can use workload identity/OIDC.

---

Step 3 — Authorize

The identity is evaluated against policy.

WHO
acme/widget

WHAT
terraform

WHERE
db.prod:5432

The policy determines whether the requested access is allowed.

---

Step 4 — Establish a session

If authorized, SkiMasque creates a short-lived network session.

Example:

SESSION
Status: Active
Destination: db.prod:5432
Duration: 20m
Bandwidth: 100 Mbps
Gateway: us-west

---

Step 5 — Connect

Traffic flows through the SkiMasque gateway into the customer's network.

Workload
   │
   │ MASQUE
   ▼
SkiMasque Gateway
   │
   ▼
Customer Network
   │
   ▼
Destination

---

Step 6 — Expire

When the session ends, access disappears.

ACTIVE
  │
  │ expiration
  ▼
EXPIRED

No manual VPN disconnect.

No standing session.

---

13. PUBLIC PAGE: IDENTITIES

Route:

/identities

Hero

Give network access an identity.

Traditional network access often starts with:

«“Where are you connecting from?”»

SkiMasque starts with:

«“Who or what is requesting access?”»

---

Identity types

Developer

developer: alice

Repository

repository: acme/widget

Workflow

workflow: deploy-production

Ref

ref: main

Application

application: terraform

Workload

repository: acme/widget
workflow: deploy-production
ref: main
application: terraform

---

Identity is not authorization

Authentication answers:

«Who are you?»

Authorization answers:

«What may you access?»

Diagram:

IDENTITY
    │
    ▼
POLICY
    │
    ├──── allowed
    │
    └──── denied

---

GitHub Actions

Example:

GitHub
  │
  ▼
Workflow
  │
  ▼
OIDC identity
  │
  ▼
SkiMasque
  │
  ▼
Policy
  │
  ▼
Temporary session

---

Developer identities

developer
    │
    ▼
skimasque exec
    │
    ▼
authenticated identity
    │
    ▼
policy
    │
    ▼
network session

---

Identity + application

A developer running Terraform and a developer running arbitrary networking tools can represent different access requests.

WHO
alice

WHAT
terraform

WHERE
db.prod:5432

versus:

WHO
alice

WHAT
curl

WHERE
db.prod:5432

Policies can distinguish these contexts.

---

Closing

Identity tells SkiMasque who is asking.

Policy determines what happens next.

---

14. PUBLIC PAGE: POLICIES

Route:

/policies

Hero

Turn workload identity into network access.

Policies define exactly what authenticated identities can access.

---

Policy model

WHO
↓
WHAT
↓
WHERE
↓
LIMITS

Example:

WHO
acme/widget
main
deploy-production

WHAT
terraform

WHERE
db.prod:5432

LIMITS
20m
100Mbps
us-west

---

Policy decision

REQUEST
   │
   ▼
IDENTITY
   │
   ▼
POLICY MATCH
   │
   ├──── YES ────► ACCESS GRANTED
   │
   └──── NO ─────► ACCESS DENIED

---

Default deny

No matching allow rule means DENY.

This should be a prominent product principle.

REQUEST
  │
  ▼
NO MATCH
  │
  ▼
ACCESS DENIED

---

Policy examples

Production deployment

name: production-deploy

identity:
  repository: acme/widget
  workflow: deploy-production
  ref: main

application:
  name: terraform

destination:
  host: db.prod
  port: 5432

limits:
  duration: 20m
  bandwidth: 100Mbps
  region: us-west

Developer database access

name: developer-db

identity:
  group: platform

application:
  name: psql

destination:
  host: dev-db.internal
  port: 5432

limits:
  duration: 60m

---

Policy UX

The control plane should never force users to understand the entire policy model immediately.

Start with:

Who needs access?
[acme/widget]

What are they running?
[terraform]

Where do they need to go?
[db.prod:5432]

How long?
[20 minutes]

Bandwidth limit?
[100 Mbps]

Egress region?
[us-west]

Then show the generated policy.

---

15. PUBLIC PAGE: CI/CD

Route:

/ci-cd

Hero

Give CI jobs access to private infrastructure without giving them the whole network.

CI/CD is one of the clearest use cases for SkiMasque.

---

Problem

A deployment job may need to reach:

database
private API
Kubernetes API
internal service
cloud control endpoint

The traditional answer is often:

GitHub Actions
      ↓
VPN / VPC
      ↓
Private network

SkiMasque makes the access specific to the job.

---

GitHub Actions flow

GitHub Actions
      │
      ▼
OIDC identity
      │
      ▼
SkiMasque
      │
      ▼
Policy
      │
      ▼
Temporary session
      │
      ▼
Gateway
      │
      ▼
Customer network
      │
      ▼
Private service

---

Example

A Terraform deployment requests:

WHO
acme/infrastructure
deploy-production
main

WHAT
terraform

WHERE
db.prod:5432

LIMITS
20 minutes
100 Mbps

Result:

ACCESS GRANTED

After 20 minutes:

ACCESS EXPIRED

---

Pull requests vs production

FEATURE BRANCH

feature/*
    ↓
staging-api:443


MAIN

main
    ↓
staging-api:443
db.prod:5432


PRODUCTION WORKFLOW

deploy-production
    ↓
db.prod:5432
api.prod:443

The identity context can change the applicable policy.

---

CI/CD integrations

Initial documentation should prioritize:

- GitHub Actions
- generic CI
- Terraform
- command-line applications

Architecture should remain extensible to:

- GitLab CI
- Buildkite
- CircleCI
- Jenkins
- other workload identity providers

---

16. PUBLIC PAGE: DEVELOPERS

Route:

/developers

Hero

Run the command. Get the access. Lose the access when you're done.

SkiMasque should feel like a command execution tool—not a VPN client.

---

CLI example

skimasque exec \
  --policy production \
  --app terraform \
  -- terraform apply

Conceptually:

COMMAND
   │
   ▼
IDENTITY
   │
   ▼
POLICY
   │
   ▼
SESSION
   │
   ▼
COMMAND EXECUTION

---

No network ceremony

Avoid:

connect VPN
wait
change network
run command
remember to disconnect

Prefer:

skimasque exec --policy production -- terraform apply

---

Developer flow

$ skimasque exec --policy production -- terraform plan

Identity:
  alice

Application:
  terraform

Policy:
  production

Destination:
  db.prod:5432

Session:
  20m

Access:
  GRANTED

---

Local development

Example:

developer
    │
    ▼
local CLI
    │
    ▼
SkiMasque identity
    │
    ▼
policy
    │
    ▼
gateway
    │
    ▼
private development service

---

17. PUBLIC PAGE: HOW SKIMASQUE COMPARES

Route:

/compare

Hero

Network access doesn't have to mean network membership.

Different tools solve different problems.

SkiMasque is specifically designed around identity-aware network access for workloads and developer commands.

---

Comparison matrix

Approach| Primary abstraction| Typical access model
Traditional VPN| Network| Join a private network
Mesh VPN| Machines/networks| Connect trusted nodes
ZTNA| Applications/users| Identity-based application access
Bastion| Infrastructure| Connect through a controlled host
PAM| Privileged access| Manage privileged sessions
SkiMasque| Workload network access| Temporary policy-controlled network capability

This table should remain descriptive rather than claiming that one approach is universally better.

---

VPN

Network first. Policy afterward.

VPNs are designed around network connectivity.

A user or machine joins a network and can then reach resources permitted by network configuration.

SkiMasque starts with an access request.

VPN

MACHINE
   ↓
NETWORK
   ↓
RESOURCE


SKIMASQUE

IDENTITY
   ↓
POLICY
   ↓
SESSION
   ↓
RESOURCE

---

Mesh VPN

Mesh VPNs are useful when the fundamental requirement is connecting machines or networks.

SkiMasque instead focuses on making the workload and access request the unit of authorization.

---

ZTNA

ZTNA systems commonly focus on identity-aware access to applications.

SkiMasque is designed around network-level connectivity for developer and workload workflows where arbitrary protocols may matter.

---

Bastions

A bastion provides a controlled entry point into infrastructure.

SkiMasque can provide temporary network connectivity without requiring every workflow to become an interactive bastion session.

---

Self-hosted runners

Putting CI runners inside a private network is a straightforward architecture.

But it makes network placement part of the security model.

SkiMasque allows workloads to remain outside the private network while receiving narrowly scoped access.

---

Closing

The important question is:

«What should the unit of network access be?»

For SkiMasque:

«The workload and its request.»

---

18. PUBLIC PAGE: DEPLOYMENT

Route:

/deployment

Hero

You choose where the network edge lives.

SkiMasque supports three deployment models.

---

Green Run — SkiMasque Cloud

YOU
  │
  ▼
APPLICATION
  │
  ▼
SKIMASQUE CLOUD
  │
  ▼
SKIMASQUE GATEWAY
  │
  ▼
YOUR NETWORK

You run your apps. We run SkiMasque.

Control plane:

SkiMasque

Gateway:

SkiMasque

Operations:

Minimal

Best for:

- getting started quickly
- teams that don't want to operate networking infrastructure
- standard deployments

---

Blue Run — Your Gateway

APPLICATION
     │
     ▼
SKIMASQUE CLOUD
     │
     ▼
YOUR GATEWAY
     │
     ▼
YOUR VPC

We run the control plane. You run the network edge.

Control plane:

SkiMasque

Gateway:

Customer

The gateway lives inside the customer's network.

This is useful when customers need:

- private routing
- internal firewall control
- customer-owned egress IP
- traffic to remain inside their infrastructure

---

Black Run — Self-hosted

APPLICATION
     │
     ▼
CUSTOMER CONTROL PLANE
     │
     ▼
CUSTOMER GATEWAY
     │
     ▼
CUSTOMER NETWORK

You run everything.

Control plane:

Customer

Gateway:

Customer

Operations:

Customer

Suitable for customers requiring full infrastructure ownership.

---

Important positioning

The run colors represent operational responsibility, not product quality.

GREEN
SkiMasque operates more.

BLUE
Shared responsibility.

BLACK
Customer operates more.

---

19. PUBLIC PAGE: GATEWAYS

Route:

/gateways

Hero

Put the network edge where your infrastructure lives.

The gateway is the point where SkiMasque-controlled sessions enter the customer's network.

---

Basic flow

WORKLOAD
   │
   ▼
SKIMASQUE CONTROL PLANE
   │
   ▼
GATEWAY
   │
   ▼
CUSTOMER NETWORK
   │
   ▼
DESTINATION

---

Customer-operated gateway

                  SKIMASQUE CLOUD
                       │
                       │ control
                       ▼
                 ┌───────────┐
                 │  GATEWAY  │
                 └─────┬─────┘
                       │
                 CUSTOMER VPC
                       │
            ┌──────────┼──────────┐
            ▼          ▼          ▼
           DB         API       K8s

The customer can configure their firewall to permit the gateway's traffic.

---

Egress IP

A customer-operated gateway can provide a stable network source for infrastructure allowlists.

SkiMasque
    │
    ▼
Gateway
    │
    │ fixed egress IP
    ▼
Firewall
    │
    ▼
Private service

---

Multiple gateways

                   CONTROL PLANE
                  /      |       \
                 /       |        \
                ▼        ▼         ▼
             US-WEST   US-EAST   EU-WEST
                │        │         │
                ▼        ▼         ▼
             NETWORK   NETWORK   NETWORK

Policy can include an egress region.

---

20. PUBLIC PAGE: SECURITY

Route:

/security

Hero

Security starts with reducing what needs to be trusted.

SkiMasque's security model is based on minimizing standing network access.

---

Security layers

┌─────────────────────────┐
│       IDENTITY          │
├─────────────────────────┤
│        POLICY           │
├─────────────────────────┤
│      SESSION            │
├─────────────────────────┤
│       GATEWAY           │
├─────────────────────────┤
│ CUSTOMER NETWORK        │
└─────────────────────────┘

---

No standing access

Instead of:

IDENTITY
   ↓
PERMANENT VPN ACCESS
   ↓
NETWORK

SkiMasque:

IDENTITY
   ↓
POLICY
   ↓
TEMPORARY SESSION
   ↓
DESTINATION
   ↓
EXPIRE

---

Default deny

NO MATCH
   ↓
DENY

Only explicit matching access grants a session.

---

Least privilege

Access can be constrained by:

- identity
- application
- destination
- protocol
- port
- duration
- bandwidth
- egress region

---

Customer firewall

SkiMasque does not eliminate the customer's network boundary.

For customer-operated gateways:

SkiMasque Gateway
       │
       ▼
Customer Firewall
       │
       ▼
Private Service

Customers can continue to enforce their own network-level controls.

---

Auditability

Every access decision should have enough context to answer:

WHO
WHAT
WHERE
WHEN
WHICH POLICY
WHICH GATEWAY
RESULT

---

21. PUBLIC PAGE: ARCHITECTURE

Route:

/architecture

Hero

Identity-aware access, built around a real network protocol.

SkiMasque separates the control plane from the network data path.

---

High-level architecture

                       CONTROL PLANE

             ┌──────────────────────────┐
             │                          │
             │       SkiMasque          │
             │                          │
             │ Identity / Policy /      │
             │ Sessions / Audit         │
             │                          │
             └────────────┬─────────────┘
                          │
                          │ authorization
                          │
                          ▼

                       DATA PLANE

WORKLOAD ───── MASQUE ───── GATEWAY ───── PRIVATE NETWORK

---

Control plane

Responsible for:

- identities
- authentication
- policies
- authorization
- session issuance
- gateway management
- audit records
- organization configuration

---

Data plane

Responsible for:

- network sessions
- traffic forwarding
- gateway connectivity
- protocol handling

---

Why separation matters

The control plane decides:

«“Should this session exist?”»

The data plane handles:

«“How does the traffic move?”»

---

22. PUBLIC PAGE: MASQUE

Route:

/technology/masque

Hero

Real MASQUE underneath.

SkiMasque is built around the IETF MASQUE architecture rather than presenting a proprietary tunnel abstraction as the product.

---

Protocol stack

Application
     │
     ▼
MASQUE
     │
     ▼
HTTP/3
     │
     ▼
QUIC
     │
     ▼
UDP / IP

---

HTTP/3 + QUIC

The transport layer provides the foundation for modern multiplexed network sessions.

---

Extended CONNECT

SkiMasque uses the HTTP/3 extended CONNECT mechanisms associated with MASQUE.

---

UDP

UDP semantics can be preserved end-to-end where supported.

---

IP support

IP support is part of the evolving SkiMasque transport implementation.

---

Important positioning

Do not make the website require visitors to understand MASQUE.

Primary message:

«SkiMasque provides identity-aware network access.»

Technical message:

«MASQUE is the network transport underneath it.»

---

23. PUBLIC PAGE: USE CASES

Route:

/use-cases

Hero

Wherever workloads need temporary access to private infrastructure.

---

Terraform

Terraform
   ↓
Production database

Policy:

terraform
→ db.prod:5432
→ 20m

---

Private APIs

CI job
   ↓
private-api.internal:443

---

Kubernetes

Deployment workflow
   ↓
Kubernetes API

---

Database migrations

migration job
   ↓
database:5432

Access can be limited to:

20 minutes

---

Developer debugging

developer
   ↓
skimasque exec
   ↓
private service

---

Infrastructure automation

CI
 ↓
Terraform / Pulumi
 ↓
Private infrastructure

---

24. PUBLIC PAGE: OPEN SOURCE

Route:

/open-source

Hero

Network access infrastructure you can inspect.

SkiMasque's open-source components provide transparency and technical credibility while the hosted service adds operational capabilities.

---

Open architecture

Explain:

- control-plane components where applicable
- gateway
- protocol implementation
- configuration
- deployment
- extension points

Do not artificially cripple core networking functionality solely to create a marketing distinction.

---

Cloud vs self-hosted

OPEN SOURCE
    │
    ▼
CORE NETWORKING
    │
    ▼
SELF-HOSTED

The hosted service adds:

managed infrastructure
+
hosted dashboard
+
billing
+
organization management
+
managed gateways
+
multi-region orchestration
+
advanced audit/search
+
enterprise integrations
+
support

---

Positioning

«Run SkiMasque yourself, or let us operate it for you.»

---

25. PUBLIC PAGE: PRICING

Route:

/pricing

Hero

Pay for the platform. Choose where traffic runs.

Pricing should initially remain simple.

---

Free

$0

For:

- evaluation
- personal projects
- small workloads

---

Team

$49 / month

For small engineering teams.

Include:

- core policies
- workload identities
- CI/CD access
- developer CLI
- basic audit
- managed gateway options

---

Business

$199 / month

For teams with production infrastructure.

Include:

- advanced policy controls
- multiple gateways
- richer audit
- team controls
- advanced integrations
- higher limits

---

Enterprise

Custom

Potential capabilities:

- SSO
- advanced RBAC
- compliance requirements
- dedicated infrastructure
- support agreements
- custom deployment
- contractual requirements

---

Pricing philosophy

Do not initially overcomplicate pricing around:

- bandwidth
- packet counts
- individual users
- number of policy rules

The product value is primarily the managed access-control platform.

---

26. PUBLIC PAGE: DOCUMENTATION

Route:

/docs

Documentation navigation

Getting Started
  Introduction
  Installation
  First Policy
  First Gateway
  First Session

Concepts
  Identities
  Policies
  Sessions
  Gateways
  Control Plane
  Data Plane

CI/CD
  GitHub Actions
  OIDC
  Terraform
  Generic CI

Developers
  CLI
  skimasque exec
  Local Development

Deployment
  SkiMasque Cloud
  Customer Gateway
  Self-hosted

Security
  Identity
  Authorization
  Network Boundaries
  Audit

Reference
  CLI
  Policy
  API
  Configuration

Architecture
  MASQUE
  HTTP/3
  QUIC

---

27. PUBLIC PAGE: FAQ

Route:

/faq

Is SkiMasque a VPN?

Not conceptually.

SkiMasque provides temporary, identity-aware network access rather than making a user or workload a permanent member of a private network.

---

Does traffic go through a SkiMasque gateway?

Yes.

The gateway is the network edge through which the authorized session reaches the destination.

---

Can the gateway run in my VPC?

Yes.

The customer-operated gateway model places the gateway inside the customer's infrastructure.

---

Do I need to change my firewall?

Customers still control their own network boundaries.

A customer-operated gateway can be allowed through existing firewall controls using its network identity/source address.

---

Does SkiMasque replace IAM?

No.

SkiMasque complements identity systems by using authenticated workload identity as an input to network authorization.

---

Does SkiMasque replace a VPN?

It can address some use cases commonly handled with VPNs, particularly temporary workload and developer access.

It is not intended to imply that every VPN use case should be replaced.

---

What happens if no policy matches?

The request is denied.

NO MATCH
   ↓
DENY

---

Does access expire?

Yes.

Sessions are designed to be short-lived and policy-controlled.

---

Can developers use it?

Yes.

The CLI is designed around commands such as:

skimasque exec --policy production -- terraform plan

---

Can CI use it?

Yes.

GitHub Actions is an initial target use case.

---

28. PUBLIC PAGE: TRUST / RELIABILITY

Route:

/trust

Hero

Network access is infrastructure. Treat it like infrastructure.

Cover:

- service architecture
- availability
- gateway health
- session handling
- auditability
- operational boundaries
- security reporting
- incident response
- data handling
- customer responsibility

---

Responsibility model

SKIMASQUE
Control plane
Platform
Managed gateways

CUSTOMER
Policies
Destinations
Firewall
Customer gateways
Identity configuration

---

29. CONTROL PLANE

The authenticated dashboard is the operational center of SkiMasque.

Route:

/app

Primary navigation:

Overview
Policies
Identities
Sessions
Gateways
Audit
Settings

Optional:

Usage
Team
API
Developer

---

30. CONTROL PLANE: GLOBAL SHELL

Desktop:

┌─────────────────────────────────────────────────────┐
│ SkiMasque                              Organization ▼│
├──────────────┬──────────────────────────────────────┤
│              │                                      │
│ Overview     │                                      │
│ Policies     │             PAGE CONTENT             │
│ Identities   │                                      │
│ Sessions     │                                      │
│ Gateways     │                                      │
│ Audit        │                                      │
│              │                                      │
│              │                                      │
│ Settings     │                                      │
│              │                                      │
└──────────────┴──────────────────────────────────────┘

---

31. CONTROL PLANE: OVERVIEW

Route:

/app

Header

Overview

Your network access at a glance.

Primary action:

[Create Policy]

---

Summary cards

ACTIVE SESSIONS
12

POLICIES
24

GATEWAYS
3

IDENTITIES
48

---

Active sessions

ACTIVE SESSIONS

acme/widget
terraform
db.prod:5432
18m remaining

platform/api
deploy
api.prod:443
7m remaining

alice
psql
dev-db:5432
42m remaining

---

Recent access

10:42:18
ACCESS GRANTED
acme/widget
terraform
db.prod:5432

10:41:52
ACCESS DENIED
acme/widget
terraform
admin.prod:443

10:39:02
ACCESS GRANTED
alice
psql
dev-db:5432

---

Gateway health

US-WEST
● Healthy
12 active sessions

US-EAST
● Healthy
4 active sessions

EU-WEST
● Healthy
2 active sessions

---

Primary diagram

IDENTITIES
     │
     ▼
POLICIES
     │
     ▼
ACTIVE SESSIONS
     │
     ▼
GATEWAYS
     │
     ▼
NETWORKS

This is the control-plane overview's primary conceptual diagram.

---

32. CONTROL PLANE: POLICIES LIST

Route:

/app/policies

Header

Policies

Define who can access what, where, and for how long.

[Create Policy]

---

Filters

Search policies...

Status
[All]

Application
[All]

Destination
[All]

---

Policy table

NAME                  IDENTITY          APP        DESTINATION       STATUS

production-deploy     acme/widget       terraform  db.prod:5432      Active

developer-db          platform          psql       dev-db:5432       Active

staging-api            acme/widget       deploy     api.staging:443   Active

---

33. CONTROL PLANE: POLICY DETAIL

Route:

/app/policies/:id

Header

production-deploy

● Active

[Edit Policy]
[Disable]

---

Policy Explorer

┌───────────────────────────────┐
│ WHO                           │
│ acme/widget                   │
│ deploy-production · main      │
├───────────────────────────────┤
│ WHAT                          │
│ terraform                     │
├───────────────────────────────┤
│ WHERE                         │
│ db.prod:5432                  │
├───────────────────────────────┤
│ LIMITS                        │
│ 20m · 100Mbps · us-west       │
├───────────────────────────────┤
│                               │
│        ACCESS GRANTED         │
│                               │
└───────────────────────────────┘

---

Recent decisions

10:42:18
GRANTED
acme/widget
terraform
db.prod:5432

10:39:21
GRANTED
acme/widget
terraform
db.prod:5432

09:58:03
DENIED
acme/widget
terraform
admin.prod:443

---

34. CONTROL PLANE: POLICY CREATION WIZARD

Route:

/app/policies/new

Step 1

Who needs access?

Fields:

Identity source
[GitHub]

Organization
[acme]

Repository
[widget]

Workflow
[deploy-production]

Branch / ref
[main]

Progress:

1 Identity
2 Application
3 Destination
4 Limits
5 Review

---

Step 2

What are they running?

Application
[terraform]

Optional command/context
[terraform apply]

---

Step 3

Where do they need to go?

Destination type
[Hostname]

Hostname
[db.prod]

Port
[5432]

Protocol
[TCP]

---

Step 4

How much access?

Duration
[20 minutes]

Bandwidth
[100 Mbps]

Gateway region
[us-west]

---

Step 5

Review

WHO
acme/widget
deploy-production
main

WHAT
terraform

WHERE
db.prod:5432

LIMITS
20m · 100Mbps · us-west

Then:

[Create Policy]

---

35. CONTROL PLANE: POLICY CREATION UX PRINCIPLE

The wizard should show the result continuously.

At every step, maintain a live summary:

ACCESS REQUEST

acme/widget
terraform
db.prod:5432
20m
100Mbps

        ?

As fields become valid:

ACCESS REQUEST

acme/widget
terraform
db.prod:5432
20m
100Mbps

        ✓

The user should never wonder what the final policy will mean.

---

36. CONTROL PLANE: IDENTITIES

Route:

/app/identities

Header

Identities

The people and workloads requesting network access.

---

Identity list

IDENTITY                    SOURCE       LAST SEEN

acme/widget                 GitHub       2m ago
acme/infrastructure         GitHub       5m ago
alice                       Developer    12m ago
platform                    Group        20m ago

---

Identity detail

acme/widget

Source
GitHub

Repository
acme/widget

Recent workflows
deploy-production
test
staging

Recent access
12 sessions
2 denied requests

---

Identity diagram

GITHUB
   │
   ▼
acme/widget
   │
   ├──── production-deploy
   │
   ├──── staging
   │
   └──── test
          │
          ▼
        POLICIES

---

37. CONTROL PLANE: SESSIONS

Route:

/app/sessions

Header

Sessions

Network access that exists right now.

---

Session table

IDENTITY          APP          DESTINATION       GATEWAY     REMAINING

acme/widget       terraform    db.prod:5432      us-west     18m
alice              psql         dev-db:5432       us-west     42m
platform/api       deploy       api.prod:443      us-east      7m

---

38. CONTROL PLANE: SESSION DETAIL

Route:

/app/sessions/:id

Header

Session

● Active

Started
10:42:18

Expires
11:02:18

---

Session identity

WHO
acme/widget
deploy-production
main

---

Session request

WHAT
terraform

WHERE
db.prod:5432

---

Limits

Duration
20m

Bandwidth
100Mbps

Gateway
us-west

---

Session flow diagram

acme/widget
     │
     │ identity
     ▼
production-deploy
     │
     │ authorized
     ▼
   SESSION
     │
     ▼
 us-west gateway
     │
     ▼
 db.prod:5432

---

Session timeline

10:42:18  REQUEST
10:42:18  AUTHENTICATED
10:42:18  AUTHORIZED
10:42:19  CONNECTED
10:42:19  ACTIVE
11:02:18  EXPIRED

---

39. CONTROL PLANE: GATEWAYS

Route:

/app/gateways

Header

Gateways

The network edges through which authorized sessions connect.

[Add Gateway]

---

Gateway cards

┌────────────────────────────┐
│ US-WEST                    │
│                            │
│ ● Healthy                  │
│                            │
│ Region                     │
│ us-west                    │
│                            │
│ Active sessions            │
│ 12                         │
│                            │
│ Egress IP                  │
│ 203.0.113.10               │
└────────────────────────────┘

---

40. CONTROL PLANE: GATEWAY DETAIL

US-WEST

● Healthy

Type
Customer Gateway

Region
us-west

Version
x.y.z

Egress IP
203.0.113.10

Active sessions
12

Last heartbeat
12 seconds ago

---

Gateway diagram

CONTROL PLANE
      │
      │ control
      ▼
   GATEWAY
      │
      │ data
      ▼
CUSTOMER NETWORK

---

Gateway health timeline

HEALTH
██████████████████████████

Keep the graph simple and readable.

---

41. CONTROL PLANE: AUDIT

Route:

/app/audit

Header

Audit

Every access request, decision, and session in one place.

---

Filters

Search...

Identity
Application
Destination
Result
Gateway
Time

---

Event list

10:42:18
ACCESS GRANTED

acme/widget
terraform
db.prod:5432

Policy:
production-deploy

Gateway:
us-west

10:41:52
ACCESS DENIED

acme/widget
terraform
admin.prod:443

Reason:
No matching allow rule

---

42. CONTROL PLANE: DENIED REQUEST

Denied requests should be understandable rather than merely technical.

ACCESS DENIED

WHO
acme/widget

WHAT
terraform

WHERE
admin.prod:443

RESULT
No matching allow rule

Then:

Applicable policies:

production-deploy
  ✓ identity matched
  ✓ application matched
  ✕ destination did not match

This is extremely useful for debugging policy configuration.

---

43. CONTROL PLANE: ACCESS DECISION EXPLAINER

This should be a reusable component.

REQUEST
   │
   ▼
┌──────────────┐
│   IDENTITY   │
│ acme/widget  │
└──────┬───────┘
       │ ✓
       ▼
┌──────────────┐
│ APPLICATION  │
│ terraform    │
└──────┬───────┘
       │ ✓
       ▼
┌──────────────┐
│ DESTINATION  │
│ db.prod:5432 │
└──────┬───────┘
       │ ✓
       ▼
┌──────────────┐
│    LIMITS    │
│ 20m / 100Mb  │
└──────┬───────┘
       │
       ▼
ACCESS GRANTED

For denied access:

DESTINATION
admin.prod:443
      │
      ✕
      ▼
ACCESS DENIED

---

44. CONTROL PLANE: SETTINGS

Route:

/app/settings

Sections:

Organization
Identity Providers
Policies
Gateways
Security
API
Members
Billing

---

Organization

Organization name
acme

Organization ID
org_xxxxx

---

Identity provider

GitHub
● Connected

Organization
acme

OIDC
Enabled

---

Security

Session defaults
Policy defaults
API credentials
Audit retention

---

45. CONTROL PLANE: TEAM

Route:

/app/settings/team

Members

alice@example.com
Admin

bob@example.com
Developer

carol@example.com
Viewer

Roles should be explicit.

---

46. CONTROL PLANE: BILLING

Route:

/app/settings/billing

Keep billing operationally simple.

Current plan
Business

Next invoice
$199

Billing period
Monthly

Usage should emphasize useful product metrics rather than raw packet counts.

---

47. CONTROL PLANE: DEVELOPER VIEW

Optional route:

/app/developer

Show:

CLI installation
Authentication
Current identity
Recent sessions
CLI credentials

Example:

Current identity

alice

Authenticated
●

Available policies

production
staging
development

---

48. CONTROL PLANE: CLI EXPERIENCE

The dashboard should make CLI usage discoverable.

Example:

$ skimasque exec \
    --policy production \
    --app terraform \
    -- terraform plan

Output:

SkiMasque

Identity     alice
Policy       production
Application  terraform

Access
  db.prod:5432     ✓

Session
  20 minutes

Connected.

---

49. CONTROL PLANE: LIVE NETWORK TOPOLOGY

Future component.

                    POLICIES
                       │
                       ▼
IDENTITIES ───────► SESSIONS
                       │
                       ▼
                    GATEWAYS
                    /      \
                   /        \
                  ▼          ▼
               US-WEST     US-EAST
                  │           │
                  ▼           ▼
              CUSTOMER    CUSTOMER
               NETWORK     NETWORK

Allow the topology to become live when enough product data exists.

---

50. CONTROL PLANE: ACCESS TIMELINE

A reusable visualization:

10:42:18  REQUEST
    │
    ▼
10:42:18  IDENTITY VERIFIED
    │
    ▼
10:42:18  POLICY MATCHED
    │
    ▼
10:42:19  SESSION CREATED
    │
    ▼
10:42:19  CONNECTED
    │
    ▼
11:02:18  SESSION EXPIRED

---

51. CONTROL PLANE: EMPTY STATES

Never use empty states like:

«“No data.”»

Instead:

No policies

No policies yet.

Create your first policy to give a workload
temporary access to private infrastructure.

[Create Policy]

No active sessions

No active sessions.

When a workload or developer receives access,
its active session will appear here.

No gateways

No gateways connected.

Add a gateway to provide a network path
to your infrastructure.

[Add Gateway]

---

52. CONTROL PLANE: DANGEROUS ACTIONS

Actions such as disabling policies or deleting gateways should show impact.

Example:

Disable production-deploy?

This policy currently permits:

acme/widget
terraform
db.prod:5432

3 active sessions currently match this policy.

[Cancel]
[Disable Policy]

Avoid generic confirmation dialogs.

---

53. PUBLIC PAGE: ABOUT

Route:

/about

Hero

Network access should work the way modern infrastructure works.

SkiMasque was built around a simple observation:

Infrastructure is increasingly automated, ephemeral, and identity-aware.

Network access should be too.

---

Philosophy

Identity
    ↓
Intent
    ↓
Policy
    ↓
Capability
    ↓
Expiration

---

54. PUBLIC PAGE: CONTACT

Route:

/contact

Hero

Talk to us about your network access problem.

Suggested form:

Name
Email
Company
Role

How does your infrastructure currently provide
network access to CI/CD or developers?

[Send]

Do not force visitors through a sales qualification funnel.

The question itself provides useful product discovery.

---

55. PUBLIC PAGE: STATUS

Route:

/status

Keep intentionally minimal.

SkiMasque Status

Control Plane
● Operational

Gateway Service
● Operational

Authentication
● Operational

API
● Operational

Historical incidents below.

---

56. DIAGRAM LIBRARY

All diagrams should be reusable components.

Directory:

templates/
└── components/
    ├── diagrams/
    │   ├── access-flow.html
    │   ├── identity-flow.html
    │   ├── policy-flow.html
    │   ├── policy-decision.html
    │   ├── access-lifecycle.html
    │   ├── gateway.html
    │   ├── network-boundary.html
    │   ├── deployment-model.html
    │   ├── ci-flow.html
    │   ├── developer-flow.html
    │   ├── control-data-plane.html
    │   ├── masque-stack.html
    │   ├── security-layers.html
    │   ├── session-flow.html
    │   ├── audit-flow.html
    │   └── topology.html

---

57. NODE COMPONENTS

templates/
└── components/
    └── nodes/
        ├── workload.html
        ├── developer.html
        ├── github.html
        ├── ci-job.html
        ├── identity.html
        ├── application.html
        ├── policy.html
        ├── session.html
        ├── gateway.html
        ├── network.html
        ├── service.html
        ├── database.html
        ├── api.html
        ├── kubernetes.html
        ├── firewall.html
        ├── cloud.html
        ├── allow.html
        └── deny.html

---

58. POLICY COMPONENTS

templates/
└── components/
    └── policy/
        ├── policy-card.html
        ├── policy-explorer.html
        ├── policy-summary.html
        ├── policy-decision.html
        ├── policy-match.html
        └── policy-diff.html

---

59. SESSION COMPONENTS

templates/
└── components/
    └── sessions/
        ├── session-card.html
        ├── session-summary.html
        ├── session-timeline.html
        ├── session-status.html
        └── session-flow.html

---

60. STATUS COMPONENTS

templates/
└── components/
    └── status/
        ├── granted.html
        ├── denied.html
        ├── active.html
        ├── expired.html
        ├── pending.html
        └── blocked.html

---

61. PUBLIC DIAGRAM SET

The initial public website should contain:

1. Identity → Policy → Access
2. WHO → WHAT → WHERE → LIMITS
3. Traditional network vs SkiMasque
4. Access lifecycle
5. Policy decision
6. GitHub Actions
7. Developer CLI
8. Gateway
9. Customer VPC
10. Deployment models
11. Security layers
12. Control plane/data plane
13. MASQUE stack
14. Multiple gateways
15. Audit flow

---

62. CONTROL PLANE DIAGRAM SET

The dashboard should contain:

1. Policy Explorer
2. Identity flow
3. Access decision
4. Session lifecycle
5. Session topology
6. Gateway topology
7. Gateway health
8. Audit timeline
9. Denied-request explanation
10. Organization network topology

---

63. ASKAMA COMPONENT ARCHITECTURE

Use Askama to populate diagrams with real product data.

Example conceptual component:

policy-explorer

should accept:

identity
application
destination
limits
decision

The same component can render:

Marketing example

or:

Live customer policy

This avoids maintaining separate visual languages.

---

64. HTML VS SVG

Use HTML/CSS for:

- cards
- policy summaries
- timelines
- tables
- badges
- simple flows
- responsive layouts

Use inline SVG for:

- architecture diagrams
- topology
- network boundaries
- complex flows
- animated traffic
- Alpine route illustrations

Do not introduce a diagramming library initially.

SkiMasque diagrams are designed illustrations, not arbitrary graph editors.

---

65. SVG ACCESSIBILITY

Every meaningful SVG should contain:

<title>
<desc>

Important information should also be represented in HTML.

Never rely on color alone.

For example:

● ACCESS GRANTED

not merely:

green circle

---

66. RESPONSIVE DIAGRAMS

Desktop:

A → B → C → D → E

Mobile:

A
↓
B
↓
C
↓
D
↓
E

Do not shrink complex diagrams until their labels become unreadable.

---

67. ANIMATION

Animations should communicate state.

Active session

Animate a subtle pulse along the connection.

Authorization

Animate:

REQUEST
  ↓
POLICY
  ↓
GRANTED

Expiration

Fade the active connection.

Denial

Show a stopped/broken route.

Respect:

prefers-reduced-motion

---

68. ALPINE DESIGN MOTIFS

Use:

Contour lines

For backgrounds and empty space.

Route lines

For network paths.

Elevation markers

For section labels or progress.

Trail markers

For status/steps.

Mountain silhouettes

For hero/footer decoration.

Ski-run markers

For deployment models.

Do not let the metaphor obscure the product.

---

69. DEPLOYMENT RUN VISUAL SYSTEM

Green Run:

SkiMasque Cloud

Blue Run:

Your Gateway

Black Run:

Self-hosted

Always display the technical name alongside the metaphor.

Example:

GREEN RUN

SkiMasque Cloud

SkiMasque operates the control plane
and gateway.

---

70. PRODUCT UX PRINCIPLE

The dashboard should answer four questions immediately:

WHAT
is configured?

WHO
can access?

WHERE
can they go?

WHAT IS HAPPENING NOW?

The user should not need to navigate through multiple screens to understand a security decision.

---

71. POLICY EXPLAINABILITY

Every access decision should be explainable.

For granted:

Access granted because:

✓ Identity matched
✓ Application matched
✓ Destination matched
✓ Policy active
✓ Session within limits

For denied:

Access denied because:

✓ Identity matched
✓ Application matched
✕ Destination did not match

This should become a major product differentiator in UX.

---

72. AUDITABILITY

An audit event should contain:

timestamp
identity
application
destination
policy
decision
gateway
session
limits

Example:

{
  "time": "10:42:18",
  "identity": "acme/widget",
  "application": "terraform",
  "destination": "db.prod:5432",
  "policy": "production-deploy",
  "decision": "granted",
  "gateway": "us-west",
  "duration": "20m"
}

---

73. PRODUCT DATA MODEL VISUALIZATION

The entire control plane can be understood as:

IDENTITY
   │
   │ evaluated by
   ▼
POLICY
   │
   │ authorizes
   ▼
SESSION
   │
   │ uses
   ▼
GATEWAY
   │
   │ reaches
   ▼
DESTINATION

Audit observes the entire chain:

IDENTITY
    │
    ▼
POLICY
    │
    ▼
SESSION
    │
    ▼
GATEWAY
    │
    ▼
DESTINATION
    │
    ▼
AUDIT EVENT

---

74. PUBLIC WEBSITE INFORMATION ARCHITECTURE

/
├── /how-it-works
├── /identities
├── /policies
├── /ci-cd
├── /developers
├── /compare
├── /deployment
├── /gateways
├── /security
├── /architecture
├── /technology/masque
├── /use-cases
├── /open-source
├── /pricing
├── /docs
├── /faq
├── /trust
├── /about
├── /contact
└── /status

---

75. CONTROL PLANE INFORMATION ARCHITECTURE

/app
├── /policies
│   ├── /new
│   └── /:id
├── /identities
│   └── /:id
├── /sessions
│   └── /:id
├── /gateways
│   └── /:id
├── /audit
├── /developer
└── /settings
    ├── /organization
    ├── /team
    ├── /identity
    ├── /security
    ├── /api
    └── /billing

---

76. PAGE RELATIONSHIPS

Public conceptual journey:

HOW IT WORKS
      │
      ▼
IDENTITIES
      │
      ▼
POLICIES
      │
      ▼
SESSIONS
      │
      ▼
GATEWAYS
      │
      ▼
NETWORK

Customer operational journey:

IDENTITY
   ↓
POLICY
   ↓
SESSION
   ↓
GATEWAY
   ↓
AUDIT

This relationship should remain consistent across the entire product.

---

77. HOMEPAGE → DASHBOARD CONTINUITY

The public website should teach exactly the same concepts the dashboard uses.

Website:

WHO
WHAT
WHERE
LIMITS

Dashboard:

WHO
WHAT
WHERE
LIMITS

Website:

IDENTITY
→ POLICY
→ SESSION
→ GATEWAY

Dashboard:

IDENTITY
→ POLICY
→ SESSION
→ GATEWAY

This dramatically reduces the learning curve.

---

78. PRIMARY USER JOURNEY

A new customer should be able to understand SkiMasque through this sequence:

LANDING PAGE
     ↓
HOW IT WORKS
     ↓
CREATE ACCOUNT
     ↓
CONNECT IDENTITY
     ↓
CREATE POLICY
     ↓
CONNECT GATEWAY
     ↓
RUN COMMAND
     ↓
SEE SESSION
     ↓
SEE AUDIT EVENT

The product should demonstrate its own conceptual model during onboarding.

---

79. ONBOARDING

After signup:

Welcome to SkiMasque.

Let's give your first workload
temporary access to a private destination.

Step 1:

Connect an identity source

Step 2:

Create a policy

Step 3:

Connect a gateway

Step 4:

Run your first command

Step 5:

Watch the session

---

80. FIRST-POLICY DEMO

Provide a safe example:

WHO
acme/widget

WHAT
terraform

WHERE
db.staging:5432

LIMITS
10 minutes

Then show:

This policy allows:

acme/widget
running terraform
to reach db.staging:5432
for up to 10 minutes.

Then:

[Create Example Policy]

---

81. FIRST-SESSION EXPERIENCE

When the first session starts, the dashboard should visibly connect the concepts:

YOUR FIRST SESSION

Identity
acme/widget

Policy
staging-db

Destination
db.staging:5432

Gateway
us-west

Status
● ACTIVE

Expires
9m 42s

Then show the route:

acme/widget
     │
     ▼
 staging-db
     │
     ▼
  session
     │
     ▼
 us-west
     │
     ▼
db.staging:5432

---

82. FIRST-AUDIT EVENT

After the session ends:

SESSION EXPIRED

The network capability granted to this workload
has ended.

No manual disconnect was required.

Then:

[View Audit Event]

This makes the core product promise tangible.

---

83. DASHBOARD DESIGN LANGUAGE

The dashboard should feel more like an operations console than a generic SaaS dashboard.

Use:

- dense but readable information
- monospaced values
- clear state indicators
- topology
- policy summaries
- timestamps
- route visualizations
- subtle grid/contour backgrounds

Avoid:

- giant generic KPI cards
- excessive gradients
- excessive rounded cards
- decorative charts without operational meaning

---

84. PRIMARY DASHBOARD CARD TYPES

Reusable cards:

PolicyCard
IdentityCard
SessionCard
GatewayCard
DecisionCard
AuditEventCard
TopologyCard
HealthCard

Each card should answer:

What is this?
What state is it in?
What does it affect?
What can I do?

---

85. PUBLIC WEBSITE COMPONENT LIBRARY

components/
├── navigation
├── hero
├── feature-grid
├── CTA
├── policy-explorer
├── diagram
├── code-example
├── comparison-table
├── deployment-card
├── run-marker
├── architecture-diagram
├── FAQ
├── testimonial
├── footer
└── Alpine-background

---

86. CONTROL PLANE COMPONENT LIBRARY

components/
├── sidebar
├── topbar
├── page-header
├── breadcrumb
├── tabs
├── table
├── card
├── badge
├── policy-explorer
├── decision
├── session
├── gateway
├── identity
├── timeline
├── topology
├── filter-bar
├── empty-state
├── confirmation
└── wizard

---

87. ICONOGRAPHY

Use a consistent technical icon set.

Suggested concepts:

Identity      person / fingerprint
Workload      cube / process
GitHub        source repository
Policy        document / branching rule
Session       clock / route
Gateway       mountain pass / edge
Network       connected nodes
Database      cylinder
API           brackets / endpoint
Allow         route marker / check
Deny          blocked route / X

Avoid making every icon a shield or lock.

SkiMasque is about controlled movement through infrastructure, not generic cybersecurity symbolism.

---

88. CODE EXAMPLE STYLE

Code examples should use:

- IBM Plex Mono
- restrained syntax highlighting
- dark pine backgrounds
- mint highlights
- copy button
- minimal chrome

Example:

skimasque exec \
  --policy production \
  --app terraform \
  -- terraform plan

Below it:

Identity: alice
Policy: production
Session: 20m

---

89. ERROR MESSAGE STYLE

Errors should explain the user's mental model.

Bad:

403 POLICY_MATCH_FAILED

Better:

Access denied.

No policy allows:

acme/widget
terraform
admin.prod:443

The identity was authenticated successfully,
but no matching allow rule was found.

---

90. SUCCESS MESSAGE STYLE

Bad:

Tunnel established.

Better:

Access granted.

acme/widget
terraform
→ db.prod:5432

Session expires in 20 minutes.

Avoid calling the connection a “VPN tunnel.”

---

91. DENIAL EXPERIENCE

Denial should never feel mysterious.

Show:

ACCESS DENIED

Identity
acme/widget

Application
terraform

Destination
admin.prod:443

Why?
No matching allow rule.

Then provide:

[View Matching Policies]
[Create Policy]

where appropriate.

---

92. POLICY DIFF

When editing a policy, show changes clearly.

BEFORE

Destination
db.prod:5432

Duration
20m

AFTER

Destination
db.prod:5432
api.prod:443

Duration
30m

Use:

+ added
- removed
~ changed

This is particularly important for security-sensitive configuration.

---

93. LIVE POLICY SIMULATION

Future feature.

Allow users to ask:

Would this request be allowed?

Input:

Identity
acme/widget

Application
terraform

Destination
db.prod:5432

Output:

ACCESS GRANTED

Matched policy:
production-deploy

Limits:
20m
100Mbps
us-west

Or:

ACCESS DENIED

No matching allow rule.

This could become one of the most useful control-plane features.

---

94. POLICY TESTING

Future feature:

Policy tests

Example:

✓ production terraform → db.prod:5432
✓ production terraform → api.prod:443
✕ production terraform → admin.prod:443
✕ feature branch → db.prod:5432

This turns policy configuration into something users can validate before deploying.

---

95. LONG-TERM CONTROL PLANE

Eventually:

Overview
Policies
Identities
Sessions
Gateways
Topology
Audit
Analytics
Team
Developer
Settings

Analytics could show:

Access requests
Granted
Denied
Expired
By identity
By destination
By application
By gateway

But analytics should never replace the primary operational model.

---

96. ADVANCED TOPOLOGY

Future visualization:

                      CONTROL PLANE
                           │
             ┌─────────────┼─────────────┐
             │             │             │
             ▼             ▼             ▼
          IDENTITY       POLICY       AUDIT
                           │
                           ▼
                        SESSION
                      /    |     \
                     /     |      \
                    ▼      ▼       ▼
                 GATEWAY GATEWAY GATEWAY
                   │       │       │
                   ▼       ▼       ▼
                 VPC     VPC      VPC
                   │       │       │
                   ▼       ▼       ▼
                  DB      API      K8s

Allow interactive inspection.

---

97. VISUAL COMPONENT PHILOSOPHY

The diagrams are not decoration.

They should communicate:

identity
→ authorization
→ capability
→ network path
→ expiration

A visitor should be able to understand SkiMasque from the diagrams even before reading every paragraph.

---

98. CONTENT HIERARCHY

Every public page should follow approximately:

HERO
 ↓
PROBLEM
 ↓
CORE IDEA
 ↓
DIAGRAM
 ↓
EXAMPLE
 ↓
DETAIL
 ↓
USE CASE
 ↓
TECHNICAL DEPTH
 ↓
CTA

Do not make every page identical.

Use the structure as a rhythm rather than a rigid template.

---

99. CTA STRATEGY

Primary CTA:

Get Started

Secondary:

See How It Works

Technical visitors:

Read the Docs

Potential customers:

Try SkiMasque

Enterprise:

Talk to Us

Avoid:

Buy Now

until the product and sales motion justify it.

---

100. THE CORE MESSAGE

Everything should ultimately reinforce one idea:

NETWORK ACCESS

should belong to

THE REQUEST

not

THE NETWORK.

SkiMasque makes that request explicit:

WHO
WHAT
WHERE
LIMITS

Then turns it into:

SHORT-LIVED
IDENTITY-BOUND
POLICY-CONTROLLED
NETWORK ACCESS

---

101. COMPLETE PRODUCT STORY

The entire SkiMasque website and control plane should tell one coherent story:

Modern workloads need private infrastructure.

        ↓

Putting them inside the network gives them
more access than they necessarily need.

        ↓

Identity provides context.

        ↓

Policy defines permission.

        ↓

A session turns permission into a temporary
network capability.

        ↓

A gateway provides the network path.

        ↓

Expiration removes the capability.

        ↓

Audit records what happened.

Or, in its shortest form:

IDENTITY
    ↓
POLICY
    ↓
SESSION
    ↓
GATEWAY
    ↓
NETWORK
    ↓
EXPIRE
    ↓
AUDIT

---

102. FINAL DESIGN PRINCIPLE

The website, documentation, and control plane should feel like different views of the same system.

Marketing explains it.

Documentation teaches it.

The control plane operates it.

The diagrams visualize it.

The CLI executes it.

The underlying protocol transports it.

Everything should converge on:

«Give every workload exactly the network access it needs, only for as long as it needs it.»

---