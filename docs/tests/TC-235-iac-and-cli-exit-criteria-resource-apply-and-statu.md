---
id: TC-235
title: IaC and CLI exit criteria — resource apply and status round-trip
type: exit-criteria
status: passing
validates:
  features:
  - FT-007
  adrs:
  - ADR-007
  - ADR-015
  - ADR-029
  - ADR-042
  - ADR-049
  - ADR-050
phase: 1
runner: scripts/run-tc.sh
runner-args: "iac-cli-exit-criteria"
last-run: 2026-04-17T19:13:00.299404881+00:00
last-run-duration: 0.0s
---

⟦Λ:ExitCriteria⟧{
  apply_creates: picloud_resource_apply(file).exit_code = 0 ∧ resource_exists = true
  status_shows: picloud_resource_status(iri).state ∈ {Ready, Pending, Failed}
  idempotent_apply: picloud_resource_apply(file) twice → no duplicate resources
  cascading_delete: remove_from_file(resource) ∧ apply → resource_deleted = true
  correlation_tracking: apply.correlation_id appears in SSE event stream
}

After writing a `.picloud` file declaring a Product with a container and a volume, verify that (1) `picloud resource apply` creates all resources, (2) `picloud resource status` shows correct state for each, (3) re-applying the same file is idempotent, (4) removing a resource from the file and reapplying cascades deletion, and (5) the CLI receives terminal events via SSE filtered by correlation ID.