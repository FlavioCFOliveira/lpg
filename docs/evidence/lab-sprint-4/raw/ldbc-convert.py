"""Rewrites the header line of LDBC SNB datagen 0.5.1 BI output
(composite-projected-fk, initial_snapshot, '|' delimiter, epoch millis) as
Neo4j import headers, following ldbc-snb-impls @ 11db98c
cypher/scripts/headers.txt (names, id spaces, types) and
import-to-neo4j.sh (labels and relationship types per file). Data lines are
copied byte for byte. Writes manifest.tsv: kind, file, labels-or-type, rows."""
import csv, glob, os, sys

src, dst = sys.argv[1], sys.argv[2]
NODES = {  # entity: (header, labels)
    "static/Organisation": ("id:ID(Organisation)|:LABEL|name:STRING|url:STRING", "Organisation"),
    "static/Place": ("id:ID(Place)|name:STRING|url:STRING|:LABEL", "Place"),
    "static/TagClass": ("id:ID(TagClass)|name:STRING|url:STRING", "TagClass"),
    "static/Tag": ("id:ID(Tag)|name:STRING|url:STRING", "Tag"),
    "dynamic/Comment": ("creationDate:LONG|id:ID(Comment)|locationIP:STRING|browserUsed:STRING|content:STRING|length:INT", "Comment;Message"),
    "dynamic/Forum": ("creationDate:LONG|id:ID(Forum)|title:STRING", "Forum"),
    "dynamic/Person": ("creationDate:LONG|id:ID(Person)|firstName:STRING|lastName:STRING|gender:STRING|birthday:LONG|locationIP:STRING|browserUsed:STRING|speaks:STRING[]|email:STRING[]", "Person"),
    "dynamic/Post": ("creationDate:LONG|id:ID(Post)|imageFile:STRING|locationIP:STRING|browserUsed:STRING|language:STRING|content:STRING|length:INT", "Post;Message"),
}
SPACE = {"Country": "Place", "City": "Place", "Company": "Organisation", "University": "Organisation"}
TYPE = {"isLocatedIn": "IS_LOCATED_IN", "isPartOf": "IS_PART_OF", "isSubclassOf": "IS_SUBCLASS_OF",
        "hasType": "HAS_TYPE", "hasCreator": "HAS_CREATOR", "hasTag": "HAS_TAG", "replyOf": "REPLY_OF",
        "containerOf": "CONTAINER_OF", "hasMember": "HAS_MEMBER", "hasModerator": "HAS_MODERATOR",
        "hasInterest": "HAS_INTEREST", "knows": "KNOWS", "likes": "LIKES", "studyAt": "STUDY_AT",
        "workAt": "WORK_AT"}
EDGE_PROP = {"classYear": "classYear:INT", "workFrom": "workFrom:INT", "creationDate": "creationDate:LONG"}
manifest = []
for path in sorted(glob.glob(os.path.join(src, "*", "*", "*.csv"))):
    rel = os.path.relpath(path, src)
    part, entity = rel.split(os.sep)[0], rel.split(os.sep)[1]
    key = f"{part}/{entity}"
    with open(path, "rb") as h:
        header = h.readline().decode().rstrip("\n").split("|")
        body = h.read()
    with open(path, newline="") as h:
        rows = sum(1 for _ in csv.reader(h, delimiter="|")) - 1
    if key in NODES:
        new, labels = NODES[key]
        assert len(new.split("|")) == len(header), (key, header)
        kind, extra = "nodes", labels
    else:
        a, t, b = entity.split("_")
        sa, sb = SPACE.get(a, a), SPACE.get(b, b)
        cols = []
        ends = iter([f":START_ID({sa})", f":END_ID({sb})"])
        for c in header:
            cols.append(next(ends) if c.endswith("Id") else EDGE_PROP[c])
        new, kind, extra = "|".join(cols), "edges", TYPE[t]
    os.makedirs(os.path.join(dst, part), exist_ok=True)
    out = part + "_" + entity + ".csv"
    with open(os.path.join(dst, out), "wb") as h:
        h.write(new.encode() + b"\n" + body)
    manifest.append((kind, out, extra, rows, new))
with open(os.path.join(dst, "manifest.tsv"), "w") as h:
    for m in manifest:
        h.write("\t".join(map(str, m)) + "\n")
n = sum(m[3] for m in manifest if m[0] == "nodes")
e = sum(m[3] for m in manifest if m[0] == "edges")
print(f"files {len(manifest)} node_rows {n} edge_rows {e}")
