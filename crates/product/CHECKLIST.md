# Product CLI — Feature Checklist

> Auto-maintained during implementation.
> Each item maps to a PRD section and/or ADR from `new features/product-prd.md` and `new features/product-adrs.md`.
> Status: [ ] not started, [~] partial/stub, [x] implemented, [T] tested
>
> Last verified: 2026-04-12 — 54 unit tests, 4 benchmarks, full E2E exercise

---

## Phase 1 — Core Graph and Context

### ADR-001: Rust as Implementation Language
- [T] Single binary compiles (`cargo build -p product`)
- [T] clap CLI argument parser
- [T] `#![deny(clippy::unwrap_used)]` — zero panics on user input

### ADR-002: YAML Front-Matter as Graph Source of Truth
- [T] Feature front-matter parser (id, title, phase, status, depends-on, adrs, tests)
- [T] ADR front-matter parser (id, title, status, features, supersedes, superseded-by)
- [T] Test criterion front-matter parser (id, title, type, status, validates, phase)
- [T] Front-matter round-trip: parse -> modify -> write

### ADR-003: Derived Graph — No Persistent Graph Store
- [T] In-memory graph rebuilt from front-matter on every command
- [T] All 5 edge types: ImplementedBy, ValidatedBy, TestedBy, Supersedes, DependsOn
- [T] Forward + reverse adjacency lists
- [T] `index.ttl` is export-only, never read by Product

### ADR-004: Markdown as Document Format
- [T] CommonMark markdown with YAML front-matter
- [T] Front-matter stripped in context bundles
- [T] Formal blocks preserved verbatim in context bundles

### ADR-005: Numeric ID Scheme
- [T] Auto-increment IDs: `FT-001`, `ADR-001`, `TC-001`
- [T] Gaps not filled — next ID is max(existing) + 1
- [T] Filename generation: `FT-001-cluster-foundation.md`
- [T] Configurable prefixes via `product.toml`
- [T] ID format validation (PREFIX-NNN, E005 on invalid)

### ADR-006: Context Bundle as Primary LLM Interface
- [T] `product context FT-XXX` with AISP `⟦Ω:Bundle⟧` header
- [T] Aggregate evidence `⟦Ε⟧` from test criteria
- [T] ADRs ordered by betweenness centrality (default) / `--order id`
- [T] `--depth N` BFS transitive context with dedup
- [T] Superseded ADRs replaced by successors
- [T] `product context ADR-XXX` / `--phase N` / `--adrs-only`

### ADR-007: Checklist is Generated, Never Hand-Edited
- [T] `product checklist generate` from front-matter
- [T] Ordered by topological sort, grouped by phase

### ADR-008: Embedded Oxigraph for SPARQL Queries
- [T] `product graph query "SELECT ..."` via Oxigraph
- [T] TTL export with all prefixes and centrality scores

### ADR-009: CI Integration via Exit Codes
- [T] Exit code 0 (clean), 1 (errors), 2 (warnings), 3 (internal)
- [T] `--format json` structured stderr output

### ADR-010: Auto-Orphan on Feature Abandonment
- [T] Feature abandonment removes from test validates.features
- [T] Orphaned tests = warnings, files not deleted

### ADR-011: AISP Formal Notation
- [T] Types, Invariants, Scenario, ExitCriteria, Evidence block parsing
- [T] E001 on delta out of range, unclosed delimiter, unknown block type
- [T] W004 on empty block body

