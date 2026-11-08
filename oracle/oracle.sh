#!/usr/bin/env bash
# Runs the Java Havana server as the reference ("oracle") for trace scenarios.
#
#   oracle.sh setup   build the jar, start MariaDB, load schema + migrations + seed, take snapshot
#   oracle.sh build   only build the jar into run/ (the database snapshot is kept)
#   oracle.sh start   start the Java server (if not running) and wait for the game port
#   oracle.sh stop    stop the Java server
#   oracle.sh reset   stop server, restore the database snapshot, start server again
#   oracle.sh status  show what is running
#
# Environment: HAVANA_DIR (the Java Havana checkout; ../aeyra-havana), ORACLE_JAVA_HOME (a JDK 25; defaults to /opt/jdk25, then JAVA_HOME), ORACLE_DB_PORT (13307),
# ORACLE_CHESSLIB (auto, the default: the real chesslib when jitpack.io answers, else the chess
# stub of no-jitpack.init.gradle; 1: always the real chesslib; 0: always the stub).
set -euo pipefail

ORACLE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# The Java Havana repository (the reference server: its sources, schema and data files), by
# default a sibling checkout named aeyra-havana.
REPO_DIR="${HAVANA_DIR:-$(cd "$ORACLE_DIR/../.." && pwd)/aeyra-havana}"
RUN_DIR="$ORACLE_DIR/run"
DB_CONTAINER="${ORACLE_DB_CONTAINER:-havana-oracle-db}"
DB_PORT="${ORACLE_DB_PORT:-13307}"
DB_IMAGE="${ORACLE_DB_IMAGE:-mariadb:11.4}"
GAME_PORT=12321
PID_FILE="$RUN_DIR/server.pid"

if [[ -n "${ORACLE_JAVA_HOME:-}" ]]; then
  JAVA_HOME="$ORACLE_JAVA_HOME"
elif [[ -x /opt/jdk25/bin/java ]]; then
  JAVA_HOME=/opt/jdk25
fi
export JAVA_HOME
JAVA="$JAVA_HOME/bin/java"

log() { echo "[oracle] $*" >&2; }

db() { docker exec -i "$DB_CONTAINER" mariadb -uroot -proot "$@"; }

wait_port() {
  local port=$1 tries=${2:-120}
  for ((i = 0; i < tries; i++)); do
    if (exec 3<>"/dev/tcp/127.0.0.1/$port") 2>/dev/null; then return 0; fi
    sleep 0.5
  done
  log "port $port did not open"; return 1
}

server_running() { [[ -f "$PID_FILE" ]] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null; }

# Whether the jar gets the real chesslib (served only by jitpack.io) or the chess stub.
use_chesslib() {
  case "${ORACLE_CHESSLIB:-auto}" in
    1) return 0 ;;
    0) return 1 ;;
    *) curl -fsS -o /dev/null --max-time 15 \
         https://jitpack.io/com/github/bhlangonijr/chesslib/1.1.1/chesslib-1.1.1.pom ;;
  esac
}

build_jar() {
  local init=()
  if use_chesslib; then
    log "building Havana-Server jar (with chesslib from jitpack.io)"
  else
    log "building Havana-Server jar (chess stub: jitpack.io unreachable or ORACLE_CHESSLIB=0)"
    init=(-I "$ORACLE_DIR/no-jitpack.init.gradle")
  fi
  (cd "$REPO_DIR" && ./gradlew -q --console=plain -Porg.gradle.java.installations.paths="$JAVA_HOME" \
     "${init[@]}" :Havana-Server:shadowJar)
  mkdir -p "$RUN_DIR"
  cp "$REPO_DIR/Havana-Server/build/libs/Havana-Server-unspecified.jar" "$RUN_DIR/Havana-Server.jar"
}

start_db() {
  if docker ps --format '{{.Names}}' | grep -qx "$DB_CONTAINER"; then return; fi
  docker rm -f "$DB_CONTAINER" >/dev/null 2>&1 || true
  docker image inspect "$DB_IMAGE" >/dev/null 2>&1 || docker pull "$DB_IMAGE" \
    || { docker pull "mirror.gcr.io/library/$DB_IMAGE" && docker tag "mirror.gcr.io/library/$DB_IMAGE" "$DB_IMAGE"; }
  log "starting MariaDB ($DB_CONTAINER on 127.0.0.1:$DB_PORT)"
  docker run -d --name "$DB_CONTAINER" -p "127.0.0.1:$DB_PORT:3306" \
    -e MARIADB_ROOT_PASSWORD=root -e MARIADB_DATABASE=havana \
    -e MARIADB_USER=havana -e MARIADB_PASSWORD=goldfish "$DB_IMAGE" >/dev/null
  for ((i = 0; i < 120; i++)); do
    if db -e 'SELECT 1' havana >/dev/null 2>&1; then return; fi
    sleep 0.5
  done
  log "MariaDB did not become ready"; return 1
}

