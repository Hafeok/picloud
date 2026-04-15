---
id: TC-228
title: Inference rule automatically assigns user to group on tag change
type: exit-criteria
status: passing
runner: cargo-test
runner-args: "tc228_inference_rule_automatically_assigns_user_to_group_on_tag_change"
validates:
  features: [FT-055, FT-057, FT-058]
  adrs: []
phase: 3
last-run: 2026-04-15T13:43:29.499715023+00:00
last-run-duration: 0.5s
---

## Description

End-to-end integration test verifying the full tag → inference → group membership pipeline:

1. Create a user identity in the RDF graph
2. Register an inference rule: "users tagged dept=engineering → member of eng-group"
3. Add the matching tag to the user via TagAdded event
4. Evaluate the inference rule (triggered by TagAdded)
5. Verify GroupMembershipChanged event is emitted with action=added
6. Project the GroupMembershipChanged event into the graph
7. Verify group membership is SPARQL-queryable
8. Add a second user with the same tag, verify they also join the group
9. Remove the tag from the first user via TagRemoved event
10. Re-evaluate the rule and verify retraction (user removed from group)
11. Verify remaining group membership is correct