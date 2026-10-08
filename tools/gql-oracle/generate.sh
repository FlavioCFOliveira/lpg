#!/bin/sh
# Writes the ANTLR parser of grammar/GQL.g4 to src/gen/ (not checked in;
# DECISION in rmp #36). The tool jar is the antlr4rust v0.6.0 release asset,
# checked by SHA-256. ANTLR_JAR names a local copy; otherwise it is
# downloaded to target/antlr/.
set -eu
cd "$(dirname "$0")"
url=https://github.com/antlr4rust/antlr4/releases/download/v0.6.0/antlr4-4.13.3-SNAPSHOT-complete.jar
sum=391e77311794f7c8fed2bc6d291f54b04725e37456d79e1981edfd3fc919b677
jar=${ANTLR_JAR:-target/antlr/antlr4-4.13.3-SNAPSHOT-complete.jar}
if [ ! -f "$jar" ]; then
    mkdir -p "$(dirname "$jar")"
    curl -fsSL -A 'lpg-lab (https://github.com/FlavioCFOliveira/lpg)' -o "$jar" "$url"
fi
echo "$sum  $jar" | sha256sum -c --quiet
rm -rf target/antlr/out src/gen
java -jar "$jar" -Dlanguage=Rust -Xexact-output-dir -o target/antlr/out grammar/GQL.g4
mkdir -p src/gen
cp target/antlr/out/gqllexer.rs target/antlr/out/gqlparser.rs target/antlr/out/gqllistener.rs src/gen/
(cd src/gen && sha256sum *.rs)
