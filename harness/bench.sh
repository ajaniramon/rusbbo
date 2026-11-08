#!/usr/bin/env bash
# Compares the Java server and the Rust rewrite under load, on the same data and the same machine.
#
#   harness/bench.sh [java|rust|both]    (default both; results in harness/run/bench/)
#
# For each server: start it on a scratch copy of the oracle's golden snapshot plus BENCH_USERS
# bench users (havana_bench), time the start until the game port opens, sample its memory (RSS)
# and CPU every second, measure it idle, then run `havana-trace load` at each BENCH_LEVELS bot
# count in turn on the same process (the bots leave between levels). Both servers run with their
# packet logging off. Needs the release binaries (cargo build --release) and the oracle jar.
#
# Environment: BENCH_USERS (500), BENCH_LEVELS ("100 300 500"), BENCH_RAMP (30 s), BENCH_DURATION
# (60 s), BENCH_INTERVAL (4000 ms between one bot's actions), BENCH_IDLE (20 s), BENCH_PER_ROOM
# (25 bots per room), ORACLE_JAVA_HOME.
set -euo pipefail

HARNESS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WS_DIR="$(cd "$HARNESS_DIR/.." && pwd)"
REPO_DIR="$(cd "$WS_DIR/.." && pwd)"
OUT="$HARNESS_DIR/run/bench"
GOLDEN="$WS_DIR/oracle/run/golden.sql"
DB_CONTAINER="${ORACLE_DB_CONTAINER:-havana-oracle-db}"
DB_PORT="${ORACLE_DB_PORT:-13307}"
USERS="${BENCH_USERS:-500}"
LEVELS="${BENCH_LEVELS:-100 300 500}"
RAMP="${BENCH_RAMP:-30}"
DURATION="${BENCH_DURATION:-60}"
INTERVAL="${BENCH_INTERVAL:-4000}"
IDLE="${BENCH_IDLE:-20}"
PER_ROOM="${BENCH_PER_ROOM:-25}"
JAVA="${ORACLE_JAVA_HOME:-/opt/jdk25}/bin/java"
JAVA_PORT=12421
RUST_PORT=12431
FIRST=1001
# Ordinary public rooms (no pool, game lobby or staff room), each holding at least 40.
ROOMS=1,2,5,7,9,17,19,20,21,22,23,24,26,28,29,30,31,32,33,34,39,40,41,42

log() { echo "[bench] $*" >&2; }
db() { docker exec -i "$DB_CONTAINER" mariadb -uroot -proot "$@"; }

