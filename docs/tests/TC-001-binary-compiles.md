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
runner-args: "binary_compiles"
---

`cargo build --release --target aarch64-unknown-linux-gnu` completes with zero errors and zero warnings. The resulting binary is a single ELF file with no dynamic library dependencies beyond `libc`.