### ADR-012: Graph Theory Foundations
- [T] Topological sort (Kahn's) with E003 cycle detection
- [T] `product feature next` uses topo sort
- [T] BFS to depth N with dedup
- [T] Betweenness centrality (Brandes') with normalization
- [T] Reverse-graph BFS for impact analysis
- [T] Parallel topo sort (unrelated features unordered)

### ADR-013: Error Model
- [T] E001–E010 error codes, W001–W007 warning codes
- [T] Rustc-style diagnostics (file, line, detail, hint)
- [T] `--format json` on graph check
- [T] `internal_error!` macro for Tier 4 (exit code 3)

### ADR-014: Schema Versioning
- [T] `schema-version` in product.toml
- [T] E008 forward incompatibility, W007 backward compat

### ADR-015: File Write Safety
- [T] Atomic writes: temp + fsync + rename
- [T] `.product.lock` with stale PID detection and 3s timeout
- [T] Tmp file cleanup on startup

### ADR-016: Formal Block Grammar
- [T] Recursive descent parser for all block types
- [T] Evidence validation (δ range, φ range)
- [T] Error/warning reporting via `parse_formal_blocks_with_diagnostics()`

### ADR-017: Migration
- [T] `product migrate from-prd/from-adrs --validate/--execute/--interactive`
- [T] Phase inference, status extraction, test criteria extraction
- [T] Source document never modified

---

## Phase 2 — Authoring, Status and Impact

- [T] `product feature/adr/test new` — scaffold with auto-incremented ID
- [T] `product feature link --adr/--test/--dep` — validates no cycles on --dep
- [T] `product feature/adr/test status` — update; ADR supersession triggers impact report
- [T] `product impact ADR-XXX / FT-XXX / TC-XXX` — reverse-graph reachability
- [T] `product migrate schema --dry-run/--execute` — v0→v1 migration (adds depends-on, bumps version)
- [T] `product checklist generate` — ordered by topological sort
- [T] `product status` with phase, coverage, dependency summary
- [T] `product test untested` and `--failing` filters
- [T] Front-matter validation on write — ID format (E005), type checking
- [T] Git-aware: warn if modified files are uncommitted on checklist generate
- [T] `schema-version = "1"` migration function registered and tested

**Exit criteria:**
- [T] Supersede ADR-002 → impact report prints before commit *(verified E2E)*
- [T] `product migrate schema` on v0 repo → files updated, version bumped *(unit tested)*
- [T] Cycle validation on link --dep *(unit tested)*

---

## Phase 3 — Graph Intelligence and CI Integration

- [T] Betweenness centrality (Brandes') — `product graph central`
- [T] ADR ordering by centrality in context bundles (default, `--order id`)
- [T] `product graph stats` — centrality summary, φ formal coverage, link density, timing
- [T] `product graph query` — embedded Oxigraph, SPARQL 1.1
- [T] Centrality scores in TTL export on `graph rebuild`
- [T] Benchmark suite — parse 200 files (2.3ms), centrality, impact, BFS — all PASS
- [T] All timing invariants validated: parse < 200ms, centrality < 100ms, impact < 50ms
- [T] `--format json` output on all list and navigation commands
- [T] Shell completions: `product completions bash/zsh/fish`

**Exit criteria:**
- [T] `product graph central` returns ranked ADRs *(verified E2E)*
- [T] Benchmark suite passes all timing invariants *(2.3ms for 200 files, all <1ms)*

---

## Unit Tests — 54 passing

| Module | Count | Tests |
|---|---|---|
| config | 6 | parse_minimal, parse_full, schema_forward_error, schema_migrate_v0_dry_run, schema_migrate_v0_execute, schema_migrate_already_current |
| parser | 9 | split_front_matter, no_front_matter, next_id, next_id_empty, id_to_filename, feature_roundtrip, adr_parse, test_parse, validate_id_valid, validate_id_invalid |
| formal | 9 | evidence_block, types_block, scenario_block, invariants_block, delta_out_of_range, empty_block_warning, unrecognised_block_type, unclosed_delimiter, valid_evidence |
| graph | 13 | topo_sort_simple, topo_sort_cycle, topo_sort_parallel, feature_next, bfs_depth_1, impact_analysis, centrality_values, check_broken_link, check_clean_0, check_warning_2, check_e003_cycle, check_w001, check_w002, check_w003, check_w005 |
| fileops | 3 | atomic_write, no_tmp_leftover, cleanup_tmp |
| rdf | 2 | turtle_prefixes, sparql_query |
| migrate | 9 | strip_number, excluded_headings, detect_phase, infer_status, extract_adr_status, prd_detects_features, adrs_extracts_tests, validate_writes_nothing, execute_creates_files |

## Benchmarks — 4 passing

| Benchmark | Result | Limit |
|---|---|---|
| Parse 200 files | 2.3ms | 200ms |
| Centrality 200 nodes | <0.1ms | 100ms |
| Impact analysis | <0.1ms | 50ms |
| BFS depth 2 | <0.1ms | 50ms |

---

## Legend

| Symbol | Meaning |
|---|---|
| `[ ]` | Not started |
| `[~]` | Partial implementation |
| `[x]` | Implemented (compiles) |
| `[T]` | Tested (unit tests + E2E verified) |
