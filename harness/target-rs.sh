#!/usr/bin/env bash
# Runs the Rust server as the target for `havana-trace check`, from the same data as the oracle.
#
#   target-rs.sh reset   rebuild, restore the fixture database, (re)start the server
#   target-rs.sh stop    stop the server
#
# The fixture is the oracle's golden snapshot (oracle/run/golden.sql) loaded into a scratch
# MariaDB database and imported with havana-import. It is rebuilt when golden.sql changes.
# The server listens on 127.0.0.1:12331, so it can run next to the Java oracle (12321):
#   havana-trace check traces/login.json --target 127.0.0.1:12331 --reset "harness/target-rs.sh reset"
#
# Environment, for running several copies at once (e.g. one per git worktree):
#   TARGET_PORT    game port (12331)
#   FIXTURE_DB     scratch MariaDB database used to build the fixture (havana_fixture)
#   ORACLE_GOLDEN  golden.sql to build the fixture from (oracle/run/golden.sql of this checkout)
set -euo pipefail

HARNESS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WS_DIR="$(cd "$HARNESS_DIR/.." && pwd)"
RUN_DIR="$HARNESS_DIR/run"
GOLDEN="${ORACLE_GOLDEN:-$WS_DIR/oracle/run/golden.sql}"
FIXTURE_DB="${FIXTURE_DB:-havana_fixture}"
DB_CONTAINER="${ORACLE_DB_CONTAINER:-havana-oracle-db}"
DB_PORT="${ORACLE_DB_PORT:-13307}"
PORT="${TARGET_PORT:-12331}"
PID_FILE="$RUN_DIR/server.pid"

log() { echo "[target-rs] $*" >&2; }

wait_port() {
  for ((i = 0; i < 120; i++)); do
    if (exec 3<>"/dev/tcp/127.0.0.1/$PORT") 2>/dev/null; then return 0; fi
    sleep 0.25
  done
  log "port $PORT did not open"; return 1
}

stop_server() {
  if [[ -f "$PID_FILE" ]] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null; then
    local pid; pid=$(cat "$PID_FILE")
    kill "$pid" 2>/dev/null || true
    for ((i = 0; i < 40; i++)); do kill -0 "$pid" 2>/dev/null || break; sleep 0.1; done
    kill -9 "$pid" 2>/dev/null || true
  fi
  rm -f "$PID_FILE"
}

build_fixture() {
  [[ -f "$GOLDEN" ]] || { log "missing $GOLDEN: run oracle/oracle.sh setup first"; exit 1; }
  if [[ "$RUN_DIR/fixture.redb" -nt "$GOLDEN" ]]; then return; fi
  log "building fixture from golden.sql"
  docker exec -i "$DB_CONTAINER" mariadb -uroot -proot \
    -e "DROP DATABASE IF EXISTS $FIXTURE_DB; CREATE DATABASE $FIXTURE_DB;"
  docker exec -i "$DB_CONTAINER" mariadb -uroot -proot "$FIXTURE_DB" < "$GOLDEN"
  # Import to a temporary file, so a failed import never leaves a fixture that looks current.
  rm -f "$RUN_DIR/fixture.redb.tmp"
  "$WS_DIR/target/debug/havana-import" --force --out "$RUN_DIR/fixture.redb.tmp" \
    --mysql "mysql://root:root@127.0.0.1:$DB_PORT/$FIXTURE_DB"
  mv "$RUN_DIR/fixture.redb.tmp" "$RUN_DIR/fixture.redb"
}

case "${1:-}" in
  reset)
    mkdir -p "$RUN_DIR"
    (cd "$WS_DIR" && cargo build -q -p havana-server -p havana-import)
    stop_server
    build_fixture
    cp "$RUN_DIR/fixture.redb" "$RUN_DIR/havana.redb"
    nohup "$WS_DIR/target/debug/havana-server" --db "$RUN_DIR/havana.redb" --bind "127.0.0.1:$PORT" \
      --dev-ticket 1=trace-sso-1 --dev-ticket 2=trace-sso-2 --dev-ticket 3=trace-sso-3 --dev-ticket 4=trace-sso-4 > "$RUN_DIR/server.log" 2>&1 < /dev/null &
    echo $! > "$PID_FILE"
    wait_port
    ;;
  stop) stop_server ;;
  *) sed -n '2,10p' "$0"; exit 1 ;;
esac
