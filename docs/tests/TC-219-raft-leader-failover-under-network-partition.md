---
id: TC-219
title: Raft leader failover under network partition
type: chaos
status: passing
validates:
  features:
  - FT-002
  adrs:
  - ADR-002
  - ADR-004
phase: 1
runner: cargo-test
runner-args: "tc219_raft_leader_failover_event_consistency"
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