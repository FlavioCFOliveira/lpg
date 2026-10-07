# Inclusive share of folded-stack weight whose stack contains a frame
# matching each regex. Usage: share.py FILE [ROOT_REGEX] -- REGEX...
import re, sys
f = sys.argv[1]; args = sys.argv[2:]
root = None
if args and args[0] != "--": root = re.compile(args[0]); args = args[1:]
args = args[1:] if args and args[0] == "--" else args
rows = []
for l in open(f):
    s, w = l.rstrip().rsplit(" ", 1); rows.append((s.split(";"), int(w)))
if root: rows = [(s, w) for s, w in rows if any(root.search(x) for x in s)]
tot = sum(w for _, w in rows)
print(f"# {f} root={root.pattern if root else '-'} weight={tot}")
for a in args:
    r = re.compile(a); c = sum(w for s, w in rows if any(r.search(x) for x in s))
    print(f"{100*c/tot:6.1f} %  {a}")
