---
id: ADR-062
title: Raft Voter Cap and Automatic Learner Promotion
status: proposed
features: []
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** Raft consensus requires a majority of voters to agree on every write. With 3 voters, the cluster tolerates 1 failure. With 5 voters, it tolerates 2. However, adding more voters beyond 5 increases write latency (more round-trips to achieve majority) without meaningful fault tolerance improvement. A 7-voter cluster tolerates 3 failures but pays latency for every write — on Raspberry Pi 5 hardware with limited network bandwidth, this cost is significant.

PiCloud clusters can grow to dozens of nodes. The platform needs to automatically manage which nodes vote and which replicate as learners, without operator intervention.

**Decision:** Cap the voter set at 5 nodes. Nodes beyond 5 are added as Raft learners — they replicate the event log and serve reads but do not participate in leader election or write quorum. The platform automatically promotes learners to voters when a voter leaves or fails.

**Configuration rules:**
- Clusters of 1–5 nodes: all nodes are voters
- Clusters of 6+ nodes: 5 voters, remaining nodes are learners
- When a voter departs, the longest-serving learner is promoted
- Promotion and demotion use the openraft `change_membership` joint consensus protocol — zero-downtime reconfiguration
- Only one membership change at a time — changes are serialized to prevent split-brain

**Automatic vs manual:** The platform manages voter configuration automatically. Operators can override via `picloud cluster promote/demote` for exceptional circumstances. Manual overrides are logged as high-severity events.

**Rationale:**
- 5 voters tolerates 2 simultaneous failures — sufficient for a home/small-office cluster
- Capping at 5 bounds write latency regardless of cluster size — critical for Pi 5 hardware where network is the bottleneck
- Learners still replicate the log and serve reads — they contribute capacity without adding consensus overhead
- Joint consensus protocol ensures writes continue during voter changes — no maintenance window needed

**Rejected alternatives:**
- **All nodes vote regardless of cluster size** — write latency grows linearly with cluster size; a 10-node cluster would have unacceptable Raft round-trip times on Pi 5 hardware
- **Fixed voter set (no automatic promotion)** — voter failure without automatic replacement reduces fault tolerance until an operator intervenes manually
- **Dynamic voter cap based on cluster size** — adds complexity without clear benefit; 5 is the sweet spot for the target hardware

**Consequences:**
- Learner nodes cannot become leader — if all 5 voters fail simultaneously, the cluster is unavailable until at least one voter recovers
- The platform must track voter/learner status in the RDF graph for scheduling and operational visibility
- Manual promotion creates a risk of exceeding the voter cap — the platform should warn but allow the override