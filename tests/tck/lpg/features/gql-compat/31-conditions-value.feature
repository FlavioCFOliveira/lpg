# Translated from gql-compat @ 9de1991 (https://github.com/tamnd/gql-compat),
# corpus/suite/31-conditions-value.yaml, licensed Apache-2.0 (LICENSE-gql-compat in this
# directory; gql-compat states no copyright notice and has no NOTICE file).
# Changed: the cases are rewritten as scenarios of lpg's conformance runner,
# with their fixtures as INSERT statements, by
# docs/evidence/lab-sprint-3/gqlcompat_translate.py. Do not edit by hand.

Feature: gql-compat 31-conditions-value

  # gql-compat @ 9de1991, corpus/suite/31-conditions-value.yaml, case condition/22008/datetime-past-the-end-of-the-calendar (ISO subclauses 20.26, 21.2)
  @clause-20.26 @clause-21.2
  Scenario: [condition/22008/datetime-past-the-end-of-the-calendar] Adding a day to the last representable date raises datetime field overflow
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN DATE '9999-12-31' + DURATION 'P1D' AS v
      """
    Then an exception condition should be raised: 22008

  # gql-compat @ 9de1991, corpus/suite/31-conditions-value.yaml, case condition/22014/incompatible-temporal-units (ISO subclauses 20.26, 20.28)
  @clause-20.26 @clause-20.28
  Scenario: [condition/22014/incompatible-temporal-units] Adding hours to a date raises incompatible temporal instant unit groups
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN DATE '2024-01-01' + DURATION 'PT1H' AS v
      """
    Then an exception condition should be raised: 22G14

  # gql-compat @ 9de1991, corpus/suite/31-conditions-value.yaml, case condition/22011/negative-substring-length (ISO subclauses 20.24)
  @clause-20.24
  Scenario: [condition/22011/negative-substring-length] Asking for a negative number of characters raises substring error
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN LEFT('abc', -1) AS v
      """
    Then an exception condition should be raised: 22011

  # gql-compat @ 9de1991, corpus/suite/31-conditions-value.yaml, case condition/22015/duration-past-its-range (ISO subclauses 20.28, 21.2)
  @clause-20.28 @clause-21.2
  Scenario: [condition/22015/duration-past-its-range] Multiplying a duration past its range raises interval field overflow
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN DURATION 'P1Y' * 1000000000000000000 AS v
      """
    Then an exception condition should be raised: 22015

  # gql-compat @ 9de1991, corpus/suite/31-conditions-value.yaml, case condition/22g04/comparing-a-number-with-a-string (ISO subclauses 19.3)
  @clause-19.3
  Scenario: [condition/22g04/comparing-a-number-with-a-string] Comparing values of unrelated types raises values not comparable
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN (1 < 'a') AS v
      """
    Then an exception condition should be raised: 22G04

  # gql-compat @ 9de1991, corpus/suite/31-conditions-value.yaml, case condition/22g12/arithmetic-on-a-string (ISO subclauses 20.21)
  @clause-20.21
  Scenario: [condition/22g12/arithmetic-on-a-string] Adding a number to a string raises invalid value type
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN 1 + 'a' AS v
      """
    Then an exception condition should be raised: 22G12 or 22G03

  # gql-compat @ 9de1991, corpus/suite/31-conditions-value.yaml, case condition/22g07/duration-field-that-is-not-a-field (ISO subclauses 20.29, 20.18)
  @clause-20.18 @clause-20.29
  Scenario: [condition/22g07/duration-field-that-is-not-a-field] A duration constructor field ISO does not define raises invalid field name
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN DURATION({yeas: 1}) AS v
      """
    Then an exception condition should be raised: 22G07

  # gql-compat @ 9de1991, corpus/suite/31-conditions-value.yaml, case condition/00001/insert-returns-no-result (ISO subclauses 13.2)
  @clause-13.2 @clause-16.5
  Scenario: [condition/00001/insert-returns-no-result] A statement with nothing to return completes with an omitted result
    Given an empty graph
    When executing query:
      """
      INSERT (:Person {name: 'Zoe'})
      """
    Then the result should be empty
