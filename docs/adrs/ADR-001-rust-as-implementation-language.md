---
id: ADR-001
title: Rust as Implementation Language
status: accepted
features: [FT-001, FT-012]
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** PiCloud must compile to a single ARM64 binary with no runtime dependencies. The platform handles storage, scheduling, and cryptography — domains where memory safety and predictable performance matter. An LLM will be writing most of the implementation code, so the language's type system and explicitness are assets.

**Decision:** Implement PiCloud in Rust.

**Rationale:**
- Single binary compilation to ARM64 with no runtime (no JVM, no GC)
- Memory safety guarantees without garbage collection pauses — critical for storage and scheduling paths
- The full stack is Rust-native: `openraft` (consensus), `oxigraph` (RDF), `youki` (OCI runtime), `mdns-sd` (mDNS) — no cross-language FFI required
- Rust's type system forces explicit error handling, which maps well to a distributed system where partial failure is the norm
- LLMs produce high-quality Rust when given explicit type contracts and architectural context

**Rejected alternatives:**
- **Go** — first instinct for systems tooling, but the key dependencies (Oxigraph, youki) are Rust-native. Go would require FFI bridges or inferior alternatives. Also, GC pauses are undesirable in storage hot paths.
- **C++** — memory safety is not guaranteed. Adds risk without benefit given Rust's maturity.