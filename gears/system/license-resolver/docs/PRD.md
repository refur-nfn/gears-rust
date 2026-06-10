<!-- cpt:
version: 1.0.0
status: draft
module: license-resolver
system: cf
-->

# PRD — License Resolver

<!-- toc -->

- [1. Overview](#1-overview)
  - [1.1 Purpose](#11-purpose)
  - [1.2 Background / Problem Statement](#12-background--problem-statement)
  - [1.3 Goals (Business Outcomes)](#13-goals-business-outcomes)
  - [1.4 Glossary](#14-glossary)
- [2. Actors](#2-actors)
  - [2.1 Human Actors](#21-human-actors)
  - [2.2 System Actors](#22-system-actors)
- [3. Operational Concept & Environment](#3-operational-concept--environment)
- [4. Scope](#4-scope)
  - [4.1 In Scope](#41-in-scope)
  - [4.2 Out of Scope](#42-out-of-scope)
- [5. Functional Requirements](#5-functional-requirements)
  - [5.1 License Resolution](#51-license-resolution)
- [6. Non-Functional Requirements](#6-non-functional-requirements)
  - [6.1 Module-Specific NFRs](#61-module-specific-nfrs)
  - [6.2 NFR Exclusions](#62-nfr-exclusions)
- [7. Public Library Interfaces](#7-public-library-interfaces)
  - [7.1 Public API Surface](#71-public-api-surface)
  - [7.2 External Integration Contracts](#72-external-integration-contracts)
- [8. Use Cases](#8-use-cases)
- [9. Acceptance Criteria](#9-acceptance-criteria)
- [10. Dependencies](#10-dependencies)
- [11. Assumptions](#11-assumptions)
- [12. Risks](#12-risks)
- [13. Open Questions](#13-open-questions)
- [14. Traceability](#14-traceability)

<!-- /toc -->

## 1. Overview

### 1.1 Purpose

License Resolver is a read-only CF/Gears system module that answers a single question: *is a specific resource
licensed (granted) to a specific subject right now?* Callers ask `is_licensed(request)` — a single `LicenseCheckRequest`
carrying the subject, the resource, optional evaluation `metadata`, and the tenant context — and receive a yes/no
decision plus structured, non-authoritative diagnostics (debug information about how the decision was reached). It is
the authoritative point-in-time license check used by other modules to gate access.

License Resolver owns no grant data; it delegates the lookup to a pluggable backend selected at runtime, mirroring
authz-resolver and tenant-resolver. This keeps licensing storage, issuance, and billing concerns out of the resolver and
behind a stable contract.

### 1.2 Background / Problem Statement

Multiple CF modules need to check whether a subject may use a licensable resource (a feature, a content item, a
capability) before granting access. The subject is whoever the license is granted to — a tenant, a user, or any future
subject type — identified by subject type + id, exactly as the authz-resolver and quota-enforcement subject models do.
Today no such check exists: CF modules simply do not contain license-resolution logic, so there is no shared contract
for it, no common GTS-typed resource identity, and no fail-closed guarantee to rely on when gating access.

A dedicated resolver consolidates the check behind one contract keyed by subject (subject type + id) with consistent
resource identity (GTS-typed) and consistent deny semantics. Because grant facts live in heterogeneous backends owned by
different vendors, the resolver must delegate the lookup rather than own a store — matching the proven authz-resolver /
tenant-resolver delegation model. (Tenancy enters only as the isolation scope, carried in the request context that the
caller derives from its `SecurityContext`.)

### 1.3 Goals (Business Outcomes)

- Single check contract: one `is_licensed` operation reused by all callers, giving modules a license check they do not
  have today and preventing future per-module divergence.
- Fail-closed guarantee: a license is never granted by default when the backend is unavailable; deny is the safe outcome
  in 100% of unavailable-backend cases.
- Backend independence: licensing backends can be swapped or added via plugin discovery with zero caller code changes.

### 1.4 Glossary

| Term                  | Definition                                                                                                                                                                            |
|-----------------------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `LicenseCheckRequest` | A single object bundling the inputs of one check: subject, resource, optional `metadata`, and tenant context. The contract's growth surface (new inputs are added as request fields). |
| Subject               | The "someone" a license is checked for, identified by subject type + id. Polymorphic — e.g. a tenant, a user, or any future subject type (the licensee is not restricted to tenants). |
| Resource              | The licensable thing, identified by a single GTS instance id (`GtsInstanceId`) — named (§2.3) or UUID-based (§2.4).                                                                   |
| Grant                 | A backend fact that a resource is licensed to a subject.                                                                                                                              |
| Plugin                | A backend implementation discovered via the GTS types registry that answers the check.                                                                                                |
| Metadata              | Caller-supplied JSON on a check, opaque to the resolver core and forwarded to the backend; the extension point for attribute/constraint-based licensing (region, country, …).         |

## 2. Actors

> **Note**: Stakeholder needs are managed at project/task level by steering committee. Document **actors** (users,
> systems) that interact with this module.

### 2.1 Human Actors

This module exposes no direct human interface; all actors are systems.

### 2.2 System Actors

#### Consuming Module

**ID**: `cpt-cf-license-resolver-actor-consuming-module`

- **Role**: Any CF/Gears module that must gate access to a licensable resource. Calls
  `is_licensed(request)` and enforces the returned decision.

#### License Backend Plugin

**ID**: `cpt-cf-license-resolver-actor-backend-plugin`

- **Role**: A vendor-supplied backend implementation that holds grant facts and answers the delegated check. Discovered
  and selected at runtime via the GTS types registry by vendor + priority.

## 3. Operational Concept & Environment

This module introduces no environment constraints beyond project defaults. Runtime, OS, lifecycle, and integration
patterns are inherited from the root PRD.

## 4. Scope

### 4.1 In Scope

- Point-in-time check of whether a single resource is licensed to a single subject.
- Tenant-scoped resolution via the request's tenant context (derived from the caller's `SecurityContext`).
- GTS-typed resource identity (named/well-known and opaque resources), referencing externally-owned resource types.
- Plugin-delegated backend selection via the GTS types registry.

### 4.2 Out of Scope

- **Listing / enumeration of granted resources** — answering "everything licensed to a subject" is a catalog/query
  concern, not a resolver concern; no list operation and no pagination.
- License issuance and revocation — grant lifecycle is owned by issuing/management modules.
- Billing and usage metering — owned by the billing/usage domain.
- Grant storage and management — the resolver owns no grant store; backends do.
- Defining resource types — resource types are owned by their respective modules and only referenced here by GTS type
  path.

## 5. Functional Requirements

> **Testing strategy**: All requirements verified via automated tests (unit, integration, e2e) targeting 90%+ code
> coverage unless otherwise specified.

### 5.1 License Resolution

#### License Check

- [ ] `p1` - **ID**: `cpt-cf-license-resolver-fr-is-licensed-check`

The system **MUST** provide a single check operation `is_licensed(request)` taking a `LicenseCheckRequest` — which
bundles the subject (whom), the resource (what), optional evaluation `metadata`, and the tenant context (the caller
derives it from its `SecurityContext`) — and returning a decision indicating whether the resource is licensed to the
subject at the time of the call, together with structured, non-authoritative **diagnostics** (a string-keyed map of
debug information about how the decision was reached — e.g. which backend answered, matched grant, denial cause).
Diagnostics are advisory only and **MUST NOT** be required for the caller to interpret the boolean outcome. The single
request object is the contract's growth surface — new inputs are added as request fields, not as new parameters or
method-signature changes. (See `cpt-cf-license-resolver-fr-evaluation-metadata` for the `metadata` field.)

- **Rationale**: A single shared check is the module's reason to exist: it provides license-resolution logic that no
  module has today, and gives one consistent contract instead of each module growing its own.
- **Actors**: `cpt-cf-license-resolver-actor-consuming-module`

#### Subject Identity

- [ ] `p1` - **ID**: `cpt-cf-license-resolver-fr-subject-identity`

The system **MUST** identify the subject of a check by a subject type plus an id, consistent with the authz-resolver
subject model. The subject type **MUST** be open-ended — a license may be granted to a tenant, a user, or any future
subject type — and the resolver **MUST NOT** assume the subject is a tenant.

- **Rationale**: Consistent, polymorphic subject identity lets callers reuse existing subject references (as
  authz-resolver and quota-enforcement do), supports licensing any subject kind, and aligns deny semantics across
  resolvers.
- **Actors**: `cpt-cf-license-resolver-actor-consuming-module`

#### Resource Identity

- [ ] `p1` - **ID**: `cpt-cf-license-resolver-fr-resource-identity`

The system **MUST** identify the resource of a check by a single **GTS instance identifier** (`GtsInstanceId`) that
encodes resource type + id via the `~` notation. The instance segment **MUST** be either a well-known name (GTS §2.3,
e.g. `…feature.v1~cf.<vendor>._.somename.v1`) or a UUID (GTS §2.4 combined notation, e.g. `…content.v1~<uuid>`). The
resolver **MUST** reference externally-owned resource types only, never defining them. Which resource types are
licensable, and their validation, are owned by the backend licensing service that answers the check.

- **Rationale**: A GTS instance identifier already encodes type + id and expresses both resource kinds — named
  (`feature~somename`, §2.3) and opaque (`content~<uuid>`, §2.4) — so a single `GtsInstanceId` is the natural identity
  and constrains the contract to a valid GTS identifier, appropriate for a cross-module contract. A discrete type field
  would only help wildcard list filtering, which is out of scope.
- **Actors**: `cpt-cf-license-resolver-actor-consuming-module`

#### Evaluation Metadata

- [ ] `p2` - **ID**: `cpt-cf-license-resolver-fr-evaluation-metadata`

The check **MAY** carry caller-supplied `metadata` (JSON) describing the evaluation context (e.g. region, country,
environment). The resolver core **MUST** treat `metadata` as opaque — it **MUST NOT** interpret or require any key — and
**MUST** forward it unchanged to the selected backend plugin, which **MAY** use it to express attribute/constraint-based
licensing (e.g. "is this resource licensed to this subject in region X?"). `metadata` is the contract's extension point:
future constraint dimensions are expressed as new keys rather than new parameters or signature changes.

- **Rationale**: Whether a resource is licensed can depend on contextual attributes (region, country, environment, …),
  and which attributes matter is backend-specific and expected to grow. Carrying them as an opaque, forwarded bag lets
  backends gate grants on that context, lets new constraint dimensions be added without changing the check signature,
  and keeps the resolver core free of any business rules about what the attributes mean.
- **Actors**: `cpt-cf-license-resolver-actor-consuming-module`, `cpt-cf-license-resolver-actor-backend-plugin`

#### Plugin-Delegated Backend

- [ ] `p1` - **ID**: `cpt-cf-license-resolver-fr-plugin-delegation`

The system **MUST** delegate the grant lookup to a backend plugin discovered via the GTS types registry and selected by
vendor + priority; the resolver **MUST NOT** hold its own grant store.

- **Rationale**: Grant facts live in heterogeneous vendor backends; delegation keeps storage out of the resolver and
  allows backends to be added or swapped without caller changes.
- **Actors**: `cpt-cf-license-resolver-actor-backend-plugin`, `cpt-cf-license-resolver-actor-consuming-module`

#### Read-Only Contract

- [ ] `p1` - **ID**: `cpt-cf-license-resolver-fr-read-only`

The resolver and its public contract **MUST** be read-only: the contract **MUST** expose only the `is_licensed` check
and **MUST NOT** offer any operation to issue, revoke, bill, manage, or list/enumerate grants, and the resolver itself
**MUST NOT** hold a grant store. This constrains the resolver only — backend plugins **MAY** be backed by read-write
systems (e.g. issuance or billing); how a backend sources or maintains grants is outside the resolver's scope.

- **Rationale**: A read-only resolver contract keeps the module simple and authoritative as a check point and prevents
  scope creep into the issuance, billing, and catalog domains, while still allowing backends to be backed by mutable
  systems behind the delegation boundary.
- **Actors**: `cpt-cf-license-resolver-actor-consuming-module`

## 6. Non-Functional Requirements

> **Global baselines**: Project-wide NFRs defined in root PRD and guidelines. Document only module-specific NFRs here.

### 6.1 Module-Specific NFRs

#### Read Latency

- [ ] `p1` - **ID**: `cpt-cf-license-resolver-nfr-read-latency`

The `is_licensed` check **MUST** complete within 50ms at p95, measured at the resolver boundary excluding backend plugin
processing time, under normal load.

- **Threshold**: 50ms p95 at the resolver boundary (excludes plugin compute), normal load.
- **Rationale**: The check sits on the access-granting path of consuming modules, so added latency directly impacts
  every gated request.
- **Architecture Allocation**: See DESIGN.md § NFR Allocation for how this is realized.

#### Fail-Closed on No Plugin

- [ ] `p1` - **ID**: `cpt-cf-license-resolver-nfr-fail-closed`

When no backend plugin is available or the backend is unreachable, the resolver **MUST** fail closed — return a
non-granted decision or an error, and **MUST NOT** grant by default — in 100% of such cases.

- **Threshold**: 0 grant-by-default outcomes across all no-plugin / backend-unavailable conditions.
- **Rationale**: Granting access when the authority cannot be reached would be a license/security violation.
- **Architecture Allocation**: See DESIGN.md § NFR Allocation for how this is realized.

#### Tenant Scoping

- [ ] `p1` - **ID**: `cpt-cf-license-resolver-nfr-tenant-scoping`

Every resolution **MUST** be scoped to the tenant carried by the request context (which the caller derives from its
`SecurityContext`), with 0 cross-tenant grant leaks tolerated. Regardless of subject type, the subject is treated as
bounded within that tenant (the current tenant-bounded model — see §11).

- **Threshold**: 0 cross-tenant resolutions; tenant scope derived solely from the request context.
- **Rationale**: Under the current model every license is tenant-bounded (a user belongs to a tenant), so the resolver
  enforces tenant isolation like other CF modules; a cross-tenant grant would expose another tenant's entitlements.
- **Architecture Allocation**: See DESIGN.md § NFR Allocation for how this is realized.

### 6.2 NFR Exclusions

- Horizontal write-scalability NFRs: N/A — the module performs no writes.

## 7. Public Library Interfaces

### 7.1 Public API Surface

#### License Resolver Client

- [ ] `p1` - **ID**: `cpt-cf-license-resolver-interface-client`

- **Type**: Rust trait (`LicenseResolverClient`)
- **Stability**: stable
- **Description**: The public client contract exposing the **single** method
  `is_licensed(request: LicenseCheckRequest) -> LicenseDecision` — the point-in-time check of whether a resource is
  licensed to a subject. `LicenseCheckRequest` bundles the subject, the resource, opaque forwarded `metadata` (JSON) for
  attribute-based constraints, and the tenant context (caller-derived from `SecurityContext`). There is no listing or
  enumeration method.
- **Breaking Change Policy**: Major version bump required to change the `LicenseCheckRequest`/`LicenseDecision` shape (a
  backward-compatible new optional request field is not breaking), the `GtsInstanceId` resource
  identity, or the decision/error semantics.

### 7.2 External Integration Contracts

#### Backend Plugin Contract

- [ ] `p2` - **ID**: `cpt-cf-license-resolver-contract-plugin`

- **Direction**: required from backend plugin
- **Protocol/Format**: Plugin trait (`LicenseResolverPluginClient`) mirroring the `is_licensed` signature, discovered
  via the GTS types registry plugin spec.
- **Compatibility**: Plugin spec is GTS-versioned; the plugin contract tracks the public client contract's major
  version.

## 8. Use Cases

#### Gate Access to a Licensable Resource

- [ ] `p2` - **ID**: `cpt-cf-license-resolver-usecase-gate-access`

**Actor**: `cpt-cf-license-resolver-actor-consuming-module`

**Preconditions**:

- A `SecurityContext` with a tenant is available.
- The resource type is a registered GTS type referenced by the caller.

**Main Flow**:

1. Consuming module assembles a `LicenseCheckRequest`: the resource's `GtsInstanceId` (named or UUID-based), the
   subject,
   any evaluation `metadata`, and the tenant context derived from its `SecurityContext`.
2. Module calls `is_licensed(request)`.
3. Resolver selects the backend plugin via the GTS registry and delegates the check, forwarding the request unchanged.
4. Resolver returns a decision (granted true/false plus diagnostics).
5. Module enforces the decision.

**Postconditions**:

- The caller has an authoritative, tenant-scoped grant decision; no state was changed.

**Alternative Flows**:

- **No plugin available / backend unreachable**: Resolver fails closed — returns not-granted or an error; the module
  denies access.

## 9. Acceptance Criteria

- [ ] `is_licensed(request)` returns a correct granted/not-granted decision for both named and opaque resources.
- [ ] No listing or enumeration capability exists in the public contract.
- [ ] When no backend plugin is available, the resolver never grants by default.
- [ ] All resolutions are tenant-scoped via the request context (derived from `SecurityContext`) with no cross-tenant
  leakage.
- [ ] Backend selection is performed by GTS registry discovery (vendor + priority) with no resolver-owned grant store.

## 10. Dependencies

| Dependency                            | Description                                                  | Criticality |
|---------------------------------------|--------------------------------------------------------------|-------------|
| GTS types registry (`types-registry`) | Backend plugin discovery (by GTS plugin spec)                | p1          |
| `SecurityContext`                     | Source of the request's tenant context (built by the caller) | p1          |
| Backend license plugin                | Holds grant facts and answers the delegated check            | p1          |

## 11. Assumptions

- Resource types consumed in checks are owned and registered by their respective modules; the resolver only references
  them by GTS type path.
- At least one backend plugin is registered in environments where license checks are expected to grant.
- Subject identity provided by callers follows the authz-resolver subject model.
- **Tenant-bounded grants (current model)**: at this stage, every license is assumed to be bounded within a single
  tenant — regardless of subject type, the subject (e.g. a user) belongs to a tenant — and the resolver enforces tenant
  isolation via the request's tenant context (derived from `SecurityContext`) as other CF modules do. Cross-tenant or
  tenant-independent licensing is not
  modeled yet; lifting this assumption would be a future, explicitly-versioned contract change.

## 12. Risks

| Risk                                           | Impact                                         | Mitigation                                                                                                                            |
|------------------------------------------------|------------------------------------------------|---------------------------------------------------------------------------------------------------------------------------------------|
| No backend plugin registered in an environment | All checks deny, blocking legitimate access    | Fail-closed by design; surface a clear `NoPluginAvailable` signal for operability.                                                    |
| Backend latency degrades the check             | Slows every gated request in consuming modules | Boundary p95 NFR; consuming modules may apply their own timeouts/fallbacks.                                                           |
| Misreferenced resource type path               | Check resolves against the wrong type          | GTS instance identifier is format-validated; the backend licensing service validates the type/licensability and denies unknown types. |

## 13. Open Questions

- Should `LicenseDecision` carry minimal grant metadata (e.g. status/expiry) beyond the boolean? — Owner:
  license-resolver maintainers; target: DESIGN phase (2026-06-30).
- What diagnostics keys/conventions do consuming modules need (denial cause, matched grant, backend id, etc.)? — Owner:
  license-resolver maintainers; target: DESIGN phase (2026-06-30).

## 14. Traceability

Links to related specification artifacts.

- **Design**: [DESIGN.md](./DESIGN.md)
- **ADRs**: [ADR/](./ADR/)
- **Features**: [features/](./features/)
