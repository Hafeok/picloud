---
id: TC-219
title: Raft leader failover under network partition
type: chaos
status: failing
validates:
  features: []
  adrs: []
phase: 1
runner: picloud-test
runner-args: "raft-leader-failover-under-network-partition"
---

⟦Σ:Types⟧{
  Node≜IRI
  Role≜Leader|Follower|Learner
  ClusterState≜⟨nodes:Node+, roles:Node→Role⟩
}

⟦Γ:Invariants⟧{
  ∀s:ClusterState: |{n∈s.nodes | s.roles(n)=Leader}| ≤ 1
}

⟦Λ:Scenario⟧{
  given≜cluster_init(nodes:3) ∧ ∃leader∈nodes: roles(leader)=Leader
  when≜network_partition(leader, majority)
  then≜within(15s): ∃n∈majority: roles(n)=Leader
       ∧ commands_accepted(n)=true
}

⟦Ε⟧⟨δ≜0.85;φ≜70;τ≜◊?⟩