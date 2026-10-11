"""List the GQL.g4 decisions that ANTLR reports as ambiguous on a corpus of GQL texts.

Corpus: the opengql/grammar samples, the six q0* inputs of docs/evidence/antlr-exp/corpus,
every GQL text of the opengql/tck and openCypher TCK scenarios (as tck_select.py extracts
them) and the setup and query texts of every gql-compat case. Prediction runs in
LL_EXACT_AMBIG_DETECTION mode (GqlProbe.java, argument "ambig").

Usage:
  python3 -I ambiguities.py GQL.g4 ANTLR_JAR WORK_DIR GRAMMAR_DIR ANTLR_EXP_CORPUS \
      OPENGQL_TCK_DIR OPENCYPHER_DIR GQL_COMPAT_DIR
Needs a JDK (JAVA_HOME) and PyYAML. Own code (MIT, lpg project).
"""
import glob
import os
import subprocess
import sys
from collections import Counter, defaultdict

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import tck_select as ts  # noqa: E402


def main():
    gpath, jar, work, gdir, aexp, gqltck, ocdir, gcdir = sys.argv[1:9]
    import yaml
    cls = ts.build(jar, work, gpath)
    corpus = {
        "samples": [open(f, encoding="utf-8").read() for f in sorted(glob.glob(os.path.join(gdir, "samples", "*")))],
        "antlr-exp": [open(f, encoding="utf-8").read() for f in sorted(glob.glob(os.path.join(aexp, "q0*.gql")))],
    }
    tcks = (("opengql/tck", gqltck, lambda g: open(os.path.join(gqltck, "data", "graphs", g + "-creator.gql"), encoding="utf-8").read()),
            ("openCypher TCK", os.path.join(ocdir, "tck"), lambda g: open(os.path.join(ocdir, "tck", "graphs", g, g + ".cypher"), encoding="utf-8").read()))
    for name, root, graph in tcks:
        texts = []
        for dp, _, fs in os.walk(os.path.join(root, "features")):
            for f in sorted(fs):
                if f.endswith(".feature"):
                    for _, _, t in ts.scenarios(os.path.join(dp, f), graph):
                        texts += t
        corpus[name] = sorted(set(texts))
    texts = []
    for f in sorted(glob.glob(os.path.join(gcdir, "corpus", "suite", "*.yaml"))):
        for c in yaml.safe_load(open(f, encoding="utf-8")).get("cases") or []:
            texts += list(c.get("setup") or []) + ([c["query"]] if c.get("query") else [])
    corpus["gql-compat"] = sorted(set(texts))

    total, per = Counter(), defaultdict(Counter)
    for name, texts in corpus.items():
        path = os.path.join(work, "ambig.bin")
        with open(path, "w", encoding="utf-8") as f:
            f.write("\0".join(texts))
        out = subprocess.run([ts.JAVA, "-Xss64m", "-cp", f"{jar}{os.pathsep}{cls}", "GqlProbe", path, "ambig"],
                             check=True, capture_output=True, text=True).stdout.splitlines()
        accepted = [line for line in out if line != "ERR"]
        print(f"{name}: {len(texts)} distinct texts, {len(accepted)} accepted")
        for line in accepted:
            for item in line.split()[1:]:
                if item.startswith("ambig:"):
                    total[item[6:]] += 1
                    per[item[6:]][name] += 1
    for item, n in total.most_common():
        print(f"{n} {item} {dict(per[item])}")


if __name__ == "__main__":
    main()
