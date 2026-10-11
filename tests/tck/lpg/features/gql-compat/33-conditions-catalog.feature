# Translated from gql-compat @ 9de1991 (https://github.com/tamnd/gql-compat),
# corpus/suite/33-conditions-catalog.yaml, licensed Apache-2.0 (LICENSE-gql-compat in this
# directory; gql-compat states no copyright notice and has no NOTICE file).
# Changed: the cases are rewritten as scenarios of lpg's conformance runner,
# with their fixtures as INSERT statements, by
# docs/evidence/lab-sprint-3/gqlcompat_translate.py. Do not edit by hand.

Feature: gql-compat 33-conditions-catalog

  # gql-compat @ 9de1991, corpus/suite/33-conditions-catalog.yaml, case condition/25g01/transaction-inside-a-transaction (ISO subclauses 8.1)
  @clause-8.1
  Scenario: [condition/25g01/transaction-inside-a-transaction] Starting a transaction inside one raises active GQL-transaction
    Given an empty graph
    And having executed:
      """
      START TRANSACTION
      """
    When executing query:
      """
      START TRANSACTION
      """
    Then an exception condition should be raised: 25G01
