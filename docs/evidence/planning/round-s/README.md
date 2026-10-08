# Round S measurements (Sprint 3, tasks #37 and #39), 2026-10-08

Own scripts (MIT, lpg project). They read the reference clones listed in the [technical specification sources](../../../technical-spec/README.md#sources); nothing here is copied from them.

| File | What it does |
|---|---|
| `g4.py` | Reads the parser rules of `GQL.g4` (alternatives, labels, ISO clause comments). |
| `anchors.tsv` | One line per code of the ISO features insert: the `GQL.g4` rules, alternatives or elements the code covers, with the evidence of each anchor. |
| `feature_map.py` | Checks every anchor against `GQL.g4`, the features insert and gql-compat; classifies every parser rule; writes `rules.tsv` and `table.md`. |
| `rules.tsv`, `table.md` | Output of `feature_map.py`: one row per rule (exact conditions), and the grouped table copied into [query-engine.md](../../../technical-spec/query-engine.md#rule-to-feature-table). |
| `GqlProbe.java` | Parses texts with the ANTLR Java target generated from the unmodified `GQL.g4` and lists the rules, alternatives and elements each parse uses; optionally the ambiguities ANTLR reports. |
| `tck_select.py` | Applies the TS-30 selection rule to opengql/tck, the openCypher TCK and gql-compat, and counts. |
| `ambiguities.py` | Lists the ambiguous `GQL.g4` decisions on the samples and the three test corpora. |
| `keywords.py` | Compares the keyword sets of `GQL.g4` with the ISO BNF insert. |
| `unicode/` | Finds the Unicode version of the `\p{ID_Start}` and `\p{ID_Continue}` sets an ANTLR tool jar generates. |

## Method of the rule table

1. **Placement rule** (gql-compat README @ `9de1991`, not ISO text; provisional, OQ-005): a construct that a features-insert code covers is optional; every other construct is mandatory.
2. **Anchors.** Each code is anchored to the narrowest `GQL.g4` rule, top-level alternative or element that its insert description names, or that the gql-compat cases citing the code exercise (`gc:` evidence, checked against the corpus by `feature_map.py`). A code whose construct is semantic, lexical or a refinement of another code has no anchor (`-`).
3. **Classification.** A rule is *mandatory* when it is reachable from `gqlProgram` through unanchored alternatives and derives a sentence without an anchored construct. Every other rule gets the codes it needs: its own condition (exact, `+` = all of, `or` = any of) and the codes of the constructs it is reachable only inside. A rule is *supported* when one of its conditions uses approved codes only.
4. **Cross-check.** The probe parses every gql-compat case; the anchors need no code for 106 of its 107 mandatory cases. The exception, `mandatory/value/list-concatenation`, writes a list constructor, which gql-compat's own GV50 cases cite under GV50, and which the opengql/tck README's statement of minimum conformance does not list among the required data types.

## Reproduce

Clones as in the specification sources; a JDK (21.0.11 was used) in `JAVA_HOME`; PyYAML; the ANTLR tool jar `antlr-4.13.2-complete.jar` (found at `/tmp/minigu/minigu-test/benches/antlr4/`) for the probe, `antlr4-4.13.3-SNAPSHOT-complete.jar` of antlr4rust release v0.6.0 (SHA-256 `391e77311794f7c8fed2bc6d291f54b04725e37456d79e1981edfd3fc919b677`) for the Unicode probe. `WORK` is a scratch directory outside the repository.

```
python3 -I -B feature_map.py /tmp/opengql-grammar/GQL.g4 "../../../standards/iso-iec-39075/ed-1/ISO_IEC_39075(en)-features.xml" anchors.tsv /tmp/gql-compat .
python3 -I -B tck_select.py /tmp/opengql-grammar/GQL.g4 "../../../standards/iso-iec-39075/ed-1/ISO_IEC_39075(en)-features.xml" anchors.tsv <antlr-4.13.2-complete.jar> $WORK /tmp/opengql-tck /tmp/opencypher /tmp/gql-compat
python3 -I -B ambiguities.py /tmp/opengql-grammar/GQL.g4 <antlr-4.13.2-complete.jar> $WORK /tmp/opengql-grammar ../../antlr-exp/corpus /tmp/opengql-tck /tmp/opencypher /tmp/gql-compat
python3 -I -B keywords.py /tmp/opengql-grammar/GQL.g4 "../../../standards/iso-iec-39075/ed-1/ISO_IEC_39075(en).bnf.txt"
# unicode/: java -jar <antlr4rust jar> -Dlanguage=Java U.g4; javac; java Dump > jar-sets.txt;
# then python3 -I -B unicode/compare.py jar-sets.txt DerivedCoreProperties-<v>.txt ... (13.0.0, 14.0.0, 15.0.0, 15.1.0, 16.0.0)
```

## Results (2026-10-08)

* `feature_map.py`, with GG03 approved (user decision 2026-10-08, "Só GG03"): 574 parser rules; 165 mandatory; 342 supported with the approved set; 229 need an unapproved code; 3 unreachable from `gqlProgram`. 175 codes have anchors, 53 have none. (Before GG03: 267 and 304.)
* `tck_select.py`: see [TS-30](../../../technical-spec/quality-and-testing.md#ts-30-testing-and-conformance).
* `ambiguities.py`: ten ambiguous decisions; see [TS-19](../../../technical-spec/query-engine.md#ts-19-parser).
* `keywords.py`: see TS-19.
* `unicode/`: the antlr4rust v0.6.0 jar generates 136 345 `ID_Start` and 139 482 `ID_Continue` code points, equal to Unicode 15.0.0 and different from 13.0.0, 14.0.0, 15.1.0 and 16.0.0.
