# Translated from gql-compat @ 9de1991 (https://github.com/tamnd/gql-compat),
# corpus/suite/12-mandatory-value.yaml, licensed Apache-2.0 (LICENSE-gql-compat in this
# directory; gql-compat states no copyright notice and has no NOTICE file).
# Changed: the cases are rewritten as scenarios of lpg's conformance runner,
# with their fixtures as INSERT statements, by
# docs/evidence/lab-sprint-3/gqlcompat_translate.py. Do not edit by hand.

Feature: gql-compat 12-mandatory-value

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/literal-integer (ISO subclauses 21.2, 4.4.1)
  @clause-21.2
  Scenario: [mandatory/value/literal-integer] An unsigned integer literal is returned as the number it spells
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN 42 AS v
      """
    Then the result should be, in order:
      | v  |
      | 42 |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/literal-character-string (ISO subclauses 21.2)
  @clause-21.2
  Scenario: [mandatory/value/literal-character-string] A character string literal in single quotes is returned unchanged
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN 'hello' AS v
      """
    Then the result should be, in order:
      | v       |
      | 'hello' |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/literal-boolean (ISO subclauses 21.2)
  @clause-21.2
  Scenario: [mandatory/value/literal-boolean] TRUE and FALSE are boolean literals
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN TRUE AS t, FALSE AS f
      """
    Then the result should be, in order:
      | t    | f     |
      | true | false |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/literal-null (ISO subclauses 21.2, 4.4.5)
  @clause-21.2
  Scenario: [mandatory/value/literal-null] NULL is a literal and survives projection as the null value
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN NULL AS v
      """
    Then the result should be, in order:
      | v    |
      | null |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/bracketed-comment (ISO subclauses 21.3)
  @clause-21.3
  Scenario: [mandatory/value/bracketed-comment] Text between a solidus-asterisk pair is a comment
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN /* a comment */ 1 AS v
      """
    Then the result should be, in order:
      | v |
      | 1 |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/arithmetic-precedence (ISO subclauses 20.21)
  @clause-20.21
  Scenario: [mandatory/value/arithmetic-precedence] Multiplication binds tighter than addition in a numeric value expression
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN 2 + 3 * 4 AS v
      """
    Then the result should be, in order:
      | v  |
      | 14 |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/arithmetic-parentheses (ISO subclauses 20.21)
  @clause-20.2 @clause-20.21
  Scenario: [mandatory/value/arithmetic-parentheses] Parentheses override operator precedence
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN (2 + 3) * 4 AS v
      """
    Then the result should be, in order:
      | v  |
      | 20 |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/arithmetic-negation (ISO subclauses 20.21)
  @clause-20.21
  Scenario: [mandatory/value/arithmetic-negation] A unary minus negates a numeric value expression
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN 7 + -2 AS v
      """
    Then the result should be, in order:
      | v |
      | 5 |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/string-concatenation (ISO subclauses 20.23)
  @clause-20.23
  Scenario: [mandatory/value/string-concatenation] The concatenation operator joins two character strings
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN 'ab' || 'cd' AS v
      """
    Then the result should be, in order:
      | v      |
      | 'abcd' |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/fold (ISO subclauses 20.24)
  @clause-20.24
  Scenario: [mandatory/value/fold] UPPER and LOWER change the case of a character string
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN UPPER('aB') AS u, LOWER('aB') AS l
      """
    Then the result should be, in order:
      | u    | l    |
      | 'AB' | 'ab' |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/char-length (ISO subclauses 20.22)
  @clause-20.22
  Scenario: [mandatory/value/char-length] CHAR_LENGTH counts the characters of a string
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN CHAR_LENGTH('abcd') AS n
      """
    Then the result should be, in order:
      | n |
      | 4 |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/trim-default (ISO subclauses 20.24)
  @clause-20.24
  Scenario: [mandatory/value/trim-default] TRIM with only a source removes spaces from both ends
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN TRIM('  ab  ') AS v
      """
    Then the result should be, in order:
      | v    |
      | 'ab' |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/normalize (ISO subclauses 20.24, 19.7)
  @clause-19.7 @clause-20.24 @clause-21.3
  Scenario: [mandatory/value/normalize] NORMALIZE returns a string that is already in normal form unchanged
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN NORMALIZE('ab', NFC) AS v
      """
    Then the result should be, in order:
      | v    |
      | 'ab' |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/normalized-predicate (ISO subclauses 19.7)
  @clause-19.7
  Scenario: [mandatory/value/normalized-predicate] IS NORMALIZED asks of a string what NORMALIZE would do to it
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN ('ab' IS NFC NORMALIZED) AS a, ('ab' IS NOT NORMALIZED) AS b
      """
    Then the result should be, in order:
      | a    | b     |
      | true | false |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/session-user (ISO subclauses 20.3, 4.2.4.1, 4.2.4.2)
  @clause-20.3
  Scenario: [mandatory/value/session-user] SESSION_USER is a value specification every session can write
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN SESSION_USER AS u
      """
    Then the statement succeeds

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/comparison-yields-boolean (ISO subclauses 19.3)
  @clause-19.3
  Scenario: [mandatory/value/comparison-yields-boolean] A comparison predicate is a value expression and projects a boolean
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN 1 < 2 AS lt, 2 <= 2 AS le, 3 <> 4 AS ne, 4 > 3 AS gt, 4 >= 4 AS ge, 5 = 5 AS eq
      """
    Then the result should be, in order:
      | lt   | le   | ne   | gt   | ge   | eq   |
      | true | true | true | true | true | true |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/numeric-comparison-across-types (ISO subclauses 22.13, 4.4.2)
  @clause-19.3
  Scenario: [mandatory/value/numeric-comparison-across-types] A number with a fractional part compares against an integer without a cast
    Given an empty graph
    And having executed:
      """
      INSERT (v1:Datum {n: 1, x: 0.5e0, flag: TRUE, tag: 'alpha'}),
             (v2:Datum {n: 2, x: 1.5e0, flag: FALSE, tag: 'beta'}),
             (v3:Datum {n: 3, x: 2.5e0, flag: TRUE, tag: 'gamma'}),
             (v4:Datum {n: 4, x: 3.5e0, flag: FALSE, tag: 'delta'})
      """
    When executing query:
      """
      MATCH (v:Datum)
      WHERE v.x > 2
      RETURN v.n AS n
      """
    Then the result should be, in any order:
      | n |
      | 3 |
      | 4 |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/null-propagates-through-arithmetic (ISO subclauses 20.21, 4.4.5)
  @clause-20.21
  Scenario: [mandatory/value/null-propagates-through-arithmetic] An arithmetic operand that is null makes the result null
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN 1 + NULL AS v
      """
    Then the result should be, in order:
      | v    |
      | null |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/boolean-operators (ISO subclauses 20.20)
  @clause-20.20
  Scenario: [mandatory/value/boolean-operators] AND, OR and NOT combine boolean values
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN (TRUE AND FALSE) AS a, (TRUE OR FALSE) AS b, (NOT TRUE) AS c
      """
    Then the result should be, in order:
      | a     | b    | c     |
      | false | true | false |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/three-valued-logic (ISO subclauses 20.20, 4.4.5)
  @clause-20.20
  Scenario: [mandatory/value/three-valued-logic] A boolean operand that is unknown gives an unknown result, not false
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN TRUE AND NULL AS v
      """
    Then the result should be, in order:
      | v    |
      | null |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/three-valued-logic-short-circuit (ISO subclauses 20.20)
  @clause-20.20
  Scenario: [mandatory/value/three-valued-logic-short-circuit] FALSE AND unknown is false, because no operand can make it true
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN FALSE AND NULL AS v
      """
    Then the result should be, in order:
      | v     |
      | false |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/parenthesized-boolean-value-expression (ISO subclauses 20.20)
  @clause-20.20
  Scenario: [mandatory/value/parenthesized-boolean-value-expression] Parentheses around a boolean expression make it an operand again
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN (TRUE OR FALSE) AND TRUE AS v
      """
    Then the result should be, in order:
      | v    |
      | true |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/boolean-test-is-true (ISO subclauses 20.20, 4.4.5)
  @clause-20.20
  Scenario: [mandatory/value/boolean-test-is-true] IS TRUE asks a three-valued expression for a two-valued answer
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN (1 = 1) IS TRUE AS v
      """
    Then the result should be, in order:
      | v    |
      | true |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/boolean-test-is-not-false (ISO subclauses 20.20, 4.4.5)
  @clause-20.20
  Scenario: [mandatory/value/boolean-test-is-not-false] NOT inverts the truth value a boolean test compares against
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN (1 = 1) IS NOT FALSE AS v
      """
    Then the result should be, in order:
      | v    |
      | true |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/boolean-test-is-unknown (ISO subclauses 20.20, 4.4.5)
  @clause-20.20
  Scenario: [mandatory/value/boolean-test-is-unknown] UNKNOWN is a truth value a boolean test may compare against
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN (NULL = 1) IS UNKNOWN AS v
      """
    Then the result should be, in order:
      | v    |
      | true |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/case-searched (ISO subclauses 20.7)
  @clause-20.7
  Scenario: [mandatory/value/case-searched] A searched case expression returns the result of its first true when clause
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN CASE WHEN 1 > 2 THEN 'no' WHEN 1 < 2 THEN 'yes' ELSE 'neither' END AS v
      """
    Then the result should be, in order:
      | v     |
      | 'yes' |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/case-simple (ISO subclauses 20.7)
  @clause-20.7
  Scenario: [mandatory/value/case-simple] A simple case expression compares its operand against each when clause
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN CASE 2 WHEN 1 THEN 'one' WHEN 2 THEN 'two' ELSE 'other' END AS v
      """
    Then the result should be, in order:
      | v     |
      | 'two' |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/case-when-operand-list (ISO subclauses 20.7)
  @clause-20.7
  Scenario: [mandatory/value/case-when-operand-list] One when clause may hold several operands separated by commas
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN CASE 1 WHEN 1, 2 THEN 'yes' ELSE 'no' END AS v
      """
    Then the result should be, in order:
      | v     |
      | 'yes' |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/case-when-operand-is-a-comparison (ISO subclauses 20.7, 19.4)
  @clause-19.4 @clause-20.7
  Scenario: [mandatory/value/case-when-operand-is-a-comparison] A when operand may be the right half of a predicate rather than a value
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN CASE 3 WHEN > 1 THEN 'yes' ELSE 'no' END AS v
      """
    Then the result should be, in order:
      | v     |
      | 'yes' |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/case-result-is-null (ISO subclauses 20.7)
  @clause-20.7
  Scenario: [mandatory/value/case-result-is-null] The result of a when clause may be the null literal
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN CASE 1 WHEN 1 THEN NULL ELSE 'other' END AS v
      """
    Then the result should be, in order:
      | v    |
      | null |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/case-else-omitted (ISO subclauses 20.7)
  @clause-20.7
  Scenario: [mandatory/value/case-else-omitted] A case expression with no matching when clause and no else is null
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN CASE WHEN 1 > 2 THEN 'no' END AS v
      """
    Then the result should be, in order:
      | v    |
      | null |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/coalesce (ISO subclauses 20.7)
  @clause-20.7
  Scenario: [mandatory/value/coalesce] COALESCE returns its first operand that is not null
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN COALESCE(NULL, 'x', 'y') AS v
      """
    Then the result should be, in order:
      | v   |
      | 'x' |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/nullif (ISO subclauses 20.7)
  @clause-20.7
  Scenario: [mandatory/value/nullif] NULLIF returns null when its operands are equal
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN NULLIF(1, 1) AS alike, NULLIF(1, 2) AS unlike
      """
    Then the result should be, in order:
      | alike | unlike |
      | null  | 1      |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/is-null-predicate (ISO subclauses 19.5)
  @clause-19.5
  Scenario: [mandatory/value/is-null-predicate] IS NULL is true of a property whose value is null
    Given an empty graph
    And having executed:
      """
      INSERT (p1:Maybe {id: 1, note: 'present'}),
             (p2:Maybe {id: 2, note: NULL})
      """
    When executing query:
      """
      MATCH (m:Maybe)
      WHERE m.note IS NULL
      RETURN m.id AS id
      """
    Then the result should be, in any order:
      | id |
      | 2  |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/is-not-null-predicate (ISO subclauses 19.5)
  @clause-19.5
  Scenario: [mandatory/value/is-not-null-predicate] IS NOT NULL is true of a property that has a value
    Given an empty graph
    And having executed:
      """
      INSERT (p1:Maybe {id: 1, note: 'present'}),
             (p2:Maybe {id: 2, note: NULL})
      """
    When executing query:
      """
      MATCH (m:Maybe)
      WHERE m.note IS NOT NULL
      RETURN m.id AS id
      """
    Then the result should be, in any order:
      | id |
      | 1  |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/comparison-with-null-is-unknown (ISO subclauses 19.3, 22.13)
  @clause-19.3
  Scenario: [mandatory/value/comparison-with-null-is-unknown] A comparison against null is unknown, so it filters the binding out
    Given an empty graph
    And having executed:
      """
      INSERT (p1:Maybe {id: 1, note: 'present'}),
             (p2:Maybe {id: 2, note: NULL})
      """
    When executing query:
      """
      MATCH (m:Maybe)
      WHERE m.note = 'present'
      RETURN m.id AS id
      """
    Then the result should be, in any order:
      | id |
      | 1  |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/string-ordering (ISO subclauses 22.14, 22.16)
  @clause-19.3
  Scenario: [mandatory/value/string-ordering] Character strings compare in the order the collation gives them
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
      WHERE p.name < 'C'
      RETURN p.name AS name
      """
    Then the result should be, in any order:
      | name    |
      | 'Alice' |
      | 'Bob'   |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/parameter-reference (ISO subclauses 20.4)
  @clause-20.4
  Scenario: [mandatory/value/parameter-reference] A general parameter reference binds a value supplied outside the statement
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    And parameters are:
      | threshold | 40 |
    When executing query:
      """
      RETURN $threshold AS v
      """
    Then the result should be, in order:
      | v  |
      | 40 |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/parameter-in-predicate (ISO subclauses 20.3, 20.4)
  @clause-20.3 @clause-20.4
  Scenario: [mandatory/value/parameter-in-predicate] A parameter is usable wherever a value specification is
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
    And parameters are:
      | threshold | 40 |
    When executing query:
      """
      MATCH (p:Person)
      WHERE p.age > $threshold
      RETURN p.name AS name
      """
    Then the result should be, in any order:
      | name    |
      | 'Carol' |
      | 'Erin'  |

  # gql-compat @ 9de1991, corpus/suite/12-mandatory-value.yaml, case mandatory/value/property-of-absent-key (ISO subclauses 20.11, 4.3.4)
  @clause-20.11
  Scenario: [mandatory/value/property-of-absent-key] Reading a property an element does not have gives null, not an error
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
      WHERE p.name = 'Alice'
      RETURN p.nickname AS v
      """
    Then the result should be, in order:
      | v    |
      | null |
