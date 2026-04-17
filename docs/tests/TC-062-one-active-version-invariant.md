---
id: TC-062
title: one_active_version_invariant
type: invariant
status: passing
validates:
  features:
  - FT-008
  adrs:
  - ADR-021
phase: 1
runner: scripts/run-tc.sh
runner-args: "one-active-version-invariant"
last-run: 2026-04-17T13:58:22.735189315+00:00
last-run-duration: 0.0s
---

⟦Σ:Types⟧{
  Product≜IRI
  Version≜IRI
  DeploymentState≜⟨product:Product, activeVersions:Version*⟩
}

⟦Γ:Invariants⟧{
  ∀s:DeploymentState: |s.activeVersions| = 1
}

⟦Λ:Scenario⟧{
  given≜resource_apply(product:"photo-app", version:"v2")
  when≜event(ProductVersionActivated)
  then≜sparql("SELECT DISTINCT ?v WHERE { <product-iri> picloud:activeVersion ?v }").rows = 1
}

⟦Ε⟧⟨δ≜0.90;φ≜80;τ≜◊⁺⟩