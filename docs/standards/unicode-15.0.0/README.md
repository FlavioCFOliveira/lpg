# Unicode Character Database 15.0.0 (extract)

Two files of the Unicode Character Database, version 15.0.0, stored **unmodified**, and the Unicode licence that covers them. They are the input of the identifier tables of the GQL lexer (`src/query/lexer/unicode.rs`), which a test regenerates from them and fails on drift ([TS-19](../../technical-spec/query-engine.md#ts-19-parser), "Identifier characters").

Version 15.0.0 is the one the ANTLR tool jar of the test oracle uses (TS-19 evidence). Changing it needs the oracle's jar to change with it.

| File | Source | SHA-256 |
|---|---|---|
| `DerivedCoreProperties.txt` | <https://www.unicode.org/Public/15.0.0/ucd/DerivedCoreProperties.txt> | `d367290bc0867e6b484c68370530bdd1a08b6b32404601b8c7accaf83e05628d` |
| `DerivedGeneralCategory.txt` | <https://www.unicode.org/Public/15.0.0/ucd/extracted/DerivedGeneralCategory.txt> | `fe29a45c0882500e591140aaa5c4f5067e6a5d746806148af34400c48b9c06f9` |
| `license.txt` | <https://www.unicode.org/license.txt> | `e7a93b009565cfce55919a381437ac4db883e9da2126fa28b91d12732bc53d96` |

Downloaded on 2026-10-08. `DerivedCoreProperties.txt` is byte-identical to the copy the Round S Unicode probe compared with the jar's sets.
