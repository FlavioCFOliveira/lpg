"""Classify every parser rule of GQL.g4 as mandatory or as needing ISO feature codes.

Provisional (OQ-005): the placement rule (a construct a feature code covers is optional,
everything else is mandatory) is gql-compat's reading (README @ 9de1991), not ISO text.

Usage:
  python3 -I feature_map.py GQL.g4 FEATURES_XML ANCHORS_TSV GQL_COMPAT_DIR OUT_DIR
Writes OUT_DIR/rules.tsv (one row per parser rule) and OUT_DIR/table.md (the grouped table
copied into docs/technical-spec/query-engine.md). Exits non-zero on any check failure.
Own code (MIT, lpg project). Standard library only.
"""
import os
import re
import sys
from collections import defaultdict, deque

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import g4  # noqa: E402

# Approved set: user decisions 2026-10-08 (rmp task #37): "Seis + as exigidas", then GG03
# ("Só GG03"). GC01 covers the graph-type creation construct (anchors.tsv, GC01 line).
APPROVED = {"GG01", "GG02", "GG03", "GP18", "GV39", "GV40", "GV41",
            "GD01", "GT01", "GP04", "GC01", "GC04", "GQ01", "GT03"}
MAX_CLAUSE = 4  # conjunctions longer than this are not needed by any rule of GQL.g4


def fail(msg):
    print("CHECK FAILED:", msg, file=sys.stderr)
    sys.exit(1)


# --- EBNF of one alternative -------------------------------------------------------------

def parse_seq(toks, i, stop):
    items = []
    while i < len(toks) and toks[i][1] not in stop:
        atom, i = parse_atom(toks, i)
        suffix = None
        if i < len(toks) and toks[i][1] in ("?", "*", "+"):
            suffix = toks[i][1]
            i += 1
        items.append((atom, suffix))
    return items, i


def parse_atom(toks, i):
    k, v = toks[i]
    if v == "(":
        alts, i = [], i + 1
        while True:
            seq, i = parse_seq(toks, i, ("|", ")"))
            alts.append(seq)
            if toks[i][1] == ")":
                return ("group", alts), i + 1
            i += 1
    if v == "~":
        atom, i = parse_atom(toks, i + 1)
        return ("tok", "~"), i
    return (("ref", v) if k == "ident" else ("tok", v)), i + 1


def required_refs(seq, rules):
    """Rule names that every derivation of this sequence must expand (groups: per branch)."""
    out = []
    for atom, suffix in seq:
        if suffix in ("?", "*"):
            continue
        if atom[0] == "ref" and atom[1] in rules:
            out.append(("one", atom[1]))
        elif atom[0] == "group":
            out.append(("any", [required_refs(s, rules) for s in atom[1]]))
    return out


def all_refs(seq, rules):
    out = []
    for atom, _ in seq:
        if atom[0] == "ref" and atom[1] in rules:
            out.append(atom[1])
        elif atom[0] == "group":
            for s in atom[1]:
                out += all_refs(s, rules)
    return out


def elements(seq):
    out = []
    for atom, _ in seq:
        if atom[0] in ("ref", "tok"):
            out.append(atom[1])
        elif atom[0] == "group":
            for s in atom[1]:
                out += elements(s)
    return out


# --- DNF over feature codes ----------------------------------------------------------------
# A condition is a frozenset of clauses; a clause is a frozenset of codes (all needed).

TRUE = frozenset([frozenset()])
FALSE = frozenset()


def simplify(c):
    cl = [x for x in c if len(x) <= MAX_CLAUSE]
    keep = [x for x in cl if not any(y < x for y in cl)]
    return frozenset(keep)


def d_or(a, b):
    return simplify(a | b)


def d_and(a, b):
    return simplify(frozenset(x | y for x in a for y in b))


def show(c):
    if c == TRUE:
        return "mandatory"
    return " or ".join(sorted("+".join(sorted(x)) for x in c))


