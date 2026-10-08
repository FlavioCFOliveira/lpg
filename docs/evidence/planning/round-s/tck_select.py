"""Count the TCK scenarios and gql-compat cases that apply to the approved feature set.

Selection rule (TS-30, provisional OQ-005):
  * A scenario instance (a Scenario, or one Examples row of a Scenario Outline) applies when
    1. every GQL text it runs (the docstrings of "executing query", "having executed",
       "executing control query", "executing the program", "having executed the program",
       Background steps included, and the setup script of a named graph it uses) is accepted
       by the parser generated from the unmodified GQL.g4;
    2. every construct those texts use is supported by the approved set (anchors.tsv), so
       no unapproved feature code is needed; and
    3. every feature-code tag it carries or inherits is approved, and it carries no
       @ignore, @Todo or unmet capability tag.
  * A gql-compat case applies when its kind is mandatory or condition (user decision
    2026-10-08), every code in its `features` list is approved, and its setup and query
    texts pass rules 1 and 2. Condition cases rejected by GQL.g4 are counted apart: some
    test syntax errors on purpose.

Usage:
  python3 -I tck_select.py GQL.g4 FEATURES_XML ANCHORS_TSV ANTLR_JAR WORK_DIR \
      OPENGQL_TCK_DIR OPENCYPHER_DIR GQL_COMPAT_DIR
Needs a JDK (javac, java), taken from JAVA_HOME when it is set. WORK_DIR receives the generated parser and classes.
Own code (MIT, lpg project). Standard library, plus PyYAML for the gql-compat case files.
"""
import os
import re
import shutil
import subprocess
import sys
from collections import Counter

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import feature_map as fm  # noqa: E402

QUERY_STEPS = ("executing query:", "having executed:", "executing control query:",
               "executing the program:", "having executed the program:")
EXCLUDE_TAGS = {"ignore", "Todo"}
# Capability tags of opengql/tck and whether lpg meets them. Label-set bounds: TS-21 "Sizes"
# (IL001: no minimum, at most 2^32 - 1 labels). Key label sets (IL003) belong to graph types
# whose constructs are excluded by rule 2 anyway; they are counted as unmet.
CAPABILITY = {"MinNodeLabelsZero": True, "MaxNodeLabelsGTOne": True,
              "MinNodeTypeKeyLabelsZero": False, "MaxNodeTypeKeyLabelsGTOne": False}
NON_FEATURE_TAGS = {"base", "skipGrammarCheck", "skipStyleCheck"}


JDK = os.environ.get("JAVA_HOME")
JAVA = os.path.join(JDK, "bin", "java") if JDK else "java"
JAVAC = os.path.join(JDK, "bin", "javac") if JDK else "javac"


def build(jar, work, gpath):
    gen, cls = os.path.join(work, "gen"), os.path.join(work, "classes")
    for d in (gen, cls):
        shutil.rmtree(d, ignore_errors=True)
        os.makedirs(d)
    shutil.copy(gpath, os.path.join(gen, "GQL.g4"))
    subprocess.run([JAVA, "-jar", jar, "-Dlanguage=Java", "-no-listener", "-no-visitor",
                    "-DcontextSuperClass=org.antlr.v4.runtime.RuleContextWithAltNum",
                    "GQL.g4"], cwd=gen, check=True)
    probe = os.path.join(os.path.dirname(os.path.abspath(__file__)), "GqlProbe.java")
    srcs = [os.path.join(gen, f) for f in os.listdir(gen) if f.endswith(".java")] + [probe]
    subprocess.run([JAVAC, "-nowarn", "-cp", jar, "-d", cls, *srcs], check=True)
    return cls


def probe(jar, cls, work, texts):
    path = os.path.join(work, "texts.bin")
    with open(path, "w", encoding="utf-8") as f:
        f.write("\0".join(texts))
    out = subprocess.run([JAVA, "-Xss64m", "-cp", f"{jar}{os.pathsep}{cls}", "GqlProbe", path],
                         check=True, capture_output=True, text=True).stdout.splitlines()
    if len(out) != len(texts):
        fm.fail(f"probe returned {len(out)} lines for {len(texts)} texts")
    return [None if line == "ERR" else set(line.split()[1:]) for line in out]


def docstring(lines, i):
    """Return (text, next index) for the docstring that starts after line i, if any."""
    j = i + 1
    if j < len(lines) and lines[j].strip() == '"""':
        indent = lines[j].index('"')
        body, j = [], j + 1
        while lines[j].strip() != '"""':
            body.append(lines[j][indent:] if lines[j][:indent].strip() == "" else lines[j])
            j += 1
        return "\n".join(body), j + 1
    return None, i + 1


def scenarios(path, graph_text):
    """Yield (name, tags, texts) for every scenario instance of one .feature file."""
    lines = open(path, encoding="utf-8").read().split("\n")
    feature_tags, pending, background = set(), set(), []
    cur, outline, i = None, None, 0
    out = []

    def close():
        if cur is None:
            return
        if cur["outline"]:
            for row in cur["examples"]:
                texts = [re.sub(r"<(\w+)>", lambda m: row.get(m.group(1), m.group(0)), t)
                         for t in cur["texts"]]
                out.append((cur["name"], cur["tags"], background + texts))
        else:
            out.append((cur["name"], cur["tags"], background + cur["texts"]))

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
            cur = {"name": s, "tags": feature_tags | pending, "texts": [],
                   "outline": s.startswith("Scenario Outline"), "examples": []}
            pending, header = set(), None
        elif s.startswith("Examples"):
            header, ex_tags, pending = None, pending, set()
        elif s.startswith("|") and cur is not None and cur["outline"]:
            cells = [c.strip() for c in s.strip("|").split("|")]
            if header is None and lines[i - 1].strip().startswith("Examples"):
                header = cells
            elif header is not None:
                if not (ex_tags & EXCLUDE_TAGS):
                    cur["examples"].append(dict(zip(header, cells)))
        else:
            m = re.match(r"(Given|And) the (\S+) graph$", s)
            if m and m.group(2) not in ("omitted", "empty"):
                text = graph_text(m.group(2))
                (background if in_background else cur["texts"]).append(text)
            if any(s.endswith(q) for q in QUERY_STEPS):
                text, i = docstring(lines, i)
                if text is not None:
                    (background if in_background else cur["texts"]).append(text)
                continue
        i += 1
    close()
    return out


