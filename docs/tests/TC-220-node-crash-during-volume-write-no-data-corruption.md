---
id: TC-220
title: Node crash during volume write — no data corruption
type: chaos
status: failing
validates:
  features: []
  adrs: []
phase: 1
runner: picloud-test
runner-args: "node-crash-during-volume-write--no-data-corruption"
---

⟦Σ:Types⟧{
  Node≜IRI
  Volume≜IRI
  WriteOp≜⟨volume:Volume, offset:u64, data:Bytes⟩
}

⟦Γ:Invariants⟧{
  ∀v:Volume: crash_during(write(v)) → checksum(v.committed_blocks) = checksum(v.replicated_blocks)
}

⟦Λ:Scenario⟧{
  given≜volume_create(size:"1Gi", replicas:2) ∧ write_stream(volume, rate:"10MB/s")
  when≜sigkill(node_hosting(volume))
  then≜after(node_restart): fsck(volume).errors = 0
       ∧ read(volume, committed_offset).data = expected_data
}

⟦Ε⟧⟨δ≜0.80;φ≜60;τ≜◊?⟩