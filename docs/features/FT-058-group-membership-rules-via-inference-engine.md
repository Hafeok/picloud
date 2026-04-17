---
id: FT-058
title: Group membership rules via inference engine
phase: 3
status: planned
depends-on: []
adrs:
- ADR-037
- ADR-038
tests:
- TC-228
domains: []
domains-acknowledged: {}
---

## Description

Group membership is managed automatically by SPARQL CONSTRUCT inference rules (ADR-037). When a user receives a tag that matches a group membership rule, the rule fires and the user is added to the group. When the tag is removed, the user is removed.

### Membership rule example

```bicep
inference-rule 'backend-group-membership' = {
  scope: 'platform'
  trigger: 'event'
  trigger-events: ['TagAdded', 'TagRemoved', 'IdentityCreated']
  reconciliation: true
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

### How it works

1. User `alice` receives tag `team:backend` → `TagAdded` event
2. Inference engine evaluates all rules triggered by `TagAdded`
3. CONSTRUCT query matches `alice` → produces `picloud:hasMember` triple
4. New triple written to platform graph → `GroupMembershipChanged` event
5. Next token issued for `alice` includes inherited roles from `backend-developers`

### Removal

When tag `team:backend` is removed from `alice`:
1. `TagRemoved` event fires
2. CONSTRUCT query no longer matches `alice`
3. Inference engine detects the retraction — removes the `picloud:hasMember` triple
4. `GroupMembershipChanged` event emitted
5. Next token excludes the group's roles

### Safety net

The 10-minute reconciliation schedule (FT-057) re-evaluates all membership rules, catching any drift from missed events.

### Latency

Tag change → group membership change → token update happens within one event cycle. The user's next authentication picks up the new roles immediately.
