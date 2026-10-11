#!/usr/bin/env python3
"""GraphML check of lpg's exports (rmp #54, #59): reads each file with
NetworkX and validates it against the GraphML 1.0 schema and against lpg's
extension schema (TS-26 item B).

Run with the laboratory interpreter, isolated:

    /tmp/lpg-lab-venv/bin/python -I graphml_check.py FILE...
    /tmp/lpg-lab-venv/bin/python -I graphml_check.py --probes DIR

``--probes DIR`` writes the three probe files of the Sprint 4 plan,
Appendix A, into DIR and checks them. The GraphML schema files are fetched
from graphml.graphdrawing.org into /tmp/lpg-graphml-xsd (User-Agent
``lpg-dev``), checked against the SHA-256 values of the plan, section 9,
and copied with their absolute schemaLocation URLs rewritten to the local
copies; they are never stored in the repository (their licence is not
confirmed). lpg's schema files are read from src/ops/bulk/ of the
repository this script lies in.

Tools: NetworkX 3.7 (BSD-3-Clause), xmlschema 4.3.2 and elementpath 5.1.4
(MIT), installed in /tmp/lpg-lab-venv. Output: one line per file.
"""

import hashlib
import pathlib
import sys
import urllib.request

import networkx
import xmlschema

BASE = "http://graphml.graphdrawing.org/xmlns/1.0/"
SHA256 = {
    "graphml.xsd": "c5d04fb93d579712aa375f3ace67fe9243d6aaa9b039576516722af2d9c5af52",
    "graphml-structure.xsd": "7809123855d2ef634dea2b678055b6230607efb9addde55a06c4afd67075a04b",
    "graphml-attributes.xsd": "e13e2d5dc72c1d8eb758309f6b26570d7e060219277773892529ec97dddf36bb",
    "graphml-parseinfo.xsd": "6d025e0b809d95fbf296139fda8721591d483de8eee1b0174b4a1508aee792ca",
}
CACHE = pathlib.Path("/tmp/lpg-graphml-xsd")
REPO = pathlib.Path(__file__).resolve().parents[3]
USER_AGENT = "lpg-dev (https://github.com/FlavioCFOliveira/lpg)"

PROBE_T = """<?xml version="1.0" encoding="UTF-8"?>
<graphml xmlns="http://graphml.graphdrawing.org/xmlns" xmlns:lpg="https://crates.io/crates/lpg/graphml">
<key id="labels" for="node" attr.name="labels" attr.type="string"/>
<key id="p" for="node" attr.name="p" attr.type="string" lpg:type="point"/>
<key id="m" for="node" attr.name="m" attr.type="string"/>
<graph id="G" edgedefault="directed">
<node id="n0" labels=":A"><data key="labels">:A</data><data key="p">{srid: 7203, x: 1.0, y: 2.0}</data><data key="m" lpg:type="long">5</data></node>
<node id="n1"><data key="m" lpg:type="string">x</data></node>
<edge id="e0" source="n0" target="n1" label="R"><data key="labels">R</data></edge>
</graph></graphml>
"""


def probes(directory):
    """Writes t, t2 (without the labels and label attributes) and t3 (t2
    without any lpg:type attribute) of the plan's Appendix A."""
    d = pathlib.Path(directory)
    d.mkdir(parents=True, exist_ok=True)
    t2 = PROBE_T.replace(' labels=":A"', "").replace(' label="R"', "")
    t3 = t2.replace(' lpg:type="point"', "").replace(' lpg:type="long"', "").replace(
        ' lpg:type="string"', ""
    )
    paths = []
    for name, text in [("t.graphml", PROBE_T), ("t2.graphml", t2), ("t3.graphml", t3)]:
        p = d / name
        p.write_text(text, encoding="utf-8")
        paths.append(p)
    return paths


def schemas():
    """The GraphML schema and lpg's extension schema, from local copies."""
    CACHE.mkdir(parents=True, exist_ok=True)
    for name, digest in SHA256.items():
        path = CACHE / name
        if not path.exists():
            req = urllib.request.Request(BASE + name, headers={"User-Agent": USER_AGENT})
            with urllib.request.urlopen(req, timeout=60) as r:
                path.write_bytes(r.read())
        got = hashlib.sha256(path.read_bytes()).hexdigest()
        if got != digest:
            sys.exit(f"{name}: SHA-256 {got} differs from {digest}")
        local = CACHE / ("local-" + name)
        text = path.read_text(encoding="utf-8").replace(BASE, CACHE.as_uri() + "/local-")
        local.write_text(text, encoding="utf-8")
    src = REPO / "src" / "ops" / "bulk"
    for name in ["graphml-lpg.xsd", "graphml-lpg-type.xsd"]:
        text = (src / name).read_text(encoding="utf-8")
        text = text.replace(BASE + "graphml.xsd", (CACHE / "local-graphml.xsd").as_uri())
        (CACHE / name).write_text(text, encoding="utf-8")
    base = xmlschema.XMLSchema(str(CACHE / "local-graphml.xsd"))
    lpg = xmlschema.XMLSchema(str(CACHE / "graphml-lpg.xsd"))
    return base, lpg


def verdict(schema, path):
    try:
        schema.validate(str(path))
        return "valid"
    except xmlschema.XMLSchemaValidationError as e:
        reason = (e.reason or str(e)).splitlines()[0]
        return f"invalid ({reason})"


def check(paths):
    base, lpg = schemas()
    for p in paths:
        try:
            g = networkx.read_graphml(str(p))
            nx_result = f"networkx=read nodes={g.number_of_nodes()} edges={g.number_of_edges()}"
        except Exception as e:  # noqa: BLE001 - every reader failure is reported
            nx_result = f"networkx=FAILED ({type(e).__name__}: {e})"
        print(f"{p.name}: {nx_result}; graphml.xsd={verdict(base, p)}; graphml-lpg.xsd={verdict(lpg, p)}")


def main(argv):
    if len(argv) >= 2 and argv[0] == "--probes":
        check(probes(argv[1]))
    elif argv:
        check([pathlib.Path(a) for a in argv])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:])
