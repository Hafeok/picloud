---
id: TC-001
title: binary_compiles
type: scenario
status: passing
validates:
  features: []
  adrs:
  - ADR-001
phase: 1
runner: picloud-test
runner-args: run --scenario binary_compiles
last-run: 2026-04-13T20:35:13.782523150+00:00
---

`cargo build --release --target aarch64-unknown-linux-gnu` completes with zero errors and zero warnings. The resulting binary is a single ELF file with no dynamic library dependencies beyond `libc`.