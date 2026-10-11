# GQL texts of lpg's tests

Every GQL text that lpg's tests write as a string literal in a call to one of the test helpers listed in `src/query/parser/differential.rs` (`TEXT_HELPERS`), one file each, named by the 64-bit FNV-1a hash of the text. The test `test_texts_match_the_oracle_labels` collects the texts from `src/` and fails when this directory differs; `LPG_UPDATE_TEST_TEXTS=1` rewrites it.

`labels.tsv` is the output of `gql-oracle label src/query/testdata/tests` (the ANTLR oracle of `tools/gql-oracle`), without the lines of this README and `labels.tsv`; `LPG_UPDATE_LABELS=1` on `differential_stored_labels_are_current` rewrites it.