port_open() { (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null; }

# Milliseconds until the port opens.
wait_port() {
  local port=$1 t0; t0=$(date +%s%N)
  for ((i = 0; i < 1200; i++)); do
    if port_open "$port"; then echo $(( ($(date +%s%N) - t0) / 1000000 )); return 0; fi
    sleep 0.05
  done
  log "port $port did not open"; return 1
}

make_db() {
  log "building havana_bench ($USERS bench users)"
  db -e 'DROP DATABASE IF EXISTS havana_bench; CREATE DATABASE havana_bench;'
  db havana_bench < "$GOLDEN"
  {
    echo "INSERT INTO users (id, username, password, figure, sex, motto, email, credits, tickets, film, \`rank\`,"
    echo "  last_online, created_at, sso_ticket, tutorial_finished, has_flash_warning) VALUES"
    for ((i = FIRST; i < FIRST + USERS; i++)); do
      sep=","; ((i == FIRST + USERS - 1)) && sep=";"
      echo "($i, 'bench$i', '', 'hd-180-1.ch-210-66.lg-270-82.sh-290-91.hr-100-61', 'M', 'bench', 'b$i@bench.local',"
      echo "  1000, 10, 10, 1, '2026-01-01 00:00:00', '2099-01-01 00:00:00', 'bench-sso-$i', 1, 0)$sep"
    done
    echo "INSERT INTO users_statistics (user_id, has_tutorial, is_guidable) VALUES"
    for ((i = FIRST; i < FIRST + USERS; i++)); do
      sep=","; ((i == FIRST + USERS - 1)) && sep=";"
      echo "($i, 0, 0)$sep"
    done
    # Every bot connects from 127.0.0.1.
    echo "UPDATE settings SET value = '0' WHERE setting = 'max.connections.per.ip';"
  } | db havana_bench
}

tickets_sql() { echo "UPDATE users SET sso_ticket = CONCAT('bench-sso-', id) WHERE id >= $FIRST;"; }

# Samples RSS (MiB) and CPU (% of one core) of $1 every second into $2 until it exits.
sampler() {
  local pid=$1 out=$2 hz; hz=$(getconf CLK_TCK)
  echo "t,rss_mib,cpu_pct" > "$out"
  local prev t0; prev=$(awk '{print $14+$15}' "/proc/$pid/stat"); t0=$(date +%s)
  while kill -0 "$pid" 2>/dev/null; do
    sleep 1
    local now rss; now=$(awk '{print $14+$15}' "/proc/$pid/stat" 2>/dev/null) || break
    rss=$(awk '/VmRSS/ {print $2}' "/proc/$pid/status" 2>/dev/null) || break
    echo "$(( $(date +%s) - t0 )),$(( rss / 1024 )),$(( (now - prev) * 100 / hz ))" >> "$out"
    prev=$now
  done
}

# Mean CPU and peak RSS in the sampler file between seconds $2 and $3.
window() {
  awk -F, -v a="$2" -v b="$3" 'NR > 1 && $1 >= a && $1 <= b { c += $3; n++; if ($2 > m) m = $2 }
    END { if (n) printf "{\"cpu_mean_pct\": %.1f, \"rss_peak_mib\": %d}", c / n, m; else print "{}" }' "$1"
}

start_java() {
  local dir="$OUT/java-run"
  rm -rf "$dir"; mkdir -p "$dir/tools"
  cp "$WS_DIR/oracle/run/Havana-Server.jar" "$WS_DIR/oracle/run/logback.xml" "$REPO_DIR/tools/figuredata.xml" "$dir/"
  cp -r "$REPO_DIR/tools/snowstorm_maps" "$dir/tools/"
  cat > "$dir/server.ini" <<EOF
[Global]
server.bind=127.0.0.1
server.port=$JAVA_PORT

[Server]
server.port=$JAVA_PORT
server.limit.bandwidth=false
server.limit.bandwidth.amount=40960

[Rcon]
rcon.bind=127.0.0.1
rcon.port=$((JAVA_PORT - 12))

[Mus]
mus.bind=127.0.0.1
mus.port=$((JAVA_PORT + 1))

[Database]
mysql.hostname=127.0.0.1
mysql.port=$DB_PORT
mysql.username=havana
mysql.password=goldfish
mysql.database=havana_bench

[Logging]
log.received.packets=false
log.sent.packets=false

[Console]
debug=false
EOF
  local h; for h in '%' 127.0.0.1 localhost; do db -e "GRANT ALL ON havana_bench.* TO 'havana'@'$h'" 2>/dev/null || true; done
  pushd "$dir" >/dev/null
  nohup "$JAVA" -Djava.net.preferIPv4Stack=true -jar Havana-Server.jar > stdout.log 2>&1 < /dev/null &
  SERVER_PID=$!
  popd >/dev/null
  STARTUP_MS=$(wait_port "$JAVA_PORT")
  tickets_sql | db havana_bench
  PORT=$JAVA_PORT
}

start_rust() {
  local dir="$OUT/rust-run" args=()
  rm -rf "$dir"; mkdir -p "$dir"
  "$WS_DIR/target/release/havana-import" --force --out "$dir/bench.redb" \
    --mysql "mysql://root:root@127.0.0.1:$DB_PORT/havana_bench" > "$dir/import.log" 2>&1
  for ((i = FIRST; i < FIRST + USERS; i++)); do args+=(--dev-ticket "$i=bench-sso-$i"); done
  pushd "$dir" >/dev/null
  nohup "$WS_DIR/target/release/havana-server" --db bench.redb --bind "127.0.0.1:$RUST_PORT" \
    "${args[@]}" > server.log 2>&1 < /dev/null &
  SERVER_PID=$!
  popd >/dev/null
  STARTUP_MS=$(wait_port "$RUST_PORT")
  PORT=$RUST_PORT
}

bench() {
  local name=$1
  log "== $name"
  "start_$name"
  log "$name: port open after ${STARTUP_MS} ms (pid $SERVER_PID)"
  local samples="$OUT/$name-samples.csv" t0; t0=$(date +%s)
  sampler "$SERVER_PID" "$samples" &
  local sampler_pid=$!
  sleep "$IDLE"
  local idle; idle=$(window "$samples" $((IDLE / 2)) "$IDLE")
  local levels="" n
  for n in $LEVELS; do
    log "$name: $n bots"
    local a=$(( $(date +%s) - t0 + RAMP + 5 ))
    "$WS_DIR/target/release/havana-trace" load --target "127.0.0.1:$PORT" --bots "$n" --first "$FIRST" \
      --rooms "$ROOMS" --per-room "$PER_ROOM" --ramp "$RAMP" --duration "$DURATION" --interval "$INTERVAL" \
      --json "$OUT/$name-$n.json" > /dev/null || log "$name: $n bots had failed logins or entries"
    local b=$(( $(date +%s) - t0 ))
    levels+="${levels:+, }\"$n\": {\"load\": $(cat "$OUT/$name-$n.json"), \"server\": $(window "$samples" "$a" "$b")}"
    sleep 5
  done
  kill "$SERVER_PID" 2>/dev/null || true
  wait "$SERVER_PID" 2>/dev/null || true
  wait "$sampler_pid" 2>/dev/null || true
  echo "{\"server\": \"$name\", \"startup_ms\": $STARTUP_MS, \"idle\": $idle, \"levels\": {$levels}}" > "$OUT/$name.json"
  log "$name: done, $OUT/$name.json"
}

which=${1:-both}
mkdir -p "$OUT"
make_db
case "$which" in
  java) bench java ;;
  rust) bench rust ;;
  both) bench java; bench rust ;;
  *) sed -n '2,13p' "$0"; exit 1 ;;
esac
