#!/usr/bin/env bash
# Regenerates crates/protocol/testdata/crypto-vectors.txt from the Java Havana classes in the oracle jar.
#   tools/crypto-vectors/run.sh [path/to/Havana-Server.jar]
# Needs a JDK 25 (ORACLE_JAVA_HOME, else /opt/jdk25) and the oracle jar (oracle/oracle.sh setup).
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WS="$(cd "$HERE/../.." && pwd)"
JAVA_HOME="${ORACLE_JAVA_HOME:-/opt/jdk25}"
JAR="${1:-$WS/oracle/run/Havana-Server.jar}"
OUT="$WS/crates/protocol/testdata/crypto-vectors.txt"
CLASSES="$(mktemp -d)"
trap 'rm -rf "$CLASSES"' EXIT

[[ -f "$JAR" ]] || { echo "missing $JAR: run oracle/oracle.sh setup, or pass the jar path" >&2; exit 1; }
mkdir -p "$(dirname "$OUT")"
"$JAVA_HOME/bin/javac" -cp "$JAR" -d "$CLASSES" "$HERE/Gen.java"
# Netty logs to stdout too; keep only the "name = value" lines.
"$JAVA_HOME/bin/java" -cp "$JAR:$CLASSES" Gen 2>/dev/null | grep -E "^[a-z][a-z0-9_.]* = " > "$OUT"
echo "wrote $OUT ($(wc -l < "$OUT") lines)" >&2
