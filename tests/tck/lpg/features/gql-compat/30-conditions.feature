# Translated from gql-compat @ 9de1991 (https://github.com/tamnd/gql-compat),
# corpus/suite/30-conditions.yaml, licensed Apache-2.0 (LICENSE-gql-compat in this
# directory; gql-compat states no copyright notice and has no NOTICE file).
# Changed: the cases are rewritten as scenarios of lpg's conformance runner,
# with their fixtures as INSERT statements, by
# docs/evidence/lab-sprint-3/gqlcompat_translate.py. Do not edit by hand.

Feature: gql-compat 30-conditions

  # gql-compat @ 9de1991, corpus/suite/30-conditions.yaml, case condition/22012/division-by-zero (ISO subclauses 20.21, 4.8.4, 4.9.1, 4.9.2)
  @clause-20.21
  Scenario: [condition/22012/division-by-zero] Dividing by zero raises data exception, division by zero
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN 1 / 0 AS v
      """
    Then an exception condition should be raised: 22012

  # gql-compat @ 9de1991, corpus/suite/30-conditions.yaml, case condition/42002/unbound-variable-in-predicate (ISO subclauses 16.13, 20.12, 19.2)
  @clause-16.13 @clause-19.2 @clause-20.12
  Scenario: [condition/42002/unbound-variable-in-predicate] A where clause naming an unbound variable is an invalid reference
    Given an empty graph
    And having executed:
      """
      INSERT (a:Person {name: 'Alice', age: 34, city: 'Berlin'}),
             (b:Person {name: 'Bob', age: 27, city: 'Paris'}),
             (c:Person {name: 'Carol', age: 41, city: 'Berlin'}),
             (d:Person {name: 'Dave', age: 27, city: 'Lisbon'}),
             (e:Person {name: 'Erin', age: 55, city: 'Paris'}),
             (acme:Company {name: 'Acme', founded: 1993}),
             (globex:Company {name: 'Globex', founded: 2004}),
             (a)-[:KNOWS {since: 2015}]->(b),
             (b)-[:KNOWS {since: 2017}]->(c),
             (a)-[:KNOWS {since: 2019}]->(c),
             (c)-[:KNOWS {since: 2020}]->(d),
             (d)-[:KNOWS {since: 2021}]->(e),
             (a)-[:WORKS_AT {role: 'engineer'}]->(acme),
             (b)-[:WORKS_AT {role: 'analyst'}]->(acme),
             (c)-[:WORKS_AT {role: 'director'}]->(globex)
      """
    When executing query:
      """
      MATCH (p:Person)
      WHERE q.age > 30
      RETURN p.name AS name
      """
    Then an exception condition should be raised: 42002

  # gql-compat @ 9de1991, corpus/suite/30-conditions.yaml, case condition/42002/redeclared-variable (ISO subclauses 16.4, 16.7, 22.1)
  @clause-16.4 @clause-16.7
  Scenario: [condition/42002/redeclared-variable] Binding one variable to two different element types is an invalid reference
    Given an empty graph
    And having executed:
      """
      INSERT (a:Person {name: 'Alice', age: 34, city: 'Berlin'}),
             (b:Person {name: 'Bob', age: 27, city: 'Paris'}),
             (c:Person {name: 'Carol', age: 41, city: 'Berlin'}),
             (d:Person {name: 'Dave', age: 27, city: 'Lisbon'}),
             (e:Person {name: 'Erin', age: 55, city: 'Paris'}),
             (acme:Company {name: 'Acme', founded: 1993}),
             (globex:Company {name: 'Globex', founded: 2004}),
             (a)-[:KNOWS {since: 2015}]->(b),
             (b)-[:KNOWS {since: 2017}]->(c),
             (a)-[:KNOWS {since: 2019}]->(c),
             (c)-[:KNOWS {since: 2020}]->(d),
             (d)-[:KNOWS {since: 2021}]->(e),
             (a)-[:WORKS_AT {role: 'engineer'}]->(acme),
             (b)-[:WORKS_AT {role: 'analyst'}]->(acme),
             (c)-[:WORKS_AT {role: 'director'}]->(globex)
      """
    When executing query:
      """
      MATCH (p:Person)-[k:KNOWS]->(q:Person), (k:Person)
      RETURN p.name AS name
      """
    Then an exception condition should be raised: 42002

  # gql-compat @ 9de1991, corpus/suite/30-conditions.yaml, case condition/22g03/non-boolean-search-condition (ISO subclauses 19.1, 16.13)
  @clause-16.13 @clause-19.1
  Scenario: [condition/22g03/non-boolean-search-condition] A search condition that is not a boolean raises invalid value type
    Given an empty graph
    And having executed:
      """
      INSERT (a:Person {name: 'Alice', age: 34, city: 'Berlin'}),
             (b:Person {name: 'Bob', age: 27, city: 'Paris'}),
             (c:Person {name: 'Carol', age: 41, city: 'Berlin'}),
             (d:Person {name: 'Dave', age: 27, city: 'Lisbon'}),
             (e:Person {name: 'Erin', age: 55, city: 'Paris'}),
             (acme:Company {name: 'Acme', founded: 1993}),
             (globex:Company {name: 'Globex', founded: 2004}),
             (a)-[:KNOWS {since: 2015}]->(b),
             (b)-[:KNOWS {since: 2017}]->(c),
             (a)-[:KNOWS {since: 2019}]->(c),
             (c)-[:KNOWS {since: 2020}]->(d),
             (d)-[:KNOWS {since: 2021}]->(e),
             (a)-[:WORKS_AT {role: 'engineer'}]->(acme),
             (b)-[:WORKS_AT {role: 'analyst'}]->(acme),
             (c)-[:WORKS_AT {role: 'director'}]->(globex)
      """
    When executing query:
      """
      MATCH (p:Person)
      WHERE p.age
      RETURN p.name AS name
      """
    Then an exception condition should be raised: 22G03

  # gql-compat @ 9de1991, corpus/suite/30-conditions.yaml, case condition/22007/invalid-date-format (ISO subclauses 21.2)
  @clause-21.2
  Scenario: [condition/22007/invalid-date-format] A date literal that is not a date raises invalid date, time, or datetime format
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN DATE '2024-13-45' AS v
      """
    Then an exception condition should be raised: 22007

  # gql-compat @ 9de1991, corpus/suite/30-conditions.yaml, case condition/22g0h/invalid-duration-format (ISO subclauses 21.2)
  @clause-21.2
  Scenario: [condition/22g0h/invalid-duration-format] A duration literal that is not a duration raises invalid duration format
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN DURATION 'not a duration' AS v
      """
    Then an exception condition should be raised: 22G0H

  # gql-compat @ 9de1991, corpus/suite/30-conditions.yaml, case condition/22g0m/multiple-assignments-to-one-property (ISO subclauses 13.3)
  @clause-13.3
  Scenario: [condition/22g0m/multiple-assignments-to-one-property] Assigning the same property twice in one statement is a multiple assignment
    Given an empty graph
    And having executed:
      """
      INSERT (w1:Person {name: 'Ada', age: 36}),
             (w2:Person {name: 'Linus', age: 54})
      """
    When executing query:
      """
      MATCH (p:Person {name: 'Ada'})
      SET p.age = 1, p.age = 2
      RETURN p.age AS age
      """
    Then an exception condition should be raised: 22G0M

  # gql-compat @ 9de1991, corpus/suite/30-conditions.yaml, case condition/01g11/null-eliminated-in-set-function (ISO subclauses 20.9)
  @clause-20.9
  Scenario: [condition/01g11/null-eliminated-in-set-function] An aggregate that discarded a null completes with a warning
    Given an empty graph
    And having executed:
      """
      INSERT (p1:Maybe {id: 1, note: 'present'}),
             (p2:Maybe {id: 2, note: NULL})
      """
    When executing query:
      """
      MATCH (m:Maybe)
      RETURN COUNT(m.note) AS n
      """
    Then the result should be, in order:
      | n |
      | 1 |
