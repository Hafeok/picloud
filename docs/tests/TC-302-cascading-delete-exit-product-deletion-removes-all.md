---
id: TC-302
title: Cascading delete exit — product deletion removes all child resources
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc302_cascading_delete_exit_product_deletion_removes_all_child_resources"
validates:
  features: [FT-031]
  adrs: []
phase: 2
last-run: 2026-04-14T08:03:06.890546797+00:00
---

## Description

Exit criterion: after product deletion, zero resources reference the deleted
product anywhere in the graph (default graph, named graph, or by IRI path).
Also verifies that sibling products and their resources are unaffected by
the cascading deletion.