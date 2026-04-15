---
id: TC-254
title: OTel environment variables present in workload container
type: scenario
status: passing
runner: cargo-test
runner-args: "tc254_otel_environment_variables_present_in_workload_container"
validates:
  features: [FT-041]
  adrs: [ADR-045]
phase: 2
last-run: 2026-04-15T10:53:27.930179986+00:00
last-run-duration: 3.6s
---

## Description

Verifies that all three OTel environment variables — `OTEL_SERVICE_NAME`,
`OTEL_EXPORTER_OTLP_ENDPOINT`, and `OTEL_RESOURCE_ATTRIBUTES` — are injected
into every workload at startup.

The test schedules binary workloads that check each env var individually:
- `OTEL_SERVICE_NAME` matches the last segment of the workload IRI
- `OTEL_EXPORTER_OTLP_ENDPOINT` is a non-empty URL ending in `/otel`
- `OTEL_RESOURCE_ATTRIBUTES` contains `picloud.product` and `picloud.workload_iri`

Also verifies container workloads are scheduled through the OTel injection path,
and that OTel vars coexist with user-supplied environment variables.