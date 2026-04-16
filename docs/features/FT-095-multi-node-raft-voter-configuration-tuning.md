---
id: FT-095
title: Multi-node Raft voter configuration tuning
phase: 4
status: complete
depends-on: []
adrs:
- ADR-062
- ADR-002
tests:
- TC-291
- TC-348
domains: []
domains-acknowledged: {}
---

## Description

Configures Raft voter and learner roles based on cluster size. In a small cluster (≤ 5 nodes), every node is a voter. In larger clusters, voter count is capped and excess nodes become learners that replicate the log but do not participate in leader election.

### Voter configuration rules

| Cluster size | Voters | Learners | Rationale |
|---|---|---|---|
| 1 node | 1 | 0 | Single-node cluster, no quorum needed |
| 2 nodes | 2 | 0 | Both vote — tolerates 0 failures but enables replication |
| 3 nodes | 3 | 0 | Classic quorum — tolerates 1 failure |
| 4–5 nodes | All | 0 | All nodes vote — tolerates ⌊(N-1)/2⌋ failures |
| 6+ nodes | 5 | N − 5 | Cap voters at 5 — adding more voters increases Raft latency without meaningful fault tolerance gain |

### Promotion and demotion

- When a node joins a cluster at 6+ nodes, it is added as a learner
- When a voter leaves or fails, the platform promotes the longest-serving learner to voter
- Promotion and demotion are Raft membership changes — they go through the standard `openraft` `change_membership` API
- Membership changes are serialized — only one promotion/demotion at a time to avoid split-brain risk

### Zero-downtime reconfiguration

- Voter changes use the Raft joint consensus protocol — the cluster continues accepting writes throughout the membership transition
- Client writes submitted during a voter change complete successfully within the normal Raft timeout
- No manual intervention is required — the platform manages voter configuration automatically on node join and leave events

### Events

- `RaftVoterPromoted` — a learner was promoted to voter, includes node IRI and reason
- `RaftVoterDemoted` — a voter was demoted to learner, includes node IRI and reason
- `RaftConfigurationChanged` — emitted after any membership change, includes the full voter/learner set

### RDF projection

Voter/learner status is projected into the cluster graph:
```turtle
<https://picloud.local/nodes/pi-node-01>
    picloud:raftRole "voter" ;
    picloud:raftVoterSince "2025-07-01T12:00:00Z"^^xsd:dateTime .

<https://picloud.local/nodes/pi-node-06>
    picloud:raftRole "learner" ;
    picloud:raftLearnerSince "2025-07-15T08:30:00Z"^^xsd:dateTime .
```

### CLI

- `picloud cluster voters` — lists current voter/learner configuration
- `picloud cluster promote <node>` — manually promote a learner to voter (operator override)
- `picloud cluster demote <node>` — manually demote a voter to learner (operator override)
