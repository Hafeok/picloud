---
id: ADR-008
title: Eventually Consistent Command Model
status: accepted
features:
- FT-002
supersedes: []
superseded-by: []
domains: []
scope: domain
content-hash: sha256:669c4b1385eef40401199c063fb30b422491da415536652d55ee4bd3d4e2af18
---

**Status:** Accepted

**Context:** The event-sourced architecture (ADR-004) means state changes are asynchronous. The CLI must reflect this. Blocking until a command is fully executed would require synchronous request-response, which conflicts with the distributed, event-driven model.

**Decision:** All CLI commands are eventually consistent. The CLI emits a command event with a correlation ID, subscribes to the platform event stream, and streams progress events until a terminal event (success or failure) arrives.

**Rationale:**
- Consistent with the event-sourced architecture — the CLI is just another event emitter and subscriber
- Long-running operations (volume allocation, multi-container deployment) stream real-time progress
- The model is transparent to the operator — they see what is happening, not just a spinner
- Commands are idempotent via client-generated idempotency keys (see ADR-015)

**User experience:**
```
$ picloud resource apply ./photo-app/
→ ResourceDeclared: photo-app
→ ResourceDeclared: media-store
→ ResourceProvisioning: media-store (allocating 100GB across 3 nodes)
→ ResourceReady: media-store
→ ResourceDeclared: api-server
→ ResourceProvisioning: api-server (scheduling on node pi-02)
→ ResourceReady: api-server
✓ photo-app deployed
```

**Rejected alternatives:**
- **Synchronous request-response** — incompatible with event-sourced architecture. Would require a separate synchronous state store.