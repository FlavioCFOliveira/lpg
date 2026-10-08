"""Minimal reader of the ANTLR 4 constructs used by opengql/grammar GQL.g4.

Returns, for every parser rule, its line, its ISO subclause comment and its
alternatives as token lists (rule references, token names, literals, operators).
Own code (MIT, lpg project).
"""
import re

TOKEN = re.compile(r"""
    (?P<ws>\s+)
  | (?P<lcomment>//[^\n]*)
  | (?P<bcomment>/\*.*?\*/)
  | (?P<literal>'(?:\\.|[^'\\])*')
  | (?P<charset>\[(?:\\.|[^\]\\])*\])
  | (?P<label>\#\s*[A-Za-z_][A-Za-z0-9_]*)
  | (?P<ident>[A-Za-z_][A-Za-z0-9_]*)
  | (?P<punct>->|\.\.|[:;|()?*+~.=,{}<>!])
""", re.S | re.X)


def tokens(text):
    pos, line = 0, 1
    while pos < len(text):
        m = TOKEN.match(text, pos)
        if not m:
            raise ValueError(f"unexpected {text[pos:pos+20]!r} at line {line}")
        kind = m.lastgroup
        val = m.group(kind)
        if kind not in ("ws", "bcomment"):
            yield kind, val, line
        line += val.count("\n")
        pos = m.end()


def read(path):
    """Return (rules, order): rules[name] = dict(line, clause, alts)."""
    text = open(path, encoding="utf-8").read()
    toks = list(tokens(text))
    rules, order = {}, []
    clause = None
    i = 0
    while i < len(toks):
        kind, val, line = toks[i]
        if kind == "lcomment":
            m = re.match(r"//\s*(\d+(?:\.\d+)*)\s+(.*)", val)
            if m:
                clause = f"{m.group(1)} {m.group(2).strip()}"
            i += 1
            continue
        if kind == "ident" and val == "options" and toks[i + 1][1] == "{":
            while toks[i][1] != "}":
                i += 1
            i += 1
            continue
        if kind == "ident" and val in ("grammar", "fragment"):
            if val == "grammar":
                while toks[i][1] != ";":
                    i += 1
                i += 1
                continue
            i += 1
            continue
        if kind == "ident" and i + 1 < len(toks):
            # rule header: name [options {...}] ':'
            j = i + 1
            while toks[j][0] == "lcomment":
                j += 1
            if toks[j][1] == "options":
                while toks[j][1] != "}":
                    j += 1
                j += 1
            if toks[j][1] != ":":
                raise ValueError(f"no ':' after {val} at line {line}")
            body, depth, j = [], 0, j + 1
            while not (toks[j][1] == ";" and depth == 0):
                k, v, _ = toks[j]
                if v == "(":
                    depth += 1
                elif v == ")":
                    depth -= 1
                if k not in ("lcomment",):
                    body.append((k, v))
                j += 1
            if val[0].islower():
                alts, cur, d, labels, lab = [], [], 0, [], None
                for k, v in body:
                    if v == "(":
                        d += 1
                    elif v == ")":
                        d -= 1
                    if v == "|" and d == 0:
                        alts.append(cur)
                        labels.append(lab)
                        cur, lab = [], None
                        continue
                    if k == "label":
                        lab = v.lstrip("#").strip()
                        continue
                    cur.append((k, v))
                alts.append(cur)
                labels.append(lab)
                # drop element labels "name =" (e.g. sign = (...))
                clean = []
                for alt in alts:
                    out = []
                    for n, (k, v) in enumerate(alt):
                        if k == "ident" and n + 1 < len(alt) and alt[n + 1][1] == "=":
                            continue
                        if v == "=":
                            continue
                        out.append((k, v))
                    clean.append(out)
                rules[val] = {"line": line, "clause": clause, "alts": clean, "labels": labels}
                order.append(val)
            i = j + 1
            continue
        i += 1
    return rules, order


def refs(alt, rules):
    return [v for k, v in alt if k == "ident" and v in rules]


def words(alt):
    return [v for k, v in alt if k == "ident" and v[0].isupper()]
