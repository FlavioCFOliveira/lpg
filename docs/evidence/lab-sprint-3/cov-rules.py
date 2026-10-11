#!/usr/bin/env python3
"""Coverage per grammar-rule function of the GQL parser (rmp #41).

Usage:
    python3 -I docs/evidence/lab-sprint-3/cov-functions.py cov.json src/query/ > cov-query.tsv
    python3 -I docs/evidence/lab-sprint-3/cov-rules.py cov-query.tsv tools/gql-oracle/grammar/GQL.g4 > table.md

Reads the per-function TSV of cov-functions.py, keeps the functions of
src/query/parser/ and src/query/lexer/mod.rs (tests excluded), merges
closures and generic instantiations into their function, and marks a
function as a rule function when its name is the snake_case of a parser
rule of GQL.g4. Prints a Markdown table, the rule functions first, and
exits 1 if a rule function was never executed. Own code (MIT, lpg).
"""

import re
import sys


def snake(rule: str) -> str:
    return re.sub(r"(?<=[a-z0-9])([A-Z])", r"_\1", rule).lower()


def parser_rules(g4: str) -> set:
    """The parser rules: GQL.g4 writes each name alone on its line."""
    text = open(g4, encoding="utf-8").read()
    return set(re.findall(r"^([a-z][A-Za-z0-9_]*)[ \t]*$", text, re.M))


def main() -> None:
    tsv, g4 = sys.argv[1], sys.argv[2]
    rules = {snake(r): r for r in parser_rules(g4)}
    funcs = {}
    for line in open(tsv, encoding="utf-8").read().splitlines()[1:]:
        file, name, count, covered, total = line.split("\t")
        short = file[file.index("src/"):]
        if not (short.startswith("src/query/parser/") or short == "src/query/lexer/mod.rs"):
            continue
        if short.endswith(("tests.rs", "differential.rs")):
            continue
        base = re.sub(r"::\{closure#\d+\}", "", name)
        base = re.sub(r"::<.*>$", "", base)
        fn = base.split("::")[-1]
        key = (short, fn)
        c, cov, tot = funcs.get(key, (0, 0, 0))
        # A closure's count is not the function's; keep the function's.
        own = base == re.sub(r"::<.*>$", "", name)
        funcs[key] = (c + (int(count) if own else 0), cov + int(covered), tot + int(total))
    rows = sorted(funcs.items(), key=lambda kv: (kv[0][1] not in rules, kv[0][0], kv[0][1]))
    n_rule = sum(1 for (f, fn) in funcs if fn in rules)
    unexecuted = [(f, fn) for (f, fn), (c, _, _) in funcs.items() if fn in rules and c == 0]
    print(f"Rule functions: {n_rule}; never executed: {len(unexecuted)}. "
          f"Other functions (helpers): {len(funcs) - n_rule}. "
          f"GQL.g4 parser rules: {len(rules)}; without a function of their own: "
          f"{len(set(rules) - {fn for (_, fn) in funcs})}.\n")
    print("| File | Function | GQL.g4 rule | Executions | Regions covered / total |")
    print("|---|---|---|---:|---:|")
    for (f, fn), (c, cov, tot) in rows:
        print(f"| `{f}` | `{fn}` | {('`' + rules[fn] + '`') if fn in rules else 'helper'} | {c} | {cov} / {tot} |")
    absent = sorted(rules[r] for r in set(rules) - {fn for (_, fn) in funcs})
    print(f"\nGQL.g4 parser rules parsed inside another function ({len(absent)}): "
          + ", ".join(f"`{r}`" for r in absent) + ".")
    sys.exit(1 if unexecuted else 0)


if __name__ == "__main__":
    main()
