# miniGU parser test inputs

The 142 inputs of the `parse!` calls in the tests of miniGU @ `fc2ccf5` (<https://github.com/TuGraph-family/miniGU>, Copyright 2022 AntGroup CO., Ltd., Apache-2.0, `LICENSE-miniGU`), `minigu/gql/parser/src/parser/impls/*.rs`, one file each: `<n>-<source file>-<line>-<miniGU rule>.gql`. The texts are unmodified. miniGU parses each with the rule named in the file name; lpg and the oracle parse each as a whole `gqlProgram`, so most are rejected by both.

`labels.tsv` is the output of `gql-oracle label src/query/testdata/minigu` (the ANTLR oracle of `tools/gql-oracle`), without the lines of this README, the licence and `labels.tsv`.