def main():
    gpath, fpath, apath, jar, work, gqltck, ocdir, gcdir = sys.argv[1:9]
    rules, order, feats, tree, whole, alt_anchor, elem_anchor, partial = fm.load(
        gpath, fpath, apath, gcdir)
    labels = {r: {lab: n for n, lab in enumerate(rules[r]["labels"], 1) if lab} for r in order}
    for (r, n) in list(alt_anchor):
        left_rec = any(a and a[0] == ("ident", r) for a in rules[r]["alts"])
        if left_rec and not any(rules[r]["labels"]):
            fm.fail(f"alternative anchor on unlabelled left-recursive rule {r}")

    # ANTLR matches a rule made only of single-token alternatives as one set (alternative 1);
    # anchors on such rules are element anchors, so their alternative numbers are ignored.
    token_set = {r for r in order
                 if all(len(a) == 1 and a[0][0] != "ident" or
                        (len(a) == 1 and a[0][1] not in rules) for a in rules[r]["alts"])}

    def needs(items):
        """Return the set of anchors (as DNF) the parse used."""
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
            return "rejected by GQL.g4", set()
        missing = set()
        for c in needs(items):
            if not any(clause <= fm.APPROVED for clause in c):
                missing |= {x for clause in c for x in clause} - fm.APPROVED
        return ("selected", set()) if not missing else ("needs unapproved codes", missing)

    cls = build(jar, work, gpath)
    report = {}

    def run_tck(name, root, graph_text):
        insts = []
        for dp, _, fs in os.walk(os.path.join(root, "features")):
            for f in sorted(fs):
                if f.endswith(".feature"):
                    insts += scenarios(os.path.join(dp, f), graph_text)
        flat = [t for _, _, texts in insts for t in texts]
        parsed = iter(probe(jar, cls, work, flat))
        counts, blockers = Counter(), Counter()
        for _, tags, texts in insts:
            results = [verdict(next(parsed)) for _ in texts]
            code_tags = {t for t in tags if t in feats}
            other = tags - set(feats) - NON_FEATURE_TAGS
            if not texts:
                state = "no GQL text"
            elif any(r[0] == "rejected by GQL.g4" for r in results):
                state = "rejected by GQL.g4"
            elif other & EXCLUDE_TAGS or any(not CAPABILITY.get(t, True) for t in other):
                state = "excluded by a non-feature tag"
            elif not code_tags <= fm.APPROVED or any(r[0] != "selected" for r in results):
                state = "needs unapproved codes"
                miss = (code_tags - fm.APPROVED).union(*[r[1] for r in results])
                for x in miss:
                    blockers[x] += 1
            else:
                state = "selected"
            counts[state] += 1
        report[name] = (len(insts), counts, blockers)

    def oc_graph(g):
        return open(os.path.join(ocdir, "tck", "graphs", g, g + ".cypher"), encoding="utf-8").read()

    def gql_graph(g):
        return open(os.path.join(gqltck, "data", "graphs", g + "-creator.gql"), encoding="utf-8").read()

    run_tck("opengql/tck", gqltck, gql_graph)
    run_tck("openCypher TCK", os.path.join(ocdir, "tck"), oc_graph)

    # gql-compat cases: kind mandatory or condition, approved `features`, and the same
    # construct check on the setup and query texts.
    import yaml  # PyYAML, needed only for the gql-compat case files
    kinds, gc = Counter(), []
    for fn in sorted(os.listdir(os.path.join(gcdir, "corpus", "suite"))):
        d = yaml.safe_load(open(os.path.join(gcdir, "corpus", "suite", fn), encoding="utf-8"))
        default = (d.get("defaults") or {}).get("kind")
        for c in d.get("cases") or []:
            kind = c.get("kind", default)
            kinds[kind] += 1
            if kind in ("mandatory", "condition"):
                texts = list(c.get("setup") or []) + ([c["query"]] if c.get("query") else [])
                gc.append((kind, set(c.get("features") or []), texts))
    parsed = iter(probe(jar, cls, work, [t for _, _, ts_ in gc for t in ts_]))
    gcounts = Counter()
    for kind, fs, texts in gc:
        results = [verdict(next(parsed)) for _ in texts]
        if not fs <= fm.APPROVED:
            state = "needs unapproved codes"
        elif any(r[0] == "rejected by GQL.g4" for r in results):
            # condition cases may test syntax errors on purpose
            state = "rejected by GQL.g4"
        elif any(r[0] != "selected" for r in results):
            state = "needs unapproved codes"
        else:
            state = "selected"
        gcounts[(kind, state)] += 1

    for name, (total, counts, blockers) in report.items():
        print(f"{name}: {total} scenario instances; " +
              "; ".join(f"{k} {v}" for k, v in sorted(counts.items())))
        print(f"  most frequent unapproved codes: " +
              ", ".join(f"{k} {v}" for k, v in blockers.most_common(12)))
    print(f"gql-compat: cases by kind {dict(sorted(kinds.items()))}")
    for k, v in sorted(gcounts.items()):
        print(f"  {k[0]}: {k[1]} {v}")


if __name__ == "__main__":
    main()
