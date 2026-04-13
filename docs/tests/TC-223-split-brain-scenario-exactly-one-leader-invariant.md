---
id: TC-223
title: Split brain scenario — exactly one leader invariant
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
runner-args: "tc223_split_brain_one_leader_invariant"
---

⟦Σ:Types⟧{
  Node≜IRI
  Role≜Leader|Follower|Learner
  ClusterState≜⟨nodes:Node+, roles:Node→Role⟩
  Partition≜⟨left:Node+, right:Node+⟩
}

⟦Γ:Invariants⟧{
  ∀s:ClusterState: |{n∈s.nodes | s.roles(n)=Leader}| = 1
  ∀p:Partition: |{n∈p.left | roles(n)=Leader}| + |{n∈p.right | roles(n)=Leader}| ≤ 1
}

⟦Λ:Scenario⟧{
  given≜cluster_init(nodes:5) ∧ ∃leader∈nodes: roles(leader)=Leader
  when≜network_partition(nodes, split:[2,3])
  then≜within(30s): |{n∈all_nodes | roles(n)=Leader}| = 1
       ∧ minority_partition.accepts_writes = false
}

⟦Ε⟧⟨δ≜0.90;φ≜75;τ≜◊?⟩