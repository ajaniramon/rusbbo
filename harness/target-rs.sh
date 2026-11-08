#!/usr/bin/env bash
# Runs the Rust server as the target for `rusbbo-trace check`, from the same data as the oracle.
#
#   target-rs.sh reset   rebuild, restore the fixture database, (re)start the server
#   target-rs.sh stop    stop the server
#
# The fixture is the oracle's golden snapshot (oracle/run/golden.sql) loaded into a scratch
# MariaDB database and imported with rusbbo-import. It is rebuilt when golden.sql changes.
# The server listens on 127.0.0.1:12331, so it can run next to the Java oracle (12321):
#   rusbbo-trace check traces/login.json --target 127.0.0.1:12331 --reset "harness/target-rs.sh reset"
#
# Environment, for running several copies at once (e.g. one per git worktree):
#   TARGET_PORT    game port (12331)
#   TARGET_RCON_PORT  RCON port (TARGET_PORT - 12: what rusbbo-trace assumes without --rcon)
#   FIXTURE_DB     scratch MariaDB database used to build the fixture (rusbbo_fixture)
#   ORACLE_GOLDEN  golden.sql to build the fixture from (oracle/run/golden.sql of this checkout)
#   TARGET_WEB_PORT  also serve the website on this port, from $HAVANA_DIR's templates and files
#                    (web traces: rusbbo-trace web-check --target 127.0.0.1:$TARGET_WEB_PORT)
set -euo pipefail

HARNESS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WS_DIR="$(cd "$HARNESS_DIR/.." && pwd)"
RUN_DIR="$HARNESS_DIR/run"
GOLDEN="${ORACLE_GOLDEN:-$WS_DIR/oracle/run/golden.sql}"
FIXTURE_DB="${FIXTURE_DB:-rusbbo_fixture}"
DB_CONTAINER="${ORACLE_DB_CONTAINER:-havana-oracle-db}"
DB_PORT="${ORACLE_DB_PORT:-13307}"
PORT="${TARGET_PORT:-12331}"
RCON_PORT="${TARGET_RCON_PORT:-$((PORT - 12))}"
PID_FILE="$RUN_DIR/server.pid"
WEB_ARGS=()
if [[ -n "${TARGET_WEB_PORT:-}" ]]; then
  # Web checks run on golden.sql plus the web traces' seed (oracle/web-seed.sql), as recorded.
  WEB_SEED="$WS_DIR/oracle/web-seed.sql"
  if [[ -z "${ORACLE_GOLDEN:-}" && -f "$WEB_SEED" ]]; then
    WEB_GOLDEN="$WS_DIR/oracle/run/golden_web.sql"
    if [[ ! "$WEB_GOLDEN" -nt "$GOLDEN" || ! "$WEB_GOLDEN" -nt "$WEB_SEED" ]]; then
      cat "$GOLDEN" "$WEB_SEED" > "$WEB_GOLDEN"
    fi
    GOLDEN="$WEB_GOLDEN"
  fi
  HAVANA="${HAVANA_DIR:-$(cd "$WS_DIR/.." && pwd)/aeyra-havana}"
  WEB_ARGS=(--web "127.0.0.1:$TARGET_WEB_PORT" --web-templates "$HAVANA/tools/www-tpl" --web-site "$HAVANA/tools/www" --web-figuredata "$HAVANA/tools/figuredata.xml")
fi

# One fixture per source dump, so web checks (golden_web.sql) and game checks (golden.sql) never
# use each other's.
FIXTURE="$RUN_DIR/fixture.redb"
[[ "$(basename "$GOLDEN")" == golden.sql ]] || FIXTURE="$RUN_DIR/fixture-$(basename "$GOLDEN" .sql).redb"

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
  if [[ "$FIXTURE" -nt "$GOLDEN" ]]; then return; fi
  log "building fixture from $(basename "$GOLDEN")"
  docker exec -i "$DB_CONTAINER" mariadb -uroot -proot \
    -e "DROP DATABASE IF EXISTS $FIXTURE_DB; CREATE DATABASE $FIXTURE_DB;"
  docker exec -i "$DB_CONTAINER" mariadb -uroot -proot "$FIXTURE_DB" < "$GOLDEN"
  # Import to a temporary file, so a failed import never leaves a fixture that looks current.
  rm -f "$FIXTURE.tmp"
  "$WS_DIR/target/debug/rusbbo-import" --force --out "$FIXTURE.tmp" \
    --mysql "mysql://root:root@127.0.0.1:$DB_PORT/$FIXTURE_DB"
  mv "$FIXTURE.tmp" "$FIXTURE"
}

case "${1:-}" in
  reset)
    mkdir -p "$RUN_DIR"
    (cd "$WS_DIR" && cargo build -q -p rusbbo-server -p rusbbo-import)
    stop_server
    build_fixture
    cp "$FIXTURE" "$RUN_DIR/rusbbo.redb"
    nohup "$WS_DIR/target/debug/rusbbo-server" --db "$RUN_DIR/rusbbo.redb" --bind "127.0.0.1:$PORT" --rcon "127.0.0.1:$RCON_PORT" "${WEB_ARGS[@]}"\
      --dev-ticket 1=trace-sso-1 --dev-ticket 2=trace-sso-2 --dev-ticket 3=trace-sso-3 --dev-ticket 4=trace-sso-4 --dev-ticket 5=trace-sso-5 --dev-ticket 6=trace-sso-6 --dev-ticket 21=trace-sso-21 --dev-ticket 22=trace-sso-22 --dev-ticket 23=trace-sso-23 --dev-ticket 24=trace-sso-24 > "$RUN_DIR/server.log" 2>&1 < /dev/null &
    echo $! > "$PID_FILE"
    wait_port
    if [[ -n "${TARGET_WEB_PORT:-}" ]]; then PORT=$TARGET_WEB_PORT wait_port; fi
    ;;
  stop) stop_server ;;
  *) sed -n '2,10p' "$0"; exit 1 ;;
esac
