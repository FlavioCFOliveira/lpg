"""Find the Unicode version of the ID_Start and ID_Continue sets an ANTLR tool jar generates.

Usage: python3 -I compare.py JAR_SETS DerivedCoreProperties-<version>.txt ...
JAR_SETS: the output of Dump.java ("<code point> S" for ID_Start, "<code point> C" for
ID_Continue only). Each DerivedCoreProperties.txt is from
https://www.unicode.org/Public/<version>/ucd/. Own code (MIT, lpg project).
"""
import sys


def ucd(path):
    s, c = set(), set()
    for line in open(path, encoding="utf-8"):
        line = line.split("#")[0].strip()
        if not line:
            continue
        f = [x.strip() for x in line.split(";")]
        a, _, b = f[0].partition("..")
        r = range(int(a, 16), int(b or a, 16) + 1)
        if f[1] == "ID_Start":
            s.update(r)
        elif f[1] == "ID_Continue":
            c.update(r)
    return s, c


def main():
    jar = {"S": set(), "C": set()}
    for line in open(sys.argv[1]):
        cp, kind = line.split()
        jar[kind].add(int(cp))
    js, jc = jar["S"], jar["S"] | jar["C"]
    print(f"jar: ID_Start {len(js)}, ID_Continue {len(jc)}")
    for path in sys.argv[2:]:
        s, c = ucd(path)
        print(f"{path}: ID_Start {len(s)} {'equal' if s == js else f'differs by {len(s ^ js)}'}; "
              f"ID_Continue {len(c)} {'equal' if c == jc else f'differs by {len(c ^ jc)}'}")


if __name__ == "__main__":
    main()
