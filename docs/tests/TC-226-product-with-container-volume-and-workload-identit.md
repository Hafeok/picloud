---
id: TC-226
title: Product with container, volume, and workload identity deploys end-to-end
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc226_product_with_container_volume_and_workload_identity_deploys_e2e"
validates:
  features: [FT-024]
  adrs: []
phase: 2
last-run: 2026-04-17T15:53:16.538537841+00:00
last-run-duration: 1.3s
---

## Description

End-to-end exit-criteria test for FT-024 (Product resource type with versioning).

Validates the full deployment lifecycle of a versioned Product that includes a Container with workload identity and a Volume:

1. **Apply** — Submit a versioned Product (v1.0.0) with a Container (including `identity` for workload identity) and a Volume via `POST /api/apply`. All three resources are declared atomically.
2. **Events** — Verify that the correct platform events are emitted: one `ProductDeployed` event for the product and two `ResourceDeclared` events for the container and volume.
3. **RDF Projection** — Verify the Oxigraph projector creates queryable triples for the product (including version), the container (including identity), and the volume.
4. **Product Status** — Query `GET /products/:name` and verify the response includes all child resources (Volume and Container).
5. **Version Upgrade** — Apply the same product at v2.0.0 with an updated container image. Verify new events are appended and the graph reflects the updated version, confirming versioning is a first-class concept.