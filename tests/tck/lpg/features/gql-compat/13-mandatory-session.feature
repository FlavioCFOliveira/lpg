# Translated from gql-compat @ 9de1991 (https://github.com/tamnd/gql-compat),
# corpus/suite/13-mandatory-session.yaml, licensed Apache-2.0 (LICENSE-gql-compat in this
# directory; gql-compat states no copyright notice and has no NOTICE file).
# Changed: the cases are rewritten as scenarios of lpg's conformance runner,
# with their fixtures as INSERT statements, by
# docs/evidence/lab-sprint-3/gqlcompat_translate.py. Do not edit by hand.

Feature: gql-compat 13-mandatory-session

  # gql-compat @ 9de1991, corpus/suite/13-mandatory-session.yaml, case mandatory/session/set-schema-at-the-root (ISO subclauses 7, 7.1, 4.5.1, 4.5.2.1, 4.5.2.3, 4.7.4, 4.10.1, 4.10.3)
  @clause-7.1 @clause-17.1
  Scenario: [mandatory/session/set-schema-at-the-root] A session may be pointed at the root of the catalog
    Given an empty graph
    When executing query:
      """
      SESSION SET SCHEMA /
      """
    Then the statement succeeds

  # gql-compat @ 9de1991, corpus/suite/13-mandatory-session.yaml, case mandatory/session/set-schema-by-a-predefined-name (ISO subclauses 7, 7.1, 17.1)
  @clause-7.1 @clause-17.1
  Scenario: [mandatory/session/set-schema-by-a-predefined-name] HOME_SCHEMA and CURRENT_SCHEMA name a schema without spelling a path
    Given an empty graph
    And having executed:
      """
      SESSION SET SCHEMA HOME_SCHEMA
      """
    When executing query:
      """
      SESSION SET SCHEMA CURRENT_SCHEMA
      """
    Then the statement succeeds

  # gql-compat @ 9de1991, corpus/suite/13-mandatory-session.yaml, case mandatory/session/set-schema-at-a-relative-path (ISO subclauses 7, 7.1, 17.1, 4.3.2)
  @clause-7.1 @clause-17.1
  Scenario: [mandatory/session/set-schema-at-a-relative-path] A schema reference may climb out of the current schema with two periods
    Given an empty graph
    When executing query:
      """
      SESSION SET SCHEMA ../gqlcompat_sibling
      """
    Then an exception condition should be raised: 42002

  # gql-compat @ 9de1991, corpus/suite/13-mandatory-session.yaml, case mandatory/session/set-graph (ISO subclauses 7, 7.1, 4.5.2.3, 4.7.5)
  @clause-7.1 @clause-11.1
  Scenario: [mandatory/session/set-graph] A session may be pointed at a graph
    Given an empty graph
    When executing query:
      """
      SESSION SET PROPERTY GRAPH CURRENT_PROPERTY_GRAPH
      """
    Then the statement succeeds

  # gql-compat @ 9de1991, corpus/suite/13-mandatory-session.yaml, case mandatory/session/set-graph-without-the-property-keyword (ISO subclauses 7, 7.1, 17.2)
  @clause-7.1 @clause-11.1 @clause-17.2
  Scenario: [mandatory/session/set-graph-without-the-property-keyword] PROPERTY is optional in the session graph clause
    Given an empty graph
    When executing query:
      """
      SESSION SET GRAPH HOME_PROPERTY_GRAPH
      """
    Then the statement succeeds

  # gql-compat @ 9de1991, corpus/suite/13-mandatory-session.yaml, case mandatory/session/close (ISO subclauses 7, 7.3, 4.5.1)
  @clause-7.3
  Scenario: [mandatory/session/close] A session may be closed by the program running in it
    Given an empty graph
    When executing query:
      """
      SESSION CLOSE
      """
    Then the statement succeeds