load_schema() {
  log "loading schema, migrations and seed"
  db -e 'DROP DATABASE IF EXISTS havana; CREATE DATABASE havana;'
  db havana < "$REPO_DIR/tools/havana.sql"
  # aeyra-settings-20261025.sql is skipped: its string literals lost their quotes and it fails to parse.
  local m
  for m in update.1.1.sql update.1.2.sql update.1.3.sql update.1.5.sql \
           aeyra-cache-20261025.sql aeyra-indexes-20261025.sql hk_audit_log.sql; do
    db havana < "$REPO_DIR/tools/migrations/$m"
  done
  db havana < "$ORACLE_DIR/seed.sql"
  docker exec "$DB_CONTAINER" mariadb-dump -uroot -proot --single-transaction --skip-dump-date havana \
    > "$RUN_DIR/golden.sql"
}

write_config() {
  cat > "$RUN_DIR/server.ini" <<EOF
[Global]
server.bind=127.0.0.1
server.port=$GAME_PORT

[Server]
server.port=$GAME_PORT
server.limit.bandwidth=false
server.limit.bandwidth.amount=40960

[Rcon]
rcon.bind=127.0.0.1
rcon.port=12309

[Mus]
mus.bind=127.0.0.1
mus.port=12322

[Database]
mysql.hostname=127.0.0.1
mysql.port=$DB_PORT
mysql.username=havana
mysql.password=goldfish
mysql.database=havana

[Logging]
log.received.packets=true
log.sent.packets=true

[Console]
debug=false
EOF
  cp "$REPO_DIR/tools/figuredata.xml" "$RUN_DIR/"
  mkdir -p "$RUN_DIR/tools"
  rm -rf "$RUN_DIR/tools/snowstorm_maps"
  cp -r "$ORACLE_DIR/../tools/snowstorm_maps" "$RUN_DIR/tools/"
}

start_server() {
  if server_running; then return; fi
  # Written on every start, so a fresh checkout (oracle/run/ holds only the jar and golden.sql)
  # never lets Havana fall back to the server.ini it generates itself (port 3306).
  write_config
  log "starting Java server"
  # Background the java process itself (not a subshell), so $! is its pid and no
  # shell is left holding our stdout open.
  pushd "$RUN_DIR" >/dev/null
  nohup "$JAVA" -Djava.net.preferIPv4Stack=true -jar Havana-Server.jar \
    > "$RUN_DIR/stdout.log" 2>&1 < /dev/null &
  echo $! > "$PID_FILE"
  popd >/dev/null
  wait_port "$GAME_PORT"
  db havana < "$ORACLE_DIR/tickets.sql"
}

stop_server() {
  if server_running; then
    local pid; pid=$(cat "$PID_FILE")
    kill "$pid" 2>/dev/null || true
    for ((i = 0; i < 40; i++)); do kill -0 "$pid" 2>/dev/null || break; sleep 0.25; done
    kill -9 "$pid" 2>/dev/null || true
  fi
  rm -f "$PID_FILE"
}

case "${1:-}" in
  setup)
    mkdir -p "$RUN_DIR"
    build_jar
    start_db
    load_schema
    write_config
    log "setup done; run '$0 start'"
    ;;
  build) build_jar ;;
  start) start_db; start_server ;;
  stop) stop_server ;;
  reset)
    stop_server
    start_db
    db -e 'DROP DATABASE IF EXISTS havana; CREATE DATABASE havana;'
    db havana < "$RUN_DIR/golden.sql"
    start_server
    ;;
  status)
    docker ps --filter "name=$DB_CONTAINER" --format '{{.Names}} {{.Status}}'
    if server_running; then echo "server pid $(cat "$PID_FILE")"; else echo "server stopped"; fi
    ;;
  *) sed -n '2,14p' "$0"; exit 1 ;;
esac
