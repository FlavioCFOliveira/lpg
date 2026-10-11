"""Write the selection manifest of the vendored conformance scenarios (rmp #52).

Applies the TS-30 selection rule of round S (`../planning/round-s/tck_select.py`, same
verdict logic, same probe) and writes one line per scenario instance:

  artefact <TAB> feature file <TAB> scenario line <TAB> example row <TAB> state <TAB> codes

* artefact: `opengql-tck` or `opencypher`;
* feature file: path under the artefact's `features/` directory;
* scenario line: the trimmed `Scenario:` or `Scenario Outline:` line;
* example row: the cells of the `Examples` row joined by ` | ` (empty for a plain scenario);
* state: `selected`, `unsupported` (needs an unapproved code), `not-gql` (rejected by GQL.g4),
  `tag` (excluded by a non-feature tag);
* codes: the unapproved codes, comma-separated, for `unsupported`; the tags for `tag`.

Only the instances of the feature files to vendor are written, and every gql-compat case.
It also prints, per artefact, the counts over the whole artefact, the feature files that hold
at least one selected instance and the named graphs those instances use: the files to vendor.

Usage:
  python3 -I -B tck_manifest.py GQL.g4 FEATURES_XML ANCHORS_TSV ANTLR_JAR WORK_DIR \
      OPENGQL_TCK_DIR OPENCYPHER_DIR GQL_COMPAT_DIR OUT_TSV
Own code (MIT, lpg project). Standard library only, plus the round-S modules.
"""
import os
import re
import sys
from collections import Counter

ROUND_S = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "planning", "round-s")
sys.path.insert(0, ROUND_S)
import feature_map as fm  # noqa: E402
import tck_select as ts  # noqa: E402


def instances(path, graph_text):
    """(scenario line, row cells, tags, texts, graphs) per instance of one .feature file.

    Same scan as tck_select.scenarios, keeping the row cells and the named graphs.
    """
    lines = open(path, encoding="utf-8").read().split("\n")
    feature_tags, pending, background, bg_graphs = set(), set(), [], []
    cur, i = None, 0
    out = []

    def close():
        if cur is None:
            return
        if cur["outline"]:
            for row in cur["examples"]:
                texts = [re.sub(r"<(\w+)>", lambda m: row.get(m.group(1), m.group(0)), t)
                         for t in cur["texts"]]
                out.append((cur["name"], " | ".join(row.values()), cur["tags"],
                            background + texts, bg_graphs + cur["graphs"]))
        else:
            out.append((cur["name"], "", cur["tags"], background + cur["texts"],
                        bg_graphs + cur["graphs"]))

    in_background, header, ex_tags = False, None, set()
    while i < len(lines):
        s = lines[i].strip()
        if s.startswith("@"):
            pending |= set(re.findall(r"@(\w+)", s))
            i += 1
            continue
        if s.startswith("Feature:"):
            feature_tags, pending = pending, set()
        elif s.startswith("Background:"):
            in_background = True
        elif s.startswith("Scenario"):
            close()
            in_background = False
            cur = {"name": s, "tags": feature_tags | pending, "texts": [], "graphs": [],
                   "outline": s.startswith("Scenario Outline"), "examples": []}
            pending, header = set(), None
        elif s.startswith("Examples"):
            header, ex_tags, pending = None, pending, set()
        elif s.startswith("|") and cur is not None and cur["outline"]:
            cells = [c.strip() for c in s.strip("|").split("|")]
            if header is None and lines[i - 1].strip().startswith("Examples"):
                header = cells
            elif header is not None:
                if not (ex_tags & ts.EXCLUDE_TAGS):
                    cur["examples"].append(dict(zip(header, cells)))
        else:
            m = re.match(r"(Given|And) the (\S+) graph$", s)
            if m and m.group(2) not in ("omitted", "empty"):
                (background if in_background else cur["texts"]).append(graph_text(m.group(2)))
                (bg_graphs if in_background else cur["graphs"]).append(m.group(2))
            if any(s.endswith(q) for q in ts.QUERY_STEPS):
                text, i = ts.docstring(lines, i)
                if text is not None:
                    (background if in_background else cur["texts"]).append(text)
                continue
        i += 1
    close()
    return out


def main():
    gpath, fpath, apath, jar, work, gqltck, ocdir, gcdir, out_tsv = sys.argv[1:10]
    rules, order, feats, tree, whole, alt_anchor, elem_anchor, partial = fm.load(
        gpath, fpath, apath, gcdir)
    labels = {r: {lab: n for n, lab in enumerate(rules[r]["labels"], 1) if lab} for r in order}
    token_set = {r for r in order
                 if all(len(a) == 1 and a[0][0] != "ident" or
                        (len(a) == 1 and a[0][1] not in rules) for a in rules[r]["alts"])}

    def needs(items):
        used = []
        for it in items:
            if it.startswith("ambig:"):
                continue
            if "/" in it:
                r, a = it.split("/", 1)
                if r in token_set:
                    continue
                n = labels[r].get(a[1:]) if a.startswith("#") else int(a)
                c = alt_anchor.get((r, n))
                if c:
                    used.append(c)
            elif ":" in it:
                r, el = it.split(":", 1)
                c = elem_anchor.get((r, el))
                if c:
                    used.append(c)
            else:
                c = whole.get(it)
                if c:
                    used.append(c)
        return used

    def verdict(items):
        if items is None:
            return "not-gql", set()
        missing = set()
        for c in needs(items):
            if not any(clause <= fm.APPROVED for clause in c):
                missing |= {x for clause in c for x in clause} - fm.APPROVED
        return ("selected", set()) if not missing else ("unsupported", missing)

    cls = ts.build(jar, work, gpath)
    rows = []

    def run(name, root, graph_text):
        feats_root = os.path.join(root, "features")
        insts = []
        for dp, _, fs in os.walk(feats_root):
            for f in sorted(fs):
                if f.endswith(".feature"):
                    p = os.path.join(dp, f)
                    rel = os.path.relpath(p, feats_root)
                    insts += [(rel,) + x for x in instances(p, graph_text)]
        flat = [t for *_, texts, _ in insts for t in texts]
        parsed = iter(ts.probe(jar, cls, work, flat))
        counts, files, graphs = Counter(), set(), set()
        for rel, scen, row, tags, texts, gs in insts:
            results = [verdict(next(parsed)) for _ in texts]
            code_tags = {t for t in tags if t in feats}
            other = tags - set(feats) - ts.NON_FEATURE_TAGS
            codes = ""
            if not texts:
                state = "no-text"
            elif any(r[0] == "not-gql" for r in results):
                state = "not-gql"
            elif other & ts.EXCLUDE_TAGS or any(not ts.CAPABILITY.get(t, True) for t in other):
                state = "tag"
                codes = ",".join(sorted(other))
            elif not code_tags <= fm.APPROVED or any(r[0] != "selected" for r in results):
                state = "unsupported"
                codes = ",".join(sorted((code_tags - fm.APPROVED).union(*[r[1] for r in results])))
            else:
                state = "selected"
                files.add(rel)
                graphs |= set(gs)
            counts[state] += 1
            rows.append((name, rel, scen, row, state, codes))
        print(f"{name}: {len(insts)} instances; " +
              "; ".join(f"{k} {v}" for k, v in sorted(counts.items())))
        print(f"  files with a selected instance ({len(files)}): " + " ".join(sorted(files)))
        print(f"  named graphs of selected instances: " + " ".join(sorted(graphs)))

    def oc_graph(g):
        return open(os.path.join(ocdir, "tck", "graphs", g, g + ".cypher"), encoding="utf-8").read()

    def gql_graph(g):
        return open(os.path.join(gqltck, "data", "graphs", g + "-creator.gql"), encoding="utf-8").read()

    run("opengql-tck", gqltck, gql_graph)
    run("opencypher", os.path.join(ocdir, "tck"), oc_graph)

    # gql-compat: mandatory and condition cases, as tck_select (the `features` list and the
    # setup and query texts). The file column holds the suite file, the scenario column the
    # case id.
    import yaml  # PyYAML, needed only for the gql-compat case files
    gc = []
    for fn in sorted(os.listdir(os.path.join(gcdir, "corpus", "suite"))):
        d = yaml.safe_load(open(os.path.join(gcdir, "corpus", "suite", fn), encoding="utf-8"))
        default = (d.get("defaults") or {}).get("kind")
        for c in d.get("cases") or []:
            kind = c.get("kind", default)
            if kind in ("mandatory", "condition"):
                texts = list(c.get("setup") or []) + ([c["query"]] if c.get("query") else [])
                gc.append((fn, c["id"], kind, set(c.get("features") or []), texts))
    parsed = iter(ts.probe(jar, cls, work, [t for *_, ts_ in gc for t in ts_]))
    counts = Counter()
    for fn, cid, kind, fs, texts in gc:
        results = [verdict(next(parsed)) for _ in texts]
        codes = ""
        if not fs <= fm.APPROVED:
            state, codes = "unsupported", ",".join(sorted(fs - fm.APPROVED))
        elif any(r[0] == "not-gql" for r in results):
            state = "not-gql"
        elif any(r[0] != "selected" for r in results):
            state = "unsupported"
            codes = ",".join(sorted(set().union(*[r[1] for r in results])))
        else:
            state = "selected"
        counts[(kind, state)] += 1
        rows.append(("gql-compat", fn, cid, kind, state, codes))
    print("gql-compat: " + "; ".join(f"{k[0]} {k[1]} {v}" for k, v in sorted(counts.items())))
    # Only the vendored TCK files (those with a selected instance) and every gql-compat case.
    vendored = {(r[0], r[1]) for r in rows if r[4] == "selected"}
    with open(out_tsv, "w", encoding="utf-8") as f:
        f.write("artefact\tfile\tscenario\trow\tstate\tcodes\n")
        for r in rows:
            if r[0] == "gql-compat" or (r[0], r[1]) in vendored:
                f.write("\t".join(r) + "\n")


if __name__ == "__main__":
    main()
