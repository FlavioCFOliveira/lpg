# lpg's own scenarios (MIT, lpg project) for the approved optional features
# that no selected opengql/tck, openCypher TCK or gql-compat scenario
# exercises: GV40, GQ01, GT01 (commit and rollback), GP18, GT03, and the
# positive forms of GD01 and GG02 (rmp #52; FR-GQL-002).

Feature: Approved optional features without TCK coverage

  @GV40
  Scenario: [1] A zoned datetime keeps its offset
    Given any graph
    When executing query:
      """
      RETURN ZONED_DATETIME('2015-07-21T21:40:32.142+01:00') AS z
      """
    Then the result should be, in order:
      | z                               |
      | '2015-07-21T21:40:32.142+01:00' |
    And no side effects

  @GV40
  Scenario: [2] A zoned time keeps its offset
    Given any graph
    When executing query:
      """
      RETURN ZONED_TIME('21:40:32+01:00') AS t
      """
    Then the result should be, in order:
      | t                |
      | '21:40:32+01:00' |
    And no side effects

  @GQ01 @GC04
  Scenario: [3] USE runs a statement on another graph
    Given an empty graph
    And having executed:
      """
      CREATE GRAPH /public/other ANY
      """
    And having executed:
      """
      USE /public/other INSERT (:A {x: 1})
      """
    When executing query:
      """
      USE /public/other MATCH (n:A) RETURN n.x AS x
      """
    Then the result should be, in order:
      | x |
      | 1 |
    And no side effects

  @GQ01 @GC04
  Scenario: [4] A write through USE leaves the session graph unchanged
    Given an empty graph
    And having executed:
      """
      CREATE GRAPH /public/other ANY
      """
    And having executed:
      """
      USE /public/other INSERT (:A {x: 1})
      """
    When executing query:
      """
      MATCH (n:A) RETURN COUNT(*) AS c
      """
    Then the result should be, in order:
      | c |
      | 0 |
    And no side effects

  @GT01 @GD01
  Scenario: [5] COMMIT makes an explicit transaction's write visible
    Given an empty graph
    And having executed:
      """
      START TRANSACTION
      """
    And having executed:
      """
      INSERT (:T {v: 1})
      """
    And having executed:
      """
      COMMIT
      """
    When executing query:
      """
      MATCH (n:T) RETURN n.v AS v
      """
    Then the result should be, in order:
      | v |
      | 1 |
    And no side effects

  @GT01 @GD01
  Scenario: [6] ROLLBACK discards an explicit transaction's write
    Given an empty graph
    And having executed:
      """
      START TRANSACTION
      """
    And having executed:
      """
      INSERT (:T {v: 1})
      """
    And having executed:
      """
      ROLLBACK
      """
    When executing query:
      """
      MATCH (n:T) RETURN COUNT(*) AS c
      """
    Then the result should be, in order:
      | c |
      | 0 |
    And no side effects

  @GP18 @GT01 @GC04 @GQ01
  Scenario: [7] A transaction creates a graph and then writes to it
    Given an empty graph
    And having executed:
      """
      START TRANSACTION
      """
    And having executed:
      """
      CREATE GRAPH /public/fresh ANY
      """
    And having executed:
      """
      USE /public/fresh INSERT (:B)
      """
    And having executed:
      """
      COMMIT
      """
    When executing query:
      """
      USE /public/fresh MATCH (n:B) RETURN COUNT(*) AS c
      """
    Then the result should be, in order:
      | c |
      | 1 |
    And no side effects

  @GT03 @GT01 @GQ01 @GC04
  Scenario: [8] One transaction writes to two graphs
    Given an empty graph
    And having executed:
      """
      CREATE GRAPH /public/g2 ANY
      """
    And having executed:
      """
      START TRANSACTION
      """
    And having executed:
      """
      INSERT (:C {g: 'default'})
      """
    And having executed:
      """
      USE /public/g2 INSERT (:C {g: 'second'})
      """
    And having executed:
      """
      COMMIT
      """
    When executing query:
      """
      USE /public/g2 MATCH (n:C) RETURN n.g AS g
      """
    Then the result should be, in order:
      | g        |
      | 'second' |
    And no side effects

  @GT03 @GT01 @GQ01 @GC04
  Scenario: [9] A rolled-back transaction leaves both graphs unchanged
    Given an empty graph
    And having executed:
      """
      CREATE GRAPH /public/g2 ANY
      """
    And having executed:
      """
      START TRANSACTION
      """
    And having executed:
      """
      INSERT (:C {g: 'default'})
      """
    And having executed:
      """
      USE /public/g2 INSERT (:C {g: 'second'})
      """
    And having executed:
      """
      ROLLBACK
      """
    When executing query:
      """
      MATCH (n:C) RETURN COUNT(*) AS c
      """
    Then the result should be, in order:
      | c |
      | 0 |
    And no side effects

  @GD01
  Scenario: [10] INSERT creates nodes, an edge and their properties
    Given an empty graph
    When executing query:
      """
      INSERT (a:P {name: 'a'})-[:R {w: 2}]->(b:P {name: 'b'})
      """
    Then the result should be empty
    And the side effects should be:
      | +nodes         | 2 |
      | +relationships | 1 |
      | +properties    | 3 |

  @GD01
  Scenario: [11] SET and REMOVE change the properties of a node
    Given an empty graph
    And having executed:
      """
      INSERT (:P {name: 'a', v: 1})
      """
    When executing query:
      """
      MATCH (n:P) SET n.v = 2 REMOVE n.name RETURN n.v AS v, n.name AS name
      """
    Then the result should be, in order:
      | v | name |
      | 2 | null |
    And the side effects should be:
      | +properties | 1 |

  @GD01
  Scenario: [12] A returned node has its labels and properties
    Given an empty graph
    And having executed:
      """
      INSERT (:P&Q {name: 'a'})
      """
    When executing query:
      """
      MATCH (n:P) RETURN n
      """
    Then the result should be, in order:
      | n                    |
      | (:P:Q {name: 'a'})   |
    And no side effects

  @GG02 @GG03 @GC04 @GQ01
  Scenario: [13] A write outside a closed graph type fails with a graph type violation
    Given an empty graph
    And having executed:
      """
      CREATE GRAPH /public/tg { (P :P {name STRING}) }
      """
    When executing query:
      """
      USE /public/tg INSERT (:Q)
      """
    Then an exception condition should be raised: G2000

  @GG02 @GG03 @GC04 @GQ01
  Scenario: [14] A write inside a closed graph type succeeds
    Given an empty graph
    And having executed:
      """
      CREATE GRAPH /public/tg { (P :P {name STRING}) }
      """
    And having executed:
      """
      USE /public/tg INSERT (:P {name: 'a'})
      """
    When executing query:
      """
      USE /public/tg MATCH (n:P) RETURN n.name AS name
      """
    Then the result should be, in order:
      | name |
      | 'a'  |
    And no side effects
