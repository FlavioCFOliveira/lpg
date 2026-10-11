# Translated from gql-compat @ 9de1991 (https://github.com/tamnd/gql-compat),
# corpus/suite/32-conditions-limits.yaml, licensed Apache-2.0 (LICENSE-gql-compat in this
# directory; gql-compat states no copyright notice and has no NOTICE file).
# Changed: the cases are rewritten as scenarios of lpg's conformance runner,
# with their fixtures as INSERT statements, by
# docs/evidence/lab-sprint-3/gqlcompat_translate.py. Do not edit by hand.

Feature: gql-compat 32-conditions-limits

  # gql-compat @ 9de1991, corpus/suite/32-conditions-limits.yaml, case condition/22g0q/edge-with-no-labels (ISO subclauses 13.2, 18.4)
  @clause-13.2 @clause-18.4
  Scenario: [condition/22g0q/edge-with-no-labels] An edge with no labels raises the edge label minimum where there is one
    Given an empty graph
    And having executed:
      """
      INSERT (w1:Person {name: 'Ada', age: 36}),
             (w2:Person {name: 'Linus', age: 54})
      """
    When executing query:
      """
      MATCH (a:Person {name: 'Ada'}), (b:Person {name: 'Linus'})
      INSERT (a)-[]->(b)
      """
    Then an exception condition should be raised: 22G0Q or 42006
