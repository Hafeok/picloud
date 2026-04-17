---
id: FT-082
title: Aggregate-scoped replay (single and batch up to 1000)
phase: 3
status: planned
depends-on: []
adrs:
- ADR-035
- ADR-032
tests:
- TC-281
- TC-338
domains: []
domains-acknowledged: {}
---

## Description

Extends replay (FT-081) with aggregate-scoped replay — replay one specific aggregate or a batch of up to 1000 aggregates (ADR-035). This covers the common operational case of targeted repair without requiring a full store replay.

### Single aggregate replay

```bash
picloud resource replay photo-app \
  --aggregate Photo \
  --id 123e4567-e89b-12d3-a456-426614174000 \
  --from "2025-06-01T00:00:00Z"
```

### Batch aggregate replay

```bash
picloud resource replay photo-app \
  --aggregate Photo \
  --ids-file ./photo-ids.txt \
  --from "2025-06-01T00:00:00Z"
```

The `--ids-file` contains one aggregate ID per line, up to 1000 IDs.

### HTTP API

```
POST /products/photo-app/event-store/photos/replay
{
  "from": "2025-06-01T00:00:00Z",
  "aggregate_type": "Photo",
  "aggregate_ids": ["uuid-1", "uuid-2"]
}
```

Returns a `replay_id`. Subscribe to the event stream filtered by `replay_id` for progress and completion.

### Scoped shadow projection

Aggregate replay builds a shadow projection only for the targeted aggregates. Triples for other aggregates in the Product's graph are untouched. The swap replaces only the affected triples.

### Concurrency limit

One active replay per Product at a time. Concurrent replay requests are queued.

### Batch size limit

Maximum 1000 aggregates per batch. Larger sets should use full product replay (FT-081).
