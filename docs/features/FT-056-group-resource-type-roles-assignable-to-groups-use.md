---
id: FT-056
title: group resource type — roles assignable to groups, users inherit
phase: 3
status: planned
depends-on: []
adrs:
- ADR-037
- ADR-009
tests:
- TC-268
- TC-325
domains: []
domains-acknowledged: {}
---

## Description

A `group` is an IAM resource that holds a set of roles (ADR-037). Users in a group inherit all roles assigned to it. Groups are the level of indirection that makes role management scalable — assign roles to the group once, and all members inherit them.

### Resource syntax

```bicep
group 'backend-developers' = {
  description: 'Backend engineering team'
  roles: ['product-developer', 'log-viewer']
  tags: { 'team': 'backend' }
}
```

### Role inheritance

- A user who is a member of a group inherits all roles assigned to that group
- A user can be in multiple groups — role sets are additive
- When a user's group membership changes, their next issued token includes the updated role set

### Group membership

Group membership is **not managed manually**. It is managed exclusively by SPARQL CONSTRUCT inference rules (FT-057, FT-058). This ensures membership is declarative, auditable, and automatically responsive to tag changes.

### RDF projection

```turtle
<https://picloud.local/groups/backend-developers>
    a picloud:Group ;
    picloud:hasRole <https://picloud.local/platform/roles/product-developer> ;
    picloud:hasRole <https://picloud.local/platform/roles/log-viewer> ;
    picloud:hasMember <https://picloud.local/platform/identities/alice> ;
    picloud:hasMember <https://picloud.local/platform/identities/bob> .
```

### Events

- `GroupCreated` — group resource declared and ready
- `GroupMembershipChanged` — member added or removed (emitted by inference engine)
- `GroupDeleted` — group removed

### Constraints

- Circular group membership (group A contains group B contains group A) is detected and rejected
- Token issuance in `picloud-iam` reads group memberships from the RDF graph before assembling claims
