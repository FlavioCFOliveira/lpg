# Translated from gql-compat @ 9de1991 (https://github.com/tamnd/gql-compat),
# corpus/suite/41-lexical.yaml, licensed Apache-2.0 (LICENSE-gql-compat in this
# directory; gql-compat states no copyright notice and has no NOTICE file).
# Changed: the cases are rewritten as scenarios of lpg's conformance runner,
# with their fixtures as INSERT statements, by
# docs/evidence/lab-sprint-3/gqlcompat_translate.py. Do not edit by hand.

Feature: gql-compat 41-lexical

  # gql-compat @ 9de1991, corpus/suite/41-lexical.yaml, case mandatory/lexical/escaped-reverse-solidus (ISO subclauses 21.2)
  @clause-21.2
  Scenario: [mandatory/lexical/escaped-reverse-solidus] Two reverse solidi in a string are one backslash
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN 'a\\b' AS v
      """
    Then the result should be, in order:
      | v        |
      | 'a\\\\b' |

  # gql-compat @ 9de1991, corpus/suite/41-lexical.yaml, case mandatory/lexical/escaped-quote (ISO subclauses 21.2)
  @clause-21.2
  Scenario: [mandatory/lexical/escaped-quote] A reverse solidus before a quote puts the quote in the string
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN 'it\'s' AS v
      """
    Then the result should be, in order:
      | v        |
      | 'it\\'s' |

  # gql-compat @ 9de1991, corpus/suite/41-lexical.yaml, case mandatory/lexical/doubled-quote (ISO subclauses 21.2)
  @clause-21.2
  Scenario: [mandatory/lexical/doubled-quote] A quote written twice is the other way to put one in a string
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN 'it''s' AS v
      """
    Then the result should be, in order:
      | v        |
      | 'it\\'s' |

  # gql-compat @ 9de1991, corpus/suite/41-lexical.yaml, case mandatory/lexical/escaped-control-characters (ISO subclauses 21.2, 20.22)
  @clause-20.22 @clause-21.2
  Scenario: [mandatory/lexical/escaped-control-characters] The five control escapes each stand for one character
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN CHARACTER_LENGTH('\t\n\r\f\b') AS n
      """
    Then the result should be, in order:
      | n |
      | 5 |

  # gql-compat @ 9de1991, corpus/suite/41-lexical.yaml, case mandatory/lexical/escaped-delimiters-of-the-other-kinds (ISO subclauses 21.2)
  @clause-21.2
  Scenario: [mandatory/lexical/escaped-delimiters-of-the-other-kinds] A string may hold the other two delimiters by escaping them
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN '\"\`' AS v
      """
    Then the result should be, in order:
      | v    |
      | '"`' |

  # gql-compat @ 9de1991, corpus/suite/41-lexical.yaml, case mandatory/lexical/unicode-escapes (ISO subclauses 21.2)
  @clause-21.2
  Scenario: [mandatory/lexical/unicode-escapes] The two unicode escapes name a character by four or six hex digits
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN 'A\U01F600' AS v
      """
    Then the result should be, in order:
      | v    |
      | 'A😀' |

  # gql-compat @ 9de1991, corpus/suite/41-lexical.yaml, case mandatory/lexical/decimal-integer-digits (ISO subclauses 21.2)
  @clause-21.2
  Scenario: [mandatory/lexical/decimal-integer-digits] An integer literal is a run of digits
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN 1902 AS v
      """
    Then the result should be, in order:
      | v    |
      | 1902 |

  # gql-compat @ 9de1991, corpus/suite/41-lexical.yaml, case mandatory/lexical/digit-group-separator (ISO subclauses 21.2)
  @clause-21.2
  Scenario: [mandatory/lexical/digit-group-separator] An underscore may stand between the digits of a number
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN 1_000 AS v
      """
    Then the result should be, in order:
      | v    |
      | 1000 |

  # gql-compat @ 9de1991, corpus/suite/41-lexical.yaml, case mandatory/lexical/doubled-double-quote-in-a-name (ISO subclauses 21.3)
  @clause-21.3
  Scenario: [mandatory/lexical/doubled-double-quote-in-a-name] A double quote inside a double-quoted name is written twice
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN 1 AS "a""b"
      """
    Then the result should be, in order:
      | a"b |
      | 1   |

  # gql-compat @ 9de1991, corpus/suite/41-lexical.yaml, case mandatory/lexical/doubled-grave-accent-in-a-name (ISO subclauses 21.3)
  @clause-21.3
  Scenario: [mandatory/lexical/doubled-grave-accent-in-a-name] A grave accent inside an accent-quoted name is written twice
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN 1 AS `a``b`
      """
    Then the result should be, in order:
      | a`b |
      | 1   |

  # gql-compat @ 9de1991, corpus/suite/41-lexical.yaml, case mandatory/lexical/the-punctuation-of-an-expression (ISO subclauses 21.3, 21.4)
  @clause-20.1 @clause-21.3 @clause-21.4
  Scenario: [mandatory/lexical/the-punctuation-of-an-expression] The operators and separators of an expression are delimiter tokens
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN (1 + 2) * 3 - 4 AS a, 12 / 4 AS b
      """
    Then the result should be, in order:
      | a | b |
      | 5 | 3 |

  # gql-compat @ 9de1991, corpus/suite/41-lexical.yaml, case mandatory/lexical/the-words-of-a-statement (ISO subclauses 21.3, 21.4)
  @clause-21.3 @clause-21.4
  Scenario: [mandatory/lexical/the-words-of-a-statement] A statement is keywords and identifiers written in Latin letters
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      MATCH (u:Solo)
      RETURN 1 AS v
      """
    Then the result should be, in order:
      | v |
      | 1 |

  # gql-compat @ 9de1991, corpus/suite/41-lexical.yaml, case mandatory/lexical/characters-outside-the-latin-alphabet (ISO subclauses 21.3, 21.2)
  @clause-21.2 @clause-21.3
  Scenario: [mandatory/lexical/characters-outside-the-latin-alphabet] A string may hold letters and digits from any script
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      RETURN 'café٣' AS v
      """
    Then the result should be, in order:
      | v       |
      | 'café٣' |

  # gql-compat @ 9de1991, corpus/suite/41-lexical.yaml, case mandatory/lexical/an-unquoted-name-is-a-non-delimited-identifier (ISO subclauses 21.1, 21.3)
  @clause-21.1 @clause-21.3
  Scenario: [mandatory/lexical/an-unquoted-name-is-a-non-delimited-identifier] An identifier written without delimiters is a non-delimited identifier
    Given an empty graph
    And having executed:
      """
      INSERT (u:Solo)
      """
    When executing query:
      """
      MATCH (u:Solo)
      RETURN 1 AS alias
      """
    Then the result should be, in order:
      | alias |
      | 1     |
