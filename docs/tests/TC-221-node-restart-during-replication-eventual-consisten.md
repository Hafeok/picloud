---
id: TC-221
title: Node restart during replication — eventual consistency restored
type: chaos
status: passing
validates:
  features: []
  adrs: []
phase: 1
runner: scripts/run-tc.sh
runner-args: "node-restart-during-replication--eventual-consistency-restored"
last-run: 2026-04-13T19:41:49.618598309+00:00
---

⟦Σ:Types⟧{
  Node≜IRI
  EventLog≜⟨entries:Event*⟩
  ReplicationState≜⟨node:Node, log:EventLog, committed_index:u64⟩
}

⟦Γ:Invariants⟧{
  ∀n:Node: after(restart(n), 30s) → n.committed_index = leader.committed_index
}

⟦Λ:Scenario⟧{
  given≜cluster_init(nodes:3) ∧ append_events(count:100)
  when≜restart(follower) during replication
  then≜within(30s): follower.committed_index = leader.committed_index
       ∧ sparql_result(follower) = sparql_result(leader)
}

⟦Ε⟧⟨δ≜0.85;φ≜65;τ≜◊?⟩