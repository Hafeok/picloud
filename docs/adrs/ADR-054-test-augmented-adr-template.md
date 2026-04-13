---
id: ADR-054
title: Test-Augmented ADR Template
status: accepted
features: [FT-009]
supersedes: []
superseded-by: []
domains: []
scope: domain
---

**Status:** Accepted

**Context:** PiCloud is a distributed platform with no external dependencies and a strong engineering culture of measuring and validating everything on real hardware. As the system grows, architectural decisions made early become invisible assumptions. Without explicit testability defined at decision time, test coverage is added retroactively (or not at all), and the tests that are added tend to test the implementation rather than the decision.

The three test suite designs established alongside this document — Scenario Harness, Chaos + Invariants, and Protocol Compliance — provide a structured vocabulary for expressing what "working correctly" means for any decision. This vocabulary must be applied at the point of making a decision, not after the fact.

**Decision:** Every ADR must include a `Test coverage` section. This section is mandatory for all new ADRs and must be present in all existing ADRs before the feature they govern enters implementation.

**Template — the `Test coverage` section must contain:**

- **Scenario tests** — one or more named scenarios from the Scenario Harness that validate the happy path and key edge cases for this decision. Each entry names the scenario file and states what it asserts.
- **Invariants** — properties that must hold continuously, including during and after faults. Each invariant is a falsifiable statement with a defined check method (SPARQL query, DNS probe, or metric threshold).
- **Protocol probes** *(only for decisions that introduce a protocol boundary)* — which RFC or specification the probe validates, and the specific assertions made.
- **Exit criteria** — measurable, pass/fail thresholds. These are the criteria that must be green before the feature is considered complete. Vague criteria such as "DNS works" are not acceptable. Every criterion must end with a number or a percentage.

**Consequences:**
- New ADRs cannot be merged without a `Test coverage` section.
- The test coverage section is the primary input for the `picloud-test` scenario catalogue. Tests are not invented separately — they are derived from ADRs.
- If a decision is difficult to test, that is a signal the decision is underspecified. Rewrite the decision, not the tests.
- All existing ADRs have been retrofitted with test coverage sections as part of this ADR's introduction.

**Rejected alternatives:**
- **Test coverage in the PRD** — PRD sections are feature-level. Test logic for a specific technical decision is too specific to live at that level and would be orphaned from the rationale it validates.
- **Separate test specification document** — a separate doc drifts from the decisions it covers. Co-location ensures the tests are updated when the decision is revised.
- **Tests only in code** — code-level tests are correct for unit and integration coverage but lack the narrative context of why a test exists. The ADR section is the human-readable contract; the code is the enforcement of it.