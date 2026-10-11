# Translated from gql-compat @ 9de1991 (https://github.com/tamnd/gql-compat),
# corpus/suite/11-mandatory-pattern.yaml, licensed Apache-2.0 (LICENSE-gql-compat in this
# directory; gql-compat states no copyright notice and has no NOTICE file).
# Changed: the cases are rewritten as scenarios of lpg's conformance runner,
# with their fixtures as INSERT statements, by
# docs/evidence/lab-sprint-3/gqlcompat_translate.py. Do not edit by hand.

Feature: gql-compat 11-mandatory-pattern

  # gql-compat @ 9de1991, corpus/suite/11-mandatory-pattern.yaml, case mandatory/pattern/node-pattern-unlabelled (ISO subclauses 16.4, 22.2, 4.2.6, 4.11.1, 4.11.4)
  @clause-16.4
  Scenario: [mandatory/pattern/node-pattern-unlabelled] A node pattern with only a variable binds every node in the graph
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
      MATCH (x)
      RETURN COUNT(*) AS n
      """
    Then the result should be, in order:
      | n |
      | 7 |

  # gql-compat @ 9de1991, corpus/suite/11-mandatory-pattern.yaml, case mandatory/pattern/edge-pattern-directed (ISO subclauses 16.7, 22.3)
  @clause-16.7
  Scenario: [mandatory/pattern/edge-pattern-directed] A right-pointing full edge pattern binds edges of a type in their direction
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
      MATCH (p:Person)-[k:KNOWS]->(q:Person)
      RETURN p.name AS knower, q.name AS known
      """
    Then the result should be, in any order:
      | knower  | known   |
      | 'Alice' | 'Bob'   |
      | 'Bob'   | 'Carol' |
      | 'Alice' | 'Carol' |
      | 'Carol' | 'Dave'  |
      | 'Dave'  | 'Erin'  |

  # gql-compat @ 9de1991, corpus/suite/11-mandatory-pattern.yaml, case mandatory/pattern/edge-property-predicate (ISO subclauses 16.7, 16.13, 16.10, 20.11, 22.6, 4.11.5)
  @clause-16.7 @clause-16.10 @clause-16.13 @clause-20.11
  Scenario: [mandatory/pattern/edge-property-predicate] A property of an edge variable is readable in a where clause
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
      MATCH (p:Person)-[k:KNOWS]->(q:Person)
      WHERE k.since > 2018
      RETURN p.name AS knower, q.name AS known
      """
    Then the result should be, in any order:
      | knower  | known   |
      | 'Alice' | 'Carol' |
      | 'Carol' | 'Dave'  |
      | 'Dave'  | 'Erin'  |

  # gql-compat @ 9de1991, corpus/suite/11-mandatory-pattern.yaml, case mandatory/pattern/edge-type-selects (ISO subclauses 16.7, 16.8)
  @clause-16.7 @clause-16.8
  Scenario: [mandatory/pattern/edge-type-selects] An edge type in a pattern excludes edges of every other type
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
      MATCH (p:Person)-[:WORKS_AT]->(c:Company)
      RETURN p.name AS person, c.name AS company
      """
    Then the result should be, in any order:
      | person  | company  |
      | 'Alice' | 'Acme'   |
      | 'Bob'   | 'Acme'   |
      | 'Carol' | 'Globex' |

  # gql-compat @ 9de1991, corpus/suite/11-mandatory-pattern.yaml, case mandatory/pattern/edge-pattern-untyped (ISO subclauses 16.7)
  @clause-16.7
  Scenario: [mandatory/pattern/edge-pattern-untyped] An empty element pattern filler matches an edge of any type
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
      MATCH (p:Person)-[]->()
      RETURN COUNT(*) AS n
      """
    Then the result should be, in order:
      | n |
      | 8 |

  # gql-compat @ 9de1991, corpus/suite/11-mandatory-pattern.yaml, case mandatory/pattern/anonymous-endpoint (ISO subclauses 16.4, 16.7)
  @clause-16.4 @clause-16.7
  Scenario: [mandatory/pattern/anonymous-endpoint] A node pattern needs no variable to constrain a path
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
      MATCH (p:Person)-[:KNOWS]->()
      RETURN COUNT(*) AS n
      """
    Then the result should be, in order:
      | n |
      | 5 |

  # gql-compat @ 9de1991, corpus/suite/11-mandatory-pattern.yaml, case mandatory/pattern/path-concatenation (ISO subclauses 16.7, 4.11.3)
  @clause-16.7
  Scenario: [mandatory/pattern/path-concatenation] Concatenated element patterns match a two-edge path
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
      MATCH (p:Person)-[:KNOWS]->(q:Person)-[:KNOWS]->(r:Person)
      RETURN p.name AS origin, r.name AS target
      """
    Then the result should be, in any order:
      | origin  | target  |
      | 'Alice' | 'Carol' |
      | 'Bob'   | 'Dave'  |
      | 'Alice' | 'Dave'  |
      | 'Carol' | 'Erin'  |

  # gql-compat @ 9de1991, corpus/suite/11-mandatory-pattern.yaml, case mandatory/pattern/path-pattern-list-joins (ISO subclauses 16.4)
  @clause-16.4
  Scenario: [mandatory/pattern/path-pattern-list-joins] Two path patterns in one graph pattern join on their shared variable
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
      MATCH (p:Person)-[:KNOWS]->(q:Person), (q)-[:KNOWS]->(r:Person)
      RETURN p.name AS origin, r.name AS target
      """
    Then the result should be, in any order:
      | origin  | target  |
      | 'Alice' | 'Carol' |
      | 'Bob'   | 'Dave'  |
      | 'Alice' | 'Dave'  |
      | 'Carol' | 'Erin'  |

  # gql-compat @ 9de1991, corpus/suite/11-mandatory-pattern.yaml, case mandatory/pattern/property-specification (ISO subclauses 16.4)
  @clause-16.4
  Scenario: [mandatory/pattern/property-specification] A property key-value pair in a pattern filters as an equality would
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
      MATCH (p:Person {name: 'Alice'})
      RETURN p.age AS age
      """
    Then the result should be, in order:
      | age |
      | 34  |

  # gql-compat @ 9de1991, corpus/suite/11-mandatory-pattern.yaml, case mandatory/pattern/element-pattern-predicate (ISO subclauses 16.4, 16.13)
  @clause-16.4 @clause-16.13
  Scenario: [mandatory/pattern/element-pattern-predicate] A local where clause inside an element pattern filters that element
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
      MATCH (p:Person WHERE p.age > 40)
      RETURN p.name AS name
      """
    Then the result should be, in any order:
      | name    |
      | 'Carol' |
      | 'Erin'  |

  # gql-compat @ 9de1991, corpus/suite/11-mandatory-pattern.yaml, case mandatory/pattern/label-disjunction (ISO subclauses 16.8, 22.5)
  @clause-16.8
  Scenario: [mandatory/pattern/label-disjunction] A label disjunction matches an element carrying either label
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
      MATCH (x:Person|Company)
      RETURN COUNT(*) AS n
      """
    Then the result should be, in order:
      | n |
      | 7 |

  # gql-compat @ 9de1991, corpus/suite/11-mandatory-pattern.yaml, case mandatory/pattern/label-negation (ISO subclauses 16.8, 22.5)
  @clause-16.8
  Scenario: [mandatory/pattern/label-negation] A label negation matches an element that does not carry the label
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
      MATCH (x:!Company)
      RETURN COUNT(*) AS n
      """
    Then the result should be, in order:
      | n |
      | 5 |

  # gql-compat @ 9de1991, corpus/suite/11-mandatory-pattern.yaml, case mandatory/pattern/label-conjunction (ISO subclauses 16.8, 22.5)
  @clause-16.8
  Scenario: [mandatory/pattern/label-conjunction] A label conjunction matches only an element carrying both labels
    Given an empty graph
    And having executed:
      """
      INSERT (m1:Person&Employee {name: 'Mika'}),
             (m2:Person {name: 'Noor'})
      """
    When executing query:
      """
      MATCH (x:Person&Employee)
      RETURN x.name AS name
      """
    Then the result should be, in any order:
      | name   |
      | 'Mika' |

  # gql-compat @ 9de1991, corpus/suite/11-mandatory-pattern.yaml, case mandatory/pattern/label-parenthesized (ISO subclauses 16.8)
  @clause-16.8
  Scenario: [mandatory/pattern/label-parenthesized] Parentheses group a label expression against the default precedence
    Given an empty graph
    And having executed:
      """
      INSERT (m1:Person&Employee {name: 'Mika'}),
             (m2:Person {name: 'Noor'})
      """
    When executing query:
      """
      MATCH (x:(Person&Employee)|Employee)
      RETURN x.name AS name
      """
    Then the result should be, in any order:
      | name   |
      | 'Mika' |

  # gql-compat @ 9de1991, corpus/suite/11-mandatory-pattern.yaml, case mandatory/pattern/exists-predicate (ISO subclauses 19.4)
  @clause-19.4
  Scenario: [mandatory/pattern/exists-predicate] An EXISTS predicate over a match statement filters on a pattern it does not project
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
      WHERE EXISTS { MATCH (p)-[:WORKS_AT]->(:Company) }
      RETURN p.name AS name
      """
    Then the result should be, in any order:
      | name    |
      | 'Alice' |
      | 'Bob'   |
      | 'Carol' |

  # gql-compat @ 9de1991, corpus/suite/11-mandatory-pattern.yaml, case mandatory/pattern/exists-predicate-negated (ISO subclauses 19.4, 20.20)
  @clause-19.4 @clause-20.20
  Scenario: [mandatory/pattern/exists-predicate-negated] NOT EXISTS keeps the bindings the pattern does not hold for
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
      WHERE NOT EXISTS { MATCH (p)-[:WORKS_AT]->(:Company) }
      RETURN p.name AS name
      """
    Then the result should be, in any order:
      | name   |
      | 'Dave' |
      | 'Erin' |

  # gql-compat @ 9de1991, corpus/suite/11-mandatory-pattern.yaml, case mandatory/pattern/optional-match (ISO subclauses 14.4)
  @clause-14.4
  Scenario: [mandatory/pattern/optional-match] OPTIONAL MATCH keeps a binding and nulls the variables its pattern did not bind
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
      OPTIONAL MATCH (p)-[:WORKS_AT]->(c:Company)
      RETURN p.name AS name, c.name AS company
      """
    Then the result should be, in any order:
      | name    | company  |
      | 'Alice' | 'Acme'   |
      | 'Bob'   | 'Acme'   |
      | 'Carol' | 'Globex' |
      | 'Dave'  | null     |
      | 'Erin'  | null     |

  # gql-compat @ 9de1991, corpus/suite/11-mandatory-pattern.yaml, case mandatory/pattern/self-loop (ISO subclauses 16.7)
  @clause-16.7
  Scenario: [mandatory/pattern/self-loop] An edge whose endpoints are the same node matches a two-node pattern
    Given an empty graph
    And having executed:
      """
      INSERT (s1:Peer {name: 'solo'}),
             (s1)-[:FRIEND]->(s1)
      """
    When executing query:
      """
      MATCH (x:Peer)-[:FRIEND]->(y:Peer)
      RETURN x.name AS source, y.name AS target
      """
    Then the result should be, in any order:
      | source | target |
      | 'solo' | 'solo' |

  # gql-compat @ 9de1991, corpus/suite/11-mandatory-pattern.yaml, case mandatory/pattern/parallel-edges (ISO subclauses 16.7, 4.11.6)
  @clause-16.7
  Scenario: [mandatory/pattern/parallel-edges] Two edges between one pair of nodes are two bindings, not one
    Given an empty graph
    And having executed:
      """
      INSERT (q1:Peer {name: 'left'}),
             (q2:Peer {name: 'right'}),
             (q1)-[:FRIEND {rank: 1}]->(q2),
             (q1)-[:FRIEND {rank: 2}]->(q2)
      """
    When executing query:
      """
      MATCH (x:Peer)-[f:FRIEND]->(y:Peer)
      RETURN f.rank AS rank
      """
    Then the result should be, in any order:
      | rank |
      | 1    |
      | 2    |
