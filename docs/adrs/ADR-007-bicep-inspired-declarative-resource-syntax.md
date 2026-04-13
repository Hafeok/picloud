---
id: ADR-007
title: Bicep-Inspired Declarative Resource Syntax
status: accepted
features: [FT-001]
supersedes: []
superseded-by: [ADR-049]
domains: []
scope: domain
---

**Status:** Accepted

**Context:** IaC is a first-class citizen in PiCloud. Operators and LLMs must be able to read and write resource definitions clearly. The syntax must be expressive enough to capture all resource types and their relationships.

**Decision:** Resource files use a Bicep-inspired syntax with typed resource declarations, property blocks, and symbolic references between resources.

**Rationale:**
- Bicep is well-understood by the target audience (developers on Microsoft stacks)
- Clear and readable — LLMs produce accurate Bicep-style syntax with minimal prompting
- Symbolic references between resources make dependencies explicit and readable
- The `resource 'type' 'name' = { }` pattern maps cleanly to PiCloud's resource model
- Avoids YAML's ambiguity (indentation errors, type coercion) and HCL's complexity

**File extension:** `.picloud`

**Rejected alternatives:**
- **YAML** — ubiquitous but ambiguous. Indentation errors are silent. Type coercion is surprising.
- **HCL (Terraform)** — well-designed but requires a large parser. Bicep is simpler and more readable.
- **Custom DSL** — unnecessary complexity. Bicep-inspired syntax covers all requirements.
- **JSON** — not human-writable at scale.