def load(gpath, fpath, apath, gcdir):
    """Read the grammar, the features insert and the anchors; check every anchor."""
    rules, order = g4.read(gpath)

    # Feature codes and insert lines.
    feats = {}
    for n, line in enumerate(open(fpath, encoding="utf-8"), 1):
        m = re.search(r"<code>(.*?)</code>", line)
        if m:
            feats[m.group(1)] = n
    if len(feats) != 228:
        fail(f"features insert has {len(feats)} codes, expected 228")

    # gql-compat case ids and their feature lists.
    cases = {}
    suite = os.path.join(gcdir, "corpus", "suite")
    for fn in sorted(os.listdir(suite)):
        cur = None
        for line in open(os.path.join(suite, fn), encoding="utf-8"):
            m = re.match(r"\s*- id: (\S+)", line)
            if m:
                cur = m.group(1)
                cases[cur] = set()
            m = re.match(r"\s*features: \[(.*)\]", line)
            if m and cur:
                cases[cur] = {x.strip() for x in m.group(1).split(",")}

    # Parse the alternatives.
    tree = {}
    for r in order:
        tree[r] = []
        for alt in rules[r]["alts"]:
            toks = [(k, v) for k, v in alt]
            seq, i = parse_seq(toks, 0, ())
            if i != len(toks):
                fail(f"cannot parse alternative of {r}")
            tree[r].append(seq)

    # Anchors.
    whole = defaultdict(lambda: FALSE)          # rule -> condition attached to the whole rule
    alt_anchor = defaultdict(lambda: FALSE)     # (rule, n) -> condition
    partial = defaultdict(set)                  # rule -> {"CODE (element)"}
    elem_anchor = defaultdict(lambda: FALSE)    # (rule, element) -> condition
    listed = set()
    for line in open(apath, encoding="utf-8"):
        if line.startswith("#") or not line.strip():
            continue
        code, anchors, evidence = line.rstrip("\n").split("\t")
        if code not in feats:
            fail(f"{code} is not in the features insert")
        listed.add(code)
        for ev in re.findall(r"gc:(\S+?)(?:;|$)", evidence):
            if ev not in cases or code not in cases[ev]:
                fail(f"{code}: gql-compat case {ev} missing or does not list {code}")
        if anchors == "-":
            continue
        for a in anchors.split():
            parts = a.split("+")
            base, extra = parts[0], parts[1:]
            for e in extra:
                if e not in feats:
                    fail(f"{code}: {e} is not in the features insert")
            cond = frozenset([frozenset([code, *extra])])
            m = re.fullmatch(r"([A-Za-z_][A-Za-z0-9_]*)(?:/(\d+)|:(\w+))?", base)
            if not m or m.group(1) not in rules:
                fail(f"{code}: anchor {a} names no parser rule")
            r = m.group(1)
            if m.group(2):
                n = int(m.group(2))
                if not 1 <= n <= len(tree[r]):
                    fail(f"{code}: {r} has no alternative {n}")
                alt_anchor[(r, n)] = d_or(alt_anchor[(r, n)], cond)
            elif m.group(3):
                el = m.group(3)
                if not any(el in elements(s) for s in tree[r]):
                    fail(f"{code}: {el} is not an element of {r}")
                elem_anchor[(r, el)] = d_or(elem_anchor[(r, el)], cond)
                whole_alt = [n for n, s in enumerate(tree[r], 1)
                             if len(s) == 1 and s[0][1] is None and s[0][0][0] in ("ref", "tok")
                             and s[0][0][1] == el and el not in rules]
                if whole_alt:
                    for n in whole_alt:
                        alt_anchor[(r, n)] = d_or(alt_anchor[(r, n)], cond)
                else:
                    partial[r].add(f"{'+'.join([code, *extra])} ({el})")
            else:
                whole[r] = d_or(whole[r], cond)
    missing = set(feats) - listed
    if missing:
        fail(f"codes without a line in anchors.tsv: {sorted(missing)}")
    return rules, order, feats, tree, whole, alt_anchor, elem_anchor, partial


