---
id: FT-009
title: Inference, Metrics & Alerts
phase: 2
status: complete
depends-on:
- FT-008
adrs:
- ADR-036
- ADR-037
- ADR-038
- ADR-039
- ADR-040
- ADR-041
- ADR-043
- ADR-044
- ADR-045
- ADR-046
- ADR-054
- ADR-055
- ADR-056
tests:
- TC-107
- TC-108
- TC-109
- TC-110
- TC-111
- TC-112
- TC-113
- TC-114
- TC-115
- TC-116
- TC-117
- TC-118
- TC-119
- TC-120
- TC-121
- TC-122
- TC-123
- TC-124
- TC-125
- TC-126
- TC-127
- TC-128
- TC-129
- TC-130
- TC-131
- TC-132
- TC-136
- TC-137
- TC-138
- TC-139
- TC-140
- TC-141
- TC-142
- TC-143
- TC-144
- TC-145
- TC-146
- TC-147
- TC-148
- TC-149
- TC-150
- TC-151
- TC-152
- TC-185
- TC-186
- TC-187
- TC-188
- TC-189
- TC-190
- TC-191
- TC-192
- TC-193
- TC-194
- TC-195
- TC-213
- TC-214
- TC-215
- TC-216
- TC-217
domains:
- observability
- data-model
- products
domains-acknowledged: {}
---

### Tagging

Every platform resource supports an arbitrary set of `key:value` tags. Tags are declared in resource definition files and manageable via CLI. Tag changes emit `TagAdded` and `TagRemoved` events, which are projected into the RDF graph and immediately trigger inference rule evaluation.

```bicep
container 'api-server' = {
  product: 'photo-app'
  image: 'photo-api:1.0.0'
  tags: {
    'team': 'backend'
    'environment': 'production'
  }
}
```

### Groups

A `group` is an IAM resource that holds a set of roles. Users in a group inherit all roles assigned to it. Group membership is managed automatically by SPARQL CONSTRUCT inference rules — never by manual assignment.

```bicep
group 'backend-developers' = {
  roles: ['product-developer', 'log-viewer']
  tags: { 'team': 'backend' }
}

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
            picloud:tag [ picloud:tagKey "team" ; picloud:tagValue "backend" ] .
    }
  '''
}
```

When a user receives the tag `team:backend`, the rule fires within one event cycle and the user is added to the group. Their next token includes the inherited roles.

### Inference rules

SPARQL CONSTRUCT queries are a first-class resource type. Rules run on matching events and on a 10-minute reconciliation schedule. Produced triples are written to the appropriate named graph. New or retracted triples emit events.

Two inference layers work together:
- **RDFS/OWL inference** (Oxigraph built-in) — structural facts from ontology axioms. Subclass hierarchies, transitive properties, equivalences. Always live, no trigger needed.
- **SPARQL CONSTRUCT rules** — operational rules. Group membership, alert conditions, derived state. Event-driven with reconciliation safety net.

### Hardware metrics

The platform ships a built-in metrics agent in the `picloud-server` binary. Every node samples hardware metrics every 15 seconds and emits `MetricRecorded` events:

- CPU usage (%) — per core and aggregate
- Memory used / total (MB)
- Disk used / total / read rate / write rate
- CPU temperature (°C)
- Network bytes in/out

The RDF projector writes the latest values as triples on each node's IRI, overwriting previous values. Historical values are queryable via event log replay.

Product workloads emit their own domain metrics (request counts, error rates, latency) as events to the product event bus. The platform does not collect these — workloads emit them, the SDK provides helpers.

### Alerts

Alerts are produced by SPARQL CONSTRUCT rules that assert `picloud:Alert` triples. When a new alert triple is materialised, the platform emits `AlertFired`. When the condition clears and the triple is retracted, `AlertResolved` is emitted. No built-in notification targets — subscribers build notification products on top.

**Built-in platform alert rules:**

| Condition | Threshold | Severity |
|---|---|---|
| CPU temperature | > 80°C | critical |
| CPU temperature | > 70°C | warning |
| Memory usage | > 90% | critical |
| Memory usage | > 80% | warning |
| Disk usage | > 90% | critical |
| Node unreachable | Raft heartbeat missed | critical |
| Workload failed | `ResourceStatus = Failed` | critical |

Active alerts are always queryable from the cluster graph:
```bash
picloud graph query --sparql "SELECT * WHERE { ?a a picloud:Alert . }"
```

---