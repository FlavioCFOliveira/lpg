"""Translate the selected gql-compat cases into lpg's own scenarios (rmp #52).

User decision 2026-10-08 (rmp #37, "Como input, traduzido"): gql-compat's mandatory and
condition cases feed lpg's own scenarios, translated and attributed. This script reads the
cases that `tck_manifest.py` selected (`tests/tck/selection.tsv`, artefact `gql-compat`, state
`selected`) and writes one `.feature` file per gql-compat suite file under
`tests/tck/lpg/features/gql-compat/`, in the step vocabulary of lpg's conformance runner
(`src/tck/run.rs`):

* the case's fixture (data, `corpus/suite/00-fixtures.yaml`) becomes one `INSERT` statement in
  a `having executed:` step; `params` becomes `parameters are:`; each `setup` statement becomes
  a `having executed:` step;
* the query becomes `executing query:`;
* `expect` becomes `the result should be, in any order:` (`unordered: true`) or `in order:`,
  `the result should be empty`, `the statement succeeds`, or `an exception condition should
  be raised:` with the case's code and its `also_gqlstatus` codes joined by ` or `;
* the tags name the case's ISO subclauses that are `GQL.g4` subclause headings, and the
  headings of its `productions`, as `@clause-N.N`.

A case is not translated when its condition cannot arise in lpg, and the reason is printed:
`unless` names a feature lpg supports; `limit` names an implementation-defined item whose
approved value makes the condition unreachable; `fault` needs a transport (the API is
in-process).

Usage:
  python3 -I -B gqlcompat_translate.py GQL_COMPAT_DIR SELECTION_TSV GQL_G4 OUT_DIR
Own code (MIT, lpg project); PyYAML for the case files.
"""
import csv
import datetime
import os
import re
import sys

import yaml

COMMIT = "9de1991"
SUPPORTED = {"GC01", "GC04", "GD01", "GG01", "GG02", "GG03", "GP04", "GP18", "GQ01", "GT01",
             "GT03", "GV39", "GV40", "GV41"}
# Implementation-defined items whose approved value makes a `limit` case unreachable
# (functional spec BR-GQL-002): IL001, no minimum label-set cardinality for a node, applies only
# to the node case; IW014, two identifiers are confusable only if equal, so 42004 never arises.
class Loader(yaml.SafeLoader):
    """YAML 1.2 booleans: only `true` and `false` (PyYAML follows YAML 1.1, where `yes` and
    `no` are booleans; gql-compat's expected cells use `yes` as a string)."""


Loader.yaml_implicit_resolvers = {
    k: [(tag, rx) for tag, rx in v if tag != "tag:yaml.org,2002:bool"]
    for k, v in yaml.SafeLoader.yaml_implicit_resolvers.items()}
Loader.add_implicit_resolver("tag:yaml.org,2002:bool", re.compile(r"^(?:true|True|TRUE|false|False|FALSE)$"),
                             list("tTfF"))

UNREACHABLE_LIMIT = {("IL001", "condition/22g0n/node-with-no-labels"),
                     ("IW014", "condition/42004/two-identifiers-that-look-the-same")}


def gql_value(v):
    """A property value of a fixture, written in approved GQL."""
    if v is None:
        return "NULL"
    if isinstance(v, bool):
        return "TRUE" if v else "FALSE"
    if isinstance(v, int):
        return str(v)
    if isinstance(v, float):
        # `1.5` needs GL04; the exponent form `1.5e0` does not (OQ-005).
        return repr(v) + "e0"
    if isinstance(v, str):
        return "'" + v.replace("\\", "\\\\").replace("'", "\\'") + "'"
    raise SystemExit(f"fixture value {v!r} has no approved GQL form")


def props(p):
    if not p:
        return ""
    return " {" + ", ".join(f"{k}: {gql_value(v)}" for k, v in p.items()) + "}"


def fixture_insert(fx):
    """One INSERT statement for a fixture, or None for an empty one."""
    items = []
    for n in fx.get("nodes") or []:
        labels = ":" + "&".join(n["labels"]) if n.get("labels") else ""
        items.append(f"({n['key']}{labels}{props(n.get('props'))})")
    for e in fx.get("edges") or []:
        if e.get("undirected"):
            raise SystemExit("undirected fixture edges need GH02")
        items.append(f"({e['from']})-[:{e['type']}{props(e.get('props'))}]->({e['to']})")
    if not items:
        return None
    return "INSERT " + ",\n       ".join(items)


def cell(v):
    """A value of an expected row, in the TCK table notation."""
    if v is None:
        return "null"
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, (int, float)):
        return repr(v)
    if isinstance(v, (datetime.date, datetime.datetime)):
        return "'" + v.isoformat() + "'"
    if isinstance(v, str):
        # The value literal escapes `\` and `'`; the Gherkin table cell then escapes `\`
        # and `|` again.
        lit = "'" + v.replace("\\", "\\\\").replace("'", "\\'") + "'"
        return lit.replace("\\", "\\\\").replace("|", "\\|")
    if isinstance(v, list):
        return "[" + ", ".join(cell(x) for x in v) + "]"
    raise SystemExit(f"expected value {v!r} has no table form")


def docstring(text, indent="      "):
    body = "\n".join(indent + line if line else "" for line in text.rstrip("\n").split("\n"))
    return f'{indent}"""\n{body}\n{indent}"""\n'


def table(rows, indent="      "):
    widths = [max(len(r[i]) for r in rows) for i in range(len(rows[0]))]
    return "".join(indent + "| " + " | ".join(c.ljust(w) for c, w in zip(r, widths)) + " |\n"
                   for r in rows)


def main():
    gcdir, sel_path, g4_path, out = sys.argv[1:5]
    selected = {r["scenario"] for r in csv.DictReader(open(sel_path, encoding="utf-8"),
                                                       delimiter="\t")
                if r["artefact"] == "gql-compat" and r["state"] == "selected"}
    headings = {}
    for line in open(g4_path, encoding="utf-8"):
        m = re.match(r"// (\d+(?:\.\d+)*) (.*)$", line.rstrip("\n"))
        if m:
            headings[m.group(1)] = m.group(2)
    suite = os.path.join(gcdir, "corpus", "suite")
    fixtures = {f["name"]: f for f in
                yaml.load(open(os.path.join(suite, "00-fixtures.yaml")), Loader=Loader)["fixtures"]}
    os.makedirs(out, exist_ok=True)
    for old in os.listdir(out):
        if old.endswith(".feature"):
            os.remove(os.path.join(out, old))
    written, skipped = 0, []
    for fn in sorted(os.listdir(suite)):
        d = yaml.load(open(os.path.join(suite, fn), encoding="utf-8"), Loader=Loader)
        defaults = d.get("defaults") or {}
        scenarios = []
        for c in d.get("cases") or []:
            if c["id"] not in selected:
                continue
            if c.get("unless") in SUPPORTED:
                skipped.append((c["id"], f"unless {c['unless']}: lpg supports {c['unless']}"))
                continue
            if c.get("limit") and (c["limit"], c["id"]) in UNREACHABLE_LIMIT:
                skipped.append((c["id"], f"limit {c['limit']}: unreachable under lpg's approved value"))
                continue
            if c.get("fault"):
                skipped.append((c["id"], f"fault {c['fault']}: needs a transport"))
                continue
            fx_name = c.get("fixture", defaults.get("fixture"))
            clauses = [s for s in c.get("subclauses", []) if s in headings]
            for p in c.get("productions", []):
                clauses += [n for n, h in headings.items() if f"<{p}>" in h]
            clauses = sorted(set(clauses), key=lambda s: [int(x) for x in s.split(".")])
            if not clauses:
                raise SystemExit(f"{c['id']}: no GQL.g4 subclause")
            s = f"  # gql-compat @ {COMMIT}, corpus/suite/{fn}, case {c['id']}"
            s += f" (ISO subclauses {', '.join(c.get('subclauses', []))})\n"
            s += "  " + " ".join(f"@clause-{n}" for n in clauses) + "\n"
            s += f"  Scenario: [{c['id']}] {' '.join(c['name'].split())}\n"
            s += "    Given an empty graph\n"
            if fx_name:
                ins = fixture_insert(fixtures[fx_name])
                if ins:
                    s += "    And having executed:\n" + docstring(ins)
            if c.get("params"):
                s += "    And parameters are:\n"
                s += table([[k, cell(v)] for k, v in c["params"].items()])
            for st in c.get("setup") or []:
                s += "    And having executed:\n" + docstring(st)
            s += "    When executing query:\n" + docstring(c["query"])
            e = c["expect"]
            kind = e["kind"]
            if kind == "rows":
                order = "in any order" if e.get("unordered") else "in order"
                rows = [list(e["columns"])] + [[cell(v) for v in r] for r in e.get("rows") or []]
                s += f"    Then the result should be, {order}:\n" + table(rows)
            elif kind == "empty":
                s += "    Then the result should be empty\n"
            elif kind == "accept":
                s += "    Then the statement succeeds\n"
            elif kind == "error":
                codes = [e["gqlstatus"]] + list(e.get("also_gqlstatus") or [])
                s += f"    Then an exception condition should be raised: {' or '.join(codes)}\n"
            else:
                raise SystemExit(f"{c['id']}: unknown expectation {kind}")
            scenarios.append(s)
        if not scenarios:
            continue
        stem = fn[:-len(".yaml")]
        with open(os.path.join(out, stem + ".feature"), "w", encoding="utf-8") as f:
            f.write(f"# Translated from gql-compat @ {COMMIT} (https://github.com/tamnd/gql-compat),\n"
                    f"# corpus/suite/{fn}, licensed Apache-2.0 (LICENSE-gql-compat in this\n"
                    f"# directory; gql-compat states no copyright notice and has no NOTICE file).\n"
                    f"# Changed: the cases are rewritten as scenarios of lpg's conformance runner,\n"
                    f"# with their fixtures as INSERT statements, by\n"
                    f"# docs/evidence/lab-sprint-3/gqlcompat_translate.py. Do not edit by hand.\n\n")
            f.write(f"Feature: gql-compat {stem}\n\n")
            f.write("\n".join(scenarios))
        written += len(scenarios)
    print(f"translated {written} of {len(selected)} selected cases")
    for cid, why in skipped:
        print(f"  not translated: {cid}: {why}")


if __name__ == "__main__":
    main()
