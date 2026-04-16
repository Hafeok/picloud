---
id: ADR-037
title: Groups as IAM Resource with SPARQL CONSTRUCT Membership Rules
status: accepted
features:
- FT-009
- FT-056
- FT-058
supersedes: []
superseded-by: []
domains: []
scope: domain
content-hash: sha256:efdd5ff9213e0bc29dfd2580dc3a32b925eb93f222b3c097c1390613ada2a4f7
---

**Status:** Accepted

**Context:** Managing individual user role assignments does not scale. When a new team member joins, an operator should not need to manually assign every role. Groups provide a level of indirection — assign roles to a group, users inherit them. Membership should be automatic where possible, driven by tags and inference rules rather than manual assignment.

**Decision:** `Group` is a new IAM resource. A group has roles assigned to it. Users in a group inherit all roles assigned to that group. Group membership is managed via SPARQL CONSTRUCT rules that evaluate on every relevant event and on a 10-minute reconciliation schedule.

**Group resource:**
```bicep
group 'backend-developers' = {
  description: 'Backend engineering team'
  roles: ['product-developer', 'log-viewer']
  tags: {
    'team': 'backend'
  }
}
```

**Membership rule resource:**
```bicep
inference-rule 'backend-group-membership' = {
  description: 'Add users tagged team:backend to backend-developers group'
  scope: 'platform'
  trigger: 'event'             // run on TagAdded, TagRemoved, IdentityCreated
  reconciliation: true         // also run every 10 minutes
  construct: '''
    CONSTRUCT {
      <https://picloud.local/groups/backend-developers>
          picloud:hasMember ?user .
    }
    WHERE {
      ?user a picloud:HumanIdentity ;
            picloud:tag [
                picloud:tagKey "team" ;
                picloud:tagValue "backend"
            ] .
    }
  '''
}
```

**How membership works:**
1. A `TagAdded` event fires (user gets tag `team:backend`)
2. The inference engine evaluates all rules triggered by `TagAdded`
3. CONSTRUCT query runs — produces `picloud:hasMember` triples
4. New triples are written to the platform graph
5. A `GroupMembershipChanged` event is emitted
6. IAM token issuance reads group memberships from the graph — next token the user receives includes the inherited roles

**Removal:** When a tag is removed, the CONSTRUCT query no longer produces the membership triple. The inference engine detects the retraction and emits `GroupMembershipChanged`. The triple is removed from the graph.

**RDF representation:**
```turtle
<https://picloud.local/groups/backend-developers>
    a picloud:Group ;
    picloud:hasRole <https://picloud.local/platform/roles/product-developer> ;
    picloud:hasRole <https://picloud.local/platform/roles/log-viewer> ;
    picloud:hasMember <https://picloud.local/platform/identities/alice> ;
    picloud:hasMember <https://picloud.local/platform/identities/bob> .
```

**Rationale:**
- Groups decouple role assignment from individual users — one group change affects all members
- SPARQL CONSTRUCT rules make membership declarative and auditable — the rule is a resource in the graph
- Event-driven evaluation gives immediate effect — a tag change cascades to group membership to token permissions within one event cycle
- 10-minute reconciliation catches any drift between events
- The graph is always the source of truth — no separate membership database

**Rejected alternatives:**
- **Manual group membership only** — does not scale; every new user requires manual role assignment across all relevant groups.
- **Attribute-based access control (ABAC) without groups** — evaluating policies at every access check is expensive; groups materialise permissions once and serve them at read time.

**Consequences:**
- Token issuance in `picloud-iam` must read group memberships from the RDF graph before assembling claims
- `GroupMembershipChanged` must be a platform event so downstream systems can react
- A user can be in multiple groups — role sets are additive
- Circular group membership (group A contains group B contains group A) must be detected and rejected