def main():
    gpath, fpath, apath, gcdir, outdir = sys.argv[1:6]
    rules, order, feats, tree, whole, alt_anchor, elem_anchor, partial = load(
        gpath, fpath, apath, gcdir)

    # need(r): condition under which rule r can derive a sentence (least fixpoint).
    def seq_need(reqs, need):
        c = TRUE
        for kind, x in reqs:
            if kind == "one":
                c = d_and(c, need[x])
            else:
                g = FALSE
                for branch in x:
                    g = d_or(g, seq_need(branch, need))
                c = d_and(c, g)
        return c

    reqs = {r: [required_refs(s, rules) for s in tree[r]] for r in order}

    def alt_cond(r, n):
        a = alt_anchor[(r, n)]
        return TRUE if a == FALSE else a

    need = {r: FALSE for r in order}
    for _ in range(200):
        changed = False
        for r in order:
            c = FALSE
            for n, rq in enumerate(reqs[r], 1):
                c = d_or(c, d_and(alt_cond(r, n), seq_need(rq, need)))
            if whole[r] != FALSE:
                c = d_and(whole[r], c)
            if c != need[r]:
                need[r], changed = c, True
        if not changed:
            break
    else:
        fail("need() did not converge")

    # ctx(r): condition under which rule r is reachable from gqlProgram (least fixpoint).
    ctx = {r: FALSE for r in order}
    ctx["gqlProgram"] = TRUE
    for _ in range(200):
        changed = False
        new = {r: FALSE for r in order}
        new["gqlProgram"] = TRUE
        for p in order:
            if ctx[p] == FALSE:
                continue
            for n, s in enumerate(tree[p], 1):
                base = d_and(d_and(ctx[p], alt_cond(p, n)), whole[p] if whole[p] != FALSE else TRUE)
                for x in all_refs(s, rules):
                    new[x] = d_or(new[x], base)
        for r in order:
            if new[r] != ctx[r]:
                ctx[r], changed = new[r], True
        if not changed:
            break
    else:
        fail("ctx() did not converge")

    # A rule is usable when it is reachable and derivable: cond = ctx AND need.
    rows = []
    for r in order:
        if ctx[r] == FALSE:
            cond = None
        else:
            cond = d_and(ctx[r], need[r])
        part = set(partial[r])
        for n in range(1, len(tree[r]) + 1):
            if alt_anchor[(r, n)] != FALSE:
                part.add(f"{show(alt_anchor[(r, n)])} (alternative {n})")
        rows.append((r, rules[r]["line"], rules[r]["clause"], cond, sorted(part),
                     need[r], ctx[r]))

    def label(cond, own, where):
        """Display: the rule's own condition exactly; its context as the codes involved."""
        if cond is None:
            return "unreachable from gqlProgram"
        if cond == TRUE:
            return "mandatory"
        text = "" if own == TRUE else show(own)
        if where != TRUE:
            inside = sorted({c for clause in where for c in clause})
            text += ("; " if text else "") + "inside " + ", ".join(inside)
        return text

    def status(cond):
        if cond is None:
            return "unreachable"
        if cond == TRUE:
            return "supported"
        return "supported" if any(x <= APPROVED for x in cond) else "not supported"

    os.makedirs(outdir, exist_ok=True)
    with open(os.path.join(outdir, "rules.tsv"), "w", encoding="utf-8") as f:
        f.write("rule\tline\tclause\tneeds\tstatus\tpartial\n")
        for r, line, clause, cond, part, own, where in rows:
            f.write(f"{r}\t{line}\t{clause}\t{'-' if cond is None else show(cond)}\t"
                    f"{status(cond)}\t{'; '.join(part)}\n")

    groups = defaultdict(list)
    for r, line, clause, cond, part, own, where in rows:
        k = label(cond, own, where)
        st = "never parsed" if cond is None else status(cond)
        groups[(k, st)].append(r)
    with open(os.path.join(outdir, "table.md"), "w", encoding="utf-8") as f:
        f.write("| Needs | Status | Rules |\n|---|---|---|\n")
        def key(k):
            return (k[0] != "mandatory", k[0].startswith("unreachable"), k[0].startswith("inside"), k)
        for k in sorted(groups, key=key):
            names = ", ".join(f"`{x}`" for x in groups[k])
            f.write(f"| {k[0]} | {k[1]} | {names} ({len(groups[k])}) |\n")
        f.write("\n| Partial anchors inside supported rules | Rule |\n|---|---|\n")
        for r, line, clause, cond, part, own, where in rows:
            if part and cond is not None and status(cond) == "supported":
                f.write(f"| {'; '.join(part)} | `{r}` |\n")

    n_mand = sum(1 for row in rows if row[3] == TRUE)
    n_sup = sum(1 for row in rows if status(row[3]) == "supported")
    print(f"rules {len(rows)}; mandatory {n_mand}; supported with the approved set {n_sup}; "
          f"needing an unapproved code {sum(1 for row in rows if status(row[3]) == 'not supported')}; "
          f"unreachable {sum(1 for row in rows if row[3] is None)}")
    for code in sorted(APPROVED):
        if code not in feats:
            fail(f"approved {code} not in the insert")
        print(f"approved {code}: insert line {feats[code]}")


if __name__ == "__main__":
    main()
