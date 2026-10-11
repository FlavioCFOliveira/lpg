"""Compare the keyword sets of GQL.g4 with those of the ISO BNF insert.

Usage: python3 -I keywords.py GQL.g4 "ISO_IEC_39075(en).bnf.txt"
GQL.g4: the sections "// Reserved words", "// Prereserved words", "// Nonreserved words"
(token rules NAME: 'TEXT';). BNF: the productions <reserved word>, <pre-reserved word>,
<non-reserved word>. Own code (MIT, lpg project). Standard library only.
"""
import re
import sys


def g4_sets(path):
    sets, cur = {}, None
    names = {"// Reserved words": "reserved", "// Prereserved words": "pre-reserved",
             "// Nonreserved words": "non-reserved"}
    for n, line in enumerate(open(path, encoding="utf-8"), 1):
        s = line.strip()
        if s in names:
            cur = names[s]
            sets[cur] = {}
            continue
        if cur is None:
            continue
        m = re.match(r"[A-Z_][A-Z0-9_]*\s*:\s*'([^']+)'\s*;", s)
        if m:
            sets[cur][m.group(1).upper()] = n
        elif s and not s.startswith("//"):
            cur = None
    return sets


def bnf_sets(path):
    text = open(path, encoding="utf-8").read()
    out = {}
    for key, prod in (("reserved", "reserved word"), ("pre-reserved", "pre-reserved word"),
                      ("non-reserved", "non-reserved word")):
        m = re.search(r"<%s> ::=(.*?)\n\n" % re.escape(prod), text, re.S)
        out[key] = set(re.findall(r"\b([A-Z][A-Z0-9_]*)\b", m.group(1)))
    return out


def main():
    g, b = g4_sets(sys.argv[1]), bnf_sets(sys.argv[2])
    for k in ("reserved", "pre-reserved", "non-reserved"):
        gs, bs = set(g[k]), b[k]
        print(f"{k}: GQL.g4 {len(gs)} (lines {min(g[k].values())}-{max(g[k].values())}), BNF {len(bs)}")
        print(f"  only in BNF: {sorted(bs - gs)}")
        print(f"  only in GQL.g4: {sorted(gs - bs)}")
    allg = set().union(*map(set, g.values()))
    allb = set().union(*b.values())
    print(f"in no GQL.g4 keyword set: {sorted(allb - allg)}")
    for k in ("reserved", "pre-reserved", "non-reserved"):
        moved = {w: [j for j in g if w in g[j]] for w in b[k] if w in allg and w not in g[k]}
        if moved:
            print(f"BNF {k} words in another GQL.g4 set: {moved}")


if __name__ == "__main__":
    main()
