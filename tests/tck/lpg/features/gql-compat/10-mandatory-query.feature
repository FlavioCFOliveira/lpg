# Translated from gql-compat @ 9de1991 (https://github.com/tamnd/gql-compat),
# corpus/suite/10-mandatory-query.yaml, licensed Apache-2.0 (LICENSE-gql-compat in this
# directory; gql-compat states no copyright notice and has no NOTICE file).
# Changed: the cases are rewritten as scenarios of lpg's conformance runner,
# with their fixtures as INSERT statements, by
# docs/evidence/lab-sprint-3/gqlcompat_translate.py. Do not edit by hand.

Feature: gql-compat 10-mandatory-query

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/match-node-by-label (ISO subclauses 14.4, 14.11, 16.4)
  @clause-14.4 @clause-14.11 @clause-16.4
  Scenario: [mandatory/query/match-node-by-label] A match statement binds every node carrying a label and a return statement projects one property
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
      RETURN p.name AS name
      """
    Then the result should be, in any order:
      | name    |
      | 'Alice' |
      | 'Bob'   |
      | 'Carol' |
      | 'Dave'  |
      | 'Erin'  |

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/match-second-label (ISO subclauses 14.4, 16.8)
  @clause-14.4 @clause-16.8
  Scenario: [mandatory/query/match-second-label] A label in a node pattern selects only the nodes that carry it
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
      MATCH (c:Company)
      RETURN c.name AS name
      """
    Then the result should be, in any order:
      | name     |
      | 'Acme'   |
      | 'Globex' |

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/return-multiple-items (ISO subclauses 14.11)
  @clause-14.11
  Scenario: [mandatory/query/return-multiple-items] A return item list projects more than one value per binding
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
      WHERE p.city = 'Lisbon'
      RETURN p.name AS name, p.age AS age
      """
    Then the result should be, in any order:
      | name   | age |
      | 'Dave' | 27  |

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/return-distinct (ISO subclauses 14.11, 22.12)
  @clause-14.11
  Scenario: [mandatory/query/return-distinct] DISTINCT in a return statement body removes duplicate records
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
      RETURN DISTINCT p.city AS city
      """
    Then the result should be, in any order:
      | city     |
      | 'Berlin' |
      | 'Paris'  |
      | 'Lisbon' |

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/distinct-folds-two-nulls-into-one (ISO subclauses 14.11, 22.11, 22.12, 4.4.3)
  @clause-14.11
  Scenario: [mandatory/query/distinct-folds-two-nulls-into-one] DISTINCT folds two nulls together, which equality would not have done
    Given an empty graph
    And having executed:
      """
      INSERT (r1:Item {name: 'a', rank: 3}),
             (r2:Item {name: 'b', rank: NULL}),
             (r3:Item {name: 'c', rank: 1}),
             (r4:Item {name: 'd', rank: NULL}),
             (r5:Item {name: 'e', rank: 2})
      """
    When executing query:
      """
      MATCH (i:Item)
      RETURN DISTINCT i.rank AS rank
      """
    Then the result should be, in any order:
      | rank |
      | 1    |
      | 2    |
      | 3    |
      | null |

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/where-comparison (ISO subclauses 16.13, 19.3)
  @clause-16.13 @clause-19.1 @clause-19.3
  Scenario: [mandatory/query/where-comparison] A where clause keeps only the bindings its search condition holds for
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
      WHERE p.age > 30
      RETURN p.name AS name
      """
    Then the result should be, in any order:
      | name    |
      | 'Alice' |
      | 'Carol' |
      | 'Erin'  |

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/where-conjunction (ISO subclauses 16.13, 20.20)
  @clause-16.13 @clause-19.1 @clause-20.20
  Scenario: [mandatory/query/where-conjunction] AND in a search condition requires both operands
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
      WHERE p.age > 25 AND p.city = 'Berlin'
      RETURN p.name AS name
      """
    Then the result should be, in any order:
      | name    |
      | 'Alice' |
      | 'Carol' |

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/where-disjunction (ISO subclauses 16.13, 20.20)
  @clause-16.13 @clause-20.20
  Scenario: [mandatory/query/where-disjunction] OR in a search condition accepts either operand
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
      WHERE p.city = 'Lisbon' OR p.age = 55
      RETURN p.name AS name
      """
    Then the result should be, in any order:
      | name   |
      | 'Dave' |
      | 'Erin' |

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/where-negation (ISO subclauses 16.13, 20.20)
  @clause-16.13 @clause-20.20
  Scenario: [mandatory/query/where-negation] NOT inverts a search condition that is true or false
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
      WHERE NOT p.city = 'Berlin'
      RETURN p.name AS name
      """
    Then the result should be, in any order:
      | name   |
      | 'Bob'  |
      | 'Dave' |
      | 'Erin' |

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/order-by-ascending (ISO subclauses 14.9, 16.16, 16.17, 4.16.3.2)
  @clause-14.9 @clause-16.16 @clause-16.17
  Scenario: [mandatory/query/order-by-ascending] An order by clause after a return statement sorts on a projected column
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
      RETURN p.name AS name
      ORDER BY name
      """
    Then the result should be, in order:
      | name    |
      | 'Alice' |
      | 'Bob'   |
      | 'Carol' |
      | 'Dave'  |
      | 'Erin'  |

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/order-by-descending-then-ascending (ISO subclauses 16.16, 16.17, 22.14)
  @clause-16.16 @clause-16.17
  Scenario: [mandatory/query/order-by-descending-then-ascending] A sort specification list breaks ties on the next key and honours DESC
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
      RETURN p.name AS name, p.age AS age
      ORDER BY age DESC, name ASC
      """
    Then the result should be, in order:
      | name    | age |
      | 'Erin'  | 55  |
      | 'Carol' | 41  |
      | 'Alice' | 34  |
      | 'Bob'   | 27  |
      | 'Dave'  | 27  |

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/count-star (ISO subclauses 20.9)
  @clause-20.9
  Scenario: [mandatory/query/count-star] COUNT of asterisk counts bindings without reading a value
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
      RETURN COUNT(*) AS n
      """
    Then the result should be, in order:
      | n |
      | 5 |

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/aggregate-sum-min-max (ISO subclauses 20.9)
  @clause-20.9
  Scenario: [mandatory/query/aggregate-sum-min-max] SUM, MIN and MAX reduce a whole binding table to one record
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
      RETURN SUM(p.age) AS total, MIN(p.age) AS youngest, MAX(p.age) AS oldest
      """
    Then the result should be, in order:
      | total | youngest | oldest |
      | 184   | 27       | 55     |

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/aggregate-avg (ISO subclauses 20.9)
  @clause-20.9
  Scenario: [mandatory/query/aggregate-avg] AVG of exact numbers is the exact mean, not an integer quotient
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
      RETURN AVG(p.age) AS mean
      """
    Then the result should be, in order:
      | mean |
      | 36.8 |

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/aggregate-count-distinct (ISO subclauses 20.9, 22.12)
  @clause-20.9
  Scenario: [mandatory/query/aggregate-count-distinct] COUNT DISTINCT counts distinct values, not bindings
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
      RETURN COUNT(DISTINCT p.age) AS ages
      """
    Then the result should be, in order:
      | ages |
      | 4    |

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/implicit-grouping (ISO subclauses 14.11, 22.15)
  @clause-14.11 @clause-20.9
  Scenario: [mandatory/query/implicit-grouping] A return statement groups on its non-aggregated items without a group by clause
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
      RETURN p.city AS city, COUNT(*) AS n
      """
    Then the result should be, in any order:
      | city     | n |
      | 'Berlin' | 2 |
      | 'Paris'  | 2 |
      | 'Lisbon' | 1 |

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/aggregate-over-empty-input (ISO subclauses 20.9)
  @clause-20.9
  Scenario: [mandatory/query/aggregate-over-empty-input] COUNT over no bindings is zero and still returns one record
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
      WHERE p.age > 200
      RETURN COUNT(*) AS n
      """
    Then the result should be, in order:
      | n |
      | 0 |

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/match-with-no-results (ISO subclauses 14.4)
  @clause-14.4
  Scenario: [mandatory/query/match-with-no-results] A pattern that matches nothing yields no records and is not an error
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
      MATCH (x:Nonexistent)
      RETURN x.name AS name
      """
    Then the result should be empty

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/finish (ISO subclauses 14.10)
  @clause-14.10
  Scenario: [mandatory/query/finish] FINISH ends a query with no records rather than with an error
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
      FINISH
      """
    Then the result should be empty

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/return-without-match (ISO subclauses 14.3, 14.11)
  @clause-14.3 @clause-14.11
  Scenario: [mandatory/query/return-without-match] A linear query statement may be a return statement alone
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN 1 AS one
      """
    Then the result should be, in order:
      | one |
      | 1   |

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/order-by-as-a-statement (ISO subclauses 14.9)
  @clause-14.9
  Scenario: [mandatory/query/order-by-as-a-statement] An order by and page statement stands on its own inside a linear query statement
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
      ORDER BY p.age, p.name
      RETURN p.name AS name, p.age AS age
      """
    Then the result should be, in order:
      | name    | age |
      | 'Bob'   | 27  |
      | 'Dave'  | 27  |
      | 'Alice' | 34  |
      | 'Carol' | 41  |
      | 'Erin'  | 55  |

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/the-shape-of-a-program (ISO subclauses 6, 9, 9.1, 14, 14.1, 16.3, 22.8, 4.7.1, 4.7.3, 4.10.4.1, 4.10.4.3, 4.10.4.5)
  @clause-6 @clause-9.1 @clause-9.2 @clause-14.1 @clause-14.2 @clause-14.3 @clause-14.4 @clause-14.10 @clause-16.3
  Scenario: [mandatory/query/the-shape-of-a-program] The smallest useful GQL program is a query statement and the standard names every rule between the two
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
      WHERE p.city = 'Lisbon'
      RETURN p.name AS name
      """
    Then the result should be, in any order:
      | name   |
      | 'Dave' |

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/select-over-a-graph-match (ISO subclauses 14.12)
  @clause-14.12
  Scenario: [mandatory/query/select-over-a-graph-match] A select statement projects the rows a graph match binds
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
      SELECT p.name AS name, p.age AS age
      FROM CURRENT_PROPERTY_GRAPH MATCH (p:Person)
      WHERE p.age > 40
      """
    Then the result should be, in any order:
      | name    | age |
      | 'Carol' | 41  |
      | 'Erin'  | 55  |

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/select-from-a-nested-query (ISO subclauses 14.12)
  @clause-14.12
  Scenario: [mandatory/query/select-from-a-nested-query] A select statement may read from a nested query instead of from a graph
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
      SELECT *
      FROM { MATCH (p:Person) WHERE p.city = 'Lisbon' RETURN p.name AS name }
      """
    Then the result should be, in order:
      | name   |
      | 'Dave' |

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/select-from-two-graph-matches (ISO subclauses 14.12)
  @clause-14.12
  Scenario: [mandatory/query/select-from-two-graph-matches] A select statement body may list more than one graph match
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
      SELECT p.name AS person, c.name AS company
      FROM CURRENT_PROPERTY_GRAPH MATCH (p:Person),
           CURRENT_PROPERTY_GRAPH MATCH (c:Company)
      WHERE p.name = 'Alice' AND c.name = 'Acme'
      """
    Then the result should be, in order:
      | person  | company |
      | 'Alice' | 'Acme'  |

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/select-distinct (ISO subclauses 14.12)
  @clause-14.12
  Scenario: [mandatory/query/select-distinct] A set quantifier in a select statement drops the duplicate rows
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
      SELECT DISTINCT p.city AS city
      FROM CURRENT_PROPERTY_GRAPH MATCH (p:Person)
      """
    Then the result should be, in any order:
      | city     |
      | 'Berlin' |
      | 'Paris'  |
      | 'Lisbon' |

  # gql-compat @ 9de1991, corpus/suite/10-mandatory-query.yaml, case mandatory/query/select-having (ISO subclauses 14.12)
  @clause-14.12
  Scenario: [mandatory/query/select-having] A having clause tests the group a select statement aggregated
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
      SELECT COUNT(*) AS n
      FROM CURRENT_PROPERTY_GRAPH MATCH (p:Person)
      HAVING COUNT(*) > 3
      """
    Then the result should be, in order:
      | n |
      | 5 |
