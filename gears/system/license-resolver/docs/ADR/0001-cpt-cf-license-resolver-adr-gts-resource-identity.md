---
status: accepted
date: 2026-06-08
---

# ADR-0001: Resource Identity as a Single GTS Instance ID

<!-- toc -->

- [Context and Problem Statement](#context-and-problem-statement)
- [Decision Drivers](#decision-drivers)
- [Considered Options](#considered-options)
- [Decision Outcome](#decision-outcome)
  - [Consequences](#consequences)
  - [Confirmation](#confirmation)
- [Pros and Cons of the Options](#pros-and-cons-of-the-options)
  - [(a) Single `GtsInstanceId`](#a-single-gtsinstanceid)
  - [(b) Split `ResourceRef { resource_type, resource_id }`](#b-split-resourceref--resource_type-resource_id)
  - [(c) Free-form / opaque string](#c-free-form--opaque-string)
- [More Information](#more-information)
- [Traceability](#traceability)

<!-- /toc -->

**ID**: `cpt-cf-license-resolver-adr-gts-resource-identity`

## Context and Problem Statement

License resolver answers "is THIS resource granted to THIS subject?", so every check must carry a stable, unambiguous
identity for the resource being licensed. A licensable resource comes in two kinds: a **named** resource — a stable,
human-meaningful instance such as a specific feature — and an **opaque** resource — a runtime instance addressed by a
UUID, for example a content item. Resources are heterogeneous and externally owned (a feature type is owned by the
feature module, a content type by the content module, etc.). The contract must represent "resource type + id" so that
both kinds are expressible. The Global Type System (GTS) already provides exactly this shape: an **instance identifier**
joins a type chain and an instance segment with `~`, and supports both well-known/named instances (§2.3) and anonymous
instances addressed by UUID (§2.4, combined notation). How should the cross-module contract represent resource identity?

## Decision Drivers

* A GTS **instance identifier** already combines a type chain and an instance segment — i.e. it is exactly "resource
  type + id" expressed as a single value — and both resource kinds (named, opaque) map onto it directly.
* Both forms must be expressible: named/well-known (§2.3) and opaque/anonymous-by-UUID (§2.4 combined notation).
* This is a contract consumed by other modules, so the identity should be **constrained to a valid GTS instance
  identifier**, not an arbitrary string.
* The `resource_type` must stay referenceable and derivable from the identity, so the backend can validate it and the
  resolver can reference it.
* Which resources can be licensed — the catalog of licensable resource types and the rules for validating them — is
  determined by the backend licensing service (a license enforcement/management service) that implements the resolver
  plugin, not by the resolver core, which only references resource types by GTS type path and never defines or validates
  them.
* There is no listing/enumeration in scope, so a separate discrete `resource_type` field (useful for wildcard list
  filtering) is not needed.

## Considered Options

* (a) A single `GtsInstanceId` resource identity (well-known/named §2.3, or UUID-based anonymous §2.4)
* (b) A split `ResourceRef { resource_type: GtsTypeId, resource_id }` carrying type and id as separate fields
* (c) A free-form / opaque string with no GTS structure

## Decision Outcome

Chosen option: **(a) a single `GtsInstanceId`** as the resource identity. The check contract identifies a resource by
one GTS instance identifier whose instance segment is either a well-known name (§2.3, e.g.
`…feature.v1~cf.<vendor>._.somename.v1`) or a UUID (§2.4 combined notation, e.g. `…content.v1~<uuid>`). This covers both
resource kinds — named and opaque — in a single value, and constrains the contract to a valid GTS instance identifier
rather than an arbitrary string. The resolver core does not validate the type or own the licensable-type catalog: the
full instance id is propagated to the selected backend plugin (the licensing service), which owns those, validates the
resource, and answers the check.

Option (b) was rejected: once listing/enumeration is out of scope, a discrete `resource_type` field carries no advantage
for a *concrete-instance* check and just splits one identity across two fields (the type is derivable from the instance
id anyway). Note `authz-resolver` splits because its `resource_type` is a *set/wildcard expression* for permissions, not
a concrete instance — a different problem. Option (c) was rejected because dropping GTS structure loses the typed
identity (nothing can validate the type) and lets callers pass ambiguous identifiers across a shared contract.

### Consequences

* The SDK identifies a resource by a `GtsInstanceId` (named §2.3 or UUID-based §2.4). A helper may build one from a
  separate type + id and parse one back.
* The resolver core never declares, owns, or validates resource types; it forwards the resource identity
  (`GtsInstanceId`) to the backend unchanged.
* The backend licensing service (the plugin) determines which resource types are licensable and validates the resource;
  the full `GtsInstanceId` is propagated to it unchanged and it answers the check.
* Telemetry is dimensioned by the **derived type** (bounded cardinality); the full instance id is never used as a label.
* For opaque resources, the `type~<uuid>` form is the GTS §2.4 *combined* notation (the spec's optional form, not the
  storage-canonical split); this is an accepted trade-off for a single-value contract identifier.

### Confirmation

Confirmed by design and code review of the SDK contract (the resource identity is a `GtsInstanceId`; a helper
builds/parses it), plus unit tests asserting that named (§2.3) and UUID (§2.4) forms both round-trip and that the full
instance id is forwarded to the backend unchanged. (Resource-type licensability and validation are the backend's
concern, tested there — not in the resolver core.)

## Pros and Cons of the Options

### (a) Single `GtsInstanceId`

One GTS instance identifier carries type and id via `~`.

* Good, because it is the literal GTS expression of "resource type + id" and represents both named and opaque resources
  in one value.
* Good, because it constrains the contract to a valid GTS instance identifier — appropriate for a cross-module contract.
* Good, because both named (§2.3) and UUID (§2.4) forms are expressible in one type, while the `resource_type` stays
  derivable from it and its licensability and validation stay with the backend licensing service.
* Neutral, because callers holding a separate type + id compose them into the instance id (a helper covers this).
* Bad, because for opaque resources it uses the §2.4 *combined* notation rather than the storage-canonical
  `{type, UUID}` split.

### (b) Split `ResourceRef { resource_type, resource_id }`

Type and id as separate fields.

* Good, because the type is a discrete field, trivially validatable and usable as a telemetry dimension.
* Good, because for opaque/UUID resources it is GTS's canonical shape (§2.4 keeps type and UUID separate), so it carries
  no combined-notation trade-off.
* Good, because it mirrors `authz-resolver`'s `Resource { resource_type, id }`, so it is familiar to callers and easy to
  reuse.
* Bad, because a concrete-instance check needs one identity, not two fields; the discrete type adds value mainly for
  wildcard *list* filtering, which is out of scope.
* Bad, because it diverges from the natural single-value `type~id` GTS instance-identifier notation.

### (c) Free-form / opaque string

An unstructured string with no GTS typing.

* Good, because it imposes no schema.
* Bad, because it loses GTS validation entirely, so the registry cannot validate the type and externally-owned types are
  no longer referenced in a structured way.
* Bad, because ambiguity across a shared contract undermines correct routing and auditing.

## More Information

GTS guidelines `guidelines/GTS.md` §2.3 (well-known instances) and §2.4 (anonymous instances, with optional combined
`type~uuid` notation). Both a named resource and an opaque (UUID-addressed) resource are single GTS instance
identifiers. The resolver references externally-owned resource types only; any types it owns (e.g. its plugin spec) live
under `gts.cf.bss.licensing.*`.

## Traceability

- **PRD**: [PRD.md](../PRD.md)
- **DESIGN**: [DESIGN.md](../DESIGN.md)

This decision directly addresses the following requirements or design elements:

* `cpt-cf-license-resolver-fr-resource-identity` — defines resource identity as a single `GtsInstanceId` (named or
  UUID-based), which this ADR records as canonical.
* `cpt-cf-license-resolver-fr-subject-identity` — the subject + resource pair of a check relies on this resource shape.
* `cpt-cf-license-resolver-principle-gts-typed-resource-identity` — this ADR is the rationale for that DESIGN principle.
* `cpt-cf-license-resolver-constraint-gts-via-types-registry` — the type derived from the instance id is what gets
  validated against the registry.
