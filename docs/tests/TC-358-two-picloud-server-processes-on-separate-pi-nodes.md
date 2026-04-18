---
id: TC-358
title: Two picloud-server processes on separate Pi nodes join a single Raft cluster via mDNS
type: scenario
status: passing
runner: cargo-test
runner-args: "tc358_two_picloud_server_processes_join_single_cluster_via_mdns"
validates:
  features: [FT-013, FT-014]
  adrs: [ADR-004]
phase: 1
last-run: 2026-04-18T18:30:22.849110776+00:00
last-run-duration: 1.6s
---

## Description

**Observed symptom (2026-04-18, Pi cluster):** Started release `picloud-server`
on node3 (192.168.88.22) and worker02 (192.168.88.20). Each process came up
healthy (`/health` = ok) but `GET /` showed two *different* `cluster_id`
values:

- node3:    `cluster_id = ab27e839-9435-4082-a95b-78f456b61fff`, nodes = [node3]
- worker02: `cluster_id = 84bd6f28-4b07-462c-a1b0-c5a76112351f`, nodes = [worker02]

After 10+ seconds neither node appeared in the other's `nodes[]` list.
Expected behaviour per ADR-004 and the PRD "nodes discover each other via
mDNS, form a Raft cluster": the second process should discover the first
via mDNS and join its existing cluster rather than bootstrap a new one.

Existing TC-237 / TC-293 exercise mDNS discovery in unit tests on localhost
and both pass, so the regression is in the handoff between discovery and
Raft enrollment when running as real binaries on the LAN.

**Reproduced 2026-04-18 (second run).** Server logs on both nodes show the
same pattern within ~15 ms of mDNS `Registered`:

```
Registered mDNS service            → t+0ms
Cluster node started, browsing     → t+0ms
No peers discovered — bootstrapping as single-node Raft cluster   → t+16ms
```

Root cause likely: the bootstrap decision is taken synchronously right after
mDNS register, before the async browse loop has had time to observe any peer
TXT records. Even a 7 s stagger between node3 and worker02 does not help —
by the time worker02 browses, node3's already-bootstrapped cluster is just a
bare mDNS record, and worker02 still logs "No peers discovered". There is
no enrollment handshake.

Second, entangled symptom: on restart the sled-backed Raft store refuses
re-init with `not allowed to initialize due to current raft state`, so the
server falls back to in-memory state and *generates a fresh node_id*,
guaranteeing a cluster_id mismatch across restarts. This is downstream of
the same bug — once a node bootstraps a solo cluster, nothing later can
merge it into a peer's cluster.

## Invariant

When two `picloud-server` processes are started on the same LAN with the
same cluster domain, the second to start MUST join the first's cluster.
Specifically, within 30 s of the second node's start:

- Both nodes' `GET /` reports the same `cluster_id`.
- Both nodes' `GET /` lists both nodes in `nodes[]`.
- Exactly one node has `isLeader: true`.

## Shape of the Rust test

`#[tokio::test] async fn tc358_two_picloud_server_processes_join_single_cluster_via_mdns()`
in an integration test that:

1. Binds two in-process `picloud-server` instances on distinct loopback
   addresses (or ephemeral ports) sharing a domain.
2. Starts node A, waits for `/health` ok.
3. Starts node B 500 ms later, waits for `/health` ok.
4. Polls `GET /` on both for up to 30 s until `cluster_id` matches and
   `nodes[]` includes both — asserts this happens.
5. Asserts exactly one `isLeader: true` across